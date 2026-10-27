//! Ending a session early (Node: `WeTypeVoice.fail`, `externalStop`, `cleanup`).
//! Both stop the capture and drop the device at once, then release the bridge only
//! after a start key still in flight is balanced by a stop key. A stale session
//! therefore never sends a key into a newer one.

use crate::bridge::Shared;
use crate::pcm::seconds;
use crate::reply;
use crate::session::{stop_watch, Session};
use anyhow::Error;
use serde_json::json;
use std::sync::Arc;

/// Failure: report the error to the board and balance any start key.
pub(crate) fn fail(shared: &Arc<Shared>, s: &Arc<Session>, error: Error) {
    let sink = {
        let mut st = s.lock();
        if st.aborted {
            return;
        }
        st.aborted = true;
        st.cancel_sink();
        st.sink.take()
    };
    stop_watch(s);
    drop(sink);
    let message = error.to_string();
    shared
        .log
        .add(json!({"event": "voice-error", "stage": "wetype", "message": message}));
    let send = s.reply();
    send(reply::voice("error", &message));
    settle_later(shared, s);
}

/// WeChat ended the session by itself. `timer` is the sequence number of the timer
/// that fired; a stale timer is ignored.
pub(crate) fn external_stop(shared: &Arc<Shared>, s: &Arc<Session>, timer: Option<u64>) {
    let (sink, send, bytes) = {
        let mut st = s.lock();
        let stale_timer = timer.is_some_and(|seq| seq != st.timer_seq);
        if st.aborted || stale_timer || !shared.is_current(s) {
            return;
        }
        st.external_closed = true;
        st.aborted = true;
        st.ended = true;
        st.started = false;
        st.cancel_timer();
        st.cancel_sink();
        (st.sink.take(), st.reply.clone(), st.bytes)
    };
    stop_watch(s);
    // WeChat has already stopped: discard the remaining PCM and never press the key.
    drop(sink);
    send(reply::voice("finished", "电脑已结束语音"));
    shared
        .log
        .add(json!({"event": "wetype-external-stop", "audioSeconds": seconds(bytes)}));
    settle_later(shared, s);
}

fn settle_later(shared: &Arc<Shared>, s: &Arc<Session>) {
    let (shared, s) = (shared.clone(), s.clone());
    let runtime = s.runtime.clone();
    runtime.spawn(settle(shared, s));
}

/// Waits for a start key in flight, balances it if one is still owed, then releases the bridge.
async fn settle(shared: Arc<Shared>, s: Arc<Session>) {
    s.wait_activation().await;
    if s.claim_stop_key() {
        if let Err(error) = shared.press().await {
            shared.log.add(json!({"event": "voice-error", "stage": "wetype-stop", "message": error.to_string()}));
        }
    }
    shared.release(&s);
    s.cleanup.set();
}
