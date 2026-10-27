//! The board's connection: `GET /device` WebSocket with a token check, a state frame every second,
//! and recording controls and audio from the board.
use crate::runtime::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use panel_core::config::ScreenConfig;
use panel_core::debug_log::now_ms;
use panel_core::presence::RunningProviders;
use panel_core::state::Card;
use panel_voice::Reply;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::{self, UnboundedSender};

pub const MAX_MESSAGE_BYTES: usize = 8192;
const HEARTBEAT: Duration = Duration::from_secs(30);

/// What the app knows about the board. Sent to the UI as `device`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub connected: bool,
    pub ip: Option<String>,
    pub ssid: Option<String>,
    pub ssids: Vec<String>,
    pub fw: Option<i64>,
    pub connected_at: i64,
    pub last_seen: i64,
    pub rssi: Option<i64>,
}

struct Connection {
    id: u64,
    outbound: UnboundedSender<Message>,
}

/// Connections, the last hello, and the current voice owner. Shared by every board task.
pub struct DeviceHub {
    app: AppHandle,
    connections: Mutex<Vec<Connection>>,
    info: Mutex<DeviceInfo>,
    voice_owner: Mutex<Option<u64>>,
    next_id: AtomicU64,
    last_enter: Mutex<Option<Instant>>,
}

impl DeviceHub {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            connections: Mutex::new(Vec::new()),
            info: Mutex::new(DeviceInfo::default()),
            voice_owner: Mutex::new(None),
            next_id: AtomicU64::new(1),
            last_enter: Mutex::new(None),
        }
    }

    pub fn info(&self) -> DeviceInfo {
        self.info.lock().clone()
    }

    /// Sends one text frame to every connected board. Connections that have closed are dropped.
    pub fn broadcast(&self, text: &str) {
        self.connections
            .lock()
            .retain(|connection| connection.outbound.send(Message::Text(text.into())).is_ok());
    }

    /// Asks every board to delete one saved Wi-Fi profile.
    pub fn forget(&self, ssid: &str) {
        self.broadcast(&json!({"type": "forget", "ssid": ssid}).to_string());
    }

    fn connect(&self, outbound: UnboundedSender<Message>, peer_ip: String, now: i64) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.connections.lock().push(Connection { id, outbound });
        let info = {
            let mut info = self.info.lock();
            info.connected = true;
            info.ip = Some(peer_ip);
            info.connected_at = now;
            info.last_seen = now;
            info.clone()
        };
        self.emit(&info);
        id
    }

    fn disconnect(&self, id: u64) {
        let online = {
            let mut connections = self.connections.lock();
            connections.retain(|connection| connection.id != id);
            !connections.is_empty()
        };
        let info = {
            let mut info = self.info.lock();
            info.connected = online;
            info.clone()
        };
        self.emit(&info);
    }

    fn touch(&self, now: i64) {
        self.info.lock().last_seen = now;
    }

    fn record_rssi(&self, rssi: i64) {
        self.info.lock().rssi = Some(rssi);
    }

    /// The board's hello: its address, firmware, current network and remembered networks.
    fn hello(&self, message: &Value, now: i64) {
        let info = {
            let mut info = self.info.lock();
            if let Some(ip) = message.get("ip").and_then(Value::as_str) {
                info.ip = Some(ip.to_string());
            }
            if let Some(ssid) = message.get("ssid").and_then(Value::as_str) {
                info.ssid = Some(ssid.to_string());
            }
            if let Some(list) = message.get("ssids").and_then(Value::as_array) {
                info.ssids = list
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
            }
            if let Some(fw) = message.get("fw").and_then(Value::as_i64) {
                info.fw = Some(fw);
            }
            info.last_seen = now;
            info.clone()
        };
        self.emit(&info);
    }

    /// Only the connection that started a recording may stream audio or stop it.
    fn claim_voice(&self, id: u64) -> bool {
        let mut owner = self.voice_owner.lock();
        match *owner {
            Some(current) if current != id => false,
            _ => {
                *owner = Some(id);
                true
            }
        }
    }

    fn is_voice_owner(&self, id: u64) -> bool {
        *self.voice_owner.lock() == Some(id)
    }

    /// Clears the owner if it is `id`; returns whether it was.
    fn release_voice(&self, id: u64) -> bool {
        let mut owner = self.voice_owner.lock();
        if *owner == Some(id) {
            *owner = None;
            true
        } else {
            false
        }
    }

    fn emit(&self, info: &DeviceInfo) {
        let _ = self.app.emit("device", info);
    }
}

#[derive(Serialize)]
struct StateFrame<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    now: i64,
    config: &'a ScreenConfig,
    cards: &'a [Card],
    running: RunningProviders,
}

/// The state frame the board renders from; `type` must be the first JSON key.
pub fn state_frame(
    now: i64,
    config: &ScreenConfig,
    cards: &[Card],
    running: RunningProviders,
) -> String {
    serde_json::to_string(&StateFrame {
        kind: "state",
        now,
        config,
        cards,
        running,
    })
    .unwrap_or_default()
}

#[derive(Deserialize)]
struct DeviceQuery {
    token: Option<String>,
}

/// Routes served on the device port.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/device", get(upgrade))
        .with_state(state)
}

/// Binds the port and serves until the app exits. A bind failure is kept for the UI.
pub async fn run(state: Arc<AppState>) {
    let port = state.port;
    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            state.set_server_error(bind_message(port, &error));
            return;
        }
    };
    let app = router(Arc::clone(&state)).into_make_service_with_connect_info::<SocketAddr>();
    if let Err(error) = axum::serve(listener, app).await {
        state.set_server_error(format!("服务异常退出：{error}"));
    }
}

fn bind_message(port: u16, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::AddrInUse {
        format!("端口 {port} 已被占用，旧版 Node 服务可能仍在运行")
    } else {
        format!("端口 {port} 无法监听：{error}")
    }
}

/// The token is checked before the upgrade, so a wrong token never gets a socket.
async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(query): Query<DeviceQuery>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !token_matches(query.token.as_deref(), &state.identity.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| serve(socket, state, peer.ip().to_string()))
}

fn token_matches(given: Option<&str>, expected: &str) -> bool {
    given.is_some_and(|given| bool::from(given.as_bytes().ct_eq(expected.as_bytes())))
}

/// One board connection: reads its messages, writes queued frames, and pings it every 30 s. A
/// board that does not answer a ping is dropped.
async fn serve(socket: WebSocket, state: Arc<AppState>, peer_ip: String) {
    let (outbound, mut queued) = mpsc::unbounded_channel::<Message>();
    let id = state
        .devices
        .connect(outbound.clone(), peer_ip, now_ms() as i64);
    let _ = outbound.send(Message::Text(state.current_frame().into()));
    state.log.add(json!({"event": "device-connected"}));
    let (mut sink, mut stream) = socket.split();
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.tick().await;
    let mut alive = true;
    let mut close_code = None;
    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Close(frame))) => {
                    close_code = frame.map(|frame| frame.code);
                    break;
                }
                Some(Ok(message)) => {
                    state.devices.touch(now_ms() as i64);
                    if matches!(message, Message::Pong(_)) {
                        alive = true;
                    }
                    handle_message(&state, id, &outbound, message);
                }
                _ => break,
            },
            Some(message) = queued.recv() => {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
            _ = heartbeat.tick() => {
                if !alive {
                    break;
                }
                alive = false;
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
        }
    }
    finish(&state, id, close_code);
}

/// Ends a board connection. A recording it owned is cancelled.
fn finish(state: &AppState, id: u64, close_code: Option<u16>) {
    if state.devices.release_voice(id) {
        if let Some(voice) = &state.voice {
            voice.cancel();
        }
    }
    state.devices.disconnect(id);
    state
        .log
        .add(json!({"event": "device-disconnected", "code": close_code}));
}

fn handle_message(
    state: &AppState,
    id: u64,
    outbound: &UnboundedSender<Message>,
    message: Message,
) {
    match message {
        Message::Binary(pcm) => {
            if state.devices.is_voice_owner(id) {
                if let Some(voice) = &state.voice {
                    voice.audio(&pcm);
                }
            }
        }
        Message::Text(text) => {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                handle_json(state, id, outbound, &value);
            }
        }
        _ => {}
    }
}

/// Malformed or unknown messages are ignored, as the Node version did.
fn handle_json(state: &AppState, id: u64, outbound: &UnboundedSender<Message>, message: &Value) {
    let now = now_ms() as i64;
    match message.get("type").and_then(Value::as_str) {
        Some("voice-performance") => record_performance(state, message),
        Some("voice-debug") => record_button(state, message),
        Some("ack") => acknowledge(state, message),
        Some("hello") => state.devices.hello(message, now),
        Some("voice") => voice_action(state, id, outbound, message),
        Some("key") => enter_action(state, message),
        _ => {}
    }
}

/// Only paired device sockets reach this handler. Accept only plain Enter and suppress
/// accidental duplicate presses; do not expose arbitrary keys or command execution.
fn take_enter(message: &Value, last: &mut Option<Instant>, now: Instant) -> bool {
    if message.get("key").and_then(Value::as_str) != Some("Enter") {
        return false;
    }
    if last.is_some_and(|previous| now.duration_since(previous) < Duration::from_millis(250)) {
        return false;
    }
    *last = Some(now);
    true
}

fn enter_action(state: &AppState, message: &Value) {
    if !take_enter(
        message,
        &mut state.devices.last_enter.lock(),
        Instant::now(),
    ) {
        return;
    }
    let log = state.log.clone();
    tokio::task::spawn_blocking(move || {
        match panel_voice::send_enter() {
            Ok(()) => log.add(json!({"event": "keyboard-enter"})),
            Err(error) => log.add(json!({"event": "keyboard-error", "message": error})),
        };
    });
}

fn record_performance(state: &AppState, message: &Value) {
    let mut event = json!({
        "event": "device-performance",
        "recording": flag(message.get("recording")),
        "screenLocked": flag(message.get("screenLocked")),
    });
    for key in [
        "loopMs",
        "readMs",
        "sendMs",
        "readErrors",
        "sendErrors",
        "rssi",
    ] {
        event[key] = js_number(message.get(key));
    }
    state.log.add(event);
    if let Some(rssi) = js_number(message.get("rssi")).as_i64() {
        state.devices.record_rssi(rssi);
    }
}

fn record_button(state: &AppState, message: &Value) {
    let source = if message.get("source").and_then(Value::as_str) == Some("touch") {
        "屏幕触摸"
    } else {
        "BOOT"
    };
    state.log.add(json!({
        "event": "button",
        "source": source,
        "deviceMs": js_number(message.get("deviceMs")),
    }));
}

fn acknowledge(state: &AppState, message: &Value) {
    let provider = message.get("provider").and_then(Value::as_str);
    let id = message.get("id").and_then(Value::as_str);
    if let (Some(provider), Some(id)) = (provider, id) {
        let _ = state.acknowledge(provider, id);
    }
}

fn voice_action(state: &AppState, id: u64, outbound: &UnboundedSender<Message>, message: &Value) {
    match message.get("action").and_then(Value::as_str) {
        Some("start") => {
            if state.devices.claim_voice(id) {
                start_voice(state, outbound);
            }
        }
        Some("stop") if state.devices.is_voice_owner(id) => {
            if let Some(voice) = &state.voice {
                voice.stop(reply_to(outbound));
            }
        }
        _ => {}
    }
}

fn start_voice(state: &AppState, outbound: &UnboundedSender<Message>) {
    match &state.voice {
        Some(voice) => voice.start(reply_to(outbound)),
        None => {
            reply_to(outbound)(json!({"type": "voice", "state": "error", "text": "语音功能未启用"}))
        }
    }
}

/// Replies go to this connection; once it has closed they are dropped.
fn reply_to(outbound: &UnboundedSender<Message>) -> Reply {
    let outbound = outbound.clone();
    Arc::new(move |value: Value| {
        let _ = outbound.send(Message::Text(value.to_string().into()));
    })
}

/// JavaScript's `Number(value) || 0`: numbers and numeric strings pass through, anything else is 0.
/// Whole numbers stay whole so the UI prints `12`, not `12.0`.
fn js_number(value: Option<&Value>) -> Value {
    let number = match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(flag)) => Some(f64::from(u8::from(*flag))),
        _ => None,
    };
    match number {
        Some(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 9e15 => json!(n as i64),
        Some(n) if n.is_finite() => json!(n),
        _ => json!(0),
    }
}

/// JavaScript truthiness for the fields the board sends as flags.
fn flag(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_button_rejects_other_keys_and_suppresses_duplicate_presses() {
        let now = Instant::now();
        let mut last = None;
        for invalid in [
            json!({}),
            json!({"key":"A"}),
            json!({"key":["Enter"]}),
            json!({"key":"Return"}),
        ] {
            assert!(!take_enter(&invalid, &mut last, now));
            assert!(last.is_none());
        }
        let enter = json!({"type":"key", "key":"Enter"});
        assert!(take_enter(&enter, &mut last, now));
        assert!(!take_enter(
            &enter,
            &mut last,
            now + Duration::from_millis(100)
        ));
        assert!(!take_enter(
            &enter,
            &mut last,
            now + Duration::from_millis(249)
        ));
        assert!(take_enter(
            &enter,
            &mut last,
            now + Duration::from_millis(250)
        ));
    }

    #[test]
    fn token_comparison_is_exact() {
        assert!(token_matches(Some("abc"), "abc"));
        assert!(!token_matches(Some("abd"), "abc"));
        assert!(!token_matches(Some("ab"), "abc"));
        assert!(!token_matches(None, "abc"));
    }

    #[test]
    fn numbers_follow_javascript_or_zero() {
        assert_eq!(js_number(Some(&json!(12))), json!(12));
        assert_eq!(js_number(Some(&json!(12.5))), json!(12.5));
        assert_eq!(js_number(Some(&json!("-43"))), json!(-43));
        assert_eq!(js_number(Some(&json!("abc"))), json!(0));
        assert_eq!(js_number(None), json!(0));
        assert_eq!(js_number(Some(&json!(null))), json!(0));
    }

    #[test]
    fn flags_follow_javascript_truthiness() {
        assert!(flag(Some(&json!(true))));
        assert!(!flag(Some(&json!(0))));
        assert!(!flag(Some(&json!(""))));
        assert!(!flag(None));
        assert!(flag(Some(&json!("yes"))));
    }

    #[test]
    fn state_frame_starts_with_type_and_keeps_firmware_fields() {
        let frame = state_frame(
            42,
            &ScreenConfig::default(),
            &[],
            RunningProviders::default(),
        );
        assert!(frame.starts_with(r##"{"type":"state","now":42,"config":{"split":50,"brightness":160,"claudeColor":"#df9877""##), "{frame}");
        let parsed: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(parsed["cards"], json!([]));
        assert_eq!(
            parsed["running"],
            json!({"claude":false,"codex":false,"deepseek":false})
        );
    }

    #[test]
    fn three_provider_frame_fits_the_board_message_limit_and_keeps_order() {
        use panel_core::state::{Event, Provider, State};
        let mut state = State::default();
        for provider in Provider::ALL {
            for i in 0..3 {
                state.apply(&Event {
                    provider,
                    session: format!("task-{i}"),
                    time: "2026-10-27T03:00:00Z".into(),
                    event: "UserPromptSubmit".into(),
                    cwd: Some("D:/workspace/project".into()),
                    tool: None,
                    notification: None,
                    title: Some("任务标题".repeat(20)),
                });
            }
        }
        let snapshot = state.snapshot(1_793_070_000_000);
        let running = RunningProviders {
            codex: true,
            ..Default::default()
        };
        let frame = state_frame(42, &ScreenConfig::default(), &snapshot.cards, running);
        assert!(frame.len() < MAX_MESSAGE_BYTES, "{} bytes", frame.len());
        let parsed: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(parsed["cards"][0]["provider"], "claude");
        assert_eq!(parsed["cards"][1]["provider"], "codex");
        assert_eq!(parsed["cards"][2]["provider"], "deepseek");
        // Offline providers keep their history and stable protocol indices.
        assert_eq!(
            parsed["running"],
            json!({"claude":false,"codex":true,"deepseek":false})
        );
    }
}
