use super::AudioEngine;
use std::process::Command;

pub struct FFmpegEngine;

impl FFmpegEngine {
    pub fn new() -> Self { Self {} }
    pub fn check_availability() -> bool {
        Command::new("ffmpeg").arg("-version").output().is_ok()
    }
}

impl AudioEngine for FFmpegEngine {
    fn name(&self) -> &str { "FFmpeg Stub" }
    fn load(&mut self, _path: &str) -> Result<f64, String> { Ok(0.0) }
    fn play(&mut self) {}
    fn pause(&mut self) {}
    fn seek(&mut self, _time: f64) {}
    fn set_volume(&mut self, _vol: f32) {}
}