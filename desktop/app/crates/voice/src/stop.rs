//! The stop sequence (Node: `WeTypeVoice.stop`). It runs after `VoiceBridge::stop`
//! has accepted the request: drain the cable, wait for WeChat to read the tail,
//! then press the stop key if the start key is still owed one.

use crate::abort;
use crate::bridge::Shared;
use crate::pcm::seconds;
use crate::ports::{AudioSink, DrainStats};
use crate::reply;
use crate::session::{stop_watch, Session};
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Lets WeChat take the last syllables before the stop key (Node: 500 ms).
const STOP_SETTLE: Duration = Duration::from_millis(500);

pub(crate) async fn run(shared: Arc<Shared>, s: Arc<Session>) {
    s.ready.wait().await;
    if s.is_aborted() {
        return;
    }
    let Some(sink) = s.take_sink() else {
        return;
    };
    match drain(sink).await {
        Ok(stats) => log_drained(&shared, &stats),
        Err(error) => {
            abort::fail(&shared, &s, error);
            return;
        }
    }
    if s.is_aborted() {
        return;
    }
    tokio::time::sleep(STOP_SETTLE).await;
    if s.is_aborted() {
        return;
    }
    if let Err(error) = press_stop_key(&shared, &s).await {
        abort::fail(&shared, &s, error);
        return;
    }
    stop_watch(&s);
    if s.is_aborted() {
        return;
    }
    shared.release(&s);
    s.cleanup.set();
    let bytes = s.lock().bytes;
    shared.log.add(json!({
        "event": "wetype-stopped",
        "audioSeconds": seconds(bytes),
        "wallSeconds": s.started_at.elapsed().as_secs_f64(),
    }));
    let send = s.reply();
    send(reply::voice("finished", "微信语音已结束，请查看输入框"));
}

/// Waits for the cable to play everything and releases the device.
async fn drain(sink: Box<dyn AudioSink>) -> Result<DrainStats> {
    tokio::task::spawn_blocking(move || sink.finish()).await?
}

fn log_drained(shared: &Shared, stats: &DrainStats) {
    shared.log.add(json!({
        "event": "virtual-mic-drained",
        "audioSeconds": stats.audio_seconds,
        "underruns": stats.underruns,
    }));
}

/// Presses the stop key if one is owed. A failed press gives the key back, so the
/// failure path makes exactly one more attempt, as the Node version did.
async fn press_stop_key(shared: &Arc<Shared>, s: &Session) -> Result<()> {
    if !s.claim_stop_key() {
        return Ok(());
    }
    shared.press().await.inspect_err(|_| s.owe_stop_key())
}
