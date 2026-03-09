// src/audio/ffmpeg.rs

use super::AudioEngine;
use std::process::{Command, Stdio};
use std::path::{Path, PathBuf};
use std::fs;
use tokio::time::timeout;
use std::env;
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering}; 
use std::thread;
use std::time::Duration;
use tauri::{Window, Emitter, Manager}; 
use zip::ZipArchive;
use rodio::{OutputStreamHandle, Sink, Source, buffer::SamplesBuffer};

// 🔥 引入 Windows 专属的进程扩展，用于隐藏控制台窗口
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// 🔥 复用 Galaxy 的超级安全上混模块与配置
use super::galaxy::{UpmixSource, ChannelConfig};

pub struct FFmpegEngine {
    sink: Arc<Mutex<Sink>>,
    stream_handle: OutputStreamHandle,
    current_samples: Option<Arc<Vec<f32>>>, 
    sample_rate: u32,
    target_volume: Arc<RwLock<f32>>, 
    fade_token: Arc<AtomicUsize>,
    last_seek_pos: Arc<RwLock<f64>>,
    is_playing: Arc<AtomicBool>,
    channel_mode: Arc<RwLock<ChannelConfig>>,
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
            channel_mode: Arc::new(RwLock::new(ChannelConfig::Stereo)),
        } 
    }

    fn get_ffmpeg_dir() -> PathBuf {
        let mut p = env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        p.pop(); 
        p.join("engine").join("ffmpeg")
    }

    fn get_ffmpeg_exe() -> PathBuf {
        let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        Self::get_ffmpeg_dir().join(exe_name)
    }

    pub fn check_availability(_app_handle: &tauri::AppHandle) -> bool {
        let exe_path = Self::get_ffmpeg_exe();
        if exe_path.exists() {
            let mut cmd = Command::new(&exe_path);
            cmd.arg("-version");
            
            // 🔥 魔法注入：隐藏检查版本时的黑框
            #[cfg(target_os = "windows")]
            {
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }

            if cmd.output().is_ok() { return true; }
        }
        false
    }

    pub async fn download_and_install(window: Window) -> Result<(), String> {
        let bin_dir = Self::get_ffmpeg_dir();
        if !bin_dir.exists() { fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?; }

        #[cfg(windows)]
        let url = "https://ghproxy.net/https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";
        #[cfg(not(windows))]
        let url = ""; 
        
        if url.is_empty() { return Err("Auto-download currently only supports Windows.".to_string()); }

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10)) 
            .build()
            .map_err(|e| format!("构建下载客户端失败: {}", e))?;

        match client.head(url).send().await {
            Ok(resp) if resp.status().is_success() => {},
            Ok(resp) => {
                let _ = window.emit("ffmpeg-status", "error");
                return Err(format!("下载源不可达: {}", resp.status()));
            },
            Err(e) => {
                let _ = window.emit("ffmpeg-status", "error");
                return Err(format!("网络无法访问: {}", e));
            }
        }

        window.emit("ffmpeg-status", "downloading").unwrap();
        let mut response = client.get(url).send().await.map_err(|e| format!("建立下载流失败: {}", e))?;
        
        let total_size = response.content_length().unwrap_or(0);
        let mut downloaded: u64 = 0;
        let mut chunks = Vec::new();
        let chunk_timeout = Duration::from_secs(15); 

        loop {
            match timeout(chunk_timeout, response.chunk()).await {
                Ok(Ok(Some(chunk))) => {
                    downloaded += chunk.len() as u64;
                    chunks.extend_from_slice(&chunk);
                    if total_size > 0 { 
                        let percent = (downloaded as f64 / total_size as f64) * 90.0;
                        let _ = window.emit("ffmpeg-progress", percent); 
                    }
                },
                Ok(Ok(None)) => {
                    break;
                },
                Ok(Err(e)) => {
                    chunks.clear(); 
                    let _ = window.emit("ffmpeg-status", "cooling"); 
                    return Err(format!("传输异常中断: {}", e));
                },
                Err(_) => {
                    chunks.clear(); 
                    let _ = window.emit("ffmpeg-status", "cooling"); 
                    return Err("数据流卡死，已触发引擎冷却保护机制".to_string());
                }
            }
        }
        
        let _ = window.emit("ffmpeg-status", "extracting"); 
        let _ = window.emit("ffmpeg-progress", 95.0);
        
        let cursor = Cursor::new(chunks);
        let mut archive = ZipArchive::new(cursor).map_err(|e| format!("解压失败: {}", e))?;
        
        let mut extracted = false;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let name = file.name().to_string();
            if (name.ends_with("ffmpeg.exe") || name.ends_with("ffmpeg")) && !name.contains("ffplay") && !name.contains("ffprobe") {
                let target_path = Self::get_ffmpeg_exe();
                if let Some(parent) = target_path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
                let mut outfile = fs::File::create(&target_path).map_err(|e| e.to_string())?;
                std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
                extracted = true;
                break; 
            }
        }
        
        let _ = window.emit("ffmpeg-progress", 100.0);
        
        if extracted && Self::check_availability(&window.app_handle()) { 
            let _ = window.emit("ffmpeg-status", "ready"); 
            Ok(()) 
        } else { 
            let _ = window.emit("ffmpeg-status", "error"); 
            Err("FFmpeg 核心校验失败，请重试".to_string()) 
        }
    }

    fn run_fade_in(&self, duration_ms: u64) {
        let sink_clone = self.sink.clone();
        let target_vol_lock = self.target_volume.clone();
        let my_token = self.fade_token.fetch_add(1, Ordering::SeqCst) + 1;
        let token_ref = self.fade_token.clone();

        if let Ok(sink) = sink_clone.lock() {
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
                if let Ok(sink) = sink_clone.lock() { sink.set_volume(current_vol); }
                thread::sleep(step_duration);
            }
            if token_ref.load(Ordering::SeqCst) == my_token {
                if let Ok(sink) = sink_clone.lock() { sink.set_volume(target_vol); }
            }
        });
    }

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
                if let Ok(sink) = sink_clone.lock() { sink.set_volume(current_vol); }
                thread::sleep(step_duration);
            }
            if token_ref.load(Ordering::SeqCst) == my_token {
                if let Ok(sink) = sink_clone.lock() { sink.pause(); }
            }
        });
    }
}

impl AudioEngine for FFmpegEngine {
    fn name(&self) -> &str { "FFmpeg Pipe (with DSP)" }

    fn load(&mut self, path: &str) -> Result<f64, String> {
        self.fade_token.fetch_add(1, Ordering::SeqCst);
        self.is_playing.store(true, Ordering::SeqCst);
        if let Ok(sink) = self.sink.lock() { sink.stop(); }

        let ffmpeg_exe = Self::get_ffmpeg_exe();
        
        let mut cmd = Command::new(&ffmpeg_exe);
        cmd.args(&[
            "-i", path, "-f", "f32le", "-ac", "2", "-ar", "44100", 
            "-vn", "-sn", "-map_metadata", "-1", "-v", "quiet", "pipe:1"
        ])
        .stdout(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let child = cmd.spawn().map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

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

        let target_channels = *self.channel_mode.read().unwrap() as u16;
        let buffer = SamplesBuffer::new(2, 44100, samples_arc.to_vec());
        let duration = buffer.total_duration().unwrap_or(Duration::from_secs(0)).as_secs_f64();

        let mut sink = self.sink.lock().unwrap();
        if !sink.empty() { sink.stop(); }
        *sink = Sink::try_new(&self.stream_handle).map_err(|e| e.to_string())?;
        
        let mixed = UpmixSource::new(buffer, target_channels);
        sink.append(mixed);
        
        drop(sink); 
        self.run_fade_in(150);

        Ok(duration)
    }

    fn play(&mut self) {
        self.is_playing.store(true, Ordering::SeqCst);
        self.run_fade_in(500);
    }

    fn pause(&mut self) {
        self.is_playing.store(false, Ordering::SeqCst);
        self.run_fade_out_pause(500);
    }

    fn seek(&mut self, time: f64) {
        if let Ok(mut pos) = self.last_seek_pos.write() { *pos = time; }
        self.fade_token.fetch_add(1, Ordering::SeqCst);
        let should_be_playing = self.is_playing.load(Ordering::SeqCst);

        if let Some(samples_arc) = &self.current_samples {
             if let Ok(sink) = self.sink.lock() {
                 sink.stop(); 
                 
                 let target_channels = *self.channel_mode.read().unwrap() as u16;
                 let play_samples = samples_arc.to_vec();
                 let source = SamplesBuffer::new(2, self.sample_rate, play_samples);
                 let new_source = source.skip_duration(Duration::from_secs_f64(time));
                 
                 let mixed = UpmixSource::new(new_source, target_channels);
                 sink.append(mixed);
                 
                 if should_be_playing { drop(sink); self.run_fade_in(50); } 
                 else {
                     sink.pause();
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
            if should_be_playing { sink.set_volume(vol); }
        }
    }

    // 🔥 解析前端传参：兼容真实的物理 106 / 108 模式
    fn set_channel_mode(&mut self, _mode: u16) {
        let config = match _mode {
            6 => ChannelConfig::Surround51, 
            8 => ChannelConfig::Surround71, 
            106 => ChannelConfig::True51,
            108 => ChannelConfig::True71,
            _ => ChannelConfig::Stereo,
        };
        *self.channel_mode.write().unwrap() = config;
    }

    fn update_output_stream(&mut self, handle: OutputStreamHandle) {
        self.fade_token.fetch_add(1, Ordering::SeqCst);
        self.stream_handle = handle.clone();
        
        if let Ok(new_sink) = Sink::try_new(&handle) {
            let mut guard = self.sink.lock().unwrap();
            let should_be_playing = self.is_playing.load(Ordering::SeqCst);
            let seek_pos = *self.last_seek_pos.read().unwrap();
            let target_vol = *self.target_volume.read().unwrap();
            let target_channels = *self.channel_mode.read().unwrap() as u16;
            
            *guard = new_sink; 
            
            if let Some(samples_arc) = &self.current_samples {
                let play_samples = samples_arc.to_vec();
                let source = SamplesBuffer::new(2, self.sample_rate, play_samples);
                let new_source = source.skip_duration(Duration::from_secs_f64(seek_pos));
                let mixed = UpmixSource::new(new_source, target_channels);
                guard.append(mixed);
            }

            if should_be_playing {
                guard.set_volume(target_vol);
                guard.play();
            } else {
                guard.set_volume(target_vol); 
                guard.pause(); 
            }
        }
    }
}