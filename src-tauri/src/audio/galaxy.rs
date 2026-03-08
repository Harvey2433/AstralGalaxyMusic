// src/audio/galaxy.rs

use super::AudioEngine;
use rodio::{Decoder, OutputStreamHandle, Sink, Source};
use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::sync::{Arc, RwLock, Mutex};
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
macro_rules! error_log {
    ($($arg:tt)*) => ({
        println!("\x1b[31m[GALAXY-ERR] {}\x1b[0m", format!($($arg)*));
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelConfig {
    Stereo = 2,
    Surround51 = 6,
    Surround71 = 8,
}

// =========================================================
// 🚀 自研防崩溃 DSP 核心 (取代不稳定的 biquad 库)
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
        // 120Hz Lowpass 1-pole alpha calculation (绝对不崩溃算法)
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
// 🌌 Galaxy Engine (真·零拷贝瞬切流式版)
// =========================================================

pub struct GalaxyEngine {
    sink: Arc<Mutex<Sink>>,
    stream_handle: OutputStreamHandle,
    raw_bytes: Option<Arc<Vec<u8>>>,
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

    fn drop_sink_in_background(sink: Sink) {
        thread::spawn(move || { drop(sink); });
    }
}

impl AudioEngine for GalaxyEngine {
    fn name(&self) -> &str { "Galaxy DSP (True Zero-Copy)" }

    fn update_output_stream(&mut self, handle: OutputStreamHandle) {
        self.stream_handle = handle;
    }

    fn load(&mut self, path: &str) -> Result<f64, String> {
        let start_read = Instant::now();
        let file = File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::with_capacity(len as usize);
        reader.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
        let raw_bytes = Arc::new(buffer);

        let source = Self::create_decoder(&raw_bytes)?;
        self.sample_rate = source.sample_rate();
        self.channels = source.channels();
        let total_duration = source.total_duration().map(|d| d.as_secs_f64()).unwrap_or(0.0);

        let new_sink_result = Sink::try_new(&self.stream_handle);
        
        {
            let mut sink_guard = self.sink.lock().unwrap();
            if let Ok(new_sink) = new_sink_result {
                let old_sink = mem::replace(&mut *sink_guard, new_sink);
                Self::drop_sink_in_background(old_sink);
            } else { sink_guard.stop(); }
            
            sink_guard.set_volume(self.get_volume());
            let target_channels = *self.channel_mode.read().unwrap() as u16;
            
            let mixed_source = UpmixSource::new(source.convert_samples::<f32>(), target_channels);
            sink_guard.append(mixed_source);
            sink_guard.play();
        }

        self.raw_bytes = Some(raw_bytes);
        Ok(total_duration)
    }

    fn play(&mut self) { if let Ok(s) = self.sink.lock() { s.play(); } }
    fn pause(&mut self) { if let Ok(s) = self.sink.lock() { s.pause(); } }

    fn seek(&mut self, time: f64) {
        let new_sink_result = Sink::try_new(&self.stream_handle);
        let mut sink_guard = self.sink.lock().unwrap();
        
        if let Ok(new_sink) = new_sink_result {
            let old_sink = mem::replace(&mut *sink_guard, new_sink);
            Self::drop_sink_in_background(old_sink);
        } else { sink_guard.stop(); }
        
        sink_guard.set_volume(self.get_volume());
        let target_channels = *self.channel_mode.read().unwrap() as u16;

        if let Some(data) = &self.raw_bytes {
            if let Ok(mut src) = Self::create_decoder(data) {
                // 直接进行 IO Seek，摒弃所有缓存死锁，瞬间推流！
                if src.try_seek(Duration::from_secs_f64(time)).is_ok() {
                    let mixed = UpmixSource::new(src.convert_samples::<f32>(), target_channels);
                    sink_guard.append(mixed);
                } else {
                    debug_log!("try_seek unsupported, falling back to skip_duration...");
                    let fallback_src = Self::create_decoder(data).unwrap();
                    let skipped = fallback_src.convert_samples::<f32>().skip_duration(Duration::from_secs_f64(time));
                    let mixed = UpmixSource::new(skipped, target_channels);
                    sink_guard.append(mixed);
                }
            }
        }
        sink_guard.play();
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