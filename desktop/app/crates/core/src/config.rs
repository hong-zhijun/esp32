//! Persisted settings. `ScreenConfig` is sent to the board verbatim as `config`,
//! so its JSON field names must stay identical to what the firmware reads.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenConfig {
    pub split: u8,
    pub brightness: u8,
    pub claude_color: String,
    pub codex_color: String,
    #[serde(default = "default_deepseek_color")]
    pub deepseek_color: String,
    pub show_tool: bool,
    pub show_elapsed: bool,
    pub swap: bool,
}

impl Default for ScreenConfig {
    fn default() -> Self {
        Self {
            split: 50,
            brightness: 160,
            claude_color: "#df9877".into(),
            codex_color: "#ffffff".into(),
            deepseek_color: default_deepseek_color(),
            show_tool: true,
            show_elapsed: true,
            swap: false,
        }
    }
}

impl ScreenConfig {
    /// Same limits as the Node version's `validateConfig`.
    pub fn validate(&self) -> Result<(), String> {
        if !(35..=65).contains(&self.split) {
            return Err("分栏比例需在 35–65 之间".into());
        }
        if self.brightness < 25 {
            return Err("亮度需在 25–255 之间".into());
        }
        for color in [&self.claude_color, &self.codex_color, &self.deepseek_color] {
            let ok = color.len() == 7
                && color.starts_with('#')
                && color[1..].chars().all(|c| c.is_ascii_hexdigit());
            if !ok {
                return Err("颜色格式应为 #RRGGBB".into());
            }
        }
        Ok(())
    }
}

fn default_deepseek_color() -> String {
    "#7c9fff".into()
}

/// A key combination for the WeChat input method's "start voice input" shortcut.
/// `key` uses normalized names: F1–F12, Space, A–Z, 0–9, Escape, Tab, Enter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hotkey {
    pub key: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    /// Command on macOS, Windows key on Windows.
    #[serde(default)]
    pub meta: bool,
    /// macOS Fn / Globe key. Ignored on Windows.
    #[serde(default)]
    pub fn_key: bool,
}

impl Hotkey {
    /// Matches the shortcuts verified with the Node version.
    pub fn platform_default() -> Self {
        if cfg!(target_os = "macos") {
            Self {
                key: "Space".into(),
                ctrl: false,
                alt: false,
                shift: false,
                meta: false,
                fn_key: true,
            }
        } else {
            Self {
                key: "F8".into(),
                ctrl: false,
                alt: false,
                shift: false,
                meta: false,
                fn_key: false,
            }
        }
    }

    /// Human readable label such as `Fn＋空格` or `Ctrl＋Shift＋F8`.
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.fn_key {
            parts.push("Fn");
        }
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push(if cfg!(target_os = "macos") {
                "Option"
            } else {
                "Alt"
            });
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.meta {
            parts.push(if cfg!(target_os = "macos") {
                "Command"
            } else {
                "Win"
            });
        }
        let key = if self.key == "Space" {
            "空格"
        } else {
            self.key.as_str()
        };
        parts.push(key);
        parts.join("＋")
    }

    pub fn validate(&self) -> Result<(), String> {
        let k = self.key.as_str();
        let ok = matches!(k, "Space" | "Escape" | "Tab" | "Enter")
            || (k.len() == 1
                && k.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
            || (k.starts_with('F')
                && k[1..]
                    .parse::<u8>()
                    .map(|n| (1..=12).contains(&n))
                    .unwrap_or(false));
        if ok {
            Ok(())
        } else {
            Err(format!("不支持的按键：{k}"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfig {
    /// Substring of the virtual output device name; None = platform default
    /// (`BlackHole 2ch` on macOS, `CABLE Input` on Windows).
    #[serde(default)]
    pub output_device: Option<String>,
    #[serde(default = "Hotkey::platform_default")]
    pub hotkey: Hotkey,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            output_device: None,
            hotkey: Hotkey::platform_default(),
        }
    }
}

impl VoiceConfig {
    pub fn device_name(&self) -> String {
        self.output_device
            .clone()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| default_output_device().into())
    }
}

pub fn default_output_device() -> &'static str {
    if cfg!(target_os = "macos") {
        "BlackHole 2ch"
    } else {
        "CABLE Input"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub screen: ScreenConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
}

impl AppConfig {
    /// Replaces an invalid section with its defaults. The sections are checked separately, so a
    /// bad hotkey does not discard the screen settings or the output device.
    pub fn sanitized(self) -> Self {
        let screen = if self.screen.validate().is_ok() {
            self.screen
        } else {
            ScreenConfig::default()
        };
        let hotkey = if self.voice.hotkey.validate().is_ok() {
            self.voice.hotkey
        } else {
            Hotkey::platform_default()
        };
        Self {
            screen,
            voice: VoiceConfig {
                output_device: self.voice.output_device,
                hotkey,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_json_matches_firmware_fields() {
        let v = serde_json::to_value(ScreenConfig::default()).unwrap();
        for key in [
            "split",
            "brightness",
            "claudeColor",
            "codexColor",
            "deepseekColor",
            "showTool",
            "showElapsed",
            "swap",
        ] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn upgrading_two_column_config_keeps_screen_and_voice_settings() {
        let mut previous = serde_json::to_value(AppConfig::default()).unwrap();
        previous["screen"]
            .as_object_mut()
            .unwrap()
            .remove("deepseekColor");
        previous["screen"]["brightness"] = serde_json::json!(90);
        previous["screen"]["split"] = serde_json::json!(55);
        previous["voice"]["hotkey"]["ctrl"] = serde_json::json!(true);
        let config: AppConfig = serde_json::from_value(previous).unwrap();
        let config = config.sanitized();
        assert_eq!(config.screen.brightness, 90);
        assert_eq!(config.screen.split, 55);
        assert_eq!(config.screen.deepseek_color, "#7c9fff");
        assert!(config.voice.hotkey.ctrl);
    }

    #[test]
    fn screen_validation_rejects_out_of_range_and_injection() {
        assert!(ScreenConfig::default().validate().is_ok());
        assert!(ScreenConfig {
            split: 80,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(ScreenConfig {
            brightness: 10,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(ScreenConfig {
            codex_color: "red;display:none".into(),
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn hotkey_labels_and_validation() {
        let h = Hotkey {
            key: "F8".into(),
            ctrl: true,
            alt: false,
            shift: true,
            meta: false,
            fn_key: false,
        };
        assert!(h.label().ends_with("F8"));
        assert!(h.validate().is_ok());
        assert!(Hotkey {
            key: "F13".into(),
            ..h.clone()
        }
        .validate()
        .is_err());
        assert!(Hotkey {
            key: "PageUp".into(),
            ..h
        }
        .validate()
        .is_err());
    }
}
