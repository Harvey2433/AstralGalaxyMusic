use super::AudioEngine;
use rodio::{Decoder, OutputStreamHandle, Sink, Source};
use rodio::buffer::SamplesBuffer;
use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::sync::{Arc, RwLock, Mutex};
use std::time::Duration;
use std::thread;

// 声道模式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelConfig {
    Stereo = 2,
    Surround51 = 6,
    Surround71 = 8,
}

pub struct GalaxyEngine {
    sink: Arc<Mutex<Sink>>,
    stream_handle: OutputStreamHandle,
    raw_bytes: Option<Arc<Vec<u8>>>,
    pcm_cache: Arc<RwLock<Option<Arc<Vec<f32>>>>>, // 缓存 PCM 数据用于快速 Seek
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
            pcm_cache: Arc::new(RwLock::new(None)),
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

    // 声道上混算法
    fn upmix_samples(samples: &[f32], src_channels: u16, target_mode: ChannelConfig) -> Vec<f32> {
        if src_channels != 2 { return samples.to_vec(); }
        
        let target_channels = target_mode as u16;
        if target_channels == 2 { return samples.to_vec(); }

        let mut output = Vec::with_capacity(samples.len() / 2 * target_channels as usize);
        
        for chunk in samples.chunks(2) {
            if chunk.len() < 2 { break; }
            let l = chunk[0];
            let r = chunk[1];
            
            let center = (l + r) * 0.5;
            let lfe = (l + r) * 0.1;
            
            // 5.1 / 7.1 mapping
            output.push(l); 
            output.push(r);
            output.push(center);
            output.push(lfe);
            output.push(l * 0.8);
            output.push(r * 0.8);
            
            if target_channels == 8 {
                output.push(l * 0.6);
                output.push(r * 0.6);
            }
        }
        output
    }
}

impl AudioEngine for GalaxyEngine {
    fn name(&self) -> &str { "Galaxy Hybrid (Surround+)" }

    fn update_output_stream(&mut self, handle: OutputStreamHandle) {
        self.stream_handle = handle;
    }

    fn load(&mut self, path: &str) -> Result<f64, String> {
        // 1. 停止当前播放并清空缓冲
        {
            let sink = self.sink.lock().unwrap();
            sink.stop();
            sink.clear(); // 必须调用 clear，否则 Rodio 会把残余数据播完
        }
        
        // 短暂等待资源释放
        thread::sleep(Duration::from_millis(10));

        let file = File::open(path).map_err(|e| e.to_string())?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        let len = metadata.len();
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::with_capacity(len as usize);
        reader.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
        let raw_bytes = Arc::new(buffer);

        let source = Self::create_decoder(&raw_bytes)?;
        self.sample_rate = source.sample_rate();
        self.channels = source.channels();
        let total_duration = source.total_duration().map(|d| d.as_secs_f64()).unwrap_or(0.0);

        {
            let mut sink = self.sink.lock().unwrap();
            // 尝试基于最新句柄重建 Sink (确保设备切换生效)
            if let Ok(new_sink) = Sink::try_new(&self.stream_handle) {
                *sink = new_sink;
            } else {
                sink.clear(); // 重建失败则清空旧的
            }
            sink.set_volume(self.get_volume());
            sink.append(source);
            sink.play(); // 默认自动播放，或者由前端控制 pause
        }

        self.raw_bytes = Some(raw_bytes.clone());
        
        // 🔥 核心并发修复：
        // 创建一个新的 Arc<RwLock> 替换掉 self.pcm_cache。
        // 这样，上一首歌曲未完成的后台解码线程持有的是旧的 Arc，它写入的数据
        // 将被写入到这一“废弃”的内存区域，而不会污染当前 self.pcm_cache。
        // 这彻底解决了“切歌后Seek，由于旧线程晚于新线程完成，导致缓存被覆盖为旧歌数据”的Bug。
        self.pcm_cache = Arc::new(RwLock::new(None));

        let pcm_cache_ref = self.pcm_cache.clone();
        let raw_bytes_clone = raw_bytes.clone();
        
        // 后台解码线程
        thread::spawn(move || {
            if let Ok(decoder) = Self::create_decoder(&raw_bytes_clone) {
                // 这是一个耗时操作
                let samples: Vec<f32> = decoder.convert_samples().collect();
                
                // 解码完成后，获取写锁并写入
                if let Ok(mut cache) = pcm_cache_ref.write() {
                    *cache = Some(Arc::new(samples));
                }
            }
        });

        Ok(total_duration)
    }

    fn play(&mut self) {
        let sink = self.sink.clone();
        let vol = self.get_volume();
        thread::spawn(move || {
            if let Ok(s) = sink.lock() { s.play(); }
            // 简单的淡入防止爆音
            if let Ok(s) = sink.lock() { s.set_volume(0.0); }
            for i in 1..=10 {
                thread::sleep(Duration::from_millis(15));
                if let Ok(s) = sink.lock() { s.set_volume(vol * (i as f32 / 10.0)); }
            }
        });
    }

    fn pause(&mut self) {
        let sink = self.sink.clone();
        let start_vol = self.get_volume();
        thread::spawn(move || {
            // 淡出
            for i in 0..10 {
                thread::sleep(Duration::from_millis(15));
                if let Ok(s) = sink.lock() { s.set_volume(start_vol * (1.0 - i as f32 / 10.0)); }
            }
            if let Ok(s) = sink.lock() { s.pause(); s.set_volume(start_vol); }
        });
    }

    fn seek(&mut self, time: f64) {
        // 先获取 sink 锁
        let mut sink = self.sink.lock().unwrap();
        
        // 🔥 核心修复：Sink 被替换或追加前必须清空！
        // Rodio 的 append 是追加模式。如果不 clear，Seek 后的音频会排在当前播放缓冲的后面。
        // Drop 旧 sink 时若未 clear，旧 sink 的余音也会继续播放。
        sink.clear();

        let is_paused = sink.is_paused();
        
        // 尝试重建 sink，以防输出设备在播放中途改变了但未应用
        if let Ok(new_sink) = Sink::try_new(&self.stream_handle) { 
            *sink = new_sink; 
        }
        // 设置回音量（新 sink 默认音量是 1.0）
        sink.set_volume(self.get_volume());

        // 读取缓存锁
        let cache = self.pcm_cache.read().unwrap();
        let mode = *self.channel_mode.read().unwrap();

        if let Some(samples) = &*cache {
            // 有缓存：内存级 Seek (极速)
            let offset = (time * self.sample_rate as f64 * self.channels as f64) as usize;
            if offset < samples.len() {
                let slice = &samples[offset..];
                
                let final_samples = if self.channels == 2 && mode != ChannelConfig::Stereo {
                    Self::upmix_samples(slice, self.channels, mode)
                } else {
                    slice.to_vec()
                };
                
                let target_channels = if self.channels == 2 && mode != ChannelConfig::Stereo {
                    mode as u16
                } else {
                    self.channels
                };

                let buffer = SamplesBuffer::new(target_channels, self.sample_rate, final_samples);
                sink.append(buffer);
            }
        } else if let Some(data) = &self.raw_bytes {
            // 无缓存：IO Seek (回退方案)
            if let Ok(mut src) = Self::create_decoder(data) {
                let _ = src.try_seek(Duration::from_secs_f64(time));
                sink.append(src);
            }
        }
        
        if is_paused { sink.pause(); } else { sink.play(); }
    }

    fn set_volume(&mut self, vol: f32) {
        *self.current_volume.write().unwrap() = vol;
        if let Ok(s) = self.sink.lock() { s.set_volume(vol); }
    }

    fn set_channel_mode(&mut self, mode: u16) {
        let config = match mode {
            6 => ChannelConfig::Surround51,
            8 => ChannelConfig::Surround71,
            _ => ChannelConfig::Stereo,
        };
        *self.channel_mode.write().unwrap() = config;
    }
}