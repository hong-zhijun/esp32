//! Session state machine (port of `state.cjs`). Pure logic with no I/O: sources feed
//! normalized [`Event`]s, and the UI and the board read [`Snapshot`]s built for a given `now`.
use chrono::DateTime;
use indexmap::IndexMap;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// A working or waiting session silent for longer than this is shown as `unknown`. It is never
/// completed automatically.
pub const STALE_AFTER_MS: i64 = 5 * 60 * 1000;
/// Sessions with nothing pending drop out of snapshots after this long.
pub const RETAIN_MS: i64 = 24 * 60 * 60 * 1000;
/// Titles are capped at this many UTF-16 units, like the Node version's `slice(0, 90)`.
pub const TITLE_UNITS: usize = 90;
const TOOL_UNITS: usize = 70;
const ROW_CHARS: usize = 18;
const ROW_COUNT: usize = 3;
const WAITING_NOTIFICATIONS: [&str; 3] = ["permission_prompt", "idle_prompt", "elicitation_dialog"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
    Deepseek,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Claude, Provider::Codex, Provider::Deepseek];

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "deepseek" => Some(Self::Deepseek),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Deepseek => "deepseek",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Working,
    Waiting,
    Done,
    Interrupted,
    Unknown,
}

/// One normalized event. `time` is ISO 8601, as written by the hook or the rollout file.
#[derive(Debug, Clone)]
pub struct Event {
    pub provider: Provider,
    pub session: String,
    pub time: String,
    /// Hook-style name: `UserPromptSubmit`, `PreToolUse`, `Stop`, ...
    pub event: String,
    pub cwd: Option<String>,
    pub tool: Option<String>,
    pub notification: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub provider: Provider,
    pub state: Phase,
    pub project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub tool: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub updated_at: i64,
    pub unread: bool,
}

impl Session {
    fn new(provider: Provider, id: &str) -> Self {
        Self {
            id: id.to_string(),
            provider,
            state: Phase::Idle,
            project: String::new(),
            title: None,
            tool: String::new(),
            started_at: 0,
            ended_at: 0,
            updated_at: 0,
            unread: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub id: String,
    pub project: String,
    pub state: Phase,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    #[serde(flatten)]
    pub session: Session,
    pub count: usize,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    /// Claude, Codex, then DeepSeek; the board relies on this order.
    pub cards: [Card; 3],
    pub sessions: Vec<Session>,
}

#[derive(Debug, Default)]
pub struct State {
    sessions: IndexMap<String, Session>,
    selected: HashMap<Provider, String>,
}

impl State {
    /// Applies one event. An event older than the newest one already seen for its session is dropped.
    pub fn apply(&mut self, event: &Event) {
        if event.session.is_empty() {
            return;
        }
        let Some(time) = parse_time_ms(&event.time) else {
            return;
        };
        let key = session_key(event.provider, &event.session);
        if self
            .sessions
            .get(&key)
            .is_some_and(|known| time < known.updated_at)
        {
            return;
        }
        let session = self
            .sessions
            .entry(key)
            .or_insert_with(|| Session::new(event.provider, &event.session));
        session.updated_at = time;
        if let Some(cwd) = non_empty(&event.cwd) {
            session.project = basename(cwd).to_string();
        }
        if let Some(title) = non_empty(&event.title) {
            session.title = Some(take_units(title, TITLE_UNITS));
        }
        apply_transition(session, event, time);
    }

    /// The user cleared a completion notice.
    pub fn acknowledge(&mut self, provider: Provider, id: &str) {
        if let Some(session) = self.sessions.get_mut(&session_key(provider, id)) {
            session.unread = false;
        }
    }

    /// Clears `unread` wherever an acknowledgement covers the session's latest update.
    pub fn apply_acknowledged(&mut self, acknowledged: &BTreeMap<String, i64>) {
        for (key, session) in self.sessions.iter_mut() {
            if acknowledged
                .get(key)
                .is_some_and(|&at| at >= session.updated_at)
            {
                session.unread = false;
            }
        }
    }

    /// Pins a session to the front of its card. An empty id removes the pin.
    pub fn select(&mut self, provider: Provider, id: &str) {
        self.selected.insert(provider, id.to_string());
    }

    /// The session pinned to the front of `provider`'s card, if any.
    pub fn pinned(&self, provider: Provider) -> Option<&str> {
        self.selected
            .get(&provider)
            .map(String::as_str)
            .filter(|id| !id.is_empty())
    }

    pub fn session(&self, provider: Provider, id: &str) -> Option<&Session> {
        self.sessions.get(&session_key(provider, id))
    }

    /// A title edit changes display metadata, not the activity/completion timestamp.
    pub fn set_title(&mut self, provider: Provider, id: &str, title: &str) {
        if let Some(session) = self.sessions.get_mut(&session_key(provider, id)) {
            session.title = Some(take_units(title, TITLE_UNITS));
        }
    }

    /// Lets the sources attach titles to sessions they discovered.
    pub fn sessions_mut(&mut self) -> impl Iterator<Item = &mut Session> {
        self.sessions.values_mut()
    }

    pub fn snapshot(&self, now: i64) -> Snapshot {
        let sessions: Vec<Session> = self
            .sessions
            .values()
            .filter(|session| {
                (session.state == Phase::Done && session.unread)
                    || now - session.updated_at < RETAIN_MS
            })
            .map(|session| view(session, now))
            .collect();
        let cards = Provider::ALL.map(|provider| self.card(provider, &sessions));
        Snapshot { cards, sessions }
    }

    fn card(&self, provider: Provider, sessions: &[Session]) -> Card {
        let mut rows: Vec<&Session> = sessions
            .iter()
            .filter(|session| session.provider == provider)
            .collect();
        rows.sort_by(|a, b| {
            urgency(b)
                .cmp(&urgency(a))
                .then(b.updated_at.cmp(&a.updated_at))
        });
        let chosen = self
            .selected
            .get(&provider)
            .and_then(|id| rows.iter().copied().find(|session| session.id == *id))
            .or_else(|| rows.first().copied());
        let list: Vec<Row> = rows
            .iter()
            .take(ROW_COUNT)
            .map(|session| Row {
                id: session.id.clone(),
                project: row_label(session),
                state: session.state,
            })
            .collect();
        Card {
            session: chosen
                .cloned()
                .unwrap_or_else(|| Session::new(provider, "")),
            count: rows.len(),
            rows: list,
        }
    }
}

/// The per-event transitions of `state.cjs`.
fn apply_transition(session: &mut Session, event: &Event, time: i64) {
    match event.event.as_str() {
        "UserPromptSubmit" => {
            session.state = Phase::Working;
            session.started_at = time;
            session.ended_at = 0;
            session.unread = false;
            session.tool.clear();
        }
        "PreToolUse" | "PostToolUse" => {
            session.state = Phase::Working;
            session.tool = take_units(event.tool.as_deref().unwrap_or(""), TOOL_UNITS);
            if session.started_at == 0 {
                session.started_at = time;
            }
        }
        "PermissionRequest" => session.state = Phase::Waiting,
        "Notification"
            if event
                .notification
                .as_deref()
                .is_some_and(|kind| WAITING_NOTIFICATIONS.contains(&kind)) =>
        {
            session.state = Phase::Waiting;
        }
        "Stop" => {
            session.state = Phase::Done;
            session.ended_at = time;
            session.unread = true;
            session.tool.clear();
        }
        "Interrupt" => {
            session.state = Phase::Interrupted;
            session.ended_at = time;
            session.tool.clear();
        }
        "Error" => {
            session.state = Phase::Unknown;
            session.ended_at = time;
            session.tool.clear();
        }
        "SessionEnd" => {
            session.state = if session.unread {
                Phase::Done
            } else {
                Phase::Idle
            };
            if session.ended_at == 0 {
                session.ended_at = time;
            }
        }
        _ => {}
    }
}

/// What a snapshot shows for a stored session: stale activity reads as `unknown`, and a
/// completion that was already cleared reads as `idle`.
fn view(session: &Session, now: i64) -> Session {
    let mut view = session.clone();
    let stale = now - session.updated_at > STALE_AFTER_MS;
    if matches!(session.state, Phase::Working | Phase::Waiting) && stale {
        view.state = Phase::Unknown;
    } else if session.state == Phase::Done && !session.unread {
        view.state = Phase::Idle;
    }
    view
}

/// Waiting first, then running, then unread completions, then everything else.
fn urgency(session: &Session) -> u8 {
    match session.state {
        Phase::Waiting => 4,
        Phase::Working => 3,
        _ if session.unread => 2,
        _ => 1,
    }
}

/// Row label: the task title when known, otherwise the project, otherwise the id.
fn row_label(session: &Session) -> String {
    let text = [
        session.title.as_deref(),
        Some(session.project.as_str()),
        Some(session.id.as_str()),
    ]
    .into_iter()
    .flatten()
    .find(|text| !text.is_empty())
    .unwrap_or("");
    text.chars().take(ROW_CHARS).collect()
}

/// Key of a session in acknowledgement files and the state map, e.g. `claude:<id>`.
pub fn session_key(provider: Provider, id: &str) -> String {
    format!("{}:{}", provider.as_str(), id)
}

/// ISO 8601 text to epoch milliseconds, the way `Date.parse` read it for the Node version.
pub fn parse_time_ms(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp_millis())
}

/// The first `max_units` UTF-16 code units of `text`, matching JavaScript's `slice(0, max)`.
pub fn take_units(text: &str, max_units: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().take(max_units).collect();
    String::from_utf16_lossy(&units)
}

/// Last segment of a Windows or POSIX path.
pub fn basename(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000_000;
    const DAY: i64 = 24 * 60 * 60 * 1000;

    fn iso(ms: i64) -> String {
        DateTime::from_timestamp_millis(ms)
            .unwrap()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    }

    fn event(provider: Provider, session: &str, name: &str, at: i64) -> Event {
        Event {
            provider,
            session: session.into(),
            time: iso(at),
            event: name.into(),
            cwd: None,
            tool: None,
            notification: None,
            title: None,
        }
    }

    #[test]
    fn parallel_sessions_stay_separate_and_acks_do_not_finish_work() {
        let mut state = State::default();
        state.apply(&event(Provider::Claude, "a", "UserPromptSubmit", T0));
        state.apply(&event(Provider::Claude, "b", "Stop", T0));
        state.acknowledge(Provider::Claude, "b");
        assert_eq!(
            state.session(Provider::Claude, "a").unwrap().state,
            Phase::Working
        );
        assert!(!state.session(Provider::Claude, "b").unwrap().unread);
    }

    #[test]
    fn unknown_notifications_do_not_imply_approval_and_old_events_cannot_overwrite_completion() {
        let mut state = State::default();
        state.apply(&event(Provider::Claude, "x", "Stop", T0));
        state.apply(&event(Provider::Claude, "x", "PreToolUse", T0 - 1));
        let mut auth = event(Provider::Claude, "x", "Notification", T0 + 1);
        auth.notification = Some("auth_success".into());
        state.apply(&auth);
        assert_eq!(state.snapshot(T0 + 2).cards[0].session.state, Phase::Done);
    }

    #[test]
    fn waiting_notifications_are_recognised() {
        let mut state = State::default();
        let mut prompt = event(Provider::Claude, "x", "Notification", T0);
        prompt.notification = Some("permission_prompt".into());
        state.apply(&prompt);
        assert_eq!(
            state.session(Provider::Claude, "x").unwrap().state,
            Phase::Waiting
        );
    }

    #[test]
    fn stale_work_is_unknown_never_completed() {
        let mut state = State::default();
        state.apply(&event(Provider::Codex, "x", "UserPromptSubmit", T0));
        assert_eq!(
            state.snapshot(T0 + 300_001).cards[1].session.state,
            Phase::Unknown
        );
        assert_eq!(
            state.session(Provider::Codex, "x").unwrap().state,
            Phase::Working
        );
    }

    #[test]
    fn completion_survives_until_acknowledged_or_restarted() {
        let mut state = State::default();
        state.apply(&event(Provider::Codex, "x", "Stop", T0));
        for elapsed in [600_001, 2 * DAY] {
            let card = &state.snapshot(T0 + elapsed).cards[1];
            assert_eq!(card.session.state, Phase::Done);
            assert!(card.session.unread);
        }
        state.acknowledge(Provider::Codex, "x");
        assert_eq!(
            state.snapshot(T0 + 600_001).cards[1].session.state,
            Phase::Idle
        );
        assert!(state.snapshot(T0 + 2 * DAY).sessions.is_empty());
        state.apply(&event(
            Provider::Codex,
            "x",
            "UserPromptSubmit",
            T0 + 2 * DAY,
        ));
        assert_eq!(
            state.snapshot(T0 + 2 * DAY).cards[1].session.state,
            Phase::Working
        );
    }

    #[test]
    fn each_card_lists_three_sessions_most_urgent_first() {
        let mut state = State::default();
        for (offset, (id, name)) in [
            ("a", "Stop"),
            ("b", "UserPromptSubmit"),
            ("c", "Stop"),
            ("d", "Stop"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut event = event(Provider::Claude, id, name, T0 + offset as i64);
            event.cwd = Some(format!("D:/w/{id}"));
            state.apply(&event);
        }
        let rows = &state.snapshot(T0 + 10).cards[0].rows;
        assert_eq!(rows.len(), 3);
        assert_eq!(
            (rows[0].id.as_str(), rows[0].project.as_str(), rows[0].state),
            ("b", "b", Phase::Working)
        );
    }

    #[test]
    fn tasks_in_one_project_keep_separate_rows_and_titles() {
        let mut state = State::default();
        for (id, name) in [("a", "UserPromptSubmit"), ("b", "Stop"), ("c", "Stop")] {
            let mut event = event(Provider::Codex, id, name, T0);
            event.cwd = Some("/work/shared".into());
            event.title = Some(format!("任务 {id}"));
            state.apply(&event);
        }
        let rows = &state.snapshot(T0 + 1).cards[1].rows;
        assert_eq!(rows.len(), 3);
        let ids: std::collections::HashSet<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(rows[0].project, "任务 a");
    }

    #[test]
    fn row_labels_are_capped_at_eighteen_characters() {
        let mut state = State::default();
        let mut event = event(Provider::Claude, "x", "Stop", T0);
        event.title = Some("一二三四五六七八九十一二三四五六七八九十".into());
        state.apply(&event);
        assert_eq!(
            state.snapshot(T0).cards[0].rows[0].project.chars().count(),
            ROW_CHARS
        );
    }

    #[test]
    fn pinned_session_leads_its_card_until_unpinned() {
        let mut state = State::default();
        state.apply(&event(Provider::Claude, "a", "Stop", T0));
        state.apply(&event(Provider::Claude, "b", "UserPromptSubmit", T0 + 1));
        state.select(Provider::Claude, "a");
        assert_eq!(state.snapshot(T0 + 2).cards[0].session.id, "a");
        state.select(Provider::Claude, "");
        assert_eq!(state.snapshot(T0 + 2).cards[0].session.id, "b");
    }

    #[test]
    fn empty_card_is_idle_with_no_rows() {
        let card = &State::default().snapshot(T0).cards[1];
        assert_eq!(card.session.state, Phase::Idle);
        assert_eq!((card.count, card.rows.len()), (0, 0));
    }

    #[test]
    fn events_without_valid_time_or_session_are_ignored() {
        let mut state = State::default();
        let mut no_time = event(Provider::Claude, "x", "Stop", T0);
        no_time.time = "now".into();
        state.apply(&no_time);
        state.apply(&event(Provider::Claude, "", "Stop", T0));
        assert!(state.snapshot(T0).sessions.is_empty());
    }

    #[test]
    fn acknowledgements_from_file_clear_only_current_updates() {
        let mut state = State::default();
        state.apply(&event(Provider::Codex, "x", "Stop", T0));
        let mut acked = BTreeMap::new();
        acked.insert("codex:x".to_string(), T0 - 1);
        state.apply_acknowledged(&acked);
        assert!(state.session(Provider::Codex, "x").unwrap().unread);
        acked.insert("codex:x".to_string(), T0);
        state.apply_acknowledged(&acked);
        assert!(!state.session(Provider::Codex, "x").unwrap().unread);
    }

    #[test]
    fn text_limits_count_utf16_units_and_paths_split_on_both_separators() {
        assert_eq!(take_units("标题很长的任务", 3), "标题很");
        assert_eq!(basename("D:\\w\\esp32\\"), "esp32");
        assert_eq!(basename("/work/shared"), "shared");
        assert_eq!(basename("/"), "");
    }

    #[test]
    fn phase_serializes_as_protocol_names() {
        assert_eq!(serde_json::to_value(Phase::Unknown).unwrap(), "unknown");
        assert_eq!(serde_json::to_value(Provider::Codex).unwrap(), "codex");
    }

    #[test]
    fn third_provider_is_independent_and_can_be_pinned_and_acknowledged() {
        let mut state = State::default();
        state.apply(&event(Provider::Codex, "same-id", "UserPromptSubmit", T0));
        state.apply(&event(Provider::Deepseek, "same-id", "Stop", T0));
        state.apply(&event(
            Provider::Deepseek,
            "waiting",
            "PermissionRequest",
            T0 + 1,
        ));
        assert_eq!(
            state
                .snapshot(T0 + 2)
                .cards
                .map(|card| card.session.provider),
            Provider::ALL
        );
        assert_eq!(state.snapshot(T0 + 2).cards[2].session.id, "waiting");
        state.select(Provider::Deepseek, "same-id");
        state.acknowledge(Provider::Deepseek, "same-id");
        assert_eq!(state.snapshot(T0 + 2).cards[2].session.state, Phase::Idle);
        assert_eq!(
            state.snapshot(T0 + 2).cards[1].session.state,
            Phase::Working
        );
    }
}
