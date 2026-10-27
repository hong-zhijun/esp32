//! State of one voice session: what the Node version kept on `this.session`.
//! Sessions are shared as `Arc<Session>`; ownership is by identity, so a stale
//! session can only touch its own state, never a newer one.

use crate::ports::{AudioSink, Canceller, WatchHandle};
use crate::Reply;
use anyhow::Result;
use parking_lot::{Mutex, MutexGuard};
use std::sync::Arc;
use std::time::Instant;
use tokio::runtime::Handle;
use tokio::sync::watch;

/// A one-shot event that any number of tasks can await.
#[derive(Clone)]
pub(crate) struct Signal(Arc<watch::Sender<bool>>);

impl Signal {
    pub(crate) fn new() -> Self {
        Self(Arc::new(watch::channel(false).0))
    }

    pub(crate) fn set(&self) {
        self.0.send_replace(true);
    }

    pub(crate) async fn wait(&self) {
        let mut rx = self.0.subscribe();
        let _ = rx.wait_for(|done| *done).await;
    }
}

pub(crate) struct Session {
    /// Device name requested by the settings at start time.
    pub(crate) device: String,
    pub(crate) started_at: Instant,
    /// Used to spawn timers from threads that are not tokio workers (the watcher).
    pub(crate) runtime: Handle,
    /// The open sequence has finished, successfully or not (Node: `readyPromise`).
    pub(crate) ready: Signal,
    /// Cleanup is done and the session has released the bridge (Node: `cleanup`).
    pub(crate) cleanup: Signal,
    state: Mutex<State>,
}

pub(crate) struct State {
    pub(crate) reply: Reply,
    pub(crate) sink: Option<Box<dyn AudioSink>>,
    /// Stays available while a drain is in progress, so an abort can interrupt it.
    pub(crate) canceller: Option<Canceller>,
    pub(crate) watch: Option<WatchHandle>,
    /// Audio received before the start key has taken effect.
    pub(crate) buffered: Vec<Vec<u8>>,
    /// PCM bytes received so far, including buffered ones.
    pub(crate) bytes: u64,
    pub(crate) forwarding: bool,
    /// A start key was pressed and no stop key has been pressed yet.
    pub(crate) started: bool,
    /// Stop was requested; no more audio is accepted.
    pub(crate) ended: bool,
    /// The session failed or was ended from outside; nothing else may act on it.
    pub(crate) aborted: bool,
    pub(crate) external_closed: bool,
    pub(crate) seen_active: bool,
    /// The start key in flight, if any. Failure cleanup waits for it.
    pub(crate) activation: Option<Signal>,
    /// Incremented to cancel a pending external-stop timer.
    pub(crate) timer_seq: u64,
    pub(crate) last_level_log: Option<Instant>,
}

impl Session {
    pub(crate) fn new(device: String, reply: Reply, runtime: Handle) -> Self {
        Self {
            device,
            started_at: Instant::now(),
            runtime,
            ready: Signal::new(),
            cleanup: Signal::new(),
            state: Mutex::new(State {
                reply,
                sink: None,
                canceller: None,
                watch: None,
                buffered: Vec::new(),
                bytes: 0,
                forwarding: false,
                started: false,
                ended: false,
                aborted: false,
                external_closed: false,
                seen_active: false,
                activation: None,
                timer_seq: 0,
                last_level_log: None,
            }),
        }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock()
    }

    pub(crate) fn is_aborted(&self) -> bool {
        self.lock().aborted
    }

    pub(crate) fn reply(&self) -> Reply {
        self.lock().reply.clone()
    }

    /// Stores the opened device. If the session already failed, the sink is dropped
    /// (closing the device) and `false` is returned.
    pub(crate) fn attach_sink(&self, sink: Box<dyn AudioSink>) -> bool {
        let mut st = self.lock();
        if st.aborted {
            return false;
        }
        st.canceller = Some(sink.canceller());
        st.sink = Some(sink);
        true
    }

    pub(crate) fn take_sink(&self) -> Option<Box<dyn AudioSink>> {
        self.lock().sink.take()
    }

    /// Marks the start key as in flight. `None` if the session already failed.
    pub(crate) fn begin_activation(&self) -> Option<Signal> {
        let mut st = self.lock();
        if st.aborted {
            return None;
        }
        let done = Signal::new();
        st.activation = Some(done.clone());
        Some(done)
    }

    /// Records the outcome of the start key. A key that was not pressed (or whose
    /// session ended during the press) is not owed a stop key.
    pub(crate) fn finish_activation(&self, done: &Signal, pressed: bool) {
        {
            let mut st = self.lock();
            st.started = pressed && !st.external_closed;
        }
        done.set();
    }

    /// Waits for a start key that is in flight, if any.
    pub(crate) async fn wait_activation(&self) {
        let pending = self.lock().activation.clone();
        if let Some(done) = pending {
            done.wait().await;
        }
    }

    /// Takes responsibility for the stop key. Returns `false` if none is owed.
    pub(crate) fn claim_stop_key(&self) -> bool {
        std::mem::replace(&mut self.lock().started, false)
    }

    /// Gives the stop key back after a failed press, so the failure path retries it.
    pub(crate) fn owe_stop_key(&self) {
        self.lock().started = true;
    }
}

impl State {
    pub(crate) fn cancel_timer(&mut self) {
        self.timer_seq += 1;
    }

    /// Cuts the audio short. Called once, when the session ends without finishing normally.
    pub(crate) fn cancel_sink(&mut self) {
        if let Some(cancel) = self.canceller.take() {
            cancel();
        }
    }

    /// Sends PCM to the cable. A session without a device (not opened yet) drops it.
    pub(crate) fn forward(&mut self, pcm: &[u8]) -> Result<()> {
        match self.sink.as_mut() {
            Some(sink) => sink.write(pcm),
            None => Ok(()),
        }
    }

    /// Writes the buffered start-up audio and switches to live forwarding.
    /// Returns `false` if the session failed meanwhile.
    pub(crate) fn start_forwarding(&mut self) -> Result<bool> {
        for chunk in std::mem::take(&mut self.buffered) {
            if self.aborted {
                return Ok(false);
            }
            self.forward(&chunk)?;
        }
        if self.aborted {
            return Ok(false);
        }
        self.forwarding = true;
        Ok(true)
    }
}

/// Removes the capture watch. Called outside the session lock.
pub(crate) fn stop_watch(s: &Session) {
    let watch = {
        let mut st = s.lock();
        st.cancel_timer();
        st.watch.take()
    };
    if let Some(stop) = watch {
        stop();
    }
}
