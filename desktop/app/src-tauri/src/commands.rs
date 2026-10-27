//! Tauri commands called by the UI. Blocking work runs on `spawn_blocking`.
use crate::device_server::DeviceInfo;
use crate::net;
use crate::provision::{self, PortInfo, ProvisionRequest};
use crate::runtime::{AppState, UiSnapshot};
use panel_core::config::{Hotkey, ScreenConfig, VoiceConfig};
use panel_core::debug_log::now_ms;
use panel_core::hooks_install::{self, HooksStatus};
use panel_core::paths;
use panel_voice::VoiceStatus;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{async_runtime, AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

type Shared<'a> = State<'a, Arc<AppState>>;

const SSID_MAX_BYTES: usize = 32;
const VOICE_OFF: &str = "语音功能已关闭（开发模式）";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksView {
    pub path: String,
    #[serde(flatten)]
    pub status: HooksStatus,
}

#[tauri::command]
pub async fn get_snapshot(state: Shared<'_>) -> Result<UiSnapshot, String> {
    Ok(state.refresh(now_ms() as i64, false).ui)
}

#[tauri::command]
pub async fn save_screen_config(state: Shared<'_>, config: ScreenConfig) -> Result<(), String> {
    state.save_screen(config)
}

#[tauri::command]
pub async fn select_session(state: Shared<'_>, provider: String, id: String) -> Result<(), String> {
    state.select(&provider, &id)
}

#[tauri::command]
pub async fn ack_session(state: Shared<'_>, provider: String, id: String) -> Result<(), String> {
    state.acknowledge(&provider, &id)
}

#[tauri::command]
pub async fn list_serial_ports() -> Result<Vec<PortInfo>, String> {
    async_runtime::spawn_blocking(provision::list_ports)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_addresses() -> Vec<String> {
    net::lan_ipv4().iter().map(ToString::to_string).collect()
}

#[tauri::command]
pub async fn provision(state: Shared<'_>, request: ProvisionRequest) -> Result<(), String> {
    let _busy = state.begin_provisioning()?;
    let ports = async_runtime::spawn_blocking(provision::list_ports)
        .await
        .map_err(|error| error.to_string())?;
    provision::validate(&request, &net::lan_ipv4(), &ports)?;
    let target = state.provision_target();
    async_runtime::spawn_blocking(move || provision::provision(&request, &target))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn voice_status(state: Shared<'_>) -> Result<VoiceStatus, String> {
    if state.voice.is_none() {
        return Ok(VoiceStatus {
            ready: false,
            message: VOICE_OFF.into(),
            device: None,
            hotkey_label: state.config().voice.hotkey.label(),
        });
    }
    let settings = state.voice_settings();
    async_runtime::spawn_blocking(move || panel_voice::check(&settings))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_audio_devices(state: Shared<'_>) -> Result<Vec<String>, String> {
    if state.voice.is_none() {
        return Ok(Vec::new());
    }
    async_runtime::spawn_blocking(panel_voice::list_output_devices)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_voice_config(state: Shared<'_>, config: VoiceConfig) -> Result<(), String> {
    state.save_voice(config)
}

#[tauri::command]
pub async fn test_hotkey(state: Shared<'_>, hotkey: Hotkey) -> Result<(), String> {
    hotkey.validate()?;
    if state.voice.is_none() {
        return Err(VOICE_OFF.into());
    }
    async_runtime::spawn_blocking(move || panel_voice::send_hotkey(&hotkey))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn hooks_status() -> Result<HooksView, String> {
    let exe = current_exe()?;
    let path = paths::claude_settings_file();
    async_runtime::spawn_blocking(move || HooksView {
        path: path.display().to_string(),
        status: hooks_install::status(&path, &exe),
    })
    .await
    .map_err(|error| error.to_string())
}

/// Returns the backup file written before the settings were changed, if any.
#[tauri::command]
pub async fn install_hooks() -> Result<Option<String>, String> {
    let exe = current_exe()?;
    let path = paths::claude_settings_file();
    async_runtime::spawn_blocking(move || {
        hooks_install::install(&path, &exe)
            .map(|backup| backup.map(|file| file.display().to_string()))
            .map_err(|error| format!("{error:#}"))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_debug_log(state: Shared<'_>) -> Result<Vec<Value>, String> {
    Ok(state.log.entries())
}

#[tauri::command]
pub async fn device_info(state: Shared<'_>) -> Result<DeviceInfo, String> {
    Ok(state.devices.info())
}

#[tauri::command]
pub async fn forget_wifi(state: Shared<'_>, ssid: String) -> Result<(), String> {
    if ssid.is_empty() || ssid.len() > SSID_MAX_BYTES {
        return Err("SSID 不正确".into());
    }
    state.devices.forget(&ssid);
    Ok(())
}

#[tauri::command]
pub async fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|error| error.to_string())
}

fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| format!("无法定位程序路径：{error}"))
}

/// Opens the pane; the user must explicitly grant access in macOS.
#[tauri::command]
pub async fn open_keyboard_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        async_runtime::spawn_blocking(|| {
            let status = std::process::Command::new("/usr/bin/open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )
                .status()
                .map_err(|error| error.to_string())?;
            if status.success() {
                Ok(())
            } else {
                Err("无法打开系统设置".into())
            }
        })
        .await
        .map_err(|error| error.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("请在系统设置中检查键盘控制权限".into())
    }
}
