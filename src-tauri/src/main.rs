#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio; 

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
// 🔥 修复点 1: 必须导入 Read trait 才能使用 read_to_end
use std::io::Read; 

// 🔥 修改点: 引入 Manager 以获取 app_handle
use tauri::{State, Emitter, Window, Manager}; 
use lofty::{read_from_path, Accessor, TaggedFileExt, AudioFile}; 
use rfd::FileDialog;
use base64::{Engine as _, engine::general_purpose};
use rayon::prelude::*;
// 🔥 修复点 2: 显式导入 UTF_8 和 GBK
use encoding_rs::{GBK, UTF_8}; 
use audio::AudioManager; 
// 🔥 新增: 引入 FFmpeg 引擎用于静态检测
use audio::ffmpeg::FFmpegEngine;

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

// 🔥 修改核心: 初始化引擎逻辑，支持 FFmpeg 自动下载
#[tauri::command]
async fn init_audio_engine(window: Window, state: State<'_, AppState>, engine_id: String) -> Result<String, String> {
    if engine_id == "ffmpeg" {
        // 1. 检查环境变量及本地安装
        let available = FFmpegEngine::check_availability(window.app_handle());
        
        if available {
            // 可用则直接切换
            state.audio_manager.lock().unwrap().switch_engine(&engine_id)?;
            return Ok("ENGINE_FFMPEG_READY".to_string());
        } else {
            // 2. 不可用，启动后台下载任务
            let win_clone = window.clone();
            tauri::async_runtime::spawn(async move {
                // 执行下载与解压逻辑
                if let Err(e) = FFmpegEngine::download_and_install(win_clone.clone()).await {
                    println!("FFmpeg auto-install failed: {}", e);
                    // 发送错误状态给前端
                    let _ = win_clone.emit("ffmpeg-status", "error");
                }
            });
            
            // 返回下载中状态，前端据此显示加载/进度UI
            return Ok("DOWNLOADING".to_string());
        }
    }
    
    // 其他引擎（如 Galaxy）直接切换
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

// 🔥 新增辅助命令：同步前端状态用
#[tauri::command]
fn get_current_engine(state: State<AppState>) -> String {
    // 这里简单返回 active_engine 的名称，实际生产中可以在 AudioManager 里存一个 enum 状态
    // 为了简化，这里我们根据 name 判断，或者你可以让 AudioManager 增加一个 get_engine_id 方法
    // 这里暂时假设如果不是 FFmpeg 就默认是 Galaxy
    let name = state.audio_manager.lock().unwrap().active_engine.name().to_string();
    if name.contains("FFmpeg") { "ffmpeg".to_string() } else { "galaxy".to_string() }
}

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
            get_lyrics, get_current_engine // <--- 注册了新命令
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}