// src/audio/ffmpeg.rs

use super::AudioEngine;
use std::process::{Command, Stdio};
use std::path::PathBuf;
use std::fs;
use tokio::time::timeout;
use std::env;
use std::io::{Cursor, Read, BufReader, BufRead}; 
use std::sync::{Arc, Mutex, RwLock, OnceLock};
use std::sync::atomic::{AtomicUsize, AtomicBool, AtomicU32, AtomicU64, Ordering}; 
use std::thread;
use std::time::{Duration, Instant};
use tauri::{Window, Emitter, Manager}; 
use zip::ZipArchive;
use rodio::{OutputStreamHandle, Sink, Source, buffer::SamplesBuffer};
use rodio::cpal::traits::{HostTrait, DeviceTrait};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use super::galaxy::{UpmixSource, ChannelConfig};

// =================================================================
// ⏱️ 全局高精度原子时钟基准 (Lock-Free Epoch)
// =================================================================
static TIME_EPOCH: OnceLock<Instant> = OnceLock::new();
#[inline(always)]
fn get_time_epoch() -> Instant {
    *TIME_EPOCH.get_or_init(Instant::now)
}
#[inline(always)]
fn f64_to_bits(f: f64) -> u64 { f.to_bits() }
#[inline(always)]
fn f64_from_bits(b: u64) -> f64 { f64::from_bits(b) }

fn get_dynamic_target_sr() -> u32 {
    if let Some(device) = rodio::cpal::default_host().default_output_device() {
        if let Ok(config) = device.default_output_config() {
            return config.sample_rate().0;
        }
    }
    48000
}

pub struct FFmpegEngine {
    sink: Arc<Mutex<Sink>>,
    stream_handle: OutputStreamHandle,
    current_samples: Option<Arc<Vec<f32>>>, 
    sample_rate: u32,
    current_volume: Arc<AtomicU32>, 
    playback_pos: Arc<AtomicU64>,
    last_play_us: Arc<AtomicU64>,
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
            sample_rate: 48000, 
            current_volume: Arc::new(AtomicU32::new(1f32.to_bits())), 
            playback_pos: Arc::new(AtomicU64::new(f64_to_bits(0.0))),
            last_play_us: Arc::new(AtomicU64::new(u64::MAX)),
            is_playing: Arc::new(AtomicBool::new(false)),
            channel_mode: Arc::new(RwLock::new(ChannelConfig::Stereo)),
            fade_token: Arc::new(AtomicUsize::new(0)),
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
            #[cfg(target_os = "windows")]
            { cmd.creation_flags(0x08000000); }
            if let Ok(output) = cmd.output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                return stdout.contains("soxr");
            }
        }
        false
    }

    pub async fn download_and_install(window: Window) -> Result<(), String> {
        let bin_dir = Self::get_ffmpeg_dir();
        if !bin_dir.exists() { fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?; }
        #[cfg(windows)]
        let url = "https://ghproxy.net/https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";
        let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build().map_err(|e| e.to_string())?;
        window.emit("ffmpeg-status", "downloading").unwrap();
        let mut response = client.get(url).send().await.map_err(|e| e.to_string())?;
        let total_size = response.content_length().unwrap_or(0);
        let mut downloaded: u64 = 0;
        let mut chunks = Vec::new();
        loop {
            match timeout(Duration::from_secs(15), response.chunk()).await {
                Ok(Ok(Some(chunk))) => {
                    downloaded += chunk.len() as u64; chunks.extend_from_slice(&chunk);
                    if total_size > 0 { let _ = window.emit("ffmpeg-progress", (downloaded as f64 / total_size as f64) * 90.0); }
                },
                Ok(Ok(None)) => break,
                _ => return Err("Download Failed".into()),
            }
        }
        window.emit("ffmpeg-status", "extracting");
        let mut archive = ZipArchive::new(Cursor::new(chunks)).map_err(|e| e.to_string())?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            if file.name().ends_with("ffmpeg.exe") {
                let target_path = Self::get_ffmpeg_exe();
                if let Some(p) = target_path.parent() { fs::create_dir_all(p).ok(); }
                let mut out = fs::File::create(&target_path).map_err(|e| e.to_string())?;
                std::io::copy(&mut file, &mut out).ok();
                break;
            }
        }
        window.emit("ffmpeg-status", "ready");
        Ok(())
    }
}

impl AudioEngine for FFmpegEngine {
    fn name(&self) -> &str { "FFmpeg soxr-VHQ (Mastering Grade)" }

    fn get_current_time(&self) -> f64 {
        let pos = f64_from_bits(self.playback_pos.load(Ordering::Relaxed));
        let start_us = self.last_play_us.load(Ordering::Relaxed);
        if start_us != u64::MAX {
            let epoch = get_time_epoch();
            let now_us = Instant::now().duration_since(epoch).as_micros() as u64;
            let elapsed = now_us.saturating_sub(start_us) as f64 / 1_000_000.0;
            pos + elapsed
        } else {
            pos
        }
    }

    fn update_output_stream(&mut self, handle: OutputStreamHandle) {
        let was_playing = self.is_playing.load(Ordering::SeqCst);
        let current_time = (self.get_current_time() - 0.4).max(0.0);

        if was_playing { 
            self.is_playing.store(false, Ordering::SeqCst); 
            if let Ok(s) = self.sink.lock() { s.pause(); }
            thread::sleep(Duration::from_millis(50)); 
        }
        
        self.stream_handle = handle.clone();
        self.seek(current_time);
        
        if was_playing { 
            self.play(); 
        }
    }

    fn load(&mut self, path: &str) -> Result<f64, String> {
        if self.is_playing.load(Ordering::SeqCst) { self.is_playing.store(false, Ordering::SeqCst); thread::sleep(Duration::from_millis(40)); }

        let ffmpeg_exe = Self::get_ffmpeg_exe();
        let target_sr = get_dynamic_target_sr();
        
        println!("\x1b[36m[FFMPEG] Audio Engine Decoder Initialized: Target SR = {}Hz, Channels = 2\x1b[0m", target_sr);
        
        let mut cmd = Command::new(&ffmpeg_exe);
        cmd.args(&[
            "-i", path, "-f", "f32le", "-ac", "2", "-ar", &target_sr.to_string(), 
            "-af", "aresample=resampler=soxr:precision=28:cheby=1:dither_method=triangular,alimiter=limit=0.99:attack=1:release=20:asc=0",
            "-vn", "-sn", "-map_metadata", "-1", "-v", "error", "pipe:1"
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        { cmd.creation_flags(0x08000000); }

        let mut child = cmd.spawn().map_err(|e| format!("Spawn failed: {}", e))?;
        let mut stdout = child.stdout.take().ok_or("Stdout failed")?;
        let stderr = child.stderr.take().ok_or("Stderr failed")?;

        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(l) = line { eprintln!("\x1b[33m[FFMPEG LOG] {}\x1b[0m", l); }
            }
        });

        let mut raw_bytes = Vec::new();
        stdout.read_to_end(&mut raw_bytes).map_err(|e| e.to_string())?;

        if raw_bytes.is_empty() { return Err("FFmpeg output is empty. Check logs.".into()); }

        let sample_count = raw_bytes.len() / 4;
        let mut samples = Vec::with_capacity(sample_count);
        for chunk in raw_bytes.chunks_exact(4) {
            samples.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }

        let samples_arc = Arc::new(samples);
        self.current_samples = Some(samples_arc.clone());
        self.sample_rate = target_sr;
        
        self.playback_pos.store(f64_to_bits(0.0), Ordering::SeqCst);
        let epoch = get_time_epoch();
        if self.is_playing.load(Ordering::SeqCst) {
            let now_us = Instant::now().duration_since(epoch).as_micros() as u64;
            self.last_play_us.store(now_us, Ordering::SeqCst);
        } else {
            self.last_play_us.store(u64::MAX, Ordering::SeqCst);
        }
        
        self.fade_token.fetch_add(1, Ordering::SeqCst);

        let target_channels = *self.channel_mode.read().unwrap() as u16;
        let buffer = SamplesBuffer::new(2, target_sr, samples_arc.to_vec());
        let duration = buffer.total_duration().unwrap_or(Duration::from_secs(0)).as_secs_f64();

        let mut sink_guard = self.sink.lock().unwrap();
        *sink_guard = Sink::try_new(&self.stream_handle).unwrap();
        sink_guard.set_volume(1.0);
        sink_guard.append(UpmixSource::new(buffer, target_channels, self.is_playing.clone(), self.current_volume.clone()));
        sink_guard.play();

        Ok(duration)
    }

    fn play(&mut self) {
        if self.is_playing.swap(true, Ordering::SeqCst) { return; }
        
        let epoch = get_time_epoch();
        let now_us = Instant::now().duration_since(epoch).as_micros() as u64;
        self.last_play_us.store(now_us, Ordering::SeqCst);
        
        if let Ok(s) = self.sink.lock() { s.play(); } 
    }

    fn pause(&mut self) {
        if !self.is_playing.swap(false, Ordering::SeqCst) { return; }
        
        let start_us = self.last_play_us.swap(u64::MAX, Ordering::SeqCst);
        if start_us != u64::MAX {
            let epoch = get_time_epoch();
            let now_us = Instant::now().duration_since(epoch).as_micros() as u64;
            let elapsed = now_us.saturating_sub(start_us) as f64 / 1_000_000.0;
            
            let mut current = self.playback_pos.load(Ordering::Relaxed);
            loop {
                let new_val = f64_from_bits(current) + elapsed;
                match self.playback_pos.compare_exchange_weak(current, f64_to_bits(new_val), Ordering::SeqCst, Ordering::Relaxed) {
                    Ok(_) => break,
                    Err(x) => current = x,
                }
            }
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
        if is_playing_now { self.is_playing.store(false, Ordering::SeqCst); thread::sleep(Duration::from_millis(40)); }
        
        self.playback_pos.store(f64_to_bits(time), Ordering::SeqCst);
        let epoch = get_time_epoch();
        if is_playing_now {
            let now_us = Instant::now().duration_since(epoch).as_micros() as u64;
            self.last_play_us.store(now_us, Ordering::SeqCst);
        } else {
            self.last_play_us.store(u64::MAX, Ordering::SeqCst);
        }
        
        {
            let mut sink_guard = self.sink.lock().unwrap();
            *sink_guard = Sink::try_new(&self.stream_handle).unwrap();
        }
        let target_channels = *self.channel_mode.read().unwrap() as u16;
        if let Some(samples_arc) = &self.current_samples {
             let source = SamplesBuffer::new(2, self.sample_rate, samples_arc.to_vec()).skip_duration(Duration::from_secs_f64(time));
             let sink_guard = self.sink.lock().unwrap();
             sink_guard.set_volume(1.0);
             sink_guard.append(UpmixSource::new(source, target_channels, self.is_playing.clone(), self.current_volume.clone()));
        }
        if is_playing_now { self.is_playing.store(true, Ordering::SeqCst); self.sink.lock().unwrap().play(); }
    }

    fn set_volume(&mut self, vol: f32) { self.current_volume.store(vol.to_bits(), Ordering::SeqCst); }

    fn set_channel_mode(&mut self, _mode: u16) {
        let config = match _mode { 6 => ChannelConfig::Surround51, 8 => ChannelConfig::Surround71, 106 => ChannelConfig::True51, 108 => ChannelConfig::True71, _ => ChannelConfig::Stereo };
        *self.channel_mode.write().unwrap() = config;
    }
}