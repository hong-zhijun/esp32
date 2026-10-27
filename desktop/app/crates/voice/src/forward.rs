//! Board audio (Node: `WeTypeVoice.audio`). Audio that arrives before the start key
//! has taken effect is buffered; after that it is forwarded as it arrives.

use crate::abort;
use crate::bridge::Shared;
use crate::pcm::{level, seconds};
use crate::session::{Session, State};
use anyhow::anyhow;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 3 s of start-up audio at 16 kHz mono 16-bit: WeChat never came up beyond this.
const STARTUP_LIMIT_BYTES: u64 = 96_000;
const LEVEL_LOG_INTERVAL: Duration = Duration::from_millis(500);
const STARTUP_TOO_SLOW: &str = "微信输入法启动过慢，录音已停止";

pub(crate) fn on_audio(shared: &Arc<Shared>, s: &Arc<Session>, pcm: &[u8]) {
    let mut st = s.lock();
    if st.ended || st.aborted {
        return;
    }
    st.bytes += pcm.len() as u64;
    let log = level_event_if_due(&mut st, s, pcm);
    let outcome = if st.forwarding {
        st.forward(pcm)
    } else if st.bytes > STARTUP_LIMIT_BYTES {
        Err(anyhow!(STARTUP_TOO_SLOW))
    } else {
        st.buffered.push(pcm.to_vec());
        Ok(())
    };
    drop(st);
    if let Some(event) = log {
        shared.log.add(event);
    }
    if let Err(error) = outcome {
        abort::fail(shared, s, error);
    }
}

/// Throttled to one `audio-level` entry per interval.
fn level_event_if_due(st: &mut State, s: &Session, pcm: &[u8]) -> Option<Value> {
    let now = Instant::now();
    if st
        .last_level_log
        .is_some_and(|last| now.duration_since(last) < LEVEL_LOG_INTERVAL)
    {
        return None;
    }
    st.last_level_log = Some(now);
    let stats = level(pcm);
    Some(json!({
        "event": "audio-level",
        "peak": stats.peak,
        "rms": stats.rms,
        "signal": stats.has_signal(),
        "audioSeconds": seconds(st.bytes),
        "wallSeconds": now.duration_since(s.started_at).as_secs_f64(),
    }))
}
