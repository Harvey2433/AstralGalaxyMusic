use tauri::{State, Window, Emitter, Manager};
use std::path::Path;
use rfd::FileDialog;
use rayon::prelude::*;
use crate::audio::ffmpeg::FFmpegEngine;
use super::state::AppState;
use super::utils::{extract_metadata, parse_lyrics_file};

#[tauri::command]
pub async fn get_lyrics(path: String) -> Result<String, String> {
    parse_lyrics_file(path)
}

#[tauri::command]
pub async fn import_music(window: Window) -> Result<(), String> {
    // 🔥 加上 set_parent(&window) 使其成为原生级别的模态窗口，彻底冻结并阻塞主程序！
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
        // 如果取消了选择，发送 cancel 解除前端的锁
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
            state.audio_manager.lock().unwrap().switch_engine(&engine_id)?;
            return Ok("ENGINE_FFMPEG_READY".to_string());
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
    state.audio_manager.lock().unwrap().switch_engine(&engine_id)
}

#[tauri::command]
pub async fn player_load_track(state: State<'_, AppState>, path: String) -> Result<f64, String> {
    if !Path::new(&path).exists() { return Err("FILE_NOT_FOUND".to_string()); }
    let manager = state.audio_manager.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<f64, String> {
        manager.lock().unwrap().load(&path)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn player_play(state: State<AppState>) { state.audio_manager.lock().unwrap().play(); }
#[tauri::command]
pub fn player_pause(state: State<AppState>) { state.audio_manager.lock().unwrap().pause(); }

#[tauri::command]
pub async fn player_seek(window: Window, state: State<'_, AppState>, time: f64) -> Result<(), String> {
    let manager = state.audio_manager.clone();
    let _ = window.emit("seek-start", ());
    let result = tauri::async_runtime::spawn_blocking(move || {
        manager.lock().unwrap().seek(time);
    }).await.map_err(|e| e.to_string());
    let _ = window.emit("seek-end", time);
    result
}

#[tauri::command]
pub fn player_set_volume(state: State<AppState>, vol: f32) { state.audio_manager.lock().unwrap().set_volume(vol); }
#[tauri::command]
pub fn player_set_channels(state: State<AppState>, mode: u16) { state.audio_manager.lock().unwrap().set_channels(mode); }
#[tauri::command]
pub fn get_output_devices(state: State<AppState>) -> Vec<String> { state.audio_manager.lock().unwrap().get_audio_devices() }
#[tauri::command]
pub fn set_output_device(state: State<AppState>, device: String) -> Result<String, String> { state.audio_manager.lock().unwrap().set_audio_device(&device) }
#[tauri::command]
pub fn get_current_engine(state: State<AppState>) -> String {
    let name = state.audio_manager.lock().unwrap().active_engine.name().to_string();
    if name.contains("FFmpeg") { "ffmpeg".to_string() } else { "galaxy".to_string() }
}