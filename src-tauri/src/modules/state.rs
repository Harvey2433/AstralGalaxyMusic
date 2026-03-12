use std::sync::mpsc::Sender;
use crate::audio::AudioCommand;

pub struct AppState {
    pub audio_tx: Sender<AudioCommand>,
}