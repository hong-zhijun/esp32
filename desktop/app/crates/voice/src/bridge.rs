//! `VoiceBridge`: the non-blocking entry points. Each one changes the session state
//! synchronously and hands any waiting to a tokio task (see `start`, `stop`, `abort`).

use crate::abort;
use crate::audio_out::CpalFactory;
use crate::forward;
use crate::hotkey::SystemTrigger;
use crate::ports::{AudioSinkFactory, CaptureWatcher, Trigger};
use crate::reply;
use crate::session::Session;
use crate::watch::SystemWatcher;
use crate::{start, stop, Reply, VoiceSettings};
use anyhow::{anyhow, Result};
use panel_core::debug_log::DebugLog;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

const BUSY: &str = "上一段语音还未结束";
const SCREEN_DISCONNECTED: &str = "屏幕断开，已停止微信语音";

/// One bridge per app. Cheap to clone (shared inner state).
/// All methods are non-blocking: waits run on spawned tokio tasks, so they
/// must be called from inside a tokio runtime.
#[derive(Clone)]
pub struct VoiceBridge {
    shared: Arc<Shared>,
}

/// Everything the session tasks need. Shared by all sessions of one bridge.
pub(crate) struct Shared {
    pub(crate) settings: Arc<Mutex<VoiceSettings>>,
    pub(crate) log: DebugLog,
    pub(crate) factory: Arc<dyn AudioSinkFactory>,
    pub(crate) trigger: Arc<dyn Trigger>,
    pub(crate) watcher: Arc<dyn CaptureWatcher>,
    /// The session that owns the bridge, from start until its cleanup releases it.
    slot: Mutex<Option<Arc<Session>>>,
}

impl VoiceBridge {
    /// Real implementation: cpal output, OS key synthesis, macOS capture watcher.
    pub fn new(settings: VoiceSettings, log: DebugLog) -> Self {
        let settings = Arc::new(Mutex::new(settings));
        let trigger = Arc::new(SystemTrigger::new(settings.clone()));
        Self::assemble(
            settings,
            log,
            Arc::new(CpalFactory),
            trigger,
            Arc::new(SystemWatcher),
        )
    }

    #[cfg(test)]
    pub(crate) fn with_parts(
        settings: VoiceSettings,
        log: DebugLog,
        factory: Arc<dyn AudioSinkFactory>,
        trigger: Arc<dyn Trigger>,
        watcher: Arc<dyn CaptureWatcher>,
    ) -> Self {
        Self::assemble(
            Arc::new(Mutex::new(settings)),
            log,
            factory,
            trigger,
            watcher,
        )
    }

    fn assemble(
        settings: Arc<Mutex<VoiceSettings>>,
        log: DebugLog,
        factory: Arc<dyn AudioSinkFactory>,
        trigger: Arc<dyn Trigger>,
        watcher: Arc<dyn CaptureWatcher>,
    ) -> Self {
        let shared = Shared {
            settings,
            log,
            factory,
            trigger,
            watcher,
            slot: Mutex::new(None),
        };
        Self {
            shared: Arc::new(shared),
        }
    }

    pub fn update_settings(&self, settings: VoiceSettings) {
        *self.shared.settings.lock() = settings;
    }

    /// Board pressed "start". Replies through `reply` for the whole session.
    pub fn start(&self, reply: Reply) {
        let device = self.shared.settings.lock().device.clone();
        let session = Arc::new(Session::new(device, reply.clone(), Handle::current()));
        if !self.shared.claim(&session) {
            reply(reply::voice("error", BUSY));
            return;
        }
        self.shared
            .log
            .add(json!({"event": "recording-start", "backend": "wetype"}));
        let runtime = session.runtime.clone();
        runtime.spawn(start::run(self.shared.clone(), session));
    }

    /// One binary frame from the session owner.
    pub fn audio(&self, pcm: &[u8]) {
        if let Some(session) = self.shared.current() {
            forward::on_audio(&self.shared, &session, pcm);
        }
    }

    /// Board pressed "stop".
    pub fn stop(&self, reply: Reply) {
        self.begin_stop(reply);
    }

    /// Owner disconnected: fail the session ("屏幕断开，已停止微信语音").
    pub fn cancel(&self) {
        if let Some(session) = self.shared.current() {
            abort::fail(&self.shared, &session, anyhow!(SCREEN_DISCONNECTED));
        }
    }

    /// True while a session exists (including cleanup after failure).
    pub fn busy(&self) -> bool {
        self.shared.current().is_some()
    }

    /// Accepts a stop request synchronously and returns the task that completes it.
    /// `None` when there is nothing to stop or stop was already requested.
    pub(crate) fn begin_stop(&self, reply: Reply) -> Option<JoinHandle<()>> {
        let session = self.shared.current()?;
        {
            let mut st = session.lock();
            if st.ended {
                return None;
            }
            st.ended = true;
            st.reply = reply;
        }
        let runtime = session.runtime.clone();
        Some(runtime.spawn(stop::run(self.shared.clone(), session)))
    }
}

#[cfg(test)]
impl VoiceBridge {
    pub(crate) fn current_session(&self) -> Option<Arc<Session>> {
        self.shared.current()
    }
}

impl Shared {
    /// Installs `session` as the owner if the bridge is free.
    fn claim(&self, session: &Arc<Session>) -> bool {
        let mut slot = self.slot.lock();
        if slot.is_some() {
            return false;
        }
        *slot = Some(session.clone());
        true
    }

    pub(crate) fn current(&self) -> Option<Arc<Session>> {
        self.slot.lock().clone()
    }

    pub(crate) fn is_current(&self, session: &Arc<Session>) -> bool {
        self.slot
            .lock()
            .as_ref()
            .is_some_and(|owner| Arc::ptr_eq(owner, session))
    }

    /// Frees the bridge, but only if `session` still owns it.
    pub(crate) fn release(&self, session: &Arc<Session>) {
        let mut slot = self.slot.lock();
        if slot
            .as_ref()
            .is_some_and(|owner| Arc::ptr_eq(owner, session))
        {
            *slot = None;
        }
    }

    pub(crate) fn hotkey_label(&self) -> String {
        self.settings.lock().hotkey.label()
    }

    /// Presses the shortcut on the blocking pool; the key synthesis sleeps between events.
    pub(crate) async fn press(&self) -> Result<()> {
        let trigger = self.trigger.clone();
        tokio::task::spawn_blocking(move || trigger.press()).await?
    }
}
