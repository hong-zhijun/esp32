//! Wireless WeChat-input-method voice bridge.
//!
//! Flow: board PCM (16 kHz mono i16 LE) → virtual audio output device
//! (BlackHole / VB-CABLE) → WeChat input method, which is started and stopped by
//! synthesizing its global voice shortcut.
use panel_core::config::Hotkey;
use serde_json::Value;
use std::sync::Arc;

mod abort;
mod audio_out;
mod bridge;
mod forward;
mod hotkey;
mod pcm;
mod ports;
mod reply;
mod resample;
mod session;
mod start;
mod stop;
mod watch;

#[cfg(test)]
mod tests;

pub use bridge::VoiceBridge;

/// Sends one JSON message back to the board, e.g.
/// `{"type":"voice","state":"partial"|"finished"|"error","text":"..."}`.
/// The `type` key must come first (serde_json's `json!` keeps insertion order
/// only with the `preserve_order` feature — build the string or enable it).
pub type Reply = Arc<dyn Fn(Value) + Send + Sync>;

/// Settings the bridge needs; updated live from the UI.
#[derive(Debug, Clone)]
pub struct VoiceSettings {
    /// Substring of the output device name (see `panel_core::config::VoiceConfig::device_name`).
    pub device: String,
    pub hotkey: Hotkey,
}

/// Status line for the UI.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub ready: bool,
    /// Short Chinese message, e.g. "CABLE Input 已就绪" or the error.
    pub message: String,
    /// Resolved device name when found.
    pub device: Option<String>,
    pub hotkey_label: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct KeyboardStatus {
    pub ready: bool,
    pub message: String,
}

/// Read-only permission check; never prompts or sends a key.
pub fn keyboard_status() -> KeyboardStatus {
    let ready = hotkey::keyboard_allowed();
    KeyboardStatus {
        ready,
        message: if ready { "键盘控制已就绪" } else if cfg!(target_os = "macos") {
            "macOS 尚未允许 AI 状态屏控制键盘，录音快捷键和回车暂不可用。请在辅助功能中添加 AI Status Panel.app 并打开开关。"
        } else { "此系统暂不支持键盘控制" }.into(),
    }
}

/// Checks virtual audio and keyboard access. Never sends a key or requests permission.
pub fn check(settings: &VoiceSettings) -> VoiceStatus {
    checked_status(
        audio_out::probe(&settings.device),
        settings.hotkey.label(),
        keyboard_status(),
    )
}

fn checked_status(
    audio: anyhow::Result<String>,
    hotkey_label: String,
    keyboard: KeyboardStatus,
) -> VoiceStatus {
    match audio {
        Ok(device) => VoiceStatus {
            ready: keyboard.ready,
            message: if keyboard.ready {
                format!("{device} 已就绪")
            } else {
                format!("{device} 已就绪；{}", keyboard.message)
            },
            device: Some(device),
            hotkey_label,
        },
        Err(error) => VoiceStatus {
            ready: false,
            message: error.to_string(),
            device: None,
            hotkey_label,
        },
    }
}

#[cfg(test)]
mod readiness_tests {
    use super::*;
    #[test]
    fn virtual_audio_alone_does_not_imply_keyboard_or_voice_ready() {
        let blocked = checked_status(
            Ok("BlackHole 2ch".into()),
            "Fn＋空格".into(),
            KeyboardStatus {
                ready: false,
                message: "需要辅助功能权限".into(),
            },
        );
        assert!(!blocked.ready);
        assert_eq!(blocked.device.as_deref(), Some("BlackHole 2ch"));
        assert!(blocked.message.contains("辅助功能"));
        let allowed = checked_status(
            Ok("BlackHole 2ch".into()),
            "Fn＋空格".into(),
            KeyboardStatus {
                ready: true,
                message: String::new(),
            },
        );
        assert!(allowed.ready);
    }
    #[test]
    fn audio_failure_remains_unready_with_keyboard_access() {
        let status = checked_status(
            Err(anyhow::anyhow!("设备不存在")),
            "Fn＋空格".into(),
            KeyboardStatus {
                ready: true,
                message: String::new(),
            },
        );
        assert!(!status.ready);
        assert!(status.device.is_none());
        assert_eq!(status.message, "设备不存在");
    }
}

/// Names of all output devices with ≥ 2 channels, for the UI dropdown.
pub fn list_output_devices() -> Vec<String> {
    audio_out::list_output_devices()
}

/// Sends the shortcut once (UI "test" button).
pub fn send_hotkey(hotkey: &Hotkey) -> Result<(), String> {
    hotkey::send(hotkey).map_err(|error| error.to_string())
}

/// The screen's return button sends one unmodified Enter to the focused application.
pub fn send_enter() -> Result<(), String> {
    send_hotkey(&Hotkey {
        key: "Enter".into(),
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
        fn_key: false,
    })
}
