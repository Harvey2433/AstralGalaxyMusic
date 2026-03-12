use super::AudioEngine;
use rodio::{Decoder, OutputStreamHandle, Sink, Source};
use std::fs::File;
use std::io::{Cursor, Read};
use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::thread;
use std::mem;

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

#[derive(Clone)]
pub struct AntiAliasFilter {
    x1: f32, x2: f32,
    y1: f32, y2: f32,
    b0: f32, b1: f32, b2: f32,
    a1: f32, a2: f32,
    active: bool,
}

impl AntiAliasFilter {
    pub fn new(sample_rate: u32) -> Self {
        if sample_rate <= 48000 {
            return Self { x1:0., x2:0., y1:0., y2:0., b0:1., b1:0., b2:0., a1:0., a2:0., active: false };
        }
        
        let w0 = 2.0 * std::f32::consts::PI * 22000.0 / sample_rate as f32;
        let alpha = w0.sin() / (2.0 * 0.7071); 
        let cos_w0 = w0.cos();
        let a0 = 1.0 + alpha;

        Self {
            x1: 0., x2: 0., y1: 0., y2: 0.,
            b0: ((1.0 - cos_w0) / 2.0) / a0,
            b1: (1.0 - cos_w0) / a0,
            b2: ((1.0 - cos_w0) / 2.0) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            active: true,
        }
    }

    #[inline(always)]
    pub fn process(&mut self, x0: f32) -> f32 {
        if !self.active { return x0; }
        let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
                 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1; self.x1 = x0;
        self.y2 = self.y1; self.y1 = y0;
        y0
    }
}

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
    pub virtualize: bool,
    current_frame: Vec<f32>,
    dsp: SpatialProcessor, 
    filter_l: AntiAliasFilter, 
    filter_r: AntiAliasFilter, 
}

impl<I: Source<Item = f32>> UpmixSource<I> {
    pub fn new(input: I, config_code: u16) -> Self {
        let sample_rate = input.sample_rate();
        let (target_channels, virtualize) = match config_code {
            6 => (6, true),
            8 => (8, true),
            106 => (6, false),
            108 => (8, false),
            _ => (2, false),
        };
        Self { 
            input, 
            target_channels, 
            virtualize, 
            current_frame: Vec::with_capacity(8), 
            dsp: SpatialProcessor::new(sample_rate),
            filter_l: AntiAliasFilter::new(sample_rate),
            filter_r: AntiAliasFilter::new(sample_rate),
        }
    }

    #[inline(always)]
    fn smart_clip(mut val: f32) -> f32 {
        val *= 1.25; 
        let abs_val = val.abs();
        if abs_val <= 0.75 {
            val
        } else if abs_val < 1.1 {
            let knee = 0.75;
            let diff = abs_val - knee;
            val.signum() * (knee + diff - (diff * diff) / 0.5)
        } else {
            val.signum() * 0.98
        }
    }
}

impl<I: Source<Item = f32>> Iterator for UpmixSource<I> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.current_frame.is_empty() {
            let raw_l = self.input.next()?;
            let raw_r = if self.input.channels() == 1 { raw_l } else { self.input.next().unwrap_or(raw_l) };
            
            if self.input.channels() > 2 {
                for _ in 2..self.input.channels() { let _ = self.input.next(); }
            }

            let l = self.filter_l.process(raw_l);
            let r = self.filter_r.process(raw_r);

            if self.target_channels == 2 && !self.virtualize {
                self.current_frame.push(Self::smart_clip(r));
                self.current_frame.push(Self::smart_clip(l));
                return self.current_frame.pop();
            }
            
            let (lfe_raw, rear_l_raw, rear_r_raw) = self.dsp.process(l, r);
            let center = (l + r) * 0.5;
            
            if self.virtualize {
                if self.target_channels == 6 {
                    let mix_l = l * 0.75 + center * 0.3 + lfe_raw * 0.6 - rear_r_raw * 0.45;
                    let mix_r = r * 0.75 + center * 0.3 + lfe_raw * 0.6 - rear_l_raw * 0.45;
                    self.current_frame.push(Self::smart_clip(mix_l)); 
                    self.current_frame.push(Self::smart_clip(mix_r)); 
                } else {
                    let mix_l = l * 0.65 + center * 0.3 + lfe_raw * 0.7 - rear_r_raw * 0.55 + rear_l_raw * 0.2;
                    let mix_r = r * 0.65 + center * 0.3 + lfe_raw * 0.7 - rear_l_raw * 0.55 + rear_r_raw * 0.2;
                    self.current_frame.push(Self::smart_clip(mix_l)); 
                    self.current_frame.push(Self::smart_clip(mix_r)); 
                }
            } else {
                let lfe = lfe_raw * 1.2;
                self.current_frame.push(Self::smart_clip(l));          
                self.current_frame.push(Self::smart_clip(r));          
                self.current_frame.push(Self::smart_clip(center));     
                self.current_frame.push(Self::smart_clip(lfe));        
                self.current_frame.push(Self::smart_clip(rear_l_raw)); 
                self.current_frame.push(Self::smart_clip(rear_r_raw)); 
                
                if self.target_channels == 8 {
                    self.current_frame.push(Self::smart_clip(rear_l_raw * 0.8)); 
                    self.current_frame.push(Self::smart_clip(rear_r_raw * 0.8)); 
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
    
    playback_pos: Arc<RwLock<f64>>,
    last_play_instant: Arc<RwLock<Option<Instant>>>,
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
            playback_pos: Arc::new(RwLock::new(0.0)),
            last_play_instant: Arc::new(RwLock::new(None)),
        }
    }

    fn create_decoder(data: &Arc<Vec<u8>>) -> Result<Decoder<Cursor<Vec<u8>>>, String> {
        let cursor = Cursor::new(data.to_vec()); 
        Decoder::new(cursor).map_err(|e| e.to_string())
    }

    fn get_volume(&self) -> f32 { *self.current_volume.read().unwrap() }

    pub fn get_current_time(&self) -> f64 {
        let mut pos = *self.playback_pos.read().unwrap();
        if let Some(inst) = *self.last_play_instant.read().unwrap() {
            pos += inst.elapsed().as_secs_f64();
        }
        pos
    }
}

impl AudioEngine for GalaxyEngine {
    fn name(&self) -> &str { "Galaxy DSP (Sync Blocking Zero-Copy)" }

    fn update_output_stream(&mut self, handle: OutputStreamHandle) {
        let current_time = self.get_current_time();
        self.stream_handle = handle.clone();
        self.seek(current_time);
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
        
        *self.playback_pos.write().unwrap() = 0.0;
        *self.last_play_instant.write().unwrap() = if self.is_playing.load(Ordering::SeqCst) { Some(Instant::now()) } else { None };

        {
            let mut sink_guard = self.sink.lock().unwrap();
            if let Ok(new_sink) = Sink::try_new(&self.stream_handle) {
                // 🔥 绝杀修复：直接同步替换并销毁，彻底禁止 thread::spawn 异步竞态！
                *sink_guard = new_sink;
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
                    if count % 16384 == 0 {
                        if session_ref.load(Ordering::SeqCst) != my_session { return; }
                        thread::sleep(Duration::from_millis(2));
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
        *self.last_play_instant.write().unwrap() = Some(Instant::now());
        if let Ok(s) = self.sink.lock() { s.play(); } 
    }
    
    fn pause(&mut self) { 
        self.is_playing.store(false, Ordering::SeqCst);
        let mut pos = self.playback_pos.write().unwrap();
        let mut inst = self.last_play_instant.write().unwrap();
        if let Some(i) = *inst {
            *pos += i.elapsed().as_secs_f64();
        }
        *inst = None;
        if let Ok(s) = self.sink.lock() { s.pause(); } 
    }

    fn seek(&mut self, time: f64) {
        let my_seek_session = self.seek_session.fetch_add(1, Ordering::SeqCst) + 1;
        let is_playing_now = self.is_playing.load(Ordering::SeqCst);

        *self.playback_pos.write().unwrap() = time;
        *self.last_play_instant.write().unwrap() = if is_playing_now { Some(Instant::now()) } else { None };

        {
            let mut sink_guard = self.sink.lock().unwrap();
            if let Ok(new_sink) = Sink::try_new(&self.stream_handle) {
                // 🔥 绝杀修复：严禁把旧 Sink 踢到后台销毁，同步锁死！
                *sink_guard = new_sink;
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