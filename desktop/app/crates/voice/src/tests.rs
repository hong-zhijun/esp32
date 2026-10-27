//! Ports of the seven cases in `wetype-test.cjs`, run against fake device, key and
//! capture-state implementations. Time is paused, so the 250/350/500 ms waits are instant.

use crate::ports::{
    AudioSink, AudioSinkFactory, Canceller, CaptureWatcher, DrainStats, Trigger, WatchEvent,
    WatchHandle,
};
use crate::session::Session;
use crate::{Reply, VoiceBridge, VoiceSettings};
use anyhow::{anyhow, Result};
use panel_core::config::Hotkey;
use panel_core::debug_log::DebugLog;
use serde_json::Value;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

type Replies = Arc<Mutex<Vec<Value>>>;
type WatchCallback = Arc<Mutex<Option<Box<dyn Fn(WatchEvent) + Send>>>>;

#[derive(Default)]
struct Cable {
    received: Vec<u8>,
    finished: bool,
    cancelled: bool,
}

struct FakeSink {
    cable: Arc<Mutex<Cable>>,
}

impl AudioSink for FakeSink {
    fn device_name(&self) -> &str {
        "Test cable"
    }

    fn canceller(&self) -> Canceller {
        let cable = self.cable.clone();
        Box::new(move || cable.lock().unwrap().cancelled = true)
    }

    fn write(&mut self, pcm16k: &[u8]) -> Result<()> {
        self.cable
            .lock()
            .unwrap()
            .received
            .extend_from_slice(pcm16k);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<DrainStats> {
        self.cable.lock().unwrap().finished = true;
        Ok(DrainStats {
            audio_seconds: 0.0,
            underruns: 0,
        })
    }
}

/// Opens a fake cable. `missing` simulates no virtual device; `gate` holds the open
/// until the test releases it, like a slow device.
#[derive(Default)]
struct FakeFactory {
    cable: Arc<Mutex<Cable>>,
    missing: bool,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
}

impl AudioSinkFactory for FakeFactory {
    fn open(&self, device: &str) -> Result<Box<dyn AudioSink>> {
        let gate = self.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
        if self.missing {
            return Err(anyhow!(
                "未找到虚拟音频输出设备：{device}；请先安装虚拟声卡"
            ));
        }
        self.cable.lock().unwrap().finished = false;
        Ok(Box::new(FakeSink {
            cable: self.cable.clone(),
        }))
    }
}

/// Counts shortcut presses. The first press can be held open until the test releases it.
#[derive(Default)]
struct FakeTrigger {
    /// One entry per press: whether the cable had already finished draining.
    presses: Arc<Mutex<Vec<bool>>>,
    cable: Arc<Mutex<Cable>>,
    first_press: Mutex<Option<(oneshot::Sender<()>, mpsc::Receiver<()>)>>,
}

impl Trigger for FakeTrigger {
    fn press(&self) -> Result<()> {
        let count = {
            let mut presses = self.presses.lock().unwrap();
            presses.push(self.cable.lock().unwrap().finished);
            presses.len()
        };
        if count == 1 {
            if let Some((entered, release)) = self.first_press.lock().unwrap().take() {
                let _ = entered.send(());
                let _ = release.recv();
            }
        }
        Ok(())
    }
}

/// Hands the watch callback to the test so it can play the capture state.
struct FakeWatcher {
    callback: WatchCallback,
}

impl CaptureWatcher for FakeWatcher {
    fn watch(&self, on_event: Box<dyn Fn(WatchEvent) + Send>) -> WatchHandle {
        *self.callback.lock().unwrap() = Some(on_event);
        Box::new(|| {})
    }
}

#[derive(Default)]
struct Options {
    missing_cable: bool,
    open_gate: Option<mpsc::Receiver<()>>,
    first_press: Option<(oneshot::Sender<()>, mpsc::Receiver<()>)>,
}

struct Rig {
    bridge: VoiceBridge,
    cable: Arc<Mutex<Cable>>,
    presses: Arc<Mutex<Vec<bool>>>,
    watch: WatchCallback,
    log: DebugLog,
}

impl Rig {
    fn new(options: Options) -> Self {
        let cable = Arc::new(Mutex::new(Cable::default()));
        let presses = Arc::new(Mutex::new(Vec::new()));
        let watch: WatchCallback = Arc::new(Mutex::new(None));
        let log = DebugLog::new();
        let factory = FakeFactory {
            cable: cable.clone(),
            missing: options.missing_cable,
            gate: Mutex::new(options.open_gate),
        };
        let trigger = FakeTrigger {
            presses: presses.clone(),
            cable: cable.clone(),
            first_press: Mutex::new(options.first_press),
        };
        let watcher = FakeWatcher {
            callback: watch.clone(),
        };
        let settings = VoiceSettings {
            device: "CABLE Input".into(),
            hotkey: Hotkey::platform_default(),
        };
        let bridge = VoiceBridge::with_parts(
            settings,
            log.clone(),
            Arc::new(factory),
            Arc::new(trigger),
            Arc::new(watcher),
        );
        Self {
            bridge,
            cable,
            presses,
            watch,
            log,
        }
    }

    fn presses(&self) -> usize {
        self.presses.lock().unwrap().len()
    }

    fn pressed_after_drain(&self) -> Vec<bool> {
        self.presses.lock().unwrap().clone()
    }

    fn received(&self) -> Vec<u8> {
        self.cable.lock().unwrap().received.clone()
    }

    fn emit(&self, event: WatchEvent) {
        if let Some(callback) = self.watch.lock().unwrap().as_ref() {
            callback(event);
        }
    }

    fn session(&self) -> Arc<Session> {
        self.bridge.current_session().expect("a session is active")
    }

    fn count_events(&self, name: &str) -> usize {
        self.log
            .entries()
            .iter()
            .filter(|row| row["event"] == name)
            .count()
    }
}

fn recorder() -> (Reply, Replies) {
    let replies: Replies = Arc::new(Mutex::new(Vec::new()));
    let sink = replies.clone();
    (
        Arc::new(move |value| sink.lock().unwrap().push(value)),
        replies,
    )
}

fn last(replies: &Replies) -> Value {
    replies
        .lock()
        .unwrap()
        .last()
        .cloned()
        .expect("a reply was sent")
}

fn finished_count(replies: &Replies) -> usize {
    replies
        .lock()
        .unwrap()
        .iter()
        .filter(|row| row["state"] == "finished")
        .count()
}

async fn stop_and_wait(bridge: &VoiceBridge, reply: Reply) {
    bridge
        .begin_stop(reply)
        .expect("stop is accepted")
        .await
        .unwrap();
}

fn pcm(byte: u8, len: usize) -> Vec<u8> {
    vec![byte; len]
}

#[tokio::test(start_paused = true)]
async fn wireless_bridge_buffers_initial_audio_streams_exact_pcm_then_drains_and_stops() {
    let rig = Rig::new(Options::default());
    let (reply, replies) = recorder();
    rig.bridge.start(reply.clone());
    let session = rig.session();
    let (first, second) = (pcm(7, 640), pcm(8, 640));

    rig.bridge.audio(&first);
    session.ready.wait().await;
    assert_eq!(rig.presses(), 1);

    rig.bridge.audio(&second);
    stop_and_wait(&rig.bridge, reply).await;

    assert_eq!(rig.received(), [first, second].concat());
    assert_eq!(rig.presses(), 2);
    assert_eq!(last(&replies)["state"], "finished");
    assert!(rig.bridge.current_session().is_none());
    assert_eq!(rig.count_events("wetype-started"), 1);
    // The stop key is pressed only after the cable has drained.
    assert_eq!(rig.pressed_after_drain(), vec![false, true]);
}

#[tokio::test(start_paused = true)]
async fn consecutive_recordings_each_balance_their_own_keys() {
    let rig = Rig::new(Options::default());
    for _ in 0..2 {
        let (reply, replies) = recorder();
        rig.bridge.start(reply.clone());
        let session = rig.session();
        session.ready.wait().await;
        rig.bridge.audio(&pcm(5, 640));
        stop_and_wait(&rig.bridge, reply).await;
        assert_eq!(last(&replies)["state"], "finished");
        assert!(rig.bridge.current_session().is_none());
    }
    assert_eq!(rig.pressed_after_drain(), vec![false, true, false, true]);
    assert_eq!(rig.received().len(), 2 * 640);
}

#[tokio::test(start_paused = true)]
async fn missing_virtual_cable_never_presses_the_voice_shortcut() {
    let rig = Rig::new(Options {
        missing_cable: true,
        ..Options::default()
    });
    let (reply, replies) = recorder();
    rig.bridge.start(reply);
    let session = rig.session();

    session.ready.wait().await;
    session.cleanup.wait().await;

    assert_eq!(rig.presses(), 0);
    assert_eq!(last(&replies)["state"], "error");
    assert!(last(&replies)["text"]
        .as_str()
        .unwrap()
        .contains("未找到虚拟音频输出设备"));
    assert!(rig.bridge.current_session().is_none());
}

#[tokio::test(start_paused = true)]
async fn disconnect_stops_forwarding_and_balances_the_start_key_with_one_stop_key() {
    let rig = Rig::new(Options::default());
    let (reply, replies) = recorder();
    rig.bridge.start(reply);
    let session = rig.session();
    session.ready.wait().await;

    rig.bridge.cancel();
    rig.bridge.audio(&pcm(0, 640));
    session.cleanup.wait().await;

    assert_eq!(rig.presses(), 2);
    assert!(rig.received().is_empty());
    assert!(
        rig.cable.lock().unwrap().cancelled,
        "the cable is cut short"
    );
    assert_eq!(last(&replies)["state"], "error");
    assert_eq!(last(&replies)["text"], "屏幕断开，已停止微信语音");
}

#[tokio::test(start_paused = true)]
async fn startup_buffering_is_bounded_while_wechat_is_unavailable() {
    let (release_open, open_gate) = mpsc::channel();
    let rig = Rig::new(Options {
        open_gate: Some(open_gate),
        ..Options::default()
    });
    let (reply, replies) = recorder();
    rig.bridge.start(reply);
    let session = rig.session();

    rig.bridge.audio(&pcm(0, 96_002));
    release_open.send(()).unwrap();
    session.ready.wait().await;
    session.cleanup.wait().await;

    assert_eq!(rig.presses(), 0);
    assert!(rig.bridge.current_session().is_none());
    assert!(last(&replies)["text"].as_str().unwrap().contains("过慢"));
}

#[tokio::test(start_paused = true)]
async fn disconnect_during_shortcut_startup_completes_cleanup_before_another_session() {
    let (entered, first_press_entered) = oneshot::channel();
    let (release, first_press_release) = mpsc::channel();
    let rig = Rig::new(Options {
        first_press: Some((entered, first_press_release)),
        ..Options::default()
    });
    let (reply, _) = recorder();
    rig.bridge.start(reply);
    let session = rig.session();
    first_press_entered.await.unwrap();

    rig.bridge.cancel();
    assert!(
        Arc::ptr_eq(&rig.session(), &session),
        "ownership lasts until cleanup"
    );
    let (second, second_replies) = recorder();
    rig.bridge.start(second);
    assert_eq!(last(&second_replies)["state"], "error");
    assert_eq!(rig.presses(), 1);

    release.send(()).unwrap();
    session.ready.wait().await;
    session.cleanup.wait().await;

    assert_eq!(rig.presses(), 2);
    assert!(rig.bridge.current_session().is_none());
    assert!(rig.received().is_empty());
}

#[tokio::test(start_paused = true)]
async fn computer_stop_stops_pcm_and_finishes_without_pressing_again() {
    let rig = Rig::new(Options::default());
    let (reply, replies) = recorder();
    rig.bridge.start(reply);
    let session = rig.session();
    session.ready.wait().await;

    rig.emit(WatchEvent::Active(false));
    tokio::time::sleep(Duration::from_millis(280)).await;
    assert!(
        Arc::ptr_eq(&rig.session(), &session),
        "idle before WeChat was active is not a stop"
    );

    rig.emit(WatchEvent::Active(true));
    rig.emit(WatchEvent::Active(false));
    tokio::time::sleep(Duration::from_millis(280)).await;
    session.cleanup.wait().await;

    rig.bridge.audio(&pcm(0, 640));
    assert!(rig.received().is_empty());
    assert_eq!(last(&replies)["state"], "finished");
    assert_eq!(last(&replies)["text"], "电脑已结束语音");
    assert_eq!(rig.presses(), 1);
    assert!(rig.bridge.current_session().is_none());
}

#[tokio::test(start_paused = true)]
async fn brief_input_transition_does_not_end_recording_and_close_during_tail_does_not_toggle_again()
{
    let rig = Rig::new(Options::default());
    let (reply, replies) = recorder();
    rig.bridge.start(reply.clone());
    let session = rig.session();
    session.ready.wait().await;

    rig.emit(WatchEvent::Active(true));
    rig.emit(WatchEvent::Active(false));
    rig.emit(WatchEvent::Active(true));
    tokio::time::sleep(Duration::from_millis(280)).await;
    assert!(Arc::ptr_eq(&rig.session(), &session));

    let stopping = rig.bridge.begin_stop(reply).expect("stop is accepted");
    rig.emit(WatchEvent::Active(false));
    stopping.await.unwrap();
    session.cleanup.wait().await;

    assert_eq!(rig.presses(), 1);
    assert_eq!(finished_count(&replies), 1);
    assert!(rig.bridge.current_session().is_none());
}

#[tokio::test(start_paused = true)]
async fn capture_query_failure_cancels_pending_stop_and_recovery_can_finish() {
    let rig = Rig::new(Options::default());
    let (reply, replies) = recorder();
    rig.bridge.start(reply);
    let session = rig.session();
    session.ready.wait().await;

    rig.emit(WatchEvent::Active(true));
    rig.emit(WatchEvent::Active(false));
    rig.emit(WatchEvent::Failed("capture device changed".into()));
    tokio::time::sleep(Duration::from_millis(280)).await;
    assert!(rig.bridge.busy(), "a failed query is not a confirmed stop");
    assert_eq!(finished_count(&replies), 0);
    assert_eq!(rig.count_events("wetype-monitor-error"), 1);

    rig.emit(WatchEvent::Active(false));
    tokio::time::sleep(Duration::from_millis(280)).await;
    session.cleanup.wait().await;
    assert_eq!(finished_count(&replies), 1);
    assert_eq!(rig.presses(), 1, "external stop must not reopen WeType");
    assert!(!rig.bridge.busy());
}

#[tokio::test(start_paused = true)]
async fn second_start_while_busy_is_refused() {
    let rig = Rig::new(Options::default());
    rig.bridge.start(recorder().0);
    let (reply, replies) = recorder();

    rig.bridge.start(reply);

    assert_eq!(last(&replies)["text"], "上一段语音还未结束");
    assert!(rig.bridge.busy());
}

#[test]
fn every_reply_serializes_type_first() {
    for state in ["partial", "finished", "error"] {
        let wire = serde_json::to_string(&crate::reply::voice(state, "text")).unwrap();
        assert!(wire.starts_with(r#"{"type":"voice","state":""#), "{wire}");
    }
}
