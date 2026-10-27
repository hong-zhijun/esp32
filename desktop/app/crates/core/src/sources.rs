//! Event sources (port of `sources.cjs`): the Claude hook log and Codex rollout files.
//! Both are polled: each `tick` reads whatever was appended since the previous one.
use crate::paths::DataPaths;
use crate::state::{take_units, Event, Provider, State, TITLE_UNITS};
use crate::store;
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Rollout discovery period. Each scan walks every date folder, so it stays this slow.
const SCAN_EVERY_MS: i64 = 10_000;
/// Rollouts that were never tracked and are older than this are ignored.
const RECENT_MS: i64 = 24 * 60 * 60 * 1000;
/// The first read of a large rollout only covers this tail.
const TAIL_BYTES: u64 = 512 * 1024;
/// Most bytes read from one file in one tick.
const READ_CHUNK: u64 = 2 * 1024 * 1024;

/// Polls the Claude hook log and every tracked Codex rollout.
pub struct Sources {
    claude_titles: crate::claude_titles::ClaudeTitles,
    deepseek: crate::deepseek::DeepseekSource,
    claude_event_files: Vec<PathBuf>,
    tracked_file: PathBuf,
    codex_home: PathBuf,
    files: IndexMap<PathBuf, FileState>,
    tracked: BTreeSet<String>,
    titles: HashMap<String, String>,
    last_scan: i64,
    errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Claude,
    Codex,
}

#[derive(Debug)]
struct FileState {
    kind: Kind,
    /// Bytes consumed so far.
    offset: u64,
    /// Trailing bytes of a line that has not ended yet.
    carry: Vec<u8>,
    /// Codex `session_meta` payload: session id, cwd and subagent marker.
    meta: Value,
    /// Internal Codex subagent rollouts are never shown.
    ignored: bool,
    /// After a tail seek the first line is usually cut in half, so it is dropped.
    skip_first: bool,
}

impl FileState {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            offset: 0,
            carry: Vec::new(),
            meta: Value::Null,
            ignored: false,
            skip_first: false,
        }
    }
}

impl Sources {
    pub fn new(paths: &DataPaths, codex_home: PathBuf) -> Self {
        Self::with_homes(
            paths,
            codex_home,
            crate::deepseek::home_dir(),
            crate::claude_titles::home_dir(),
        )
        .with_claude_event_files(crate::paths::claude_hook_event_files(paths))
    }

    pub fn with_homes(
        paths: &DataPaths,
        codex_home: PathBuf,
        deepseek_home: PathBuf,
        claude_home: PathBuf,
    ) -> Self {
        Self {
            claude_titles: crate::claude_titles::ClaudeTitles::new(claude_home),
            deepseek: crate::deepseek::DeepseekSource::with_home(paths, deepseek_home),
            claude_event_files: vec![paths.events()],
            tracked_file: paths.tracked_codex(),
            codex_home,
            files: IndexMap::new(),
            tracked: store::read_json(&paths.tracked_codex()).unwrap_or_default(),
            titles: HashMap::new(),
            last_scan: 0,
            errors: Vec::new(),
        }
    }

    /// Additional hook locations are registered even before they exist, so a
    /// newly launched packaged Claude writer is picked up on the next tick.
    pub fn with_claude_event_files(mut self, files: Vec<PathBuf>) -> Self {
        for file in files {
            if !self.claude_event_files.contains(&file) {
                self.claude_event_files.push(file);
            }
        }
        for file in &self.claude_event_files {
            self.files
                .entry(file.clone())
                .or_insert_with(|| FileState::new(Kind::Claude));
        }
        self
    }

    /// The most recent read or save problem, for diagnostics.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Rescans for rollouts when due, then reads new lines from every watched file.
    pub fn tick(&mut self, state: &mut State, now: i64) {
        self.errors.retain(|error| !error.starts_with("deepseek_"));
        if let Err(error) = self.deepseek.tick(state, now) {
            self.errors = vec![error];
        }
        if now - self.last_scan > SCAN_EVERY_MS {
            self.scan(state, now);
            self.last_scan = now;
        }
        for (path, file) in self.files.iter_mut() {
            if let Err(code) = poll_file(path, file, state, &self.titles) {
                self.errors = vec![code];
            }
        }
        self.claude_titles.tick(state, now);
    }

    /// Finds rollouts under `sessions/`, tracks the recent ones, and refreshes titles.
    fn scan(&mut self, state: &mut State, now: i64) {
        let mut found = Vec::new();
        if let Err(code) = collect_rollouts(&self.codex_home.join("sessions"), &mut found) {
            self.errors = vec![code];
        }
        let mut changed = false;
        for path in found {
            let key = path.to_string_lossy().into_owned();
            if !self.tracked.contains(&key) {
                // Rollouts stay in their creation-date folder even when resumed months later,
                // so only recently touched ones are adopted.
                let Some(modified) = modified_ms(&path) else {
                    continue;
                };
                if now - modified > RECENT_MS {
                    continue;
                }
                self.tracked.insert(key);
                changed = true;
            }
            self.files
                .entry(path)
                .or_insert_with(|| FileState::new(Kind::Codex));
        }
        if changed && store::write_json(&self.tracked_file, &self.tracked).is_err() {
            self.errors = vec!["save_error".into()];
        }
        self.reload_titles(state);
        for file in &self.claude_event_files {
            self.files
                .entry(file.clone())
                .or_insert_with(|| FileState::new(Kind::Claude));
        }
    }

    /// Task names come from `session_index.jsonl`; a missing file simply means no titles.
    fn reload_titles(&mut self, state: &mut State) {
        if let Ok(text) = fs::read_to_string(self.codex_home.join("session_index.jsonl")) {
            for line in text.lines() {
                if let Some((id, name)) = parse_index_line(line) {
                    self.titles.insert(id, name);
                }
            }
        }
        for session in state.sessions_mut() {
            if session.provider == Provider::Codex {
                if let Some(title) = self.titles.get(&session.id) {
                    session.title = Some(title.clone());
                }
            }
        }
    }
}

/// Maps one Codex rollout row to an event. Only explicit boundaries count: a task starting,
/// completing or being aborted, and tool calls.
pub fn codex_event(row: &Value, meta: &Value) -> Option<Event> {
    let null = Value::Null;
    let payload = row.get("payload").unwrap_or(&null);
    let name = match (str_field(row, "type")?, str_field(payload, "type")?) {
        ("event_msg", "task_started") => "UserPromptSubmit",
        ("event_msg", "task_complete") => "Stop",
        ("event_msg", "turn_aborted") => "Interrupt",
        ("response_item", "function_call" | "custom_tool_call") => "PreToolUse",
        _ => return None,
    };
    Some(Event {
        provider: Provider::Codex,
        session: first_non_empty(meta, &["id", "session_id"]),
        time: owned(row, "timestamp"),
        event: name.into(),
        cwd: optional(meta, "cwd"),
        tool: if name == "PreToolUse" {
            optional(payload, "name")
        } else {
            None
        },
        notification: None,
        title: None,
    })
}

/// One line of the Claude hook log; `hook.rs` writes these.
pub fn claude_event(row: &Value) -> Event {
    Event {
        provider: Provider::Claude,
        session: owned(row, "session"),
        time: owned(row, "time"),
        event: owned(row, "event"),
        cwd: optional(row, "cwd"),
        tool: optional(row, "tool"),
        notification: optional(row, "notification"),
        title: None,
    }
}

fn poll_file(
    path: &Path,
    file: &mut FileState,
    state: &mut State,
    titles: &HashMap<String, String>,
) -> Result<(), String> {
    if file.ignored {
        return Ok(());
    }
    let Some(size) = or_missing(fs::metadata(path).map(|meta| meta.len()))? else {
        return Ok(());
    };
    if size < file.offset {
        // Truncated or replaced: start over.
        file.offset = 0;
        file.carry.clear();
    }
    if size == file.offset {
        return Ok(());
    }
    let Some(mut handle) = or_missing(File::open(path))? else {
        return Ok(());
    };
    if file.offset == 0 && file.kind == Kind::Codex {
        start_rollout(&mut handle, size, file)?;
        if file.ignored {
            return Ok(());
        }
    }
    let bytes = read_range(
        &mut handle,
        file.offset,
        (size - file.offset).min(READ_CHUNK),
    )?;
    file.offset += bytes.len() as u64;
    process_chunk(file, &bytes, state, titles);
    Ok(())
}

/// First read of a Codex rollout: take its metadata, skip internal subagents, and start a
/// large file at its tail.
fn start_rollout(handle: &mut File, size: u64, file: &mut FileState) -> Result<(), String> {
    let head = read_range(handle, 0, size.min(READ_CHUNK))?;
    let first_line = head.split(|byte| *byte == b'\n').next().unwrap_or_default();
    let first: Value = serde_json::from_slice(first_line).map_err(|_| "read_error".to_string())?;
    file.meta = match first.get("payload") {
        Some(payload) if payload.is_object() => payload.clone(),
        _ => Value::Object(Map::new()),
    };
    if is_subagent(&file.meta) {
        file.ignored = true;
        file.offset = size;
    } else if size > TAIL_BYTES {
        file.offset = size - TAIL_BYTES;
        file.skip_first = true;
    }
    Ok(())
}

/// Splits freshly read bytes into lines, keeping an unfinished last line for the next tick.
fn process_chunk(
    file: &mut FileState,
    bytes: &[u8],
    state: &mut State,
    titles: &HashMap<String, String>,
) {
    let mut data = std::mem::take(&mut file.carry);
    data.extend_from_slice(bytes);
    let mut lines: Vec<&[u8]> = data.split(|byte| *byte == b'\n').collect();
    file.carry = lines.pop().unwrap_or_default().to_vec();
    if file.skip_first {
        if !lines.is_empty() {
            lines.remove(0);
        }
        file.skip_first = false;
    }
    for line in lines {
        apply_line(file, line, state, titles);
    }
}

fn apply_line(
    file: &mut FileState,
    line: &[u8],
    state: &mut State,
    titles: &HashMap<String, String>,
) {
    // Malformed lines are skipped silently, as the Node version did.
    let Ok(row) = serde_json::from_slice::<Value>(line) else {
        return;
    };
    match file.kind {
        Kind::Claude => state.apply(&claude_event(&row)),
        Kind::Codex => apply_rollout_row(file, &row, state, titles),
    }
}

fn apply_rollout_row(
    file: &mut FileState,
    row: &Value,
    state: &mut State,
    titles: &HashMap<String, String>,
) {
    if str_field(row, "type") == Some("session_meta") {
        file.meta = row.get("payload").cloned().unwrap_or(Value::Null);
        return;
    }
    if let Some(mut event) = codex_event(row, &file.meta) {
        event.title = titles.get(&event.session).cloned();
        state.apply(&event);
    }
}

/// Recursively collects `*.jsonl` files under `dir`. Unreadable folders are reported but do
/// not stop the walk.
fn collect_rollouts(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("scan_error".into()),
    };
    let mut result: Result<(), String> = Ok(());
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if let Err(code) = collect_rollouts(&path, out) {
                result = Err(code);
            }
        } else if file_type.is_file() && path.to_string_lossy().ends_with(".jsonl") {
            out.push(path);
        }
    }
    result
}

/// `NotFound` is normal (a file vanished between scans); other failures are reported.
fn or_missing<T>(result: io::Result<T>) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("read_error".into()),
    }
}

fn read_range(handle: &mut File, start: u64, len: u64) -> Result<Vec<u8>, String> {
    let read_error = || "read_error".to_string();
    handle
        .seek(SeekFrom::Start(start))
        .map_err(|_| read_error())?;
    let mut bytes = Vec::new();
    handle
        .by_ref()
        .take(len)
        .read_to_end(&mut bytes)
        .map_err(|_| read_error())?;
    Ok(bytes)
}

fn modified_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    i64::try_from(modified.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()
}

/// Codex's `session_index.jsonl` line: `{"id": ..., "thread_name": ...}`.
fn parse_index_line(line: &str) -> Option<(String, String)> {
    let row: Value = serde_json::from_str(line).ok()?;
    let id = str_field(&row, "id").filter(|id| !id.is_empty())?;
    let name = str_field(&row, "thread_name").filter(|name| !name.is_empty())?;
    Some((id.to_string(), take_units(name, TITLE_UNITS)))
}

/// Internal Codex subagents (guardian, review, ...) carry `source.subagent`.
fn is_subagent(meta: &Value) -> bool {
    meta.get("source")
        .and_then(|source| source.get("subagent"))
        .is_some_and(is_truthy)
}

/// JavaScript truthiness for the JSON shapes these files contain.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        _ => true,
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn owned(value: &Value, key: &str) -> String {
    str_field(value, key).unwrap_or_default().to_string()
}

fn optional(value: &Value, key: &str) -> Option<String> {
    str_field(value, key).map(str::to_string)
}

fn first_non_empty(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|key| owned(value, key))
        .find(|text| !text.is_empty())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_log::now_ms;
    use crate::state::Phase;
    use chrono::DateTime;
    use serde_json::json;
    use std::io::Write;
    use std::time::{Duration, SystemTime};

    const T0: i64 = 1_800_000_000_000;
    const DAY: i64 = 24 * 60 * 60 * 1000;

    struct Fixture {
        _dir: tempfile::TempDir,
        paths: DataPaths,
        home: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path().join("data"));
        let home = dir.path().join("codex");
        fs::create_dir_all(paths.root()).unwrap();
        fs::create_dir_all(&home).unwrap();
        Fixture {
            _dir: dir,
            paths,
            home,
        }
    }

    fn iso(ms: i64) -> String {
        DateTime::from_timestamp_millis(ms)
            .unwrap()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    }

    fn lines(values: &[Value]) -> String {
        values.iter().map(|value| format!("{value}\n")).collect()
    }

    fn append(path: &Path, text: &str) {
        fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    fn track_rollout(sources: &mut Sources, path: &Path, now: i64) {
        sources.last_scan = now;
        sources
            .files
            .insert(path.to_path_buf(), FileState::new(Kind::Codex));
    }

    #[test]
    fn codex_rows_map_only_explicit_boundaries() {
        let meta = json!({"id": "x", "cwd": "D:\\workspace\\esp32"});
        let stop = codex_event(
            &json!({"type": "event_msg", "payload": {"type": "task_complete"}, "timestamp": "now"}),
            &meta,
        )
        .unwrap();
        assert_eq!(stop.event, "Stop");
        assert_eq!(stop.session, "x");
        assert_eq!(stop.cwd.as_deref(), Some("D:\\workspace\\esp32"));
        let message =
            json!({"type": "response_item", "payload": {"type": "message", "role": "assistant"}});
        assert!(codex_event(&message, &meta).is_none());
        let tool =
            json!({"type": "response_item", "payload": {"type": "function_call", "name": "exec"}});
        let tool = codex_event(&tool, &meta).unwrap();
        assert_eq!(
            (tool.event.as_str(), tool.tool.as_deref()),
            ("PreToolUse", Some("exec"))
        );
    }

    #[test]
    fn codex_session_falls_back_to_session_id() {
        let row = json!({"type": "event_msg", "payload": {"type": "turn_aborted"}});
        let event = codex_event(&row, &json!({"session_id": "y"})).unwrap();
        assert_eq!(
            (event.session.as_str(), event.event.as_str()),
            ("y", "Interrupt")
        );
    }

    #[test]
    fn hook_log_lines_feed_claude_sessions() {
        let fx = fixture();
        let log = [
            json!({"time": iso(T0), "event": "UserPromptSubmit", "session": "s1", "cwd": "D:\\work\\kefu"}),
            json!({"time": iso(T0 + 1), "event": "PreToolUse", "session": "s1", "cwd": "D:\\work\\kefu", "tool": "Bash"}),
        ];
        fs::write(fx.paths.events(), lines(&log)).unwrap();
        let mut state = State::default();
        let mut sources = Sources::with_homes(
            &fx.paths,
            fx.home.clone(),
            fx.home.join("deepseek"),
            fx.home.join("claude"),
        );
        sources.tick(&mut state, T0 + 2);
        let card = &state.snapshot(T0 + 2).cards[0];
        assert_eq!(
            (
                card.session.state,
                card.session.project.as_str(),
                card.session.tool.as_str()
            ),
            (Phase::Working, "kefu", "Bash")
        );
    }

    #[test]
    fn claude_hook_session_gets_conversation_title_and_live_rename_without_new_hook() {
        let fx = fixture();
        let claude_home = fx.home.join("claude");
        let title_dir = claude_home.join("projects/project/s1");
        fs::create_dir_all(&title_dir).unwrap();
        let title_file = title_dir.join("custom-title.json");
        fs::write(&title_file, json!({"customTitle":"对话标题"}).to_string()).unwrap();
        fs::write(
            fx.paths.events(),
            lines(&[
                json!({"time":iso(T0),"event":"Stop","session":"s1","cwd":"D:/w/shared-project"}),
            ]),
        )
        .unwrap();
        let mut state = State::default();
        let mut sources = Sources::with_homes(
            &fx.paths,
            fx.home.clone(),
            fx.home.join("deepseek"),
            claude_home,
        );
        sources.tick(&mut state, T0);
        assert_eq!(state.snapshot(T0).cards[0].rows[0].project, "对话标题");
        fs::write(&title_file, json!({"customTitle":"新对话标题"}).to_string()).unwrap();
        sources.tick(&mut state, T0 + 5_001);
        assert_eq!(
            state.snapshot(T0 + 5_001).cards[0].rows[0].project,
            "新对话标题"
        );
        assert_eq!(
            state.session(Provider::Claude, "s1").unwrap().updated_at,
            T0
        );
    }

    #[test]
    fn partial_lines_wait_for_their_newline() {
        let fx = fixture();
        let full = format!(
            "{}\n",
            json!({"time": iso(T0), "event": "Stop", "session": "p", "cwd": "/w/p"})
        );
        let (head, tail) = full.split_at(20);
        fs::write(fx.paths.events(), head).unwrap();
        let mut state = State::default();
        let mut sources = Sources::with_homes(
            &fx.paths,
            fx.home.clone(),
            fx.home.join("deepseek"),
            fx.home.join("claude"),
        );
        sources.tick(&mut state, T0);
        assert!(state.snapshot(T0).sessions.is_empty());
        append(&fx.paths.events(), tail);
        sources.tick(&mut state, T0 + 1);
        assert_eq!(
            state.session(Provider::Claude, "p").unwrap().state,
            Phase::Done
        );
    }

    #[test]
    fn packaged_claude_log_stays_live_after_data_migration_and_preserves_titles() {
        let fx = fixture();
        let redirected = fx
            .home
            .join("package/LocalCache/Roaming/ai-status-panel/events.jsonl");
        fs::create_dir_all(redirected.parent().unwrap()).unwrap();
        let claude_home = fx.home.join("claude");
        let title_dir = claude_home.join("projects/shared/s1");
        fs::create_dir_all(&title_dir).unwrap();
        fs::write(
            title_dir.join("custom-title.json"),
            json!({"customTitle":"正在运行的对话"}).to_string(),
        )
        .unwrap();
        let stale = json!({"time":iso(T0),"event":"Stop","session":"s1","cwd":"D:/w/project"});
        fs::write(fx.paths.events(), lines(&[stale.clone()])).unwrap();
        fs::write(&redirected,lines(&[stale,json!({"time":iso(T0+1),"event":"UserPromptSubmit","session":"s1","cwd":"D:/w/project"})])).unwrap();
        let mut sources = Sources::with_homes(
            &fx.paths,
            fx.home.clone(),
            fx.home.join("deepseek"),
            claude_home,
        )
        .with_claude_event_files(vec![redirected.clone(), redirected.clone()]);
        let mut state = State::default();
        sources.tick(&mut state, T0 + 2);
        assert_eq!(
            state.snapshot(T0 + 2).cards[0].session.state,
            Phase::Working
        );
        assert_eq!(
            state.snapshot(T0 + 2).cards[0].rows[0].project,
            "正在运行的对话"
        );
        append(
            &redirected,
            &lines(&[json!({"time":iso(T0+3),"event":"PermissionRequest","session":"s1"})]),
        );
        sources.tick(&mut state, T0 + 4);
        assert_eq!(
            state.snapshot(T0 + 4).cards[0].session.state,
            Phase::Waiting
        );
        append(
            &redirected,
            &lines(&[json!({"time":iso(T0+5),"event":"PostToolUse","session":"s1","tool":"Bash"})]),
        );
        sources.tick(&mut state, T0 + 6);
        assert_eq!(
            state.snapshot(T0 + 6).cards[0].session.state,
            Phase::Working
        );
        // A delayed stale event in the migrated copy must not overwrite current work.
        append(
            &fx.paths.events(),
            &lines(&[json!({"time":iso(T0),"event":"Stop","session":"s1"})]),
        );
        sources.tick(&mut state, T0 + 7);
        assert_eq!(
            state.snapshot(T0 + 7).cards[0].session.state,
            Phase::Working
        );
        append(
            &redirected,
            &lines(&[json!({"time":iso(T0+8),"event":"Stop","session":"s1"})]),
        );
        sources.tick(&mut state, T0 + 9);
        assert_eq!(state.snapshot(T0 + 9).cards[0].session.state, Phase::Done);
        state.acknowledge(Provider::Claude, "s1");
        sources.tick(&mut state, T0 + 10);
        assert_eq!(state.snapshot(T0 + 10).cards[0].session.state, Phase::Idle);
        assert_eq!(sources.claude_event_files.len(), 2);
    }

    #[test]
    fn redirected_hook_log_created_after_startup_is_detected_without_rescan() {
        let fx = fixture();
        let redirected = fx.home.join("new-events.jsonl");
        let mut sources = Sources::with_homes(
            &fx.paths,
            fx.home.clone(),
            fx.home.join("deepseek"),
            fx.home.join("claude"),
        )
        .with_claude_event_files(vec![redirected.clone()]);
        let mut state = State::default();
        sources.tick(&mut state, T0);
        assert!(state.snapshot(T0).sessions.is_empty());
        fs::write(
            &redirected,
            lines(&[json!({"time":iso(T0+1),"event":"UserPromptSubmit","session":"new"})]),
        )
        .unwrap();
        sources.tick(&mut state, T0 + 2);
        assert_eq!(
            state.snapshot(T0 + 2).cards[0].session.state,
            Phase::Working
        );
    }

    #[test]
    fn internal_subagent_rollouts_stay_ignored_after_new_lines() {
        let fx = fixture();
        let file = fx.home.join("rollout.jsonl");
        let meta = json!({"type": "session_meta", "payload": {"id": "internal", "source": {"subagent": {"other": "guardian"}}}});
        fs::write(&file, lines(&[meta])).unwrap();
        let mut state = State::default();
        let mut sources = Sources::with_homes(
            &fx.paths,
            fx.home.clone(),
            fx.home.join("deepseek"),
            fx.home.join("claude"),
        );
        track_rollout(&mut sources, &file, T0);
        sources.tick(&mut state, T0);
        append(
            &file,
            &lines(&[
                json!({"type": "event_msg", "timestamp": iso(T0), "payload": {"type": "task_started"}}),
            ]),
        );
        sources.tick(&mut state, T0);
        assert!(state.snapshot(T0).sessions.is_empty());
    }

    #[test]
    fn recent_rollouts_in_old_folders_are_discovered_and_survive_restarts() {
        let fx = fixture();
        let now = now_ms() as i64;
        let folder = fx.home.join("sessions").join("2020").join("01").join("01");
        fs::create_dir_all(&folder).unwrap();
        let file = folder.join("old-task.jsonl");
        fs::write(
            &file,
            lines(&[
                json!({"type": "session_meta", "payload": {"id": "old", "cwd": "/work/shared"}}),
                json!({"type": "event_msg", "timestamp": iso(now), "payload": {"type": "task_complete"}}),
            ]),
        )
        .unwrap();
        fs::write(
            fx.home.join("session_index.jsonl"),
            format!("{}\n", json!({"id": "old", "thread_name": "恢复旧任务"})),
        )
        .unwrap();

        let mut state = State::default();
        let mut sources = Sources::with_homes(
            &fx.paths,
            fx.home.clone(),
            fx.home.join("deepseek"),
            fx.home.join("claude"),
        );
        sources.tick(&mut state, now);
        assert_eq!(state.snapshot(now).cards[1].rows[0].project, "恢复旧任务");

        // The file is now old on disk, but it is tracked, so a restarted app still reads it.
        let past = SystemTime::now() - Duration::from_secs(5 * 86_400);
        fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(past)
            .unwrap();
        let mut restarted_state = State::default();
        let mut restarted = Sources::with_homes(
            &fx.paths,
            fx.home.clone(),
            fx.home.join("deepseek"),
            fx.home.join("claude"),
        );
        restarted.tick(&mut restarted_state, now);
        assert_eq!(
            restarted_state.snapshot(now + 2 * DAY).cards[1]
                .session
                .state,
            Phase::Done
        );

        let started = json!({"type": "event_msg", "timestamp": iso(now + 1000), "payload": {"type": "task_started"}});
        append(&file, &lines(&[started]));
        restarted.tick(&mut restarted_state, now + 1);
        assert_eq!(
            restarted_state
                .session(Provider::Codex, "old")
                .unwrap()
                .state,
            Phase::Working
        );
    }

    #[test]
    fn large_rollouts_are_read_from_the_tail_on_first_sight() {
        let fx = fixture();
        let file = fx.home.join("big.jsonl");
        let early =
            json!({"type": "event_msg", "timestamp": iso(T0), "payload": {"type": "task_started"}});
        let mut content = lines(&[
            json!({"type": "session_meta", "payload": {"id": "big", "cwd": "/work/big"}}),
            early,
        ]);
        let filler = json!({"type": "response_item", "payload": {"type": "message", "text": "x".repeat(100)}});
        while content.len() < 700 * 1024 {
            content.push_str(&format!("{filler}\n"));
        }
        let finished = json!({"type": "event_msg", "timestamp": iso(T0 + 5), "payload": {"type": "task_complete"}});
        content.push_str(&format!("{finished}\n"));
        fs::write(&file, content).unwrap();

        let mut state = State::default();
        let mut sources = Sources::with_homes(
            &fx.paths,
            fx.home.clone(),
            fx.home.join("deepseek"),
            fx.home.join("claude"),
        );
        track_rollout(&mut sources, &file, T0);
        sources.tick(&mut state, T0);
        let session = state.session(Provider::Codex, "big").unwrap();
        assert_eq!(session.state, Phase::Done);
        assert_eq!(
            session.started_at, 0,
            "the task_started line lies before the tail"
        );
    }
}
