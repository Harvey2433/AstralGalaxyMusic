// src/audio/ffmpeg.rs

use super::AudioEngine;
use std::process::{Command, Stdio};
use std::path::PathBuf;
use std::fs;
use tokio::time::timeout;
use std::env;
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicUsize, AtomicBool, AtomicU32, Ordering}; 
use std::thread;
use std::time::{Duration, Instant};
use tauri::{Window, Emitter, Manager}; 
use zip::ZipArchive;
use rodio::{OutputStreamHandle, Sink, Source, buffer::SamplesBuffer};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use super::galaxy::{UpmixSource, ChannelConfig};

pub struct FFmpegEngine {
    sink: Arc<Mutex<Sink>>,
    stream_handle: OutputStreamHandle,
    current_samples: Option<Arc<Vec<f32>>>, 
    sample_rate: u32,
    current_volume: Arc<AtomicU32>, 
    
    playback_pos: Arc<RwLock<f64>>,
    last_play_instant: Arc<RwLock<Option<Instant>>>,
    
    is_playing: Arc<AtomicBool>,
    channel_mode: Arc<RwLock<ChannelConfig>>,
    fade_token: Arc<AtomicUsize>,
}

impl FFmpegEngine {
    pub fn new(stream_handle: OutputStreamHandle) -> Self { 
        let sink = Sink::try_new(&stream_handle).expect("Failed to create FFmpeg Sink");
        Self { 
            sink: Arc::new(Mutex::new(sink)),
            stream_handle,
            current_samples: None,
            sample_rate: 44100, 
            current_volume: Arc::new(AtomicU32::new(1f32.to_bits())), 
            playback_pos: Arc::new(RwLock::new(0.0)),
            last_play_instant: Arc::new(RwLock::new(None)),
            is_playing: Arc::new(AtomicBool::new(false)),
            channel_mode: Arc::new(RwLock::new(ChannelConfig::Stereo)),
            fade_token: Arc::new(AtomicUsize::new(0)),
        } 
    }

    pub fn get_current_time(&self) -> f64 {
        let mut pos = *self.playback_pos.read().unwrap();
        if let Some(inst) = *self.last_play_instant.read().unwrap() {
            pos += inst.elapsed().as_secs_f64();
        }
        pos
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
}

impl AudioEngine for FFmpegEngine {
    fn name(&self) -> &str { "FFmpeg Pipe (High-Compat Bit-Perfect)" }

    fn update_output_stream(&mut self, handle: OutputStreamHandle) {
        if self.is_playing.load(Ordering::SeqCst) {
            self.is_playing.store(false, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(40)); 
        }
        
        let current_time = self.get_current_time();
        self.stream_handle = handle.clone();
        self.seek(current_time);
    }

    fn load(&mut self, path: &str) -> Result<f64, String> {
        if self.is_playing.load(Ordering::SeqCst) {
            self.is_playing.store(false, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(40)); 
        }

        let ffmpeg_exe = Self::get_ffmpeg_exe();
        
        let mut cmd = Command::new(&ffmpeg_exe);
        cmd.args(&[
            "-i", path, "-f", "f32le", "-ac", "2", 
            "-ar", "44100", 
            "-af", "aresample=resampler=swr:filter_type=cubic:precision=28,alimiter=limit=0.95:attack=5:release=50",
            "-vn", "-sn", "-map_metadata", "-1", "-v", "error", "pipe:1"
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

        let mut stdout = child.stdout.take().ok_or("Failed to open ffmpeg stdout")?;
        let mut stderr = child.stderr.take().ok_or("Failed to open ffmpeg stderr")?;
        
        let mut raw_bytes = Vec::new();
        stdout.read_to_end(&mut raw_bytes).map_err(|e| e.to_string())?;

        if raw_bytes.is_empty() { 
            let mut err_msg = String::new();
            stderr.read_to_string(&mut err_msg).ok();
            return Err(format!("FFmpeg Pipe Failed: {}", err_msg)); 
        }

        let sample_count = raw_bytes.len() / 4;
        let mut samples = Vec::with_capacity(sample_count);
        for chunk in raw_bytes.chunks_exact(4) {
            let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            samples.push(val);
        }

        let samples_arc = Arc::new(samples);
        self.current_samples = Some(samples_arc.clone());
        self.sample_rate = 44100; 
        
        *self.playback_pos.write().unwrap() = 0.0;
        *self.last_play_instant.write().unwrap() = if self.is_playing.load(Ordering::SeqCst) { Some(Instant::now()) } else { None };

        self.fade_token.fetch_add(1, Ordering::SeqCst);

        let target_channels = *self.channel_mode.read().unwrap() as u16;
        let buffer = SamplesBuffer::new(2, 44100, samples_arc.to_vec());
        let duration = buffer.total_duration().unwrap_or(Duration::from_secs(0)).as_secs_f64();

        let mut sink_guard = self.sink.lock().unwrap();
        *sink_guard = Sink::try_new(&self.stream_handle).map_err(|e| e.to_string())?;
        
        sink_guard.set_volume(1.0);
        let mixed = UpmixSource::new(buffer, target_channels, self.is_playing.clone(), self.current_volume.clone());
        sink_guard.append(mixed);
        
        sink_guard.play();

        Ok(duration)
    }

    fn play(&mut self) {
        if self.is_playing.swap(true, Ordering::SeqCst) { return; }
        
        *self.last_play_instant.write().unwrap() = Some(Instant::now());
        self.fade_token.fetch_add(1, Ordering::SeqCst); 
        
        if let Ok(s) = self.sink.lock() { s.play(); } 
    }

    fn pause(&mut self) {
        if !self.is_playing.swap(false, Ordering::SeqCst) { return; }
        
        let mut pos = self.playback_pos.write().unwrap();
        if let Some(i) = self.last_play_instant.write().unwrap().take() {
            *pos += i.elapsed().as_secs_f64();
        }

        let my_token = self.fade_token.fetch_add(1, Ordering::SeqCst) + 1;
        let token_ref = self.fade_token.clone();
        let sink_clone = self.sink.clone();
        let is_playing_flag = self.is_playing.clone();
        
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(1000)); 
            if token_ref.load(Ordering::SeqCst) == my_token && !is_playing_flag.load(Ordering::SeqCst) {
                if let Ok(s) = sink_clone.lock() { s.pause(); } 
            }
        });
    }

    fn seek(&mut self, time: f64) {
        let is_playing_now = self.is_playing.load(Ordering::SeqCst);

        // 🔥 死前平滑机制：阻断断头台效应
        if is_playing_now {
            self.is_playing.store(false, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(40)); 
        }

        *self.playback_pos.write().unwrap() = time;
        *self.last_play_instant.write().unwrap() = if is_playing_now { Some(Instant::now()) } else { None };

        {
            let mut sink_guard = self.sink.lock().unwrap();
            *sink_guard = Sink::try_new(&self.stream_handle).unwrap();
            sink_guard.pause(); 
        }

        let target_channels = *self.channel_mode.read().unwrap() as u16;
        let sink_guard = self.sink.lock().unwrap();

        if let Some(samples_arc) = &self.current_samples {
             let play_samples = samples_arc.to_vec();
             let source = SamplesBuffer::new(2, self.sample_rate, play_samples);
             let new_source = source.skip_duration(Duration::from_secs_f64(time));
             
             sink_guard.set_volume(1.0);
             let mixed = UpmixSource::new(new_source, target_channels, self.is_playing.clone(), self.current_volume.clone());
             sink_guard.append(mixed);
        }

        if is_playing_now {
            self.is_playing.store(true, Ordering::SeqCst);
            sink_guard.play(); 
        }
    }

    fn set_volume(&mut self, vol: f32) {
        self.current_volume.store(vol.to_bits(), Ordering::SeqCst);
    }

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
}