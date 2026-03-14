use super::AudioEngine;
use rodio::{Decoder, OutputStreamHandle, Sink, Source};
use std::fs::File;
use std::io::{Cursor, Read};
use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicUsize, AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};
use std::thread;

use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters, WindowFunction};
use rodio::cpal::traits::{HostTrait, DeviceTrait};

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
    True51 = 106,
    True71 = 108,
}

// =================================================================
// 🧠 动态硬件采样率嗅探器
// =================================================================
fn get_dynamic_target_sr() -> u32 {
    if let Some(device) = rodio::cpal::default_host().default_output_device() {
        if let Ok(config) = device.default_output_config() {
            let sr = config.sample_rate().0;
            debug_log!("Dynamic SR Detection: Target device runs at {}Hz. Perfect match engaged.", sr);
            return sr;
        }
    }
    debug_log!("Dynamic SR Detection failed, falling back to 48000Hz.");
    48000
}

// =================================================================
// 👑 终极特权：系统级 MMCSS 原生注入模块 (零外部依赖)
// =================================================================
#[cfg(target_os = "windows")]
pub mod mmcss {
    use std::ffi::c_void;
    
    #[link(name = "avrt")]
    extern "system" {
        pub fn AvSetMmThreadCharacteristicsW(TaskName: *const u16, TaskIndex: *mut u32) -> *mut c_void;
        pub fn AvSetMmThreadPriority(AvrtHandle: *mut c_void, Priority: i32) -> i32;
    }
    
    #[link(name = "kernel32")]
    extern "system" {
        pub fn SetThreadPriority(hThread: *mut c_void, nPriority: i32) -> i32;
        pub fn GetCurrentThread() -> *mut c_void;
    }

    pub fn elevate_thread() {
        unsafe {
            SetThreadPriority(GetCurrentThread(), 2);
            let mut task_index = 0;
            let task_name: [u16; 10] = [0x50, 0x72, 0x6f, 0x20, 0x41, 0x75, 0x64, 0x69, 0x6f, 0x00];
            let handle = AvSetMmThreadCharacteristicsW(task_name.as_ptr(), &mut task_index);
            if !handle.is_null() {
                AvSetMmThreadPriority(handle, 2);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub mod mmcss {
    pub fn elevate_thread() {}
}

// =================================================================
// 🚀 发烧级 Rubato Sinc 重采样器
// =================================================================
pub struct RubatoSource<I: Source<Item = f32>> {
    input: I,
    resampler: Option<SincFixedIn<f32>>,
    input_buffers: Vec<Vec<f32>>,
    output_buffer: Vec<f32>,
    output_pos: usize,
    channels: usize,
    chunk_size: usize,
    target_sr: u32,
    source_sr: u32,
    exhausted: bool,
}

impl<I: Source<Item = f32>> RubatoSource<I> {
    pub fn new(input: I, target_sr: u32) -> Self {
        let source_sr = input.sample_rate();
        let channels = input.channels() as usize;

        if source_sr == target_sr {
            debug_log!("Source SR ({}) matches Target SR. Bypassing Resampler.", source_sr);
            return Self {
                input, resampler: None, input_buffers: vec![], output_buffer: vec![],
                output_pos: 0, channels, chunk_size: 0, target_sr, source_sr, exhausted: false,
            };
        }

        debug_log!("Rubato Resampler Activated: {}Hz -> {}Hz", source_sr, target_sr);
        let chunk_size = 2048; 
        
        let params = SincInterpolationParameters {
            sinc_len: 256, 
            f_cutoff: 0.985, 
            interpolation: SincInterpolationType::Cubic, 
            oversampling_factor: 256, 
            window: WindowFunction::BlackmanHarris2,
        };

        let resampler = SincFixedIn::<f32>::new(
            target_sr as f64 / source_sr as f64,
            2.0,
            params,
            chunk_size,
            channels,
        ).unwrap();

        Self {
            input,
            resampler: Some(resampler),
            input_buffers: vec![Vec::with_capacity(chunk_size); channels],
            output_buffer: Vec::with_capacity(chunk_size * channels * 3), 
            output_pos: 0, channels, chunk_size, target_sr, source_sr, exhausted: false,
        }
    }

    fn process_next_chunk(&mut self) {
        if self.resampler.is_none() || self.exhausted { return; }
        for ch in 0..self.channels { self.input_buffers[ch].clear(); }
        let mut frames_read = 0;
        for _ in 0..self.chunk_size {
            let mut frame_complete = false;
            for ch in 0..self.channels {
                if let Some(s) = self.input.next() {
                    self.input_buffers[ch].push(s);
                    if ch == self.channels - 1 { frame_complete = true; }
                } else { break; }
            }
            if !frame_complete {
                for ch in 0..self.channels {
                    while self.input_buffers[ch].len() < self.chunk_size { self.input_buffers[ch].push(0.0); }
                }
                self.exhausted = true;
                break;
            }
            frames_read += 1;
        }

        if frames_read == 0 && self.exhausted { return; }
        let out_buffers = self.resampler.as_mut().unwrap().process(&self.input_buffers, None).unwrap();
        self.output_buffer.clear();
        let out_frames = out_buffers[0].len();
        let valid_out_frames = if self.exhausted {
            (frames_read as f64 * (self.target_sr as f64 / self.source_sr as f64)).round() as usize
        } else { out_frames };

        for i in 0..valid_out_frames.min(out_frames) {
            for ch in 0..self.channels {
                let mut sample = out_buffers[ch][i];
                if sample.abs() > 0.95 {
                    let overshoot = sample.abs() - 0.95;
                    sample = sample.signum() * (0.95 + overshoot * 0.5);
                }
                self.output_buffer.push(sample);
            }
        }
        self.output_pos = 0;
    }
}

impl<I: Source<Item = f32>> Iterator for RubatoSource<I> {
    type Item = f32;
    #[inline(always)]
    fn next(&mut self) -> Option<f32> {
        if self.resampler.is_none() { return self.input.next(); }
        if self.output_pos >= self.output_buffer.len() {
            if self.exhausted { return None; }
            self.process_next_chunk();
            if self.output_pos >= self.output_buffer.len() { return None; }
        }
        let val = self.output_buffer[self.output_pos];
        self.output_pos += 1;
        Some(val)
    }
}

impl<I: Source<Item = f32>> Source for RubatoSource<I> {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.channels as u16 }
    fn sample_rate(&self) -> u32 { self.target_sr } 
    fn total_duration(&self) -> Option<Duration> { self.input.total_duration() }
}

// =================================================================
// 空间混音与软拐点压限器
// =================================================================
pub struct SpatialProcessor {
    lfe_state: f32, delay_buffer: Vec<(f32, f32)>, delay_pos: usize, alpha: f32,
}

impl SpatialProcessor {
    pub fn new(sample_rate: u32) -> Self {
        let delay_samples = (sample_rate as f32 * 0.020) as usize;
        let dt = 1.0 / sample_rate as f32;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * 120.0);
        let alpha = dt / (rc + dt);
        Self { lfe_state: 0.0, delay_buffer: vec![(0.0, 0.0); delay_samples.max(1)], delay_pos: 0, alpha }
    }
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32, f32) {
        let mono = (l + r) * 0.5;
        self.lfe_state += self.alpha * (mono - self.lfe_state);
        let (delayed_l, delayed_r) = self.delay_buffer[self.delay_pos];
        self.delay_buffer[self.delay_pos] = (l, r);
        self.delay_pos = (self.delay_pos + 1) % self.delay_buffer.len();
        (self.lfe_state, delayed_l, delayed_r)
    }
}

pub struct UpmixSource<I: Source<Item = f32>> {
    input: I,
    pub target_channels: u16,
    pub virtualize: bool,
    current_frame: Vec<f32>,
    dsp: SpatialProcessor, 
    
    dc_l: f32, dc_r: f32,
    prev_l: f32, prev_r: f32,
    
    is_playing_flag: Arc<AtomicBool>,
    state_vol: f32,
    fade_step: f32,

    master_vol_target: Arc<AtomicU32>,
    master_vol_current: f32,
    master_vol_alpha: f32,
    
    is_first_run: bool, 
}

impl<I: Source<Item = f32>> UpmixSource<I> {
    pub fn new(input: I, config_code: u16, is_playing_flag: Arc<AtomicBool>, master_vol_target: Arc<AtomicU32>) -> Self {
        let sample_rate = input.sample_rate();
        let (target_channels, virtualize) = match config_code {
            6 => (6, true), 8 => (8, true), 106 => (6, false), 108 => (8, false), _ => (2, false),
        };
        Self { 
            input, target_channels, virtualize, current_frame: Vec::with_capacity(8), 
            dsp: SpatialProcessor::new(sample_rate),
            dc_l: 0.0, dc_r: 0.0, prev_l: 0.0, prev_r: 0.0,
            is_playing_flag, state_vol: 0.0, fade_step: 1.0 / (sample_rate.max(1) as f32 * 0.03), 
            master_vol_current: f32::from_bits(master_vol_target.load(Ordering::Relaxed)),
            master_vol_target, master_vol_alpha: 1.0 / (sample_rate.max(1) as f32 * 0.02), 
            is_first_run: true,
        }
    }

    // 🔥 终极修复：驯服“反相抑制”
    // 拔掉所有的额外增益，将保护阈值推到 0.98，仅作为防爆砖！
    #[inline(always)]
    fn audiophile_limiter(mut val: f32) -> f32 {
        let abs_val = val.abs();
        if abs_val <= 0.98 {
            val // 0.98 以内，原汁原味，零修改！
        } else {
            let diff = abs_val - 0.98;
            val.signum() * (0.98 + diff / (1.0 + diff * 8.0)) // 极硬拐点，仅在要爆破时才出手
        }
    }
}

impl<I: Source<Item = f32>> Iterator for UpmixSource<I> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.is_first_run {
            self.is_first_run = false;
            mmcss::elevate_thread();
            debug_log!("Real-time Audio Callback Thread elevated to MMCSS Pro Audio!");
        }

        if self.current_frame.is_empty() {
            let target_state = if self.is_playing_flag.load(Ordering::Relaxed) { 1.0 } else { 0.0 };
            if self.state_vol != target_state {
                if self.state_vol < target_state { self.state_vol = (self.state_vol + self.fade_step).min(target_state); } 
                else { self.state_vol = (self.state_vol - self.fade_step).max(target_state); }
            }
            let m = self.state_vol;

            if m == 0.0 && target_state == 0.0 {
                let out_channels = if self.virtualize { 2 } else { self.target_channels };
                for _ in 0..out_channels { self.current_frame.push(0.0); }
                return self.current_frame.pop();
            }

            let smooth_state_vol = m * m * (3.0 - 2.0 * m);
            let target_master = f32::from_bits(self.master_vol_target.load(Ordering::Relaxed));
            let vol_diff = target_master - self.master_vol_current;
            if vol_diff.abs() > 0.0001 { self.master_vol_current += vol_diff * self.master_vol_alpha; } 
            else { self.master_vol_current = target_master; }

            let final_gain = smooth_state_vol * self.master_vol_current;

            let raw_l = match self.input.next() { Some(v) => v, None => return None };
            let raw_r = if self.input.channels() == 1 { raw_l } else { self.input.next().unwrap_or(raw_l) };
            if self.input.channels() > 2 { for _ in 2..self.input.channels() { let _ = self.input.next(); } }

            let l = raw_l - self.prev_l + 0.995 * self.dc_l;
            let r = raw_r - self.prev_r + 0.995 * self.dc_r;
            self.dc_l = l; self.dc_r = r;
            self.prev_l = raw_l; self.prev_r = raw_r;

            if self.target_channels == 2 && !self.virtualize {
                self.current_frame.push(Self::audiophile_limiter(r * final_gain));
                self.current_frame.push(Self::audiophile_limiter(l * final_gain));
                return self.current_frame.pop();
            }
            
            let (lfe_raw, rear_l_raw, rear_r_raw) = self.dsp.process(l, r);
            let center = (l + r) * 0.5;
            
            if self.virtualize {
                if self.target_channels == 6 {
                    let mix_l = l * 0.75 + center * 0.3 + lfe_raw * 0.6 - rear_r_raw * 0.45;
                    let mix_r = r * 0.75 + center * 0.3 + lfe_raw * 0.6 - rear_l_raw * 0.45;
                    self.current_frame.push(Self::audiophile_limiter(mix_l * final_gain)); 
                    self.current_frame.push(Self::audiophile_limiter(mix_r * final_gain)); 
                } else {
                    let mix_l = l * 0.65 + center * 0.3 + lfe_raw * 0.7 - rear_r_raw * 0.55 + rear_l_raw * 0.2;
                    let mix_r = r * 0.65 + center * 0.3 + lfe_raw * 0.7 - rear_l_raw * 0.55 + rear_r_raw * 0.2;
                    self.current_frame.push(Self::audiophile_limiter(mix_l * final_gain)); 
                    self.current_frame.push(Self::audiophile_limiter(mix_r * final_gain)); 
                }
            } else {
                let lfe = lfe_raw * 1.2;
                self.current_frame.push(Self::audiophile_limiter(l * final_gain));          
                self.current_frame.push(Self::audiophile_limiter(r * final_gain));          
                self.current_frame.push(Self::audiophile_limiter(center * final_gain));     
                self.current_frame.push(Self::audiophile_limiter(lfe * final_gain));        
                self.current_frame.push(Self::audiophile_limiter(rear_l_raw * final_gain)); 
                self.current_frame.push(Self::audiophile_limiter(rear_r_raw * final_gain)); 
                
                if self.target_channels == 8 {
                    self.current_frame.push(Self::audiophile_limiter(rear_l_raw * 0.8 * final_gain)); 
                    self.current_frame.push(Self::audiophile_limiter(rear_r_raw * 0.8 * final_gain)); 
                } else {
                    // 🔥 核心修复：如果是 5.1 模式，我们强行压入 2 个静音采样！
                    // 作用：将 6 个采样补齐到 8 个采样（偶数对齐），彻底消灭声道漂移导致的电音！
                    // 这样即便在双声道耳机下，Rodio 也能永远精准抓到 L 和 R，而不会把 C 抓给你的耳朵！
                    self.current_frame.push(0.0); // 填充位 1 (Fake BL)
                    self.current_frame.push(0.0); // 填充位 2 (Fake BR)
                }
            }
            self.current_frame.reverse(); 
        }
        self.current_frame.pop()
    }
}

impl<I: Source<Item = f32>> Source for UpmixSource<I> {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { if self.virtualize { 2 } else { self.target_channels } } 
    fn sample_rate(&self) -> u32 { self.input.sample_rate() }
    fn total_duration(&self) -> Option<Duration> { self.input.total_duration() }
}

// =================================================================
// 后台零拷贝内存源引擎 (最核心的 O(1) 瞬切基石)
// =================================================================
#[derive(Clone)]
pub struct ArcSliceSource {
    data: Arc<Vec<f32>>, pos: usize, channels: u16, sample_rate: u32,
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
        if self.pos < self.data.len() { let val = self.data[self.pos]; self.pos += 1; Some(val) } else { None }
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

// =================================================================
// GalaxyEngine 主控 (Adaptive Sync Core)
// =================================================================
pub struct GalaxyEngine {
    sink: Arc<Mutex<Sink>>,
    stream_handle: OutputStreamHandle,
    raw_bytes: Option<Arc<Vec<u8>>>,
    decoded_samples: Arc<RwLock<Option<Arc<Vec<f32>>>>>, 
    is_decoded: Arc<AtomicBool>, 
    decode_session: Arc<AtomicUsize>, 
    is_playing: Arc<AtomicBool>, 
    sample_rate: u32,
    channels: u16,
    current_volume: Arc<AtomicU32>, 
    channel_mode: Arc<RwLock<ChannelConfig>>,
    playback_pos: Arc<RwLock<f64>>,
    last_play_instant: Arc<RwLock<Option<Instant>>>,
    fade_token: Arc<AtomicUsize>, 
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
            is_playing: Arc::new(AtomicBool::new(false)), 
            sample_rate: 44100, 
            channels: 2,
            current_volume: Arc::new(AtomicU32::new(1f32.to_bits())),
            channel_mode: Arc::new(RwLock::new(ChannelConfig::Stereo)),
            playback_pos: Arc::new(RwLock::new(0.0)),
            last_play_instant: Arc::new(RwLock::new(None)),
            fade_token: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn create_decoder(data: &Arc<Vec<u8>>) -> Result<Decoder<Cursor<Vec<u8>>>, String> {
        let cursor = Cursor::new(data.to_vec()); 
        Decoder::new(cursor).map_err(|e| e.to_string())
    }

    pub fn get_current_time(&self) -> f64 {
        let mut pos = *self.playback_pos.read().unwrap();
        if let Some(inst) = *self.last_play_instant.read().unwrap() {
            pos += inst.elapsed().as_secs_f64();
        }
        pos
    }
}

impl AudioEngine for GalaxyEngine {
    fn name(&self) -> &str { "Galaxy DSP (Adaptive Sync Core)" }

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

        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut buffer = Vec::with_capacity(len as usize);
        file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
        let raw_bytes = Arc::new(buffer);

        let source = Self::create_decoder(&raw_bytes)?;
        
        let target_sr = get_dynamic_target_sr();
        let hq_source = RubatoSource::new(source.convert_samples::<f32>(), target_sr);
        
        self.sample_rate = hq_source.sample_rate(); 
        self.channels = hq_source.channels();
        let total_duration = hq_source.total_duration().map(|d| d.as_secs_f64()).unwrap_or(0.0);

        let my_session = self.decode_session.fetch_add(1, Ordering::SeqCst) + 1;
        *self.decoded_samples.write().unwrap() = None;
        self.is_decoded.store(false, Ordering::Release);
        
        *self.playback_pos.write().unwrap() = 0.0;
        *self.last_play_instant.write().unwrap() = if self.is_playing.load(Ordering::SeqCst) { Some(Instant::now()) } else { None };

        self.fade_token.fetch_add(1, Ordering::SeqCst); 

        {
            let mut sink_guard = self.sink.lock().unwrap();
            *sink_guard = Sink::try_new(&self.stream_handle).unwrap();
            sink_guard.set_volume(1.0);
            let mixed_source = UpmixSource::new(hq_source, *self.channel_mode.read().unwrap() as u16, self.is_playing.clone(), self.current_volume.clone());
            sink_guard.append(mixed_source);
            sink_guard.play(); 
        }

        self.raw_bytes = Some(raw_bytes.clone());

        let session_ref = self.decode_session.clone();
        let samples_ref = self.decoded_samples.clone();
        let is_decoded_ref = self.is_decoded.clone();
        let raw_bytes_clone = raw_bytes.clone();
        let bg_target_sr = target_sr; 

        thread::spawn(move || {
            // 🔥 修复：取消后台解码线程的 MMCSS 特权！
            // 绝不允许它和实时播放线程抢夺 CPU，让它乖乖在 Normal 优先级跑！
            debug_log!("Background full-decode thread started (Normal Priority to protect real-time stream!).");
            
            if let Ok(decoder) = Decoder::new(Cursor::new(raw_bytes_clone.to_vec())) {
                let hq_source = RubatoSource::new(decoder.convert_samples::<f32>(), bg_target_sr);
                let mut pcm_buffer = Vec::with_capacity(bg_target_sr as usize * 2 * 180); 
                let mut count = 0;
                
                for sample in hq_source {
                    pcm_buffer.push(sample);
                    count += 1;
                    
                    if count % 4096 == 0 {
                        if session_ref.load(Ordering::SeqCst) != my_session { return; }
                        thread::sleep(Duration::from_millis(1));
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
        if self.is_playing.swap(true, Ordering::SeqCst) { return; }
        *self.last_play_instant.write().unwrap() = Some(Instant::now());
        self.fade_token.fetch_add(1, Ordering::SeqCst); 
        if let Ok(s) = self.sink.lock() { s.play(); } 
    }
    
    fn pause(&mut self) { 
        if !self.is_playing.swap(false, Ordering::SeqCst) { return; }
        let mut pos = self.playback_pos.write().unwrap();
        if let Some(i) = self.last_play_instant.write().unwrap().take() { *pos += i.elapsed().as_secs_f64(); }

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
        if is_playing_now {
            self.is_playing.store(false, Ordering::SeqCst);
            if let Ok(s) = self.sink.lock() { s.pause(); }
        }

        *self.playback_pos.write().unwrap() = time;
        *self.last_play_instant.write().unwrap() = if is_playing_now { Some(Instant::now()) } else { None };

        if !self.is_decoded.load(Ordering::Acquire) {
            debug_log!("Seek triggered before full-decode complete. Synchronously waiting for background process...");
            while !self.is_decoded.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(50));
            }
            debug_log!("Background process finished! Executing zero-copy instant seek.");
        }

        let target_channels = *self.channel_mode.read().unwrap() as u16;
        let mut sink_guard = self.sink.lock().unwrap();
        *sink_guard = Sink::try_new(&self.stream_handle).unwrap();
        
        if let Some(samples_arc) = self.decoded_samples.read().unwrap().clone() {
            let source = ArcSliceSource::new(samples_arc, self.channels, self.sample_rate)
                .skip_duration(Duration::from_secs_f64(time));
            sink_guard.append(UpmixSource::new(source, target_channels, self.is_playing.clone(), self.current_volume.clone()));
        }
        
        sink_guard.set_volume(1.0); 
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
            6 => ChannelConfig::Surround51, 8 => ChannelConfig::Surround71, 
            106 => ChannelConfig::True51, 108 => ChannelConfig::True71, _ => ChannelConfig::Stereo,
        };
        *self.channel_mode.write().unwrap() = config;
    }
}