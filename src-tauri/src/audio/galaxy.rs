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

// 🔥 新增：真实声道的标识 106 / 108
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelConfig {
    Stereo = 2,
    Surround51 = 6,
    Surround71 = 8,
    True51 = 106,
    True71 = 108,
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
    pub virtualize: bool, // 🔥 新增：虚拟降维标志
    current_frame: Vec<f32>,
    dsp: SpatialProcessor, 
}

impl<I: Source<Item = f32>> UpmixSource<I> {
    pub fn new(input: I, config_code: u16) -> Self {
        let sample_rate = input.sample_rate();
        // 🔥 解析魔法指令，决定是真的多声道还是耳机虚拟
        let (target_channels, virtualize) = match config_code {
            6 => (6, true),
            8 => (8, true),
            106 => (6, false),
            108 => (8, false),
            _ => (2, false),
        };
        Self { input, target_channels, virtualize, current_frame: Vec::with_capacity(8), dsp: SpatialProcessor::new(sample_rate) }
    }
    #[inline(always)]
    fn clamp(val: f32) -> f32 { val.max(-1.0).min(1.0) }
}

// =========================================================
// 🔥 真环绕与虚拟声场分发矩阵
// =========================================================
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
            let center = (l + r) * 0.5;
            
            if self.virtualize {
                // --- 🎧 Virtual 虚拟降维空间矩阵 (骗过大脑) ---
                if self.target_channels == 6 {
                    let mix_l = l * 0.75 + center * 0.3 + lfe_raw * 0.6 - rear_r_raw * 0.45;
                    let mix_r = r * 0.75 + center * 0.3 + lfe_raw * 0.6 - rear_l_raw * 0.45;
                    self.current_frame.push(Self::clamp(mix_l)); 
                    self.current_frame.push(Self::clamp(mix_r)); 
                } else {
                    let mix_l = l * 0.65 + center * 0.3 + lfe_raw * 0.7 - rear_r_raw * 0.55 + rear_l_raw * 0.2;
                    let mix_r = r * 0.65 + center * 0.3 + lfe_raw * 0.7 - rear_l_raw * 0.55 + rear_r_raw * 0.2;
                    self.current_frame.push(Self::clamp(mix_l)); 
                    self.current_frame.push(Self::clamp(mix_r)); 
                }
            } else {
                // --- 🔊 真实物理多声道输出 (家庭影院满血火力) ---
                let lfe = lfe_raw * 1.2;
                self.current_frame.push(Self::clamp(l));          
                self.current_frame.push(Self::clamp(r));          
                self.current_frame.push(Self::clamp(center));     
                self.current_frame.push(Self::clamp(lfe));        
                self.current_frame.push(Self::clamp(rear_l_raw)); 
                self.current_frame.push(Self::clamp(rear_r_raw)); 
                
                if self.target_channels == 8 {
                    self.current_frame.push(Self::clamp(rear_l_raw * 0.8)); 
                    self.current_frame.push(Self::clamp(rear_r_raw * 0.8)); 
                }
            }
            
            self.current_frame.reverse(); 
        }
        self.current_frame.pop()
    }
}

impl<I: Source<Item = f32>> Source for UpmixSource<I> {
    fn current_frame_len(&self) -> Option<usize> { None }
    // 🔥 虚拟时伪装 2 声道，真实模式老实输出多声道！
    fn channels(&self) -> u16 { if self.virtualize { 2 } else { self.target_channels } } 
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
            // 🔥 这里强制将 ChannelConfig 转换回 u16 的魔法指令丢进解析器
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

        {
            let mut sink_guard = self.sink.lock().unwrap();
            if let Ok(new_sink) = Sink::try_new(&self.stream_handle) {
                let old_sink = mem::replace(&mut *sink_guard, new_sink);
                thread::spawn(move || drop(old_sink));
            } else {
                sink_guard.stop();
            }
            sink_guard.pause(); 
        }


        let target_channels = *self.channel_mode.read().unwrap() as u16;
        
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

    // 🔥 解析前端设置模式
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