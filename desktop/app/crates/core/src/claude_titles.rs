//! Claude display metadata, read only for sessions already known from hooks.
//! Never derive a title from a prompt, message body, tool argument or reply.
use crate::state::{take_units, Provider, State, TITLE_UNITS};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const REFRESH_MS: i64 = 5_000;
const METADATA_LIMIT: u64 = 2 * 1024 * 1024;
const TRANSCRIPT_TAIL: u64 = 512 * 1024;

pub fn home_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".claude"))
}

pub struct ClaudeTitles {
    home: PathBuf,
    last_refresh: Option<i64>,
    transcripts: HashMap<PathBuf, Transcript>,
}

#[derive(Default)]
struct Transcript {
    signature: Option<(u64, Option<SystemTime>)>,
    custom: Option<String>,
    summary: Option<String>,
}

impl ClaudeTitles {
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            last_refresh: None,
            transcripts: HashMap::new(),
        }
    }

    pub fn tick(&mut self, state: &mut State, now: i64) {
        if self.last_refresh.is_some_and(|at| now - at < REFRESH_MS) {
            return;
        }
        self.last_refresh = Some(now);
        let ids: Vec<String> = state
            .sessions_mut()
            .filter(|session| session.provider == Provider::Claude && safe_id(&session.id))
            .map(|session| session.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }

        let projects: Vec<PathBuf> = fs::read_dir(self.home.join("projects"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        let live_names = live_titles(&self.home.join("sessions"));
        let mut index_titles = HashMap::new();
        for project in &projects {
            if let Some(index) = read_json(&project.join("sessions-index.json")) {
                for entry in index["entries"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(name)) = (
                        entry["sessionId"].as_str(),
                        title(entry, "customTitle").or_else(|| title(entry, "summary")),
                    ) {
                        index_titles.insert(id.to_owned(), name);
                    }
                }
            }
        }
        for id in ids {
            let mut custom_file = None;
            let mut transcript_custom = None;
            let mut transcript_summary = None;
            for project in &projects {
                if let Some(value) = read_json(&project.join(&id).join("custom-title.json")) {
                    if let Some(title) =
                        title(&value, "customTitle").or_else(|| title(&value, "title"))
                    {
                        custom_file = Some(title);
                    }
                }
                let path = project.join(format!("{id}.jsonl"));
                let cached = self.transcripts.entry(path.clone()).or_default();
                cached.refresh(&path, &id);
                transcript_custom = cached.custom.clone().or(transcript_custom);
                transcript_summary = cached.summary.clone().or(transcript_summary);
            }
            let resolved = custom_file
                .or(transcript_custom)
                .or_else(|| live_names.get(&id).cloned())
                .or_else(|| index_titles.get(&id).cloned())
                .or(transcript_summary);
            if let Some(title) = resolved {
                state.set_title(Provider::Claude, &id, &title);
            }
        }
    }
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn title(row: &Value, key: &str) -> Option<String> {
    row[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| take_units(s, TITLE_UNITS))
}

fn read_json(path: &Path) -> Option<Value> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > METADATA_LIMIT {
        return None;
    }
    serde_json::from_reader(file.take(METADATA_LIMIT)).ok()
}

fn live_titles(dir: &Path) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let Some(row) = read_json(&entry.path()) else {
            continue;
        };
        if let (Some(id), Some(name)) = (row["sessionId"].as_str(), title(&row, "name")) {
            // The .key sidecars contain peer credentials and are never opened.
            names.insert(id.into(), name);
        }
    }
    names
}

impl Transcript {
    fn refresh(&mut self, path: &Path, id: &str) {
        let Ok(mut file) = File::open(path) else {
            return;
        };
        let Ok(meta) = file.metadata() else { return };
        let signature = (meta.len(), meta.modified().ok());
        if self.signature.as_ref() == Some(&signature) {
            return;
        }
        let offset = meta.len().saturating_sub(TRANSCRIPT_TAIL);
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return;
        }
        let mut bytes = Vec::new();
        if file.take(TRANSCRIPT_TAIL).read_to_end(&mut bytes).is_err() {
            return;
        }
        if self
            .signature
            .as_ref()
            .is_some_and(|(size, _)| *size > meta.len())
        {
            *self = Self::default();
        }
        self.signature = Some(signature);
        let mut lines = bytes.split(|&byte| byte == b'\n');
        if offset > 0 {
            lines.next();
        }
        for line in lines {
            // Only metadata records are decoded. Prompt or tool content is never a fallback.
            let Ok(text) = std::str::from_utf8(line) else {
                continue;
            };
            if !text.contains("\"custom-title\"") && !text.contains("\"summary\"") {
                continue;
            }
            let Ok(row) = serde_json::from_slice::<Value>(line) else {
                continue;
            };
            if row["sessionId"]
                .as_str()
                .is_some_and(|session| session != id)
            {
                continue;
            }
            match row["type"].as_str() {
                Some("custom-title") => {
                    if let Some(value) = title(&row, "customTitle") {
                        self.custom = Some(value);
                    }
                }
                Some("summary") => {
                    if let Some(value) = title(&row, "summary") {
                        self.summary = Some(value);
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Event, Phase};
    use serde_json::json;

    fn state() -> State {
        let mut state = State::default();
        state.apply(&Event {
            provider: Provider::Claude,
            session: "same-project-a".into(),
            time: "2026-10-27T03:00:00Z".into(),
            event: "Stop".into(),
            cwd: Some("D:/w/project".into()),
            tool: None,
            title: None,
            notification: None,
        });
        state
    }

    #[test]
    fn custom_title_file_wins_and_rename_does_not_change_completion_or_unread() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/project");
        let session = project.join("same-project-a");
        fs::create_dir_all(&session).unwrap();
        fs::write(project.join("sessions-index.json"), json!({"entries":[{"sessionId":"same-project-a","summary":"generated","firstPrompt":"PRIVATE"}]}).to_string()).unwrap();
        fs::write(project.join("same-project-a.jsonl"), format!("{}\n",json!({"type":"custom-title","sessionId":"same-project-a","customTitle":"transcript title"}))).unwrap();
        fs::write(
            session.join("custom-title.json"),
            json!({"customTitle":" 用户命名 "}).to_string(),
        )
        .unwrap();
        let mut titles = ClaudeTitles::new(dir.path().to_path_buf());
        let mut state = state();
        let before = state
            .session(Provider::Claude, "same-project-a")
            .unwrap()
            .clone();
        titles.tick(&mut state, 10_000);
        let first = state.snapshot(before.updated_at);
        assert_eq!(first.cards[0].rows[0].project, "用户命名");
        assert_eq!(first.cards[0].session.state, Phase::Done);
        assert_eq!(first.cards[0].session.updated_at, before.updated_at);
        assert!(first.cards[0].session.unread);
        state.acknowledge(Provider::Claude, "same-project-a");
        fs::write(
            session.join("custom-title.json"),
            json!({"customTitle":"修改后的标题"}).to_string(),
        )
        .unwrap();
        titles.tick(&mut state, 15_001);
        let renamed = state.snapshot(before.updated_at);
        assert_eq!(renamed.cards[0].rows[0].project, "修改后的标题");
        assert_eq!(renamed.cards[0].session.state, Phase::Idle);
        assert!(!renamed.cards[0].session.unread);
        assert_eq!(renamed.cards[0].session.updated_at, before.updated_at);
        assert!(!serde_json::to_string(&renamed).unwrap().contains("PRIVATE"));
    }

    #[test]
    fn missing_titles_fall_back_to_project_and_prompt_text_is_never_used() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("same-project-a.jsonl"),
            format!(
                "{}\n",
                json!({"type":"user","message":{"content":"PRIVATE"}})
            ),
        )
        .unwrap();
        fs::write(
            project.join("sessions-index.json"),
            json!({"entries":[{"sessionId":"same-project-a","firstPrompt":"PRIVATE"}]}).to_string(),
        )
        .unwrap();
        let mut titles = ClaudeTitles::new(dir.path().to_path_buf());
        let mut state = state();
        titles.tick(&mut state, 0);
        assert_eq!(
            state.snapshot(1_793_070_000_000).cards[0].rows[0].project,
            "project"
        );
    }

    #[test]
    fn title_lookup_uses_session_id_not_shared_project_name_and_supports_live_metadata() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sessions")).unwrap();
        fs::write(
            dir.path().join("sessions/123.json"),
            json!({"sessionId":"same-project-a","name":"运行中的对话"}).to_string(),
        )
        .unwrap();
        fs::write(dir.path().join("sessions/123.key"), b"not json; never open").unwrap();
        let mut titles = ClaudeTitles::new(dir.path().to_path_buf());
        let mut state = state();
        titles.tick(&mut state, 0);
        assert_eq!(
            state
                .session(Provider::Claude, "same-project-a")
                .unwrap()
                .title
                .as_deref(),
            Some("运行中的对话")
        );
        assert!(!safe_id("../other"));
        assert!(!safe_id("x/y"));
    }

    #[test]
    fn transcript_custom_title_and_index_summary_are_supported() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("sessions-index.json"),
            json!({"entries":[{"sessionId":"same-project-a","summary":"索引标题"}]}).to_string(),
        )
        .unwrap();
        let mut titles = ClaudeTitles::new(dir.path().to_path_buf());
        let mut state = state();
        titles.tick(&mut state, 0);
        assert_eq!(
            state
                .session(Provider::Claude, "same-project-a")
                .unwrap()
                .title
                .as_deref(),
            Some("索引标题")
        );
        fs::write(project.join("same-project-a.jsonl"),format!("{}\n",json!({"type":"custom-title","sessionId":"same-project-a","customTitle":"对话新标题"}))).unwrap();
        titles.tick(&mut state, 5_001);
        assert_eq!(
            state
                .session(Provider::Claude, "same-project-a")
                .unwrap()
                .title
                .as_deref(),
            Some("对话新标题")
        );
    }
}
