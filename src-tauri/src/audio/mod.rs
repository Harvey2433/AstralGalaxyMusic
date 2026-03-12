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
    pub active_engine: Box<dyn AudioEngine>,
    _stream: Option<StreamHolder>, 
    stream_handle: OutputStreamHandle,
    pub current_device_mode: String,
    pub last_resolved_default: String,
}

impl AudioManager {
    pub fn new() -> Self {
        let host = rodio::cpal::default_host();
        let default_name = host.default_output_device()
            .and_then(|d| d.name().ok())
            .unwrap_or_else(|| "Unknown".to_string());

        let (stream, stream_handle) = OutputStream::try_default().unwrap();
        let default_engine = galaxy::GalaxyEngine::new(stream_handle.clone());
        
        Self {
            active_engine: Box::new(default_engine),
            _stream: Some(StreamHolder(stream)),
            stream_handle,
            current_device_mode: "Default".to_string(),
            last_resolved_default: default_name,
        }
    }

    pub fn check_device_status(&mut self) -> Option<String> {
        let host = rodio::cpal::default_host();
        let mut device_exists = false;
        
        if let Ok(mut devices) = host.output_devices() {
            let target_name = if self.current_device_mode == "Default" {
                &self.last_resolved_default
            } else {
                &self.current_device_mode
            };
            device_exists = devices.any(|d| d.name().ok().as_ref() == Some(target_name));
        }
        
        if !device_exists {
            return Some("Default".to_string());
        }
        
        if self.current_device_mode == "Default" {
            if let Some(current_default) = host.default_output_device().and_then(|d| d.name().ok()) {
                if current_default != self.last_resolved_default {
                    return Some("Default".to_string());
                }
            }
        }
        
        None
    }

    pub fn check_and_recover_default_device(&mut self) {
        if self.current_device_mode == "Default" {
            let host = rodio::cpal::default_host();
            if let Some(current_default) = host.default_output_device().and_then(|d| d.name().ok()) {
                if current_default != self.last_resolved_default {
                    println!("[AUDIO] Default hardware changed: {} -> {}. Auto-recovering...", self.last_resolved_default, current_default);
                    self.last_resolved_default = current_default.clone();
                    
                    if let Ok((new_stream, new_handle)) = OutputStream::try_default() {
                        // 🔥 绝杀修复：必须先让引擎切断旧的 Sink 羁绊，才能销毁底层 Stream
                        self.active_engine.update_output_stream(new_handle.clone());
                        self._stream = Some(StreamHolder(new_stream));
                        self.stream_handle = new_handle;
                        println!("[AUDIO] Stream successfully migrated to new default device.");
                    }
                }
            }
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
        self.current_device_mode = device_name.to_string();

        if device_name == "Default" {
            let host = rodio::cpal::default_host();
            self.last_resolved_default = host.default_output_device()
                .and_then(|d| d.name().ok())
                .unwrap_or_else(|| "Unknown".to_string());

            let (stream, stream_handle) = OutputStream::try_default().map_err(|e| e.to_string())?;
            // 🔥 绝杀修复：先替换引擎中的 Sink（同步销毁旧 Sink），再丢弃旧的 OutputStream
            self.active_engine.update_output_stream(stream_handle.clone());
            self._stream = Some(StreamHolder(stream));
            self.stream_handle = stream_handle;
            return Ok("Switched to Default".to_string());
        }

        let host = rodio::cpal::default_host();
        let device = host.output_devices().map_err(|e| e.to_string())?
            .find(|d| d.name().map(|n| n == device_name).unwrap_or(false));

        if let Some(device) = device {
            match OutputStream::try_from_device(&device) {
                Ok((new_stream, new_handle)) => {
                    // 🔥 绝杀修复：严格保证 drop 顺序，杜绝 WASAPI 死锁
                    self.active_engine.update_output_stream(new_handle.clone());
                    self._stream = Some(StreamHolder(new_stream)); 
                    self.stream_handle = new_handle;
                    Ok(format!("Switched to {}", device_name))
                },
                Err(e) => Err(format!("Failed to init device: {}", e)),
            }
        } else {
            Err("Device not found".to_string())
        }
    }

    pub fn switch_engine(&mut self, engine_id: &str) -> Result<String, String> {
        self.check_and_recover_default_device();
        match engine_id {
            "galaxy" => {
                self.active_engine = Box::new(galaxy::GalaxyEngine::new(self.stream_handle.clone()));
                Ok("ENGINE_GALAXY_READY".to_string())
            }
            "ffmpeg" => {
                self.active_engine = Box::new(ffmpeg::FFmpegEngine::new(self.stream_handle.clone()));
                Ok("ENGINE_FFMPEG_READY".to_string())
            }
            _ => Err("UNKNOWN_ENGINE".to_string())
        }
    }

    pub fn load(&mut self, path: &str) -> Result<f64, String> { 
        self.check_and_recover_default_device();
        self.active_engine.load(path) 
    }
    pub fn play(&mut self) { 
        self.check_and_recover_default_device();
        self.active_engine.play() 
    }
    pub fn pause(&mut self) { self.active_engine.pause() }
    pub fn seek(&mut self, time: f64) { 
        self.check_and_recover_default_device();
        self.active_engine.seek(time) 
    }
    pub fn set_volume(&mut self, vol: f32) { self.active_engine.set_volume(vol) }
    pub fn set_channels(&mut self, mode: u16) { self.active_engine.set_channel_mode(mode); }
}