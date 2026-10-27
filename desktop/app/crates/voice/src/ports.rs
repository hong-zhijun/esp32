//! The bridge's only contact with the outside world. Real implementations live in
//! `audio_out`, `hotkey` and `watch`; tests inject fakes through these traits.
//! All methods are blocking and are called on tokio's blocking pool.

use anyhow::Result;

/// What the virtual cable reported after the last sample was played.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DrainStats {
    pub audio_seconds: f64,
    pub underruns: u64,
}

/// Aborts a sink from outside: a `finish` in progress returns at once and queued audio is dropped.
pub(crate) type Canceller = Box<dyn Fn() + Send + Sync>;

/// Receives 16 kHz mono PCM and plays it into the virtual cable.
pub(crate) trait AudioSink: Send {
    fn device_name(&self) -> &str;
    fn write(&mut self, pcm16k: &[u8]) -> Result<()>;
    fn canceller(&self) -> Canceller;
    /// Waits until the cable has played everything, then releases the device.
    fn finish(self: Box<Self>) -> Result<DrainStats>;
}

pub(crate) trait AudioSinkFactory: Send + Sync {
    fn open(&self, device: &str) -> Result<Box<dyn AudioSink>>;
}

/// Presses the WeChat voice shortcut once.
pub(crate) trait Trigger: Send + Sync {
    fn press(&self) -> Result<()>;
}

/// The platform watcher reports capture state without reading microphone samples.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WatchEvent {
    /// WeChat input method is (or is not) capturing audio.
    Active(bool),
    /// The state could not be read. This is never treated as "stopped".
    Failed(String),
}

/// Stops the watch when called.
pub(crate) type WatchHandle = Box<dyn FnOnce() + Send>;

pub(crate) trait CaptureWatcher: Send + Sync {
    fn watch(&self, on_event: Box<dyn Fn(WatchEvent) + Send>) -> WatchHandle;
}
