//! Observe WeType's WASAPI capture sessions. This does not open a recording stream,
//! read audio, inspect windows or send keys. All COM objects stay on this MTA thread.
use crate::ports::{WatchEvent, WatchHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::{
    eCapture, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(super) fn watch(on_event: Box<dyn Fn(WatchEvent) + Send>) -> WatchHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    // Failure to start the observer must not crash the voice session.
    let callback = Arc::new(std::sync::Mutex::new(on_event));
    let worker_callback = callback.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("wetype-watch".into())
        .spawn(move || poll(&stop_flag, &*worker_callback.lock().unwrap()))
    {
        callback.lock().unwrap()(WatchEvent::Failed(format!(
            "微信采音状态检测启动失败：{error}"
        )));
    }
    Box::new(move || stop.store(true, Ordering::Relaxed))
}

/// CoUninitialize must run after every COM interface on this thread has been dropped.
struct ComApartment;

impl ComApartment {
    fn init() -> windows::core::Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn failed(error: impl std::fmt::Display) -> WatchEvent {
    WatchEvent::Failed(format!("系统暂时无法读取微信音频状态：{error}"))
}

fn poll(stop: &AtomicBool, on_event: &dyn Fn(WatchEvent)) {
    let _com = match ComApartment::init() {
        Ok(com) => com,
        Err(error) => return on_event(failed(error)),
    };
    let mut previous = None;
    while !stop.load(Ordering::Relaxed) {
        let event = match active_capture_processes() {
            Ok(processes) => WatchEvent::Active(processes.iter().any(|path| is_wetype(path))),
            Err(error) => failed(error),
        };
        if !stop.load(Ordering::Relaxed) && previous.as_ref() != Some(&event) {
            on_event(event.clone());
            previous = Some(event);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Refresh every capture endpoint, rather than only the system default microphone:
/// WeType can select CABLE Output independently of Windows' default device.
fn active_capture_processes() -> windows::core::Result<Vec<String>> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
        let mut processes = Vec::new();
        for index in 0..devices.GetCount()? {
            let device = devices.Item(index)?;
            let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
            let sessions = manager.GetSessionEnumerator()?;
            for index in 0..sessions.GetCount()? {
                let session = sessions.GetSession(index)?;
                if session.GetState()? != AudioSessionStateActive {
                    continue;
                }
                let control: IAudioSessionControl2 = session.cast()?;
                let pid = control.GetProcessId()?;
                // PID 0 is a system audio session, not the input method.
                if pid != 0 {
                    processes.push(process_path(pid)?);
                }
            }
        }
        // Any failed query above is unknown, never an inferred stop.
        Ok(processes)
    }
}

fn process_path(pid: u32) -> windows::core::Result<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(handle);
        result?;
        Ok(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

fn is_wetype(path: &str) -> bool {
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    name == "wetype.exe" || (name.starts_with("wetype_") && name.ends_with(".exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_input_method_processes_without_matching_wechat_or_other_recorders() {
        for path in [
            r"C:\Program Files\Tencent\WeType\3.0.0.38\wetype_server.exe",
            "WeType_Renderer.EXE",
            "wetype.exe",
        ] {
            assert!(is_wetype(path), "{path}");
        }
        for path in [
            r"D:\Tencent\Weixin\Weixin.exe",
            "audiodg.exe",
            "recorder.exe",
            "wetype_server.exe.bak",
        ] {
            assert!(!is_wetype(path), "{path}");
        }
    }
}
