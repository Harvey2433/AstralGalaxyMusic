use std::sync::{Arc, Mutex};
use crate::audio::AudioManager;

pub struct AppState {
    pub audio_manager: Arc<Mutex<AudioManager>>,
}