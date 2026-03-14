use tauri::{State, Window, Emitter, Manager};
use std::path::Path;
use rfd::FileDialog;
use rayon::prelude::*;
use crate::audio::ffmpeg::FFmpegEngine;
use crate::audio::AudioCommand; 
use super::state::AppState;
use super::utils::{extract_metadata, parse_lyrics_file};
use tokio::sync::oneshot;

#[tauri::command]
pub async fn get_lyrics(path: String) -> Result<String, String> {
    parse_lyrics_file(path)
}

#[tauri::command]
pub async fn import_music(window: Window) -> Result<(), String> {
    let files = FileDialog::new()
        .add_filter("Audio", &["mp3", "flac", "wav", "ogg", "m4a", "wma", "aac"])
        .set_directory("/")
        .set_parent(&window)
        .pick_files();
        
    if let Some(paths) = files {
        let total = paths.len();
        let _ = window.emit("import-start", total);
        tauri::async_runtime::spawn_blocking(move || {
            paths.par_iter().for_each(|path| {
                let track = extract_metadata(path);
                let _ = window.emit("import-track", track);
            });
            let _ = window.emit("import-finish", ());
        });
    } else {
        let _ = window.emit("import-cancel", ());
    }
    Ok(())
}

#[tauri::command]
pub fn check_file_exists(path: String) -> bool { Path::new(&path).exists() }

#[tauri::command]
pub async fn init_audio_engine(window: Window, state: State<'_, AppState>, engine_id: String) -> Result<String, String> {
    if engine_id == "ffmpeg" {
        let available = FFmpegEngine::check_availability(window.app_handle());
        if available {
            let (tx, rx) = oneshot::channel();
            state.audio_tx.send(AudioCommand::SwitchEngine(engine_id.clone(), tx)).map_err(|e| e.to_string())?;
            return rx.await.map_err(|e| e.to_string())?;
        } else {
            let win_clone = window.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = FFmpegEngine::download_and_install(win_clone.clone()).await {
                    println!("FFmpeg install failed: {}", e);
                    let _ = win_clone.emit("ffmpeg-status", "error");
                }
            });
            return Ok("DOWNLOADING".to_string());
        }
    }
    let (tx, rx) = oneshot::channel();
    state.audio_tx.send(AudioCommand::SwitchEngine(engine_id, tx)).map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn player_load_track(state: State<'_, AppState>, path: String) -> Result<f64, String> {
    if !Path::new(&path).exists() { return Err("FILE_NOT_FOUND".to_string()); }
    let (tx, rx) = oneshot::channel();
    state.audio_tx.send(AudioCommand::Load(path, tx)).map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn player_play(state: State<AppState>) { let _ = state.audio_tx.send(AudioCommand::Play); }
#[tauri::command]
pub fn player_pause(state: State<AppState>) { let _ = state.audio_tx.send(AudioCommand::Pause); }

#[tauri::command]
pub async fn player_seek(window: Window, state: State<'_, AppState>, time: f64) -> Result<(), String> {
    let _ = window.emit("seek-start", ());
    let (tx, rx) = oneshot::channel();
    state.audio_tx.send(AudioCommand::Seek(time, tx)).map_err(|e| e.to_string())?;
    let _ = rx.await;
    let _ = window.emit("seek-end", time);
    Ok(())
}

#[tauri::command]
pub fn player_set_volume(state: State<AppState>, vol: f32) { let _ = state.audio_tx.send(AudioCommand::SetVolume(vol)); }
#[tauri::command]
pub fn player_set_channels(state: State<AppState>, mode: u16) { let _ = state.audio_tx.send(AudioCommand::SetChannels(mode)); }

#[tauri::command]
pub async fn get_output_devices(state: State<'_, AppState>) -> Result<Vec<String>, String> { 
    let (tx, rx) = oneshot::channel();
    state.audio_tx.send(AudioCommand::GetDevices(tx)).map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_output_device(state: State<'_, AppState>, device: String) -> Result<String, String> { 
    let (tx, rx) = oneshot::channel();
    state.audio_tx.send(AudioCommand::SetDevice(device, tx)).map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_current_engine(state: State<'_, AppState>) -> Result<String, String> {
    let (tx, rx) = oneshot::channel();
    state.audio_tx.send(AudioCommand::GetCurrentEngine(tx)).map_err(|e| e.to_string())?;
    let name = rx.await.map_err(|e| e.to_string())?;
    if name.contains("FFmpeg") { Ok("ffmpeg".to_string()) } else { Ok("galaxy".to_string()) }
}

#[tauri::command]
pub async fn get_current_time(state: State<'_, AppState>) -> Result<f64, String> {
    let (tx, rx) = oneshot::channel();
    state.audio_tx.send(AudioCommand::GetCurrentTime(tx)).map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())
}