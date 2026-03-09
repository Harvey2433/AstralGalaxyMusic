#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod modules;

use std::sync::{Arc, Mutex};
use audio::AudioManager;
use modules::state::AppState;
use modules::commands::*; 

use tauri::{Manager, Emitter, Runtime}; 
use souvlaki::{MediaControlEvent, MediaControls, MediaPlayback, PlatformConfig};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use base64::{Engine as _, engine::general_purpose};

// 全局句柄，记录 HWND 以便原生同步 
struct SmtcHandle {
    controls: Mutex<MediaControls>,
    hwnd_ptr: isize,
}

// 统一日志输出 
fn log_smtc(msg: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    println!("[{}] {}", timestamp, msg);
}

// ============================================================
// 🔥 核心原生同步函数：解决封面不显示的一切罪魁祸首
// ============================================================
#[cfg(target_os = "windows")]
fn sync_to_windows_smtc_native(hwnd_ptr: isize, title: &str, artist: &str, image_path: Option<String>) -> windows::core::Result<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
    use windows::Media::{SystemMediaTransportControls, MediaPlaybackType};
    use windows::core::{Interface, HSTRING};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::RandomAccessStreamReference;
    use std::path::Path;

    let hwnd = HWND(hwnd_ptr as *mut core::ffi::c_void);
    
    // 1. 通过 Interop 接口获取该窗口对应的 SMTC 实例 
    let interop: ISystemMediaTransportControlsInterop = windows::core::factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
    let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd) }?;
    
    // 🔥 保证如果之前被禁用了，这次更新能把它强制叫醒
    smtc.SetIsEnabled(true)?;

    // 2. 获取 DisplayUpdater 并【必须】设置类型为 Music
    let updater = smtc.DisplayUpdater()?;
    updater.SetType(MediaPlaybackType::Music)?; 

    // 3. 设置文字信息 
    let music_props = updater.MusicProperties()?;
    music_props.SetTitle(&HSTRING::from(title))?;
    music_props.SetArtist(&HSTRING::from(artist))?;

    // 4. 处理封面图片（解决物理路径与中文编码） 
    if let Some(raw_path) = image_path {
        if let Ok(path) = Path::new(&raw_path).canonicalize() {
            // 去掉 Windows 的 UNC 前缀 \\?\ 提高兼容性 
            let path_str = path.to_str().unwrap_or_default().replace("\\\\?\\", "");
            let h_path = HSTRING::from(&path_str);
            
            // 使用同步阻塞获取文件 (get() 是关键) 
            if let Ok(file) = StorageFile::GetFileFromPathAsync(&h_path)?.get() {
                let stream_ref = RandomAccessStreamReference::CreateFromFile(&file)?;
                updater.SetThumbnail(&stream_ref)?;
                log_smtc(&format!("[NATIVE] Thumbnail set: {}", path_str));
            }
        }
    }

    // 5. 提交更新到系统面板 
    updater.Update()?;
    Ok(())
}

// ============================================================
// 🔥 新增：彻底控制 SMTC 消失/显示的核心指令
// ============================================================
#[tauri::command]
async fn toggle_smtc_active(handle: tauri::State<'_, SmtcHandle>, enable: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
        use windows::Media::SystemMediaTransportControls;
        use windows::core::Interface;

        let hwnd = HWND(handle.hwnd_ptr as *mut core::ffi::c_void);
        if let Ok(interop) = windows::core::factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>() {
            
            // 🔥 修复 E0282 报错：明确指定 `smtc` 为 `SystemMediaTransportControls` 类型
            let smtc_result: windows::core::Result<SystemMediaTransportControls> = unsafe { interop.GetForWindow(hwnd) };
            
            if let Ok(smtc) = smtc_result {
                // 强制切断或恢复钩子
                let _ = smtc.SetIsEnabled(enable);
                
                if !enable {
                    // 如果是禁用，顺手清理一下里面的缓存残留
                    if let Ok(updater) = smtc.DisplayUpdater() {
                        let _ = updater.ClearAll();
                        let _ = updater.Update();
                    }
                    log_smtc("[NATIVE] SMTC Hook completely disabled and hidden.");
                } else {
                    log_smtc("[NATIVE] SMTC Hook enabled.");
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn sync_smtc_metadata(handle: tauri::State<'_, SmtcHandle>, title: String, artist: String, cover: String) -> Result<(), String> {
    log_smtc("---------- SMTC Metadata Sync ----------");
    
    let mut extracted_path: Option<String> = None;

    // A. 处理封面落盘逻辑 
    if !cover.is_empty() && !cover.contains("DEFAULT_COVER") {
        if cover.starts_with("data:image/") {
            if let Some(base64_data) = cover.split(',').nth(1) {
                if let Ok(image_bytes) = general_purpose::STANDARD.decode(base64_data.trim()) {
                    let temp_dir = std::env::temp_dir();
                    // 加毫秒级时间戳防缓存 
                    let timestamp = chrono::Local::now().timestamp_micros();
                    let temp_path = temp_dir.join(format!("astral_cover_{}.jpg", timestamp));
                    
                    if std::fs::write(&temp_path, image_bytes).is_ok() {
                        extracted_path = Some(temp_path.to_string_lossy().to_string());
                    }
                }
            }
        } else {
            // 处理路径格式化 
            let clean = cover.replace("asset://localhost/", "").replace("file:///", "").replace("file://", "");
            let decoded = urlencoding::decode(&clean).unwrap_or(std::borrow::Cow::Borrowed(&clean)).to_string();
            let mut p = decoded.replace("/", "\\");
            if p.starts_with('\\') && p.chars().nth(2) == Some(':') { p = p[1..].to_string(); }
            extracted_path = Some(p);
        }
    }

    // B. 全量走原生调用，完全接管封面显示 
    #[cfg(target_os = "windows")]
    {
        if let Err(e) = sync_to_windows_smtc_native(handle.hwnd_ptr, &title, &artist, extracted_path) {
            log_smtc(&format!("[ERROR] Native SMTC failed: {:?}", e));
        } else {
            log_smtc("[SUCCESS] Native Metadata & Thumbnail Pushed.");
        }
    }

    Ok(())
}

#[tauri::command]
async fn sync_smtc_status(handle: tauri::State<'_, SmtcHandle>, is_playing: bool) -> Result<(), String> {
    // 播放状态还是可以用 souvlaki 的，因为它不涉及 DisplayUpdater 
    let mut controls = handle.controls.lock().unwrap();
    let playback = if is_playing { 
        MediaPlayback::Playing { progress: None } 
    } else { 
        MediaPlayback::Paused { progress: None } 
    };
    let _ = controls.set_playback(playback);
    Ok(())
}

fn main() {
    log_smtc(">>> Astral Galaxy Music Player Backend Started <<<");
    
    let audio_manager = Arc::new(Mutex::new(AudioManager::new()));
    
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState { audio_manager })
        .setup(|app| {
            let main_window = app.get_webview_window("main").unwrap();
            
            let hwnd_ptr = match main_window.window_handle().unwrap().as_raw() {
                RawWindowHandle::Win32(h) => h.hwnd.get() as isize,
                _ => 0,
            };

            // 配置 souvlaki (仅用于事件处理) 
            let config = PlatformConfig { 
                dbus_name: "AstralGalaxy", 
                display_name: "Astral Galaxy Music", 
                hwnd: Some(hwnd_ptr as *mut std::ffi::c_void) 
            };
            let mut controls = MediaControls::new(config).expect("Failed to create media controls");

            let app_handle = app.handle().clone();
            controls.attach(move |event| {
                match event {
                    MediaControlEvent::Play | MediaControlEvent::Pause | MediaControlEvent::Toggle => { 
                        let _ = app_handle.emit("smtc-toggle", ()); 
                    },
                    MediaControlEvent::Next => { let _ = app_handle.emit("smtc-next", ()); },
                    MediaControlEvent::Previous => { let _ = app_handle.emit("smtc-prev", ()); },
                    _ => {}
                }
            }).unwrap();

            app.manage(SmtcHandle { controls: Mutex::new(controls), hwnd_ptr });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import_music, check_file_exists, init_audio_engine, 
            player_load_track, player_play, player_pause, player_seek, player_set_volume,
            player_set_channels, get_output_devices, set_output_device,
            get_lyrics, get_current_engine,
            sync_smtc_metadata, sync_smtc_status,
            toggle_smtc_active // 🔥 新指令已注册
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}