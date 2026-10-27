//! Keeps Claude Code's hook configuration pointing at this executable (`"<exe>" --hook`).
//! Only entries that belong to this app are rewritten or removed: the Node version's
//! `cc-bridge/hook.cjs` commands and `--hook` commands of a panel binary. Every other key and
//! hook is kept as it was. Installs back the file up first and write it atomically.
use crate::store;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Events the status panel listens to.
pub const EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Notification",
    "Stop",
    "SessionEnd",
];
const HOOK_TIMEOUT_SECS: u64 = 5;
const LEGACY_MARKER: &str = "cc-bridge/hook.cjs";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksStatus {
    pub installed_events: Vec<String>,
    pub missing_events: Vec<String>,
    /// Some hook still runs the Node version's `cc-bridge/hook.cjs`.
    pub legacy: bool,
}

/// The hook command for this executable. Forward slashes avoid backslash escaping in shells.
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\" --hook", normalize_path(exe))
}

/// Which listed events already run this exact executable. A missing or unreadable file reports
/// every event as missing.
pub fn status(settings_path: &Path, exe: &Path) -> HooksStatus {
    let command = command_for(exe);
    let root = load_settings(settings_path)
        .ok()
        .flatten()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let hooks = root.get("hooks");
    let mut installed_events = Vec::new();
    let mut missing_events = Vec::new();
    for event in EVENTS {
        let names = event_commands(hooks, event);
        if names.contains(&command) {
            installed_events.push(event.to_string());
        } else {
            missing_events.push(event.to_string());
        }
    }
    HooksStatus {
        installed_events,
        missing_events,
        legacy: has_legacy(hooks),
    }
}

/// Points every listed event at this executable. Returns the backup of the previous file, or
/// `None` when nothing changed or there was no file to back up.
pub fn install(settings_path: &Path, exe: &Path) -> Result<Option<PathBuf>> {
    let command = command_for(exe);
    let original = load_settings(settings_path)?;
    let mut updated = original
        .clone()
        .unwrap_or_else(|| Value::Object(Map::new()));
    apply(&mut updated, &command, exe)?;
    if original.as_ref() == Some(&updated) {
        return Ok(None);
    }
    let backup = match original {
        Some(_) => Some(backup_file(settings_path)?),
        None => None,
    };
    store::write_json(settings_path, &updated)?;
    Ok(backup)
}

/// Copies the current file to `<name>.bak-<unix ms>` next to it.
fn backup_file(settings_path: &Path) -> Result<PathBuf> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let name = settings_path
        .file_name()
        .context("设置文件名无效")?
        .to_string_lossy()
        .into_owned();
    let backup = settings_path.with_file_name(format!("{name}.bak-{stamp}"));
    fs::copy(settings_path, &backup).with_context(|| format!("备份失败：{}", backup.display()))?;
    Ok(backup)
}

/// `Ok(None)` when the file does not exist. Content that cannot be parsed is an error, so a
/// damaged file is never overwritten.
fn load_settings(path: &Path) -> Result<Option<Value>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("读取失败：{}", path.display())),
    };
    if text.trim().is_empty() {
        return Ok(Some(Value::Object(Map::new())));
    }
    let value: Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "settings.json 不是有效的 JSON，已停止修改：{}",
            path.display()
        )
    })?;
    if !value.is_object() {
        bail!("settings.json 顶层不是对象，已停止修改");
    }
    Ok(Some(value))
}

/// Rewrites the `hooks` object in place: exactly one panel hook for each listed event, and no
/// panel hook under any other event.
fn apply(root: &mut Value, command: &str, exe: &Path) -> Result<()> {
    let root = root.as_object_mut().context("settings.json 顶层不是对象")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks
        .as_object_mut()
        .context("settings.hooks 不是对象，已停止修改")?;
    let names: BTreeSet<String> = hooks
        .keys()
        .cloned()
        .chain(EVENTS.iter().map(|e| e.to_string()))
        .collect();
    for name in names {
        let wanted = EVENTS.contains(&name.as_str());
        let entry = hooks
            .entry(name.clone())
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(groups) = entry.as_array_mut() else {
            if wanted {
                bail!("hooks.{name} 格式不正确，已停止修改");
            }
            continue;
        };
        let had_groups = !groups.is_empty();
        normalize_groups(groups, wanted.then_some(command), exe);
        if !wanted && had_groups && groups.is_empty() {
            hooks.remove(&name);
        }
    }
    Ok(())
}

/// Keeps the first panel hook of an event, rewritten to `command` when one is given, and drops
/// the rest. Groups emptied by that are removed. A missing hook is appended as a new group.
fn normalize_groups(groups: &mut Vec<Value>, command: Option<&str>, exe: &Path) {
    let mut kept = false;
    groups.retain_mut(|group| {
        let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true;
        };
        let had_hooks = !hooks.is_empty();
        hooks.retain_mut(|hook| {
            let is_panel = hook
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|text| is_panel_command(text, exe));
            if !is_panel {
                return true;
            }
            match command {
                Some(new_command) if !kept => {
                    kept = true;
                    hook["command"] = json!(new_command);
                    true
                }
                _ => false,
            }
        });
        !(had_hooks && hooks.is_empty())
    });
    if let (Some(command), false) = (command, kept) {
        groups.push(json!({
            "hooks": [{"type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECS}]
        }));
    }
}

/// A command that belongs to this app: the Node hook script, or a `--hook` command for a panel
/// binary (this executable, or any binary whose name contains `ai-status-panel`).
fn is_panel_command(command: &str, exe: &Path) -> bool {
    let command = normalize_text(command);
    command.contains(LEGACY_MARKER)
        || (command.contains("--hook")
            && (command.contains("ai-status-panel") || command.contains(&normalize_path(exe))))
}

fn has_legacy(hooks: Option<&Value>) -> bool {
    let Some(map) = hooks.and_then(Value::as_object) else {
        return false;
    };
    map.keys().any(|event| {
        event_commands(hooks, event)
            .iter()
            .any(|text| normalize_text(text).contains(LEGACY_MARKER))
    })
}

/// Every command string registered under `event`, across all of its matcher groups.
fn event_commands(hooks: Option<&Value>, event: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let groups = hooks
        .and_then(|hooks| hooks.get(event))
        .and_then(Value::as_array);
    for group in groups.into_iter().flatten() {
        let entries = group.get("hooks").and_then(Value::as_array);
        for hook in entries.into_iter().flatten() {
            if let Some(command) = hook.get("command").and_then(Value::as_str) {
                commands.push(command.to_string());
            }
        }
    }
    commands
}

fn normalize_path(path: &Path) -> String {
    normalize_text(&path.to_string_lossy())
}

fn normalize_text(text: &str) -> String {
    text.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const EXE: &str = "D:\\apps\\AI 状态屏\\ai-status-panel.exe";
    const LEGACY: &str = r#"node "D:\workspace\esp32\desktop\cc-bridge\hook.cjs""#;

    fn exe() -> PathBuf {
        PathBuf::from(EXE)
    }

    fn settings_in(dir: &TempDir) -> PathBuf {
        dir.path().join(".claude").join("settings.json")
    }

    fn write(path: &Path, value: &Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn backups(path: &Path) -> Vec<PathBuf> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|found| {
                found
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&format!("{name}.bak-"))
            })
            .collect()
    }

    #[test]
    fn command_uses_forward_slashes_and_quotes_the_path() {
        assert_eq!(
            command_for(Path::new("D:\\apps\\ai-status-panel.exe")),
            "\"D:/apps/ai-status-panel.exe\" --hook"
        );
    }

    #[test]
    fn missing_file_reports_every_event_missing() {
        let dir = tempfile::tempdir().unwrap();
        let status = status(&settings_in(&dir), &exe());
        assert!(status.installed_events.is_empty());
        assert_eq!(status.missing_events.len(), EVENTS.len());
        assert!(!status.legacy);
    }

    #[test]
    fn install_creates_the_file_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_in(&dir);
        assert_eq!(install(&path, &exe()).unwrap(), None);
        let status = status(&path, &exe());
        assert!(status.missing_events.is_empty());
        assert_eq!(status.installed_events.len(), EVENTS.len());
        let written = read(&path);
        assert_eq!(
            written["hooks"]["Stop"][0]["hooks"][0],
            json!({"type": "command", "command": command_for(&exe()), "timeout": 5})
        );

        let before = read(&path);
        assert_eq!(install(&path, &exe()).unwrap(), None);
        assert_eq!(read(&path), before);
        assert!(backups(&path).is_empty());
    }

    #[test]
    fn install_keeps_other_keys_and_hooks_and_backs_up_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_in(&dir);
        let original = json!({
            "model": "opus",
            "permissions": {"allow": ["Bash(ls)"]},
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "notify-me", "timeout": 9}]}],
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "lint-bash"}]}]
            }
        });
        write(&path, &original);

        let backup = install(&path, &exe()).unwrap().expect("backup path");
        assert_eq!(read(&backup), original);
        let now = read(&path);
        assert_eq!(now["model"], "opus");
        assert_eq!(now["permissions"], original["permissions"]);
        let stop = now["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0], original["hooks"]["Stop"][0]);
        assert_eq!(stop[1]["hooks"][0]["command"], command_for(&exe()));
        let pre = now["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre[0], original["hooks"]["PreToolUse"][0]);
        assert_eq!(pre.len(), 2);
    }

    #[test]
    fn legacy_hook_is_rewritten_in_place_keeping_its_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_in(&dir);
        write(
            &path,
            &json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": LEGACY, "timeout": 5, "note": "keep"}]}]}}),
        );
        assert!(status(&path, &exe()).legacy);

        install(&path, &exe()).unwrap();

        let now = read(&path);
        let stop = now["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], command_for(&exe()));
        assert_eq!(stop[0]["hooks"][0]["note"], "keep");
        assert!(!status(&path, &exe()).legacy);
    }

    #[test]
    fn previous_panel_path_is_replaced_and_duplicates_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_in(&dir);
        let old = "\"D:/old/ai-status-panel.exe\" --hook";
        write(
            &path,
            &json!({"hooks": {"Stop": [
                {"hooks": [{"type": "command", "command": old}]},
                {"hooks": [{"type": "command", "command": old}, {"type": "command", "command": "user-hook"}]}
            ]}}),
        );
        assert!(status(&path, &exe()).installed_events.is_empty());

        install(&path, &exe()).unwrap();

        let stop = read(&path)["hooks"]["Stop"].as_array().unwrap().clone();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], command_for(&exe()));
        assert_eq!(
            stop[1]["hooks"],
            json!([{"type": "command", "command": "user-hook"}])
        );
    }

    #[test]
    fn panel_hooks_under_unlisted_events_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_in(&dir);
        write(
            &path,
            &json!({"hooks": {"SubagentStop": [{"hooks": [{"type": "command", "command": LEGACY}]}]}}),
        );

        install(&path, &exe()).unwrap();

        let hooks = read(&path)["hooks"].clone();
        assert!(hooks.get("SubagentStop").is_none());
        assert!(!status(&path, &exe()).legacy);
    }

    #[test]
    fn partial_installs_are_reported_per_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_in(&dir);
        write(
            &path,
            &json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": command_for(&exe())}]}]}}),
        );
        let status = status(&path, &exe());
        assert_eq!(status.installed_events, vec!["Stop".to_string()]);
        assert_eq!(status.missing_events.len(), EVENTS.len() - 1);
    }

    #[test]
    fn unparseable_settings_are_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_in(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not json").unwrap();
        assert!(install(&path, &exe()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not json");
        assert!(backups(&path).is_empty());
    }

    #[test]
    fn non_object_hooks_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_in(&dir);
        write(&path, &json!({"hooks": []}));
        assert!(install(&path, &exe()).is_err());
    }
}
