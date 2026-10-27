//! Application shell: Tauri builder, plugins, tray icon, window lifecycle and startup.
use crate::commands;
use crate::device_server::{self, DeviceHub};
use crate::discovery;
use crate::runtime::{self, AppState};
use panel_core::debug_log::DebugLog;
use panel_core::paths::DataPaths;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

const DEFAULT_PORT: u16 = 8787;
/// Passed by the autostart entry so the app starts in the tray instead of showing the window.
const AUTOSTART_FLAG: &str = "--autostart";

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_FLAG]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::save_screen_config,
            commands::select_session,
            commands::ack_session,
            commands::list_serial_ports,
            commands::list_addresses,
            commands::provision,
            commands::voice_status,
            commands::list_audio_devices,
            commands::save_voice_config,
            commands::test_hotkey,
            commands::hooks_status,
            commands::install_hooks,
            commands::get_debug_log,
            commands::device_info,
            commands::forget_wifi,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .setup(setup)
        .on_window_event(|window, event| {
            // Closing the window hides it; the panel keeps running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run the application");
}

fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let log = DebugLog::new();
    let emitter = handle.clone();
    log.subscribe(move |entry| {
        let _ = emitter.emit("debug", entry);
    });
    let port = server_port();
    let state = Arc::new(AppState::new(
        DataPaths::default_location(),
        port,
        DeviceHub::new(handle.clone()),
        log.clone(),
        voice_enabled(),
    )?);
    app.manage(Arc::clone(&state));
    create_tray(app)?;
    tauri::async_runtime::spawn(device_server::run(Arc::clone(&state)));
    discovery::spawn(state.identity.computer_id.clone(), port, log);
    runtime::spawn_ticker(handle.clone(), state);
    if !std::env::args().any(|arg| arg == AUTOSTART_FLAG) {
        show_main_window(&handle);
    }
    Ok(())
}

/// Development switch: `AI_PANEL_NO_VOICE=1` skips the voice bridge and every call into it.
fn voice_enabled() -> bool {
    std::env::var_os("AI_PANEL_NO_VOICE").is_none()
}

/// Development override for the device port (default 8787).
fn server_port() -> u16 {
    std::env::var("AI_PANEL_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

fn create_tray(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开面板", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::new()
        .tooltip("AI 状态屏")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
