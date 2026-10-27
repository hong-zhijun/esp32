//! Opening a session (Node: `WeTypeVoice.open`): open the device, watch WeChat's
//! capture state, press the start key, give WeChat time to open the cable input,
//! then flush the start-up audio and begin live forwarding.

use crate::abort;
use crate::bridge::Shared;
use crate::ports::{AudioSink, WatchEvent};
use crate::reply;
use crate::session::Session;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Time for WeChat to open the cable input so the first syllable is kept.
const SETTLE: Duration = Duration::from_millis(350);
/// Capture idle for this long after being active means WeChat stopped by itself.
const EXTERNAL_STOP_DELAY: Duration = Duration::from_millis(250);

pub(crate) async fn run(shared: Arc<Shared>, session: Arc<Session>) {
    if let Err(error) = open_and_start(&shared, &session).await {
        abort::fail(&shared, &session, error);
    }
    session.ready.set();
}

async fn open_and_start(shared: &Arc<Shared>, s: &Arc<Session>) -> Result<()> {
    let sink = open_device(shared, s).await?;
    if !s.attach_sink(sink) {
        return Ok(());
    }
    start_watch(shared, s);
    if !press_start_key(shared, s).await? {
        return Ok(());
    }
    tokio::time::sleep(SETTLE).await;
    if !s.lock().start_forwarding()? {
        return Ok(());
    }
    let send = s.reply();
    send(reply::voice("partial", "微信输入法正在语音输入"));
    shared.log.add(json!({
        "event": "wetype-started",
        "message": format!("已发送 {}，音频转发到 {}", shared.hotkey_label(), s.device),
    }));
    Ok(())
}

async fn open_device(shared: &Arc<Shared>, s: &Session) -> Result<Box<dyn AudioSink>> {
    let factory = shared.factory.clone();
    let device = s.device.clone();
    let sink = tokio::task::spawn_blocking(move || factory.open(&device)).await??;
    shared
        .log
        .add(json!({"event": "virtual-mic-ready", "message": sink.device_name()}));
    Ok(sink)
}

/// Presses the start key. Returns `false` if the session failed before the key could be sent.
async fn press_start_key(shared: &Arc<Shared>, s: &Arc<Session>) -> Result<bool> {
    let Some(done) = s.begin_activation() else {
        return Ok(false);
    };
    let pressed = shared.press().await;
    s.finish_activation(&done, pressed.is_ok());
    pressed.map(|()| true)
}

/// The watch holds only weak references, so it cannot keep a finished session alive.
fn start_watch(shared: &Arc<Shared>, s: &Arc<Session>) {
    let (weak_shared, weak_session) = (Arc::downgrade(shared), Arc::downgrade(s));
    let handle = shared.watcher.watch(Box::new(move |event| {
        if let (Some(shared), Some(s)) = (weak_shared.upgrade(), weak_session.upgrade()) {
            on_watch_event(&shared, &s, event);
        }
    }));
    let mut st = s.lock();
    if st.aborted {
        drop(st);
        handle();
    } else {
        st.watch = Some(handle);
    }
}

fn on_watch_event(shared: &Arc<Shared>, s: &Arc<Session>, event: WatchEvent) {
    match event {
        WatchEvent::Active(true) => {
            let mut st = s.lock();
            if st.aborted {
                return;
            }
            st.cancel_timer();
            st.seen_active = true;
            drop(st);
            shared.log.add(json!({"event": "wetype-capture-active"}));
        }
        WatchEvent::Active(false) => {
            let mut st = s.lock();
            if st.aborted {
                return;
            }
            // Idle before WeChat was ever seen active is not a stop.
            st.cancel_timer();
            if st.seen_active {
                schedule_external_stop(shared, s, st.timer_seq);
            }
        }
        WatchEvent::Failed(message) => {
            s.lock().cancel_timer();
            shared
                .log
                .add(json!({"event": "wetype-monitor-error", "message": message}));
        }
    }
}

/// Fires only if no capture event arrived in the meantime (the sequence number is unchanged).
fn schedule_external_stop(shared: &Arc<Shared>, s: &Arc<Session>, seq: u64) {
    let (shared, s) = (shared.clone(), s.clone());
    let runtime = s.runtime.clone();
    runtime.spawn(async move {
        tokio::time::sleep(EXTERNAL_STOP_DELAY).await;
        abort::external_stop(&shared, &s, Some(seq));
    });
}
