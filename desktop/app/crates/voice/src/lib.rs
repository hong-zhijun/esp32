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

/// Checks that the configured output device exists and can open. Never sends a key.
pub fn check(settings: &VoiceSettings) -> VoiceStatus {
    let hotkey_label = settings.hotkey.label();
    match audio_out::probe(&settings.device) {
        Ok(device) => VoiceStatus {
            ready: true,
            message: format!("{device} 已就绪"),
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

/// Names of all output devices with ≥ 2 channels, for the UI dropdown.
pub fn list_output_devices() -> Vec<String> {
    audio_out::list_output_devices()
}

/// Sends the shortcut once (UI "test" button).
pub fn send_hotkey(hotkey: &Hotkey) -> Result<(), String> {
    hotkey::send(hotkey).map_err(|error| error.to_string())
}
