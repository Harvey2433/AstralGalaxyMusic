// src/audio/galaxy.rs

use super::AudioEngine;
use rodio::{Decoder, OutputStreamHandle, Sink, Source};
use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::sync::{Arc, RwLock, Mutex, Condvar};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use std::thread;
use std::panic;
use std::mem;
use biquad::*; 

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
// 🚀 DSP 核心: SpatialProcessor
// =========================================================
struct SpatialProcessor {
    lfe_filter_1: DirectForm2Transposed<f32>,
    lfe_filter_2: DirectForm2Transposed<f32>,
    delay_buffer: Vec<(f32, f32)>, 
    delay_pos: usize,
}

impl SpatialProcessor {
    pub fn new(sample_rate: u32) -> Self {
        // 1. 初始化 LFE 滤波器 (120Hz)
        let cutoff_val = 120.0f32.min(sample_rate as f32 / 2.0);
        let fs = Hertz::<f32>::from_hz(sample_rate as f32).expect("Invalid sample rate");
        let f0 = Hertz::<f32>::from_hz(cutoff_val).expect("Invalid cutoff frequency");

        let coeffs = Coefficients::<f32>::from_params(
            Type::LowPass, fs, f0, Q_BUTTERWORTH_F32,
        ).unwrap();

        // 2. 初始化延迟线 (20ms)
        let delay_ms = 0.020; 
        let delay_samples = (sample_rate as f32 * delay_ms) as usize;
        
        Self {
            lfe_filter_1: DirectForm2Transposed::<f32>::new(coeffs),
            lfe_filter_2: DirectForm2Transposed::<f32>::new(coeffs),
            delay_buffer: vec![(0.0, 0.0); delay_samples],
            delay_pos: 0,
        }
    }

    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32, f32) {
        // A. LFE 处理
        let mono = (l + r) * 0.5;
        let lfe_step1 = self.lfe_filter_1.run(mono);
        let lfe_out = self.lfe_filter_2.run(lfe_step1);

        // B. 延迟处理
        if !self.delay_buffer.is_empty() {
             self.delay_buffer[self.delay_pos] = (l, r);
             self.delay_pos = (self.delay_pos + 1) % self.delay_buffer.len();
        }
        
        let (delayed_l, delayed_r) = if !self.delay_buffer.is_empty() {
            self.delay_buffer[self.delay_pos]
        } else {
            (l, r) 
        };

        (lfe_out, delayed_l, delayed_r)
    }
}

// =========================================================
// 📦 内存源 (Memory Iterator)
// =========================================================
#[derive(Clone)]
pub struct MemorySource {
    data: Arc<Vec<f32>>,
    pos: usize,
    channels: u16,
    sample_rate: u32,
}

impl MemorySource {
    pub fn new(data: Arc<Vec<f32>>, offset: usize, channels: u16, sample_rate: u32) -> Self {
        Self { data, pos: offset, channels, sample_rate }
    }
}

impl Iterator for MemorySource {
    type Item = f32;
    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.pos < self.data.len() {
            let v = self.data[self.pos];
            self.pos += 1;
            Some(v)
        } else {
            None
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.data.len().saturating_sub(self.pos);
        (rem, Some(rem))
    }
}

impl Source for MemorySource {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.channels }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> { None }
}

// =========================================================
// 🎛️ 实时上混源 (Upmix Source) - 修复版
// =========================================================
pub struct UpmixSource<I: Source<Item = f32>> {
    input: I,
    target_channels: u16,
    current_frame: Vec<f32>,
    dsp: SpatialProcessor, 
}

impl<I: Source<Item = f32>> UpmixSource<I> {
    pub fn new(input: I, target_channels: u16) -> Self {
        let sample_rate = input.sample_rate();
        Self { 
            input, 
            target_channels, 
            current_frame: Vec::with_capacity(8),
            dsp: SpatialProcessor::new(sample_rate)
        }
    }
    
    // 🔥 辅助函数：安全限幅 (Hard Clipper)
    // 防止浮点数溢出导致的 "Zila" 爆音
    #[inline(always)]
    fn clamp(val: f32) -> f32 {
        val.max(-1.0).min(1.0)
    }
}

impl<I: Source<Item = f32>> Iterator for UpmixSource<I> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.input.channels() != 2 || self.target_channels == 2 {
            return self.input.next();
        }

        if self.current_frame.is_empty() {
            let l = self.input.next()?;
            let r = self.input.next()?;
            
            let (lfe_raw, rear_l_raw, rear_r_raw) = self.dsp.process(l, r);

            // --- 矩阵解码逻辑 (修正增益) ---
            
            // 1. Center: 降低 20% 增益以防止叠加失真
            let center = (l + r) * 0.4; 

            // 2. Front: 保持主声道清晰
            let front_l = l;
            let front_r = r;

            // 3. Surround: 
            // 修复核心：降低系数 (0.5+0.6 -> 1.1 溢出风险)
            // 调整为 (0.4 + 0.3) = 0.7 安全范围
            let ambience = (l - r) * 0.3; 
            let surround_l = rear_l_raw * 0.4 + ambience;
            let surround_r = rear_r_raw * 0.4 - ambience; 
            
            // 4. LFE: 增强一点存在感，但同样限幅
            let lfe = lfe_raw * 1.2;

            // --- 填充并应用限幅 ---
            self.current_frame.push(Self::clamp(front_l)); 
            self.current_frame.push(Self::clamp(front_r)); 
            self.current_frame.push(Self::clamp(center)); 
            self.current_frame.push(Self::clamp(lfe)); 
            
            // 5.1 / 7.1 分歧处理
            // 标准 5.1 顺序: FL, FR, C, LFE, RL, RR (通常映射到 5/6)
            self.current_frame.push(Self::clamp(surround_l)); 
            self.current_frame.push(Self::clamp(surround_r));
            
            if self.target_channels == 8 {
                // 7.1 Side Channels
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
// 🌌 Galaxy Engine
// =========================================================

pub struct GalaxyEngine {
    sink: Arc<Mutex<Sink>>,
    stream_handle: OutputStreamHandle,
    raw_bytes: Option<Arc<Vec<u8>>>,
    pcm_cache: Arc<RwLock<Option<Arc<Vec<f32>>>>>,
    sample_rate: u32,
    channels: u16,
    current_volume: Arc<RwLock<f32>>,
    channel_mode: Arc<RwLock<ChannelConfig>>,
    load_generation: Arc<AtomicUsize>, 
    decode_signal: Arc<(Mutex<bool>, Condvar)>,
}

impl GalaxyEngine {
    pub fn new(stream_handle: OutputStreamHandle) -> Self {
        let sink = Sink::try_new(&stream_handle).unwrap();
        Self {
            sink: Arc::new(Mutex::new(sink)),
            stream_handle,
            raw_bytes: None,
            pcm_cache: Arc::new(RwLock::new(None)),
            sample_rate: 44100,
            channels: 2,
            current_volume: Arc::new(RwLock::new(1.0)),
            channel_mode: Arc::new(RwLock::new(ChannelConfig::Stereo)),
            load_generation: Arc::new(AtomicUsize::new(0)),
            decode_signal: Arc::new((Mutex::new(false), Condvar::new())),
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
    fn name(&self) -> &str { "Galaxy DSP (Cinema Grade)" }

    fn update_output_stream(&mut self, handle: OutputStreamHandle) {
        self.stream_handle = handle;
    }

    fn load(&mut self, path: &str) -> Result<f64, String> {
        let current_gen = self.load_generation.fetch_add(1, Ordering::SeqCst) + 1;
        debug_log!(">>> LOAD START: Gen={}, Path={}", current_gen, path);

        {
            let (lock, _) = &*self.decode_signal;
            let mut finished = lock.lock().unwrap();
            *finished = false;
        }

        let start_read = Instant::now();
        let file = File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::with_capacity(len as usize);
        reader.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
        let raw_bytes = Arc::new(buffer);
        debug_log!("File read in {:?}", start_read.elapsed());

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
            } else {
                sink_guard.clear();
            }
            
            sink_guard.set_volume(self.get_volume());
            
            let target_mode = *self.channel_mode.read().unwrap();
            let target_channels = target_mode as u16;
            
            let buffered_source = source.convert_samples::<f32>().buffered();
            let mixed_source = UpmixSource::new(buffered_source, target_channels);
            
            sink_guard.append(mixed_source);
            sink_guard.play();
        }

        self.raw_bytes = Some(raw_bytes.clone());
        
        self.pcm_cache = Arc::new(RwLock::new(None));
        let pcm_cache_ref = self.pcm_cache.clone();
        let raw_bytes_clone = raw_bytes.clone();
        let generation_ref = self.load_generation.clone();
        let signal_ref = self.decode_signal.clone();
        let signal_ref_err = self.decode_signal.clone();

        thread::spawn(move || {
            let result = panic::catch_unwind(move || {
                if let Ok(decoder) = Self::create_decoder(&raw_bytes_clone) {
                    let samples: Vec<f32> = decoder.convert_samples().collect();
                    
                    if generation_ref.load(Ordering::SeqCst) == current_gen {
                        if let Ok(mut cache) = pcm_cache_ref.write() {
                            *cache = Some(Arc::new(samples));
                            debug_log!("Cache ready for Gen={}", current_gen);
                        }
                    }
                }
                let (lock, cvar) = &*signal_ref;
                let mut finished = lock.lock().unwrap();
                *finished = true;
                cvar.notify_all();
            });

            if let Err(_) = result {
                error_log!("Decoder Panic!");
                let (lock, cvar) = &*signal_ref_err;
                let mut finished = lock.lock().unwrap();
                *finished = true;
                cvar.notify_all();
            }
        });

        Ok(total_duration)
    }

    fn play(&mut self) { if let Ok(s) = self.sink.lock() { s.play(); } }
    fn pause(&mut self) { if let Ok(s) = self.sink.lock() { s.pause(); } }

    fn seek(&mut self, time: f64) {
        let current_gen = self.load_generation.load(Ordering::SeqCst);
        debug_log!("SEEK: {}s", time);

        {
            let has_cache = self.pcm_cache.read().unwrap().is_some();
            if !has_cache {
                let (lock, cvar) = &*self.decode_signal;
                let mut finished = lock.lock().unwrap();
                while !*finished && self.load_generation.load(Ordering::SeqCst) == current_gen {
                    let result = cvar.wait_timeout(finished, Duration::from_secs(20)).unwrap();
                    finished = result.0;
                    if result.1.timed_out() { break; }
                }
            }
        }

        if self.load_generation.load(Ordering::SeqCst) != current_gen { return; }

        debug_log!("Creating NEW Sink for Seek...");
        let new_sink_result = Sink::try_new(&self.stream_handle);

        let mut sink_guard = self.sink.lock().unwrap();
        
        if let Ok(new_sink) = new_sink_result {
            let old_sink = mem::replace(&mut *sink_guard, new_sink);
            Self::drop_sink_in_background(old_sink);
        } else {
            sink_guard.clear();
        }
        
        sink_guard.play(); 
        sink_guard.set_volume(self.get_volume());

        let cache = self.pcm_cache.read().unwrap();
        let mode = *self.channel_mode.read().unwrap();
        let target_channels = match mode {
            ChannelConfig::Stereo => 2,
            ChannelConfig::Surround51 => 6,
            ChannelConfig::Surround71 => 8,
        };

        let mut appended = false;

        if let Some(samples) = &*cache {
            let offset = (time * self.sample_rate as f64 * self.channels as f64) as usize;
            let align = self.channels as usize;
            let aligned_offset = offset - (offset % align);

            if aligned_offset < samples.len() {
                let source = MemorySource::new(
                    Arc::clone(samples), 
                    aligned_offset, 
                    self.channels, 
                    self.sample_rate
                );
                let mixed = UpmixSource::new(source.buffered(), target_channels);
                sink_guard.append(mixed);
                appended = true;
                debug_log!("Mem Seek OK");
            }
        } 
        
        if !appended {
            debug_log!("IO Seek...");
            if let Some(data) = &self.raw_bytes {
                if let Ok(mut src) = Self::create_decoder(data) {
                    if src.try_seek(Duration::from_secs_f64(time)).is_ok() {
                        let stream = src.convert_samples::<f32>().buffered();
                        let mixed = UpmixSource::new(stream, target_channels);
                        sink_guard.append(mixed);
                    } else {
                        error_log!("IO Seek Failed.");
                        sink_guard.pause(); 
                    }
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