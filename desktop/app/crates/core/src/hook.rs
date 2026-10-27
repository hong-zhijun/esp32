//! `--hook` mode. Claude Code runs `"<exe>" --hook` for each configured event and writes the
//! event JSON to stdin. We append one compact line to `events.jsonl` and exit. This runs inside
//! Claude Code's hook pipeline, so nothing is printed and no error is allowed to escape.
use crate::paths::DataPaths;
use serde::Serialize;
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

/// Field order matches the Node writer. Absent or null values are omitted, as `JSON.stringify`
/// drops `undefined`.
#[derive(Serialize)]
struct Line {
    time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notification: Option<Value>,
}

/// Entry point for `--hook`. Always returns normally.
pub fn run() {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    let _ = record(&input, &iso_now(), &DataPaths::default_location().events());
}

/// The JSON line for one hook payload, or `None` when the payload is not JSON.
pub fn line_for(input: &str, time: &str) -> Option<String> {
    let data: Value = serde_json::from_str(input).ok()?;
    let line = Line {
        time: time.to_string(),
        event: field(&data, "hook_event_name"),
        session: field(&data, "session_id"),
        cwd: field(&data, "cwd"),
        tool: field(&data, "tool_name"),
        notification: field(&data, "notification_type"),
    };
    serde_json::to_string(&line).ok()
}

/// Appends the line for `input` to `events_file`, creating its folder when needed.
pub fn record(input: &str, time: &str, events_file: &Path) -> io::Result<()> {
    let Some(line) = line_for(input, time) else {
        return Ok(());
    };
    if let Some(parent) = events_file.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_file)?;
    file.write_all(format!("{line}\n").as_bytes())
}

/// UTC time with milliseconds, e.g. `2026-10-26T08:30:20.345Z`, as `Date#toISOString` writes it.
fn iso_now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn field(data: &Value, key: &str) -> Option<Value> {
    data.get(key).filter(|value| !value.is_null()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIME: &str = "2026-10-26T08:30:20.345Z";

    #[test]
    fn line_keeps_node_field_order_and_drops_unused_fields() {
        let input = r#"{"hook_event_name":"PreToolUse","session_id":"s1","cwd":"D:\\w\\app","tool_name":"Bash","x":1}"#;
        assert_eq!(
            line_for(input, TIME).unwrap(),
            r#"{"time":"2026-10-26T08:30:20.345Z","event":"PreToolUse","session":"s1","cwd":"D:\\w\\app","tool":"Bash"}"#
        );
    }

    #[test]
    fn null_and_missing_fields_are_omitted() {
        let line = line_for(
            r#"{"hook_event_name":"Stop","session_id":"s","cwd":null}"#,
            TIME,
        )
        .unwrap();
        assert_eq!(
            line,
            r#"{"time":"2026-10-26T08:30:20.345Z","event":"Stop","session":"s"}"#
        );
    }

    #[test]
    fn notification_type_is_kept_under_its_own_name() {
        let input = r#"{"hook_event_name":"Notification","session_id":"s","notification_type":"permission_prompt"}"#;
        let line = line_for(input, TIME).unwrap();
        assert!(
            line.ends_with(r#""notification":"permission_prompt"}"#),
            "{line}"
        );
    }

    #[test]
    fn invalid_payloads_write_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("events.jsonl");
        record("not json", TIME, &file).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn each_record_appends_one_line_and_creates_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data").join("events.jsonl");
        record(
            r#"{"hook_event_name":"Stop","session_id":"a"}"#,
            TIME,
            &file,
        )
        .unwrap();
        record(
            r#"{"hook_event_name":"Stop","session_id":"b"}"#,
            TIME,
            &file,
        )
        .unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn timestamps_match_javascript_iso_format() {
        let now = iso_now();
        assert_eq!(now.len(), 24);
        assert!(now.ends_with('Z'));
        assert_eq!(now.as_bytes()[19], b'.');
    }
}
