//! Reports whether the WeChat input method is capturing audio (port of bridge/wetype-status.c).
//! macOS uses Core Audio process objects; Windows uses WASAPI capture sessions.

use crate::ports::{CaptureWatcher, WatchEvent, WatchHandle};

/// Watcher used by the bridge on this platform.
pub(crate) struct SystemWatcher;

impl CaptureWatcher for SystemWatcher {
    fn watch(&self, on_event: Box<dyn Fn(WatchEvent) + Send>) -> WatchHandle {
        platform::watch(on_event)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use crate::ports::{WatchEvent, WatchHandle};
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    const POLL_INTERVAL: Duration = Duration::from_millis(100);
    const SYSTEM_OBJECT: u32 = 1;
    const SCOPE_GLOBAL: u32 = four_cc(b"glob");
    const ELEMENT_MAIN: u32 = 0;
    const PROCESS_OBJECT_LIST: u32 = four_cc(b"prs#");
    const PROCESS_BUNDLE_ID: u32 = four_cc(b"pbid");
    const PROCESS_RUNNING_INPUT: u32 = four_cc(b"piri");
    const WETYPE_BUNDLE_PREFIX: &str = "com.tencent.inputmethod.wetype";
    const UNAVAILABLE: &str = "系统暂时无法读取微信音频状态";

    const fn four_cc(code: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*code)
    }

    #[repr(C)]
    struct PropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            object: u32,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    pub(super) fn watch(on_event: Box<dyn Fn(WatchEvent) + Send>) -> WatchHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        std::thread::Builder::new()
            .name("wetype-watch".into())
            .spawn(move || poll(&stop_flag, &*on_event))
            .expect("failed to start the WeChat capture watcher thread");
        Box::new(move || stop.store(true, Ordering::Relaxed))
    }

    /// Emits only when the reading changes, starting with the first reading.
    fn poll(stop: &AtomicBool, on_event: &dyn Fn(WatchEvent)) {
        let mut previous = None;
        while !stop.load(Ordering::Relaxed) {
            let event = read_event();
            if previous.as_ref() != Some(&event) {
                on_event(event.clone());
                previous = Some(event);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn read_event() -> WatchEvent {
        match capture_active() {
            Some(active) => WatchEvent::Active(active),
            None => WatchEvent::Failed(UNAVAILABLE.into()),
        }
    }

    /// `None` when the state cannot be read. That is never the same as "not capturing".
    fn capture_active() -> Option<bool> {
        for process in process_objects()? {
            let Some(bundle) = bundle_id(process) else {
                continue;
            };
            if !bundle.starts_with(WETYPE_BUNDLE_PREFIX) {
                continue;
            }
            if running_input(process)? {
                return Some(true);
            }
        }
        Some(false)
    }

    fn address(selector: u32) -> PropertyAddress {
        PropertyAddress {
            selector,
            scope: SCOPE_GLOBAL,
            element: ELEMENT_MAIN,
        }
    }

    fn process_objects() -> Option<Vec<u32>> {
        let address = address(PROCESS_OBJECT_LIST);
        let mut size = 0u32;
        check(unsafe {
            AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &address, 0, null(), &mut size)
        })?;
        let mut ids = vec![0u32; size as usize / size_of::<u32>()];
        check(unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &address,
                0,
                null(),
                &mut size,
                ids.as_mut_ptr().cast(),
            )
        })?;
        ids.truncate(size as usize / size_of::<u32>());
        Some(ids)
    }

    fn bundle_id(process: u32) -> Option<String> {
        let address = address(PROCESS_BUNDLE_ID);
        let mut bundle: CFStringRef = null();
        let mut size = size_of::<CFStringRef>() as u32;
        check(unsafe {
            AudioObjectGetPropertyData(
                process,
                &address,
                0,
                null(),
                &mut size,
                (&mut bundle as *mut CFStringRef).cast(),
            )
        })?;
        if bundle.is_null() {
            return None;
        }
        // The property returns a retained string; the wrapper releases it.
        Some(unsafe { CFString::wrap_under_create_rule(bundle) }.to_string())
    }

    fn running_input(process: u32) -> Option<bool> {
        let address = address(PROCESS_RUNNING_INPUT);
        let mut running = 0u32;
        let mut size = size_of::<u32>() as u32;
        check(unsafe {
            AudioObjectGetPropertyData(
                process,
                &address,
                0,
                null(),
                &mut size,
                (&mut running as *mut u32).cast(),
            )
        })?;
        Some(running != 0)
    }

    fn check(status: i32) -> Option<()> {
        (status == 0).then_some(())
    }
}

#[cfg(windows)]
#[path = "watch_windows.rs"]
mod platform;

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use crate::ports::{WatchEvent, WatchHandle};

    pub(super) fn watch(_on_event: Box<dyn Fn(WatchEvent) + Send>) -> WatchHandle {
        Box::new(|| {})
    }
}
