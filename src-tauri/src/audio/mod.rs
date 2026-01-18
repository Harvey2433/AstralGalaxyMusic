// src/audio/mod.rs

pub mod galaxy;
pub mod ffmpeg;

use rodio::{OutputStream, OutputStreamHandle};
use rodio::cpal::traits::{HostTrait, DeviceTrait};

// Wrapper 强制实现 Send/Sync
struct StreamHolder(OutputStream);
unsafe impl Send for StreamHolder {}
unsafe impl Sync for StreamHolder {}

pub trait AudioEngine: Send + Sync {
    fn load(&mut self, path: &str) -> Result<f64, String>;
    fn play(&mut self);
    fn pause(&mut self);
    fn seek(&mut self, time: f64);
    fn set_volume(&mut self, vol: f32);
    fn name(&self) -> &str;
    fn set_channel_mode(&mut self, _mode: u16) {}
    fn update_output_stream(&mut self, _handle: OutputStreamHandle) {} 
}

pub struct AudioManager {
    // 🔥 修改: 设为 pub 以便 main.rs 获取当前引擎状态
    pub active_engine: Box<dyn AudioEngine>,
    _stream: Option<StreamHolder>, 
    stream_handle: OutputStreamHandle,
}

impl AudioManager {
    pub fn new() -> Self {
        let (stream, stream_handle) = OutputStream::try_default().unwrap();
        // 默认使用 Galaxy 引擎
        let default_engine = galaxy::GalaxyEngine::new(stream_handle.clone());
        Self {
            active_engine: Box::new(default_engine),
            _stream: Some(StreamHolder(stream)),
            stream_handle,
        }
    }

    pub fn get_audio_devices(&self) -> Vec<String> {
        let host = rodio::cpal::default_host();
        match host.output_devices() {
            Ok(devices) => devices.filter_map(|d| d.name().ok()).collect(),
            Err(_) => vec!["Default Device".to_string()],
        }
    }

    pub fn set_audio_device(&mut self, device_name: &str) -> Result<String, String> {
        if device_name == "Default" {
            let (stream, stream_handle) = OutputStream::try_default().map_err(|e| e.to_string())?;
            self._stream = Some(StreamHolder(stream));
            self.stream_handle = stream_handle.clone();
            self.active_engine.update_output_stream(stream_handle);
            return Ok("Switched to Default".to_string());
        }

        let host = rodio::cpal::default_host();
        let device = host.output_devices().map_err(|e| e.to_string())?
            .find(|d| d.name().map(|n| n == device_name).unwrap_or(false));

        if let Some(device) = device {
            match OutputStream::try_from_device(&device) {
                Ok((new_stream, new_handle)) => {
                    self._stream = Some(StreamHolder(new_stream)); 
                    self.stream_handle = new_handle.clone();
                    self.active_engine.update_output_stream(new_handle);
                    Ok(format!("Switched to {}", device_name))
                },
                Err(e) => Err(format!("Failed to init device: {}", e)),
            }
        } else {
            Err("Device not found".to_string())
        }
    }

    pub fn switch_engine(&mut self, engine_id: &str) -> Result<String, String> {
        match engine_id {
            "galaxy" => {
                self.active_engine = Box::new(galaxy::GalaxyEngine::new(self.stream_handle.clone()));
                Ok("ENGINE_GALAXY_READY".to_string())
            }
            "ffmpeg" => {
                // 🔥 修改: 这里的 check_availability 已移除
                // 因为检测逻辑现在依赖 AppHandle (查找本地文件)，已移动到 main.rs 的 init_audio_engine 中处理
                // 当代码执行到这里时，我们假设 main.rs 已经完成了环境校验或下载解压
                self.active_engine = Box::new(ffmpeg::FFmpegEngine::new(self.stream_handle.clone()));
                Ok("ENGINE_FFMPEG_READY".to_string())
            }
            _ => Err("UNKNOWN_ENGINE".to_string())
        }
    }

    pub fn load(&mut self, path: &str) -> Result<f64, String> { self.active_engine.load(path) }
    pub fn play(&mut self) { self.active_engine.play() }
    pub fn pause(&mut self) { self.active_engine.pause() }
    pub fn seek(&mut self, time: f64) { self.active_engine.seek(time) }
    pub fn set_volume(&mut self, vol: f32) { self.active_engine.set_volume(vol) }
    pub fn set_channels(&mut self, mode: u16) { self.active_engine.set_channel_mode(mode); }
}