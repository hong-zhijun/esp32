//! Shared application state: persisted settings, session state and the once-per-second refresh
//! that feeds the UI and the board.
use crate::device_server::{self, DeviceHub, DeviceInfo};
use crate::provision::Target;
use panel_core::config::{AppConfig, ScreenConfig, VoiceConfig};
use panel_core::debug_log::{now_ms, DebugLog};
use panel_core::identity::{self, DeviceIdentity};
use panel_core::paths::{self, DataPaths};
use panel_core::presence::{ProcessMonitor, RunningProviders};
use panel_core::sources::Sources;
use panel_core::state::{session_key, Card, Provider, Session, State};
use panel_core::store;
use panel_voice::{VoiceBridge, VoiceSettings};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Everything the UI renders, sent as the `snapshot` event once a second.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSnapshot {
    pub now: i64,
    pub config: AppConfig,
    pub cards: [Card; 3],
    pub running: RunningProviders,
    pub keyboard: panel_voice::KeyboardStatus,
    pub sessions: Vec<Session>,
    pub pinned: Pins,
    pub device: DeviceInfo,
    pub server: ServerInfo,
    pub source_errors: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pins {
    pub claude: Option<String>,
    pub codex: Option<String>,
    pub deepseek: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ServerInfo {
    pub port: u16,
    pub error: Option<String>,
}

/// One refresh: the UI snapshot plus the frame for the board.
pub struct Refresh {
    pub ui: UiSnapshot,
    pub device_frame: String,
}

/// Lock-protected session state. The sources, state and acknowledgements change together.
struct Runtime {
    config: AppConfig,
    state: State,
    sources: Sources,
    acknowledged: BTreeMap<String, i64>,
    processes: ProcessMonitor,
}

pub struct AppState {
    pub paths: DataPaths,
    pub identity: DeviceIdentity,
    pub log: DebugLog,
    pub devices: DeviceHub,
    /// `None` when `AI_PANEL_NO_VOICE` disables the voice bridge (development only).
    pub voice: Option<VoiceBridge>,
    pub port: u16,
    provisioning: AtomicBool,
    server_error: Mutex<Option<String>>,
    runtime: Mutex<Runtime>,
}

/// Clears the provisioning flag when dropped, so an error or panic cannot leave it stuck.
pub struct ProvisioningGuard<'a>(&'a AtomicBool);

impl Drop for ProvisioningGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Reads `config.json`, replacing invalid sections with defaults.
fn load_config(paths: &DataPaths) -> AppConfig {
    store::read_json::<AppConfig>(&paths.config())
        .unwrap_or_default()
        .sanitized()
}

/// The voice bridge. Built inside the async runtime because it starts its own tasks.
fn build_voice(config: &AppConfig, log: &DebugLog) -> VoiceBridge {
    let settings = voice_settings(&config.voice);
    tauri::async_runtime::block_on(async { VoiceBridge::new(settings, log.clone()) })
}

impl AppState {
    pub fn new(
        paths: DataPaths,
        port: u16,
        devices: DeviceHub,
        log: DebugLog,
        voice_enabled: bool,
    ) -> anyhow::Result<Self> {
        let identity = identity::load_or_create(&paths.device())?;
        let config = load_config(&paths);
        let voice = voice_enabled.then(|| build_voice(&config, &log));
        let acknowledged = store::read_json(&paths.acknowledged()).unwrap_or_default();
        let sources = Sources::new(&paths, paths::codex_home());
        let mut processes = ProcessMonitor::default();
        processes.poll();
        Ok(Self {
            identity,
            log,
            devices,
            voice,
            port,
            provisioning: AtomicBool::new(false),
            server_error: Mutex::new(None),
            runtime: Mutex::new(Runtime {
                config,
                state: State::default(),
                sources,
                acknowledged,
                processes,
            }),
            paths,
        })
    }

    pub fn voice_settings(&self) -> VoiceSettings {
        voice_settings(&self.runtime.lock().config.voice)
    }

    pub fn config(&self) -> AppConfig {
        self.runtime.lock().config.clone()
    }

    pub fn set_server_error(&self, message: String) {
        *self.server_error.lock() = Some(message);
    }

    pub fn provision_target(&self) -> Target {
        Target {
            token: self.identity.token.clone(),
            computer_id: self.identity.computer_id.clone(),
            server_port: self.port,
        }
    }

    /// Fails when a provisioning run is already in progress.
    pub fn begin_provisioning(&self) -> Result<ProvisioningGuard<'_>, String> {
        if self.provisioning.swap(true, Ordering::SeqCst) {
            return Err("USB 配网正在进行".into());
        }
        Ok(ProvisioningGuard(&self.provisioning))
    }

    pub fn save_screen(&self, screen: ScreenConfig) -> Result<(), String> {
        screen.validate()?;
        let config = {
            let mut runtime = self.runtime.lock();
            runtime.config.screen = screen;
            runtime.config.clone()
        };
        store::write_json(&self.paths.config(), &config).map_err(|error| error.to_string())?;
        self.push_device_frame();
        Ok(())
    }

    pub fn save_voice(&self, voice: VoiceConfig) -> Result<(), String> {
        voice.hotkey.validate()?;
        let config = {
            let mut runtime = self.runtime.lock();
            runtime.config.voice = voice;
            runtime.config.clone()
        };
        store::write_json(&self.paths.config(), &config).map_err(|error| error.to_string())?;
        if let Some(bridge) = &self.voice {
            bridge.update_settings(voice_settings(&config.voice));
        }
        Ok(())
    }

    /// Pins a session to the top of its card; an empty id removes the pin.
    pub fn select(&self, provider: &str, id: &str) -> Result<(), String> {
        let provider = parse_provider(provider)?;
        self.runtime.lock().state.select(provider, id);
        self.push_device_frame();
        Ok(())
    }

    /// Clears a completion notice. The acknowledgement is remembered until the session updates.
    pub fn acknowledge(&self, provider: &str, id: &str) -> Result<(), String> {
        let provider = parse_provider(provider)?;
        let acknowledged = {
            let mut runtime = self.runtime.lock();
            let Some(updated_at) = runtime
                .state
                .session(provider, id)
                .map(|session| session.updated_at)
            else {
                return Ok(());
            };
            runtime
                .acknowledged
                .insert(session_key(provider, id), updated_at);
            runtime.state.acknowledge(provider, id);
            runtime.acknowledged.clone()
        };
        store::write_json(&self.paths.acknowledged(), &acknowledged)
            .map_err(|error| error.to_string())?;
        self.push_device_frame();
        Ok(())
    }

    /// Builds the current view. With `poll` set, the sources are read first.
    pub fn refresh(&self, now: i64, poll: bool) -> Refresh {
        let (config, snapshot, pinned, source_errors, running) = {
            let mut runtime = self.runtime.lock();
            let Runtime {
                config,
                state,
                sources,
                acknowledged,
                processes,
            } = &mut *runtime;
            if poll {
                processes.poll();
                sources.tick(state, now);
            }
            state.apply_acknowledged(acknowledged);
            let pinned = Pins {
                claude: state.pinned(Provider::Claude).map(str::to_string),
                codex: state.pinned(Provider::Codex).map(str::to_string),
                deepseek: state.pinned(Provider::Deepseek).map(str::to_string),
            };
            (
                config.clone(),
                state.snapshot(now),
                pinned,
                sources.errors().to_vec(),
                processes.running(),
            )
        };
        let device_frame =
            device_server::state_frame(now, &config.screen, &snapshot.cards, running);
        let ui = UiSnapshot {
            now,
            config,
            cards: snapshot.cards,
            running,
            keyboard: panel_voice::keyboard_status(),
            sessions: snapshot.sessions,
            pinned,
            device: self.devices.info(),
            server: ServerInfo {
                port: self.port,
                error: self.server_error.lock().clone(),
            },
            source_errors,
        };
        Refresh { ui, device_frame }
    }

    /// The state frame as of now, sent to a board when it connects.
    pub fn current_frame(&self) -> String {
        let now = now_ms() as i64;
        let runtime = self.runtime.lock();
        device_server::state_frame(
            now,
            &runtime.config.screen,
            &runtime.state.snapshot(now).cards,
            runtime.processes.running(),
        )
    }

    /// Sends the current state to every board right away.
    fn push_device_frame(&self) {
        self.devices.broadcast(&self.current_frame());
    }
}

/// Refreshes the UI and the boards once a second for the life of the app.
pub fn spawn_ticker(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            let refresh = state.refresh(now_ms() as i64, true);
            state.devices.broadcast(&refresh.device_frame);
            let _ = app.emit("snapshot", &refresh.ui);
        }
    });
}

fn parse_provider(text: &str) -> Result<Provider, String> {
    Provider::parse(text).ok_or_else(|| "未知的会话来源".to_string())
}

fn voice_settings(voice: &VoiceConfig) -> VoiceSettings {
    VoiceSettings {
        device: voice.device_name(),
        hotkey: voice.hotkey.clone(),
    }
}
