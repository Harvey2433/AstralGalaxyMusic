// src/audio/galaxy.rs

use super::AudioEngine;
use rodio::{Decoder, OutputStreamHandle, Sink, Source};
use std::fs::File;
use std::io::{Cursor, Read};
use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::thread;
use std::mem;

// --- 日志宏 ---
macro_rules! debug_log {
    ($($arg:tt)*) => ({
        let thread_id = format!("{:?}", thread::current().id()).replace("ThreadId(", "").replace(")", "");
        println!("\x1b[36m[GALAXY][T:{}] {}\x1b[0m", thread_id, format!($($arg)*));
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelConfig {
    Stereo = 2,
    Surround51 = 6,
    Surround71 = 8,
}

// =========================================================
// 🚀 自研防崩溃 DSP 核心
// =========================================================
pub struct SpatialProcessor {
    lfe_state: f32,
    delay_buffer: Vec<(f32, f32)>, 
    delay_pos: usize,
    alpha: f32,
}

impl SpatialProcessor {
    pub fn new(sample_rate: u32) -> Self {
        let delay_samples = (sample_rate as f32 * 0.020) as usize;
        let dt = 1.0 / sample_rate as f32;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * 120.0);
        let alpha = dt / (rc + dt);
        
        Self { 
            lfe_state: 0.0, 
            delay_buffer: vec![(0.0, 0.0); delay_samples.max(1)], 
            delay_pos: 0, 
            alpha 
        }
    }

    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32, f32) {
        let mono = (l + r) * 0.5;
        self.lfe_state += self.alpha * (mono - self.lfe_state);
        let lfe_out = self.lfe_state;

        let (delayed_l, delayed_r) = self.delay_buffer[self.delay_pos];
        self.delay_buffer[self.delay_pos] = (l, r);
        self.delay_pos = (self.delay_pos + 1) % self.delay_buffer.len();

        (lfe_out, delayed_l, delayed_r)
    }
}

pub struct UpmixSource<I: Source<Item = f32>> {
    input: I,
    pub target_channels: u16,
    current_frame: Vec<f32>,
    dsp: SpatialProcessor, 
}

impl<I: Source<Item = f32>> UpmixSource<I> {
    pub fn new(input: I, target_channels: u16) -> Self {
        let sample_rate = input.sample_rate();
        Self { input, target_channels, current_frame: Vec::with_capacity(8), dsp: SpatialProcessor::new(sample_rate) }
    }
    #[inline(always)]
    fn clamp(val: f32) -> f32 { val.max(-1.0).min(1.0) }
}

impl<I: Source<Item = f32>> Iterator for UpmixSource<I> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.target_channels == 2 {
            return self.input.next();
        }

        if self.current_frame.is_empty() {
            let l = self.input.next()?;
            let r = if self.input.channels() == 1 { l } else { self.input.next().unwrap_or(l) };
            
            if self.input.channels() > 2 {
                for _ in 2..self.input.channels() { let _ = self.input.next(); }
            }
            
            let (lfe_raw, rear_l_raw, rear_r_raw) = self.dsp.process(l, r);
            
            let center = (l + r) * 0.4; 
            let ambience = (l - r) * 0.3; 
            let surround_l = rear_l_raw * 0.4 + ambience;
            let surround_r = rear_r_raw * 0.4 - ambience; 
            let lfe = lfe_raw * 1.2;

            self.current_frame.push(Self::clamp(l)); 
            self.current_frame.push(Self::clamp(r)); 
            self.current_frame.push(Self::clamp(center)); 
            self.current_frame.push(Self::clamp(lfe)); 
            self.current_frame.push(Self::clamp(surround_l)); 
            self.current_frame.push(Self::clamp(surround_r));
            
            if self.target_channels == 8 {
                self.current_frame.push(Self::clamp(surround_l * 0.8)); 
                self.current_frame.push(Self::clamp(surround_r * 0.8));
            }
            self.current_frame.reverse(); 
        }
        self.current_frame.pop()
    }
}

impl<I: Source<Item = f32>> Source for UpmixSource<I> {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.target_channels }
    fn sample_rate(&self) -> u32 { self.input.sample_rate() }
    fn total_duration(&self) -> Option<Duration> { self.input.total_duration() }
}

// =========================================================
// 🔥 零拷贝游标：复用底层切片数据
// =========================================================
#[derive(Clone)]
pub struct ArcSliceSource {
    data: Arc<Vec<f32>>,
    pos: usize,
    channels: u16,
    sample_rate: u32,
}

impl ArcSliceSource {
    pub fn new(data: Arc<Vec<f32>>, channels: u16, sample_rate: u32) -> Self {
        Self { data, pos: 0, channels, sample_rate }
    }

    pub fn skip_duration(mut self, duration: Duration) -> Self {
        let offset = (duration.as_secs_f64() * self.sample_rate as f64 * self.channels as f64) as usize;
        self.pos = offset.min(self.data.len());
        self.pos -= self.pos % self.channels as usize;
        self
    }
}

impl Iterator for ArcSliceSource {
    type Item = f32;
    #[inline(always)]
    fn next(&mut self) -> Option<f32> {
        if self.pos < self.data.len() {
            let val = self.data[self.pos];
            self.pos += 1;
            Some(val)
        } else {
            None
        }
    }
}

impl Source for ArcSliceSource {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.channels }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> {
        let frames_left = (self.data.len() - self.pos) / self.channels as usize;
        Some(Duration::from_secs_f64(frames_left as f64 / self.sample_rate as f64))
    }
}

// =========================================================
// 🌌 Galaxy Engine (物理静音 + 完美同步阻塞配合前端)
// =========================================================

pub struct GalaxyEngine {
    sink: Arc<Mutex<Sink>>,
    stream_handle: OutputStreamHandle,
    raw_bytes: Option<Arc<Vec<u8>>>,
    decoded_samples: Arc<RwLock<Option<Arc<Vec<f32>>>>>, 
    is_decoded: Arc<AtomicBool>, 
    decode_session: Arc<AtomicUsize>, 
    seek_session: Arc<AtomicUsize>, 
    is_playing: Arc<AtomicBool>, 
    sample_rate: u32,
    channels: u16,
    current_volume: Arc<RwLock<f32>>,
    channel_mode: Arc<RwLock<ChannelConfig>>,
}

impl GalaxyEngine {
    pub fn new(stream_handle: OutputStreamHandle) -> Self {
        let sink = Sink::try_new(&stream_handle).unwrap();
        Self {
            sink: Arc::new(Mutex::new(sink)),
            stream_handle,
            raw_bytes: None,
            decoded_samples: Arc::new(RwLock::new(None)),
            is_decoded: Arc::new(AtomicBool::new(false)),
            decode_session: Arc::new(AtomicUsize::new(0)),
            seek_session: Arc::new(AtomicUsize::new(0)),
            is_playing: Arc::new(AtomicBool::new(false)), 
            sample_rate: 44100,
            channels: 2,
            current_volume: Arc::new(RwLock::new(1.0)),
            channel_mode: Arc::new(RwLock::new(ChannelConfig::Stereo)),
        }
    }

    fn create_decoder(data: &Arc<Vec<u8>>) -> Result<Decoder<Cursor<Vec<u8>>>, String> {
        let cursor = Cursor::new(data.to_vec()); 
        Decoder::new(cursor).map_err(|e| e.to_string())
    }

    fn get_volume(&self) -> f32 { *self.current_volume.read().unwrap() }
}

impl AudioEngine for GalaxyEngine {
    fn name(&self) -> &str { "Galaxy DSP (Sync Blocking Zero-Copy)" }

    fn update_output_stream(&mut self, handle: OutputStreamHandle) {
        self.stream_handle = handle.clone();
        if let Ok(new_sink) = Sink::try_new(&handle) {
            let mut guard = self.sink.lock().unwrap();
            let old_sink = mem::replace(&mut *guard, new_sink);
            thread::spawn(move || drop(old_sink));
        }
    }

    fn load(&mut self, path: &str) -> Result<f64, String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut buffer = Vec::with_capacity(len as usize);
        file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
        let raw_bytes = Arc::new(buffer);

        let source = Self::create_decoder(&raw_bytes)?;
        self.sample_rate = source.sample_rate();
        self.channels = source.channels();
        let total_duration = source.total_duration().map(|d| d.as_secs_f64()).unwrap_or(0.0);

        let my_session = self.decode_session.fetch_add(1, Ordering::SeqCst) + 1;
        self.seek_session.fetch_add(1, Ordering::SeqCst); 
        *self.decoded_samples.write().unwrap() = None;
        self.is_decoded.store(false, Ordering::Release);
        
        {
            let mut sink_guard = self.sink.lock().unwrap();
            if let Ok(new_sink) = Sink::try_new(&self.stream_handle) {
                let old_sink = mem::replace(&mut *sink_guard, new_sink);
                thread::spawn(move || drop(old_sink));
            } else {
                sink_guard.stop();
            }
            
            sink_guard.set_volume(self.get_volume());
            let mixed_source = UpmixSource::new(source.convert_samples::<f32>(), *self.channel_mode.read().unwrap() as u16);
            sink_guard.append(mixed_source);
            
            if self.is_playing.load(Ordering::SeqCst) { sink_guard.play(); } else { sink_guard.pause(); }
        }

        self.raw_bytes = Some(raw_bytes.clone());

        let session_ref = self.decode_session.clone();
        let samples_ref = self.decoded_samples.clone();
        let is_decoded_ref = self.is_decoded.clone();
        let raw_bytes_clone = raw_bytes.clone();

        thread::spawn(move || {
            debug_log!("Background full-decode started...");
            if let Ok(decoder) = Decoder::new(Cursor::new(raw_bytes_clone.to_vec())) {
                let mut pcm_buffer = Vec::with_capacity(44100 * 2 * 180); 
                let mut count = 0;
                
                for sample in decoder.convert_samples::<f32>() {
                    pcm_buffer.push(sample);
                    count += 1;
                    if count % 32768 == 0 {
                        if session_ref.load(Ordering::SeqCst) != my_session { return; }
                    }
                }
                
                if session_ref.load(Ordering::SeqCst) == my_session {
                    *samples_ref.write().unwrap() = Some(Arc::new(pcm_buffer));
                    is_decoded_ref.store(true, Ordering::Release);
                    debug_log!("Background full-decode complete. Ready for True O(1) instant seek.");
                }
            }
        });

        Ok(total_duration)
    }

    fn play(&mut self) { 
        self.is_playing.store(true, Ordering::SeqCst);
        if let Ok(s) = self.sink.lock() { s.play(); } 
    }
    
    fn pause(&mut self) { 
        self.is_playing.store(false, Ordering::SeqCst);
        if let Ok(s) = self.sink.lock() { s.pause(); } 
    }

    fn seek(&mut self, time: f64) {
        let my_seek_session = self.seek_session.fetch_add(1, Ordering::SeqCst) + 1;
        let is_playing_now = self.is_playing.load(Ordering::SeqCst);

        // 1. 物理斩断：前端一旦请求寻址，立马丢弃旧句柄，保持绝对死寂
        {
            let mut sink_guard = self.sink.lock().unwrap();
            if let Ok(new_sink) = Sink::try_new(&self.stream_handle) {
                let old_sink = mem::replace(&mut *sink_guard, new_sink);
                thread::spawn(move || drop(old_sink));
            } else {
                sink_guard.stop();
            }
            sink_guard.pause(); // 强行把新句柄设为静默状态！
        }

        // 2. 同步阻塞：挂起 Tauri 线程，让前端一直处于等待动画中
        let start_wait = Instant::now();
        while !self.is_decoded.load(Ordering::Acquire) {
            if self.seek_session.load(Ordering::SeqCst) != my_seek_session {
                return; // 放弃旧任务
            }
            if start_wait.elapsed().as_secs() > 45 { 
                debug_log!("Decode wait timed out.");
                break;
            }
            thread::sleep(Duration::from_millis(20)); 
        }

        if self.seek_session.load(Ordering::SeqCst) != my_seek_session { return; }

        let target_channels = *self.channel_mode.read().unwrap() as u16;
        
        // 3. 解压完成，装载新音频！
        let sink_guard = self.sink.lock().unwrap();
        if self.is_decoded.load(Ordering::Acquire) {
            if let Some(samples_arc) = self.decoded_samples.read().unwrap().clone() {
                debug_log!("Executing True O(1) zero-copy memory seek...");
                let source = ArcSliceSource::new(samples_arc, self.channels, self.sample_rate)
                    .skip_duration(Duration::from_secs_f64(time));
                sink_guard.append(UpmixSource::new(source, target_channels));
            }
        } else {
            debug_log!("Falling back to slow IO seek...");
            if let Some(data) = &self.raw_bytes {
                if let Ok(mut src) = Self::create_decoder(data) {
                    if src.try_seek(Duration::from_secs_f64(time)).is_ok() {
                        sink_guard.append(UpmixSource::new(src.convert_samples::<f32>(), target_channels));
                    } else {
                        let fallback = Self::create_decoder(data).unwrap();
                        let skipped = fallback.convert_samples::<f32>().skip_duration(Duration::from_secs_f64(time));
                        sink_guard.append(UpmixSource::new(skipped, target_channels));
                    }
                }
            }
        }
        
        // 4. 读取刚才备份的播放状态（绝对尊重前端），然后真正放出声音
        sink_guard.set_volume(self.get_volume());
        if is_playing_now { 
            sink_guard.play(); 
        } else { 
            sink_guard.pause(); 
        }
    }

    fn set_volume(&mut self, vol: f32) {
        *self.current_volume.write().unwrap() = vol;
        if let Ok(s) = self.sink.lock() { s.set_volume(vol); }
    }

    fn set_channel_mode(&mut self, _mode: u16) {
        let config = match _mode {
            6 => ChannelConfig::Surround51, 8 => ChannelConfig::Surround71, _ => ChannelConfig::Stereo,
        };
        *self.channel_mode.write().unwrap() = config;
    }
}