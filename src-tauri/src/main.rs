#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod modules;

use std::sync::Mutex;
use audio::AudioManager;
use modules::state::AppState;
use modules::commands::*; 

use tauri::{Manager, Emitter, Listener}; 
use souvlaki::{MediaControlEvent, MediaControls, MediaPlayback, PlatformConfig};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use base64::{Engine as _, engine::general_purpose};

struct SmtcHandle {
    controls: Mutex<Option<MediaControls>>,
    hwnd_ptr: isize,
}

fn log_smtc(msg: &str) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    println!("[{}] {}", timestamp, msg);
}

#[cfg(target_os = "windows")]
fn sync_to_windows_smtc_native(hwnd_ptr: isize, title: &str, artist: &str, image_path: Option<String>) -> windows::core::Result<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
    use windows::Media::{SystemMediaTransportControls, MediaPlaybackType};
    use windows::core::HSTRING;
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::RandomAccessStreamReference;
    use std::path::Path;

    let hwnd = HWND(hwnd_ptr as *mut core::ffi::c_void);
    
    let interop: ISystemMediaTransportControlsInterop = windows::core::factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
    let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd) }?;
    
    smtc.SetIsEnabled(true)?;

    let updater = smtc.DisplayUpdater()?;
    updater.SetType(MediaPlaybackType::Music)?; 

    let music_props = updater.MusicProperties()?;
    music_props.SetTitle(&HSTRING::from(title))?;
    music_props.SetArtist(&HSTRING::from(artist))?;

    if let Some(raw_path) = image_path {
        if let Ok(path) = Path::new(&raw_path).canonicalize() {
            let path_str = path.to_str().unwrap_or_default().replace("\\\\?\\", "");
            let h_path = HSTRING::from(&path_str);
            
            if let Ok(file) = StorageFile::GetFileFromPathAsync(&h_path)?.get() {
                let stream_ref = RandomAccessStreamReference::CreateFromFile(&file)?;
                updater.SetThumbnail(&stream_ref)?;
                log_smtc(&format!("[NATIVE] Thumbnail set: {}", path_str));
            }
        }
    }

    updater.Update()?;
    Ok(())
}

#[tauri::command]
async fn toggle_smtc_active(handle: tauri::State<'_, SmtcHandle>, enable: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
        use windows::Media::SystemMediaTransportControls;

        let hwnd = HWND(handle.hwnd_ptr as *mut core::ffi::c_void);
        if let Ok(interop) = windows::core::factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>() {
            let smtc_result: windows::core::Result<SystemMediaTransportControls> = unsafe { interop.GetForWindow(hwnd) };
            if let Ok(smtc) = smtc_result {
                let _ = smtc.SetIsEnabled(enable);
                if !enable {
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
async fn sync_smtc_metadata(app: tauri::AppHandle, handle: tauri::State<'_, SmtcHandle>, title: String, artist: String, cover: String) -> Result<(), String> {
    log_smtc("---------- SMTC Metadata Sync ----------");
    
    {
        let mut controls_guard = handle.controls.lock().unwrap();
        if controls_guard.is_none() {
            log_smtc("[NATIVE] First track played. Lazy initializing SMTC controls...");
            let config = PlatformConfig { 
                dbus_name: "AstralGalaxy", 
                display_name: "Astral Galaxy Music", 
                hwnd: Some(handle.hwnd_ptr as *mut std::ffi::c_void) 
            };
            
            if let Ok(mut new_controls) = MediaControls::new(config) {
                let app_clone = app.clone();
                new_controls.attach(move |event| {
                    match event {
                        MediaControlEvent::Play | MediaControlEvent::Pause | MediaControlEvent::Toggle => { 
                            let _ = app_clone.emit("smtc-toggle", ()); 
                        },
                        MediaControlEvent::Next => { let _ = app_clone.emit("smtc-next", ()); },
                        MediaControlEvent::Previous => { let _ = app_clone.emit("smtc-prev", ()); },
                        _ => {}
                    }
                }).unwrap();
                
                *controls_guard = Some(new_controls);
                log_smtc("[NATIVE] SMTC initialized and hooked successfully.");
            } else {
                log_smtc("[ERROR] Failed to initialize SMTC controls.");
            }
        }
    }

    let mut extracted_path: Option<String> = None;

    if !cover.is_empty() && !cover.contains("DEFAULT_COVER") {
        if cover.starts_with("data:image/") {
            if let Some(base64_data) = cover.split(',').nth(1) {
                if let Ok(image_bytes) = general_purpose::STANDARD.decode(base64_data.trim()) {
                    let temp_dir = std::env::temp_dir();
                    let timestamp = chrono::Local::now().timestamp_micros();
                    let temp_path = temp_dir.join(format!("astral_cover_{}.jpg", timestamp));
                    
                    if std::fs::write(&temp_path, image_bytes).is_ok() {
                        extracted_path = Some(temp_path.to_string_lossy().to_string());
                    }
                }
            }
        } else {
            let clean = cover.replace("asset://localhost/", "").replace("file:///", "").replace("file://", "");
            let decoded = urlencoding::decode(&clean).unwrap_or(std::borrow::Cow::Borrowed(&clean)).to_string();
            let mut p = decoded.replace("/", "\\");
            if p.starts_with('\\') && p.chars().nth(2) == Some(':') { p = p[1..].to_string(); }
            extracted_path = Some(p);
        }
    }

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
    let mut controls_guard = handle.controls.lock().unwrap();
    if let Some(controls) = controls_guard.as_mut() {
        let playback = if is_playing { 
            MediaPlayback::Playing { progress: None } 
        } else { 
            MediaPlayback::Paused { progress: None } 
        };
        let _ = controls.set_playback(playback);
    }
    Ok(())
}

fn main() {
    log_smtc(">>> Astral Galaxy Music Player Backend Started <<<");
    
    let audio_tx = AudioManager::start_actor();
    let tx_monitor = audio_tx.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState { audio_tx })
        .setup(move |app| {
            let main_window = app.get_webview_window("main").unwrap();
            let app_handle = app.handle().clone();
            
            let hwnd_ptr = match main_window.window_handle().unwrap().as_raw() {
                RawWindowHandle::Win32(h) => h.hwnd.get() as isize,
                _ => 0,
            };

            let window_clone = main_window.clone();
            main_window.listen("webview-ready", move |_| {
                let _ = window_clone.show();
                let _ = window_clone.set_focus();
                println!("[NATIVE] Window securely shown and focused.");
            });

            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    
                    let (req_tx, req_rx) = tokio::sync::oneshot::channel();
                    if tx_monitor.send(audio::AudioCommand::CheckDeviceStatus(req_tx)).is_ok() {
                        if let Ok(Some(device)) = req_rx.blocking_recv() {
                            println!("[AUDIO] Hardware Topology Changed! Syncing to frontend for Safe Migration to: {}", device);
                            
                            let _ = app_handle.emit("force-pause", ());
                            std::thread::sleep(std::time::Duration::from_millis(250)); 
                            
                            let (set_tx, set_rx) = tokio::sync::oneshot::channel();
                            let _ = tx_monitor.send(audio::AudioCommand::SetDevice(device, set_tx));
                            let _ = set_rx.blocking_recv(); 
                            
                            let _ = app_handle.emit("force-play", ());
                        }
                    }
                }
            });

            app.manage(SmtcHandle { controls: Mutex::new(None), hwnd_ptr });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import_music, check_file_exists, init_audio_engine, 
            player_load_track, player_play, player_pause, player_seek, player_set_volume,
            player_set_channels, get_output_devices, set_output_device,
            get_lyrics, get_current_engine,
            sync_smtc_metadata, sync_smtc_status,
            toggle_smtc_active
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}