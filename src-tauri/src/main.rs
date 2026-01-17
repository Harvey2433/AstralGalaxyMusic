#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio; 

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
// 🔥 修复点 1: 必须导入 Read trait 才能使用 read_to_end
use std::io::Read; 

use tauri::{State, Emitter, Window}; 
use lofty::{read_from_path, Accessor, TaggedFileExt, AudioFile}; 
use rfd::FileDialog;
use base64::{Engine as _, engine::general_purpose};
use rayon::prelude::*;
// 🔥 修复点 2: 显式导入 UTF_8 和 GBK
use encoding_rs::{GBK, UTF_8}; 
use audio::AudioManager; 

struct AppState {
    audio_manager: Arc<Mutex<AudioManager>>,
}

#[derive(serde::Serialize, Clone, Debug)]
struct TrackMetadata {
    path: String, title: String, artist: String, album: String, cover: String, duration: f64,
}

// 修复乱码函数
fn repair_mojibake(input: &str) -> String {
    if input.chars().any(|c| c as u32 > 0xFF) { return input.to_string(); }
    let bytes: Vec<u8> = input.chars().map(|c| c as u8).collect();
    let (decoded, _, had_errors) = GBK.decode(&bytes);
    if !had_errors { return decoded.into_owned(); }
    input.to_string()
}

// 提取元数据
fn extract_metadata(path: &PathBuf) -> TrackMetadata {
    let filename = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let mut meta = TrackMetadata {
        path: path.to_string_lossy().to_string(),
        title: filename.clone(), artist: "Unknown Artist".to_string(), album: "Unknown Album".to_string(), cover: "DEFAULT_COVER".to_string(), duration: 0.0,
    };
    if let Ok(tagged_file) = read_from_path(path) {
        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());
        let properties = tagged_file.properties();
        if let Some(t) = tag {
            if let Some(title) = t.title() { let trimmed = title.trim(); if !trimmed.is_empty() { meta.title = repair_mojibake(trimmed); } }
            if let Some(artist) = t.artist() { let trimmed = artist.trim(); if !trimmed.is_empty() { meta.artist = repair_mojibake(trimmed); } }
            if let Some(album) = t.album() { let trimmed = album.trim(); if !trimmed.is_empty() { meta.album = repair_mojibake(trimmed); } }
            let empty_tag = lofty::Tag::new(lofty::TagType::Id3v2);
            meta.cover = find_cover_image(path, tag.unwrap_or(&empty_tag));
        }
        meta.duration = properties.duration().as_secs_f64();
    }
    meta
}

// 查找封面
fn find_cover_image(file_path: &Path, tag: &lofty::Tag) -> String {
    if let Some(picture) = tag.pictures().first() {
        let base64_str = general_purpose::STANDARD.encode(picture.data());
        let mime = picture.mime_type().as_str(); 
        return format!("data:{};base64,{}", mime, base64_str);
    }
    if let Some(parent) = file_path.parent() {
        let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let exact_matches = vec![
            format!("{}.jpg", stem), format!("{}.png", stem), format!("{}.jpeg", stem)
        ];
        for name in &exact_matches {
            let img_path = parent.join(name);
            if img_path.exists() {
                if let Ok(bytes) = fs::read(img_path) {
                    let base64_str = general_purpose::STANDARD.encode(&bytes);
                    return format!("data:image/jpeg;base64,{}", base64_str);
                }
            }
        }
    }
    "DEFAULT_COVER".to_string()
}

// --- 命令区 ---

// 🔥 新增：获取歌词命令
#[tauri::command]
async fn get_lyrics(path: String) -> Result<String, String> {
    let audio_path = Path::new(&path);
    // 尝试找同名 .lrc 文件
    let lrc_path = audio_path.with_extension("lrc");

    if lrc_path.exists() {
        let mut file = fs::File::open(lrc_path).map_err(|e| e.to_string())?;
        let mut buffer = Vec::new();
        // 这里需要 use std::io::Read;
        file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;

        // 1. 尝试 UTF-8
        let (decoded, _, had_errors) = UTF_8.decode(&buffer);
        if !had_errors {
            return Ok(decoded.into_owned());
        }
        
        // 2. 如果 UTF-8 失败，尝试 GBK (兼容老歌词文件)
        let (decoded_gbk, _, _) = GBK.decode(&buffer);
        return Ok(decoded_gbk.into_owned());
    }

    // 没有歌词文件则返回空
    Ok("".to_string())
}

#[tauri::command]
async fn import_music(window: Window) -> Result<(), String> {
    let files = FileDialog::new().add_filter("Audio", &["mp3", "flac", "wav", "ogg", "m4a", "wma", "aac"]).set_directory("/").pick_files();
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
    }
    Ok(())
}

#[tauri::command]
fn check_file_exists(path: String) -> bool { Path::new(&path).exists() }

#[tauri::command]
fn init_audio_engine(state: State<AppState>, engine_id: String) -> Result<String, String> {
    state.audio_manager.lock().unwrap().switch_engine(&engine_id)
}

#[tauri::command]
async fn player_load_track(state: State<'_, AppState>, path: String) -> Result<f64, String> {
    if !Path::new(&path).exists() { return Err("FILE_NOT_FOUND".to_string()); }
    let manager = state.audio_manager.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<f64, String> {
        manager.lock().unwrap().load(&path)
    }).await.map_err(|e| e.to_string())?;
    result
}

#[tauri::command]
fn player_play(state: State<AppState>) { state.audio_manager.lock().unwrap().play(); }
#[tauri::command]
fn player_pause(state: State<AppState>) { state.audio_manager.lock().unwrap().pause(); }

#[tauri::command]
async fn player_seek(window: Window, state: State<'_, AppState>, time: f64) -> Result<(), String> {
    let manager = state.audio_manager.clone();
    let _ = window.emit("seek-start", ());
    let result = tauri::async_runtime::spawn_blocking(move || {
        manager.lock().unwrap().seek(time);
    }).await.map_err(|e| e.to_string());
    let _ = window.emit("seek-end", time);
    result
}

#[tauri::command]
fn player_set_volume(state: State<AppState>, vol: f32) { state.audio_manager.lock().unwrap().set_volume(vol); }

#[tauri::command]
fn player_set_channels(state: State<AppState>, mode: u16) { state.audio_manager.lock().unwrap().set_channels(mode); }

#[tauri::command]
fn get_output_devices(state: State<AppState>) -> Vec<String> { state.audio_manager.lock().unwrap().get_audio_devices() }

#[tauri::command]
fn set_output_device(state: State<AppState>, device: String) -> Result<String, String> { state.audio_manager.lock().unwrap().set_audio_device(&device) }

fn main() {
    let audio_manager = Arc::new(Mutex::new(AudioManager::new()));
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState { audio_manager })
        .invoke_handler(tauri::generate_handler![
            import_music, check_file_exists, init_audio_engine, 
            player_load_track, player_play, player_pause, player_seek, player_set_volume,
            player_set_channels, get_output_devices, set_output_device,
            get_lyrics // <--- 记得这里注册了新命令
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}