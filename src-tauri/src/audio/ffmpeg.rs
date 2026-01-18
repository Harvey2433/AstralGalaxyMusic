// src/audio/ffmpeg.rs

use super::AudioEngine;
use std::process::{Command, Stdio};
use std::path::{Path, PathBuf};
use std::fs;
use std::env;
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering}; 
use std::thread;
use std::time::Duration;
use tauri::{Window, Emitter, Manager}; 
use zip::ZipArchive;
use rodio::{OutputStreamHandle, Sink, Source, buffer::SamplesBuffer};

pub struct FFmpegEngine {
    sink: Arc<Mutex<Sink>>,
    stream_handle: OutputStreamHandle,
    current_samples: Option<Arc<Vec<f32>>>, 
    sample_rate: u32,
    target_volume: Arc<RwLock<f32>>, 
    fade_token: Arc<AtomicUsize>,
    last_seek_pos: Arc<RwLock<f64>>,
    // 新增：最高优先级的逻辑播放状态，与 Sink 的物理状态解耦
    is_playing: Arc<AtomicBool>,
}

impl FFmpegEngine {
    pub fn new(stream_handle: OutputStreamHandle) -> Self { 
        let sink = Sink::try_new(&stream_handle).expect("Failed to create FFmpeg Sink");
        sink.set_volume(0.0);
        Self { 
            sink: Arc::new(Mutex::new(sink)),
            stream_handle,
            current_samples: None,
            sample_rate: 44100,
            target_volume: Arc::new(RwLock::new(1.0)), 
            fade_token: Arc::new(AtomicUsize::new(0)), 
            last_seek_pos: Arc::new(RwLock::new(0.0)),
            is_playing: Arc::new(AtomicBool::new(false)),
        } 
    }

    // --- 核心辅助：通用淡入启动器 ---
    fn run_fade_in(&self, duration_ms: u64) {
        let sink_clone = self.sink.clone();
        let target_vol_lock = self.target_volume.clone();
        
        let my_token = self.fade_token.fetch_add(1, Ordering::SeqCst) + 1;
        let token_ref = self.fade_token.clone();

        // 立即操作：确保处于播放状态，且起始音量为 0
        if let Ok(sink) = sink_clone.lock() {
            // 如果队列空了，淡入也没意义，防止 rodio 内部警告
            if sink.empty() { return; }
            sink.set_volume(0.0); 
            sink.play();
        }

        thread::spawn(move || {
            let target_vol = *target_vol_lock.read().unwrap();
            if target_vol <= 0.001 { return; }

            let step_time = 15; 
            let steps = (duration_ms / step_time).max(1); 
            let step_duration = Duration::from_millis(step_time);
            
            for i in 1..=steps {
                if token_ref.load(Ordering::SeqCst) != my_token { return; }

                let current_vol = target_vol * (i as f32 / steps as f32);
                if let Ok(sink) = sink_clone.lock() {
                    sink.set_volume(current_vol);
                }
                thread::sleep(step_duration);
            }
            
            if token_ref.load(Ordering::SeqCst) == my_token {
                if let Ok(sink) = sink_clone.lock() {
                    sink.set_volume(target_vol);
                }
            }
        });
    }

    // --- 核心辅助：通用淡出暂停器 ---
    fn run_fade_out_pause(&self, duration_ms: u64) {
        let sink_clone = self.sink.clone();
        
        let my_token = self.fade_token.fetch_add(1, Ordering::SeqCst) + 1;
        let token_ref = self.fade_token.clone();

        thread::spawn(move || {
            let start_vol = if let Ok(sink) = sink_clone.lock() { sink.volume() } else { return; };

            let step_time = 15;
            let steps = (duration_ms / step_time).max(1);
            let step_duration = Duration::from_millis(step_time);

            for i in 0..steps {
                if token_ref.load(Ordering::SeqCst) != my_token { return; }

                let remaining_factor = 1.0 - (i as f32 / steps as f32);
                let current_vol = start_vol * remaining_factor;
                
                if let Ok(sink) = sink_clone.lock() {
                    sink.set_volume(current_vol);
                }
                thread::sleep(step_duration);
            }

            // 只有 Token 没变，才执行真正的暂停
            if token_ref.load(Ordering::SeqCst) == my_token {
                if let Ok(sink) = sink_clone.lock() {
                    sink.pause();
                }
            }
        });
    }

    // --- 静态方法部分 (保持不变) ---
    pub fn check_availability(app_handle: &tauri::AppHandle) -> bool {
        if Command::new("ffmpeg").arg("-version").output().is_ok() { return true; }
        if let Ok(local_bin) = Self::get_local_bin_path(app_handle) {
            let ffmpeg_exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
            let local_path = local_bin.join(ffmpeg_exe);
            if local_path.exists() {
                if let Ok(current_path) = env::var("PATH") {
                    if !current_path.contains(&local_bin.to_string_lossy().to_string()) {
                         let new_path = format!("{};{}", local_bin.to_string_lossy(), current_path);
                         unsafe { env::set_var("PATH", new_path); }
                    }
                }
                return Command::new("ffmpeg").arg("-version").output().is_ok();
            }
        }
        false
    }

    fn get_local_bin_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
        let app_data_dir = app_handle.path().app_local_data_dir().map_err(|e| e.to_string())?;
        let bin_dir = app_data_dir.join("bin");
        if !bin_dir.exists() { fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?; }
        Ok(bin_dir)
    }

    pub async fn download_and_install(window: Window) -> Result<(), String> {
        let app_handle = window.app_handle();
        let bin_dir = Self::get_local_bin_path(app_handle)?;
        #[cfg(windows)]
        let url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
        #[cfg(not(windows))]
        let url = ""; 
        if url.is_empty() { return Err("Auto-download currently only supports Windows.".to_string()); }

        window.emit("ffmpeg-status", "downloading").unwrap();
        let client = reqwest::Client::new();
        let mut response = client.get(url).send().await.map_err(|e| e.to_string())?;
        let total_size = response.content_length().unwrap_or(0);
        let mut downloaded: u64 = 0;
        let mut chunks = Vec::new();

        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            downloaded += chunk.len() as u64;
            chunks.extend_from_slice(&chunk);
            if total_size > 0 { let _ = window.emit("ffmpeg-progress", (downloaded as f64 / total_size as f64) * 90.0); }
        }
        let _ = window.emit("ffmpeg-status", "extracting"); 
        let _ = window.emit("ffmpeg-progress", 95);
        let cursor = Cursor::new(chunks);
        let mut archive = ZipArchive::new(cursor).map_err(|e| e.to_string())?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let name = file.name().to_string();
            if (name.ends_with("ffmpeg.exe") || name.ends_with("ffmpeg")) && !name.contains("ffplay") && !name.contains("ffprobe") {
                let target_path = bin_dir.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
                if let Some(parent) = target_path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
                let mut outfile = fs::File::create(&target_path).map_err(|e| e.to_string())?;
                std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
                break; 
            }
        }
        let _ = window.emit("ffmpeg-progress", 100);
        if Self::check_availability(app_handle) { let _ = window.emit("ffmpeg-status", "ready"); Ok(()) } 
        else { let _ = window.emit("ffmpeg-status", "error"); Err("Verification failed".to_string()) }
    }
}

impl AudioEngine for FFmpegEngine {
    fn name(&self) -> &str { "FFmpeg Pipe" }

    fn load(&mut self, path: &str) -> Result<f64, String> {
        self.fade_token.fetch_add(1, Ordering::SeqCst);
        // Load 被视为开始播放
        self.is_playing.store(true, Ordering::SeqCst);

        {
            if let Ok(sink) = self.sink.lock() { sink.stop(); }
        }

        let child = Command::new("ffmpeg")
            .args(&[
                "-i", path, "-f", "f32le", "-ac", "2", "-ar", "44100", 
                "-vn", "-sn", "-map_metadata", "-1", "-v", "quiet", "pipe:1"
            ])
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

        let mut stdout = child.stdout.ok_or("Failed to open ffmpeg stdout")?;
        let mut raw_bytes = Vec::new();
        stdout.read_to_end(&mut raw_bytes).map_err(|e| e.to_string())?;

        if raw_bytes.is_empty() { return Err("FFmpeg output is empty.".to_string()); }

        let sample_count = raw_bytes.len() / 4;
        let mut samples = Vec::with_capacity(sample_count);
        for chunk in raw_bytes.chunks_exact(4) {
            let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            samples.push(val);
        }

        let samples_arc = Arc::new(samples);
        self.current_samples = Some(samples_arc.clone());
        self.sample_rate = 44100;
        if let Ok(mut pos) = self.last_seek_pos.write() { *pos = 0.0; }

        let buffer = SamplesBuffer::new(2, 44100, samples_arc.to_vec());
        let duration = buffer.total_duration().unwrap_or(Duration::from_secs(0)).as_secs_f64();

        let mut sink = self.sink.lock().unwrap();
        if !sink.empty() { sink.stop(); }
        *sink = Sink::try_new(&self.stream_handle).map_err(|e| e.to_string())?;
        
        sink.append(buffer);
        drop(sink); 
        self.run_fade_in(150);

        Ok(duration)
    }

    fn play(&mut self) {
        // 更新逻辑状态
        self.is_playing.store(true, Ordering::SeqCst);
        self.run_fade_in(500);
    }

    fn pause(&mut self) {
        // 更新逻辑状态
        self.is_playing.store(false, Ordering::SeqCst);
        self.run_fade_out_pause(500);
    }

    fn seek(&mut self, time: f64) {
        // 更新位置记录
        if let Ok(mut pos) = self.last_seek_pos.write() { *pos = time; }
        
        // 增加 token，这一步会立即“杀死”所有正在运行的淡入淡出线程
        self.fade_token.fetch_add(1, Ordering::SeqCst);

        // 获取当前的逻辑意图（而不是 Sink 此时此刻的状态）
        let should_be_playing = self.is_playing.load(Ordering::SeqCst);

        if let Some(samples_arc) = &self.current_samples {
             if let Ok(sink) = self.sink.lock() {
                 sink.stop(); 
                 
                 let play_samples = samples_arc.to_vec();
                 let source = SamplesBuffer::new(2, self.sample_rate, play_samples);
                 let new_source = source.skip_duration(Duration::from_secs_f64(time));
                 sink.append(new_source);
                 
                 if should_be_playing {
                     // 如果逻辑上是播放，则淡入
                     drop(sink); 
                     self.run_fade_in(50); 
                 } else {
                     // 如果逻辑上是暂停，必须强制暂停
                     // 即使之前正在淡出（sink.is_paused() 可能为 false），这里也强制设为 Pause
                     sink.pause();
                     
                     // 恢复目标音量，以便下次 Play 时直接从正确音量开始（或由 play 负责淡入）
                     // 但为了安全，不淡入，保持静止
                     let target_vol = *self.target_volume.read().unwrap();
                     sink.set_volume(target_vol);
                 }
             }
        }
    }

    fn set_volume(&mut self, vol: f32) {
        if let Ok(mut v) = self.target_volume.write() { *v = vol; }
        let should_be_playing = self.is_playing.load(Ordering::SeqCst);
        
        if let Ok(sink) = self.sink.lock() { 
            // 只有逻辑上处于播放状态时，才直接应用音量
            // 如果处于暂停（即使正在淡出），也不应该突变音量
            if should_be_playing {
                sink.set_volume(vol); 
            }
        }
    }

    fn update_output_stream(&mut self, handle: OutputStreamHandle) {
        // 增加 token，停止所有对旧 Sink 的淡入淡出操作
        self.fade_token.fetch_add(1, Ordering::SeqCst);
        self.stream_handle = handle.clone();
        
        if let Ok(new_sink) = Sink::try_new(&handle) {
            let mut guard = self.sink.lock().unwrap();
            
            // 获取逻辑状态和位置
            let should_be_playing = self.is_playing.load(Ordering::SeqCst);
            let seek_pos = *self.last_seek_pos.read().unwrap();
            let target_vol = *self.target_volume.read().unwrap();
            
            *guard = new_sink; 
            
            // 必须重新填充数据！
            if let Some(samples_arc) = &self.current_samples {
                let play_samples = samples_arc.to_vec();
                let source = SamplesBuffer::new(2, self.sample_rate, play_samples);
                let new_source = source.skip_duration(Duration::from_secs_f64(seek_pos));
                guard.append(new_source);
            }

            // 根据逻辑状态恢复物理状态
            if should_be_playing {
                guard.set_volume(target_vol);
                guard.play();
            } else {
                guard.set_volume(target_vol); // 准备好音量
                guard.pause(); // 保持暂停
            }
        }
    }
}