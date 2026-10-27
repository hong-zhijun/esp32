//! Presses the WeChat voice shortcut on this computer's keyboard.
//! Windows: `SendInput`. macOS: `CGEvent`, with Fn posted the way `bridge/wetype-key.c` does.

use crate::ports::Trigger;
use crate::VoiceSettings;
use anyhow::Result;
use panel_core::config::Hotkey;
use parking_lot::Mutex;
use std::sync::Arc;

/// Presses the shortcut configured in the shared settings, so a settings change applies immediately.
pub(crate) struct SystemTrigger {
    settings: Arc<Mutex<VoiceSettings>>,
}

impl SystemTrigger {
    pub(crate) fn new(settings: Arc<Mutex<VoiceSettings>>) -> Self {
        Self { settings }
    }
}

impl Trigger for SystemTrigger {
    fn press(&self) -> Result<()> {
        let hotkey = self.settings.lock().hotkey.clone();
        send(&hotkey)
    }
}

/// Validates `hotkey` and sends it once.
pub(crate) fn send(hotkey: &Hotkey) -> Result<()> {
    hotkey.validate().map_err(anyhow::Error::msg)?;
    platform::send(hotkey)
}

#[cfg(windows)]
mod platform {
    use anyhow::{bail, Result};
    use panel_core::config::Hotkey;
    use std::mem::size_of;
    use std::thread;
    use std::time::Duration;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT,
    };

    const KEY_HOLD: Duration = Duration::from_millis(40);

    /// Modifiers down, key down, hold, key up, modifiers up. Fn has no Windows
    /// equivalent and is ignored.
    pub(super) fn send(hotkey: &Hotkey) -> Result<()> {
        let key = key_code(&hotkey.key)?;
        let modifiers = modifier_codes(hotkey);
        inject(
            &modifiers
                .iter()
                .map(|&vk| key_input(vk, false))
                .collect::<Vec<_>>(),
        )?;
        let pressed = inject(&[key_input(key, false)]).and_then(|()| {
            thread::sleep(KEY_HOLD);
            inject(&[key_input(key, true)])
        });
        // Release modifiers even if the key press failed, so none is left stuck down.
        let released = inject(
            &modifiers
                .iter()
                .rev()
                .map(|&vk| key_input(vk, true))
                .collect::<Vec<_>>(),
        );
        pressed.and(released)
    }

    fn inject(inputs: &[INPUT]) -> Result<()> {
        let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            bail!("发送快捷键失败，系统拒绝了键盘输入")
        }
    }

    fn key_input(vk: u16, up: bool) -> INPUT {
        let flags = if up {
            KEYEVENTF_KEYUP
        } else {
            KEYBD_EVENT_FLAGS(0)
        };
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn modifier_codes(hotkey: &Hotkey) -> Vec<u16> {
        [
            (hotkey.ctrl, VK_CONTROL),
            (hotkey.alt, VK_MENU),
            (hotkey.shift, VK_SHIFT),
            (hotkey.meta, VK_LWIN),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, vk)| vk.0)
        .collect()
    }

    /// Virtual-key codes. Letters and digits share their ASCII codes.
    fn key_code(name: &str) -> Result<u16> {
        let fixed = match name {
            "Space" => Some(0x20),
            "Escape" => Some(0x1B),
            "Tab" => Some(0x09),
            "Enter" => Some(0x0D),
            _ => None,
        };
        if let Some(code) = fixed {
            return Ok(code);
        }
        if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
            if (1..=12).contains(&n) {
                return Ok(0x70 + n - 1);
            }
        }
        match name.as_bytes() {
            [c] if c.is_ascii_uppercase() || c.is_ascii_digit() => Ok(u16::from(*c)),
            _ => bail!("不支持的按键：{name}"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn maps_normalized_names_to_virtual_keys() {
            assert_eq!(key_code("F8").unwrap(), 0x77);
            assert_eq!(key_code("F1").unwrap(), 0x70);
            assert_eq!(key_code("F12").unwrap(), 0x7B);
            assert_eq!(key_code("Space").unwrap(), 0x20);
            assert_eq!(key_code("A").unwrap(), 0x41);
            assert_eq!(key_code("Z").unwrap(), 0x5A);
            assert_eq!(key_code("0").unwrap(), 0x30);
            assert!(key_code("F13").is_err());
            assert!(key_code("a").is_err());
            assert!(key_code("PageUp").is_err());
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use anyhow::{anyhow, bail, Result};
    use core_graphics::event::{
        CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, KeyCode,
    };
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use panel_core::config::Hotkey;
    use std::thread;
    use std::time::Duration;

    /// Same timing as bridge/wetype-key.c: Fn leads the key by 20 ms, the key is held 40 ms.
    const FN_LEAD: Duration = Duration::from_millis(20);
    const KEY_HOLD: Duration = Duration::from_millis(40);

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    pub(super) fn send(hotkey: &Hotkey) -> Result<()> {
        if unsafe { AXIsProcessTrusted() } == 0 {
            bail!("请在 系统设置 → 隐私与安全性 → 辅助功能 中允许本程序控制键盘");
        }
        let key = key_code(&hotkey.key)?;
        let flags = modifier_flags(hotkey);
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|()| anyhow!("无法创建键盘事件源"))?;
        if hotkey.fn_key {
            post(&source, KeyCode::FUNCTION, true, flags, true)?;
            thread::sleep(FN_LEAD);
        }
        post(&source, key, true, flags, false)?;
        thread::sleep(KEY_HOLD);
        post(&source, key, false, flags, false)?;
        if hotkey.fn_key {
            post(
                &source,
                KeyCode::FUNCTION,
                false,
                CGEventFlags::CGEventFlagNull,
                true,
            )?;
        }
        Ok(())
    }

    /// `flags_changed` posts a modifier event (Fn), which is what wetype-key.c does for keycode 63.
    fn post(
        source: &CGEventSource,
        code: CGKeyCode,
        down: bool,
        flags: CGEventFlags,
        flags_changed: bool,
    ) -> Result<()> {
        let event = CGEvent::new_keyboard_event(source.clone(), code, down)
            .map_err(|()| anyhow!("无法创建键盘事件"))?;
        if flags_changed {
            event.set_type(CGEventType::FlagsChanged);
        }
        event.set_flags(flags);
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn modifier_flags(hotkey: &Hotkey) -> CGEventFlags {
        let mut flags = CGEventFlags::CGEventFlagNull;
        for (on, flag) in [
            (hotkey.fn_key, CGEventFlags::CGEventFlagSecondaryFn),
            (hotkey.ctrl, CGEventFlags::CGEventFlagControl),
            (hotkey.alt, CGEventFlags::CGEventFlagAlternate),
            (hotkey.shift, CGEventFlags::CGEventFlagShift),
            (hotkey.meta, CGEventFlags::CGEventFlagCommand),
        ] {
            if on {
                flags |= flag;
            }
        }
        flags
    }

    const FUNCTION_KEYS: [CGKeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];

    const LETTER_KEYS: [CGKeyCode; 26] = [
        KeyCode::ANSI_A,
        KeyCode::ANSI_B,
        KeyCode::ANSI_C,
        KeyCode::ANSI_D,
        KeyCode::ANSI_E,
        KeyCode::ANSI_F,
        KeyCode::ANSI_G,
        KeyCode::ANSI_H,
        KeyCode::ANSI_I,
        KeyCode::ANSI_J,
        KeyCode::ANSI_K,
        KeyCode::ANSI_L,
        KeyCode::ANSI_M,
        KeyCode::ANSI_N,
        KeyCode::ANSI_O,
        KeyCode::ANSI_P,
        KeyCode::ANSI_Q,
        KeyCode::ANSI_R,
        KeyCode::ANSI_S,
        KeyCode::ANSI_T,
        KeyCode::ANSI_U,
        KeyCode::ANSI_V,
        KeyCode::ANSI_W,
        KeyCode::ANSI_X,
        KeyCode::ANSI_Y,
        KeyCode::ANSI_Z,
    ];

    const DIGIT_KEYS: [CGKeyCode; 10] = [
        KeyCode::ANSI_0,
        KeyCode::ANSI_1,
        KeyCode::ANSI_2,
        KeyCode::ANSI_3,
        KeyCode::ANSI_4,
        KeyCode::ANSI_5,
        KeyCode::ANSI_6,
        KeyCode::ANSI_7,
        KeyCode::ANSI_8,
        KeyCode::ANSI_9,
    ];

    fn key_code(name: &str) -> Result<CGKeyCode> {
        let named = match name {
            "Space" => Some(KeyCode::SPACE),
            "Escape" => Some(KeyCode::ESCAPE),
            "Tab" => Some(KeyCode::TAB),
            "Enter" => Some(KeyCode::RETURN),
            _ => None,
        };
        if let Some(code) = named {
            return Ok(code);
        }
        if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()) {
            if (1..=12).contains(&n) {
                return Ok(FUNCTION_KEYS[n - 1]);
            }
        }
        match name.as_bytes() {
            [c @ b'A'..=b'Z'] => Ok(LETTER_KEYS[usize::from(c - b'A')]),
            [c @ b'0'..=b'9'] => Ok(DIGIT_KEYS[usize::from(c - b'0')]),
            _ => bail!("不支持的按键：{name}"),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use anyhow::{bail, Result};
    use panel_core::config::Hotkey;

    pub(super) fn send(_hotkey: &Hotkey) -> Result<()> {
        bail!("微信输入法桥接仅支持 macOS / Windows")
    }
}
