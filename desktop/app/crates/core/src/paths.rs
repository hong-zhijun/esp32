//! Where the app keeps its files. Everything lives under one data directory;
//! `AI_PANEL_DATA_DIR` overrides it for smoke tests and portable setups.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const DATA_DIR_ENV: &str = "AI_PANEL_DATA_DIR";

/// `$AI_PANEL_DATA_DIR`, else `<platform data dir>/ai-status-panel`.
pub fn default_data_dir() -> PathBuf {
    resolve_data_dir(std::env::var_os(DATA_DIR_ENV), dirs::data_dir())
}

fn resolve_data_dir(override_dir: Option<OsString>, platform_dir: Option<PathBuf>) -> PathBuf {
    match override_dir.filter(|value| !value.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => platform_dir
            .unwrap_or_else(home_dir)
            .join("ai-status-panel"),
    }
}

/// `$CODEX_HOME`, else `~/.codex`.
pub fn codex_home() -> PathBuf {
    match std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => home_dir().join(".codex"),
    }
}

/// Claude Code's user settings, which carry the hook configuration.
pub fn claude_settings_file() -> PathBuf {
    home_dir().join(".claude").join("settings.json")
}

/// A Claude Desktop MSIX child may write to a redirected AppData directory even
/// when the panel itself was launched outside Claude. Follow both live logs;
/// copying a historical events.jsonl during an update does not keep it live.
pub fn claude_hook_event_files(paths: &DataPaths) -> Vec<PathBuf> {
    let mut files = vec![paths.events()];
    #[cfg(windows)]
    {
        if let Some(roaming) = dirs::data_dir() {
            files.push(roaming.join("ai-status-panel/events.jsonl"));
        }
        if let Some(local) = dirs::data_local_dir() {
            files.push(local.join(
                "Packages/Claude_pzs8sxrjxfjjc/LocalCache/Roaming/ai-status-panel/events.jsonl",
            ));
        }
    }
    files.sort();
    files.dedup();
    files
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// File layout of the data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_location() -> Self {
        Self::new(default_data_dir())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn device(&self) -> PathBuf {
        self.root.join("device.json")
    }

    pub fn acknowledged(&self) -> PathBuf {
        self.root.join("acknowledged.json")
    }

    pub fn tracked_codex(&self) -> PathBuf {
        self.root.join("tracked-codex.json")
    }

    /// Claude hook events, appended by `--hook` mode.
    pub fn events(&self) -> PathBuf {
        self.root.join("events.jsonl")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_wins_over_platform_directory() {
        let platform = Some(PathBuf::from("/platform"));
        assert_eq!(
            resolve_data_dir(Some("/custom".into()), platform.clone()),
            PathBuf::from("/custom")
        );
        assert_eq!(
            resolve_data_dir(Some("".into()), platform),
            PathBuf::from("/platform/ai-status-panel")
        );
    }

    #[test]
    fn files_sit_directly_under_the_root() {
        let paths = DataPaths::new("/data");
        assert_eq!(paths.config(), Path::new("/data").join("config.json"));
        assert_eq!(paths.events(), Path::new("/data").join("events.jsonl"));
        assert_eq!(
            paths.tracked_codex(),
            Path::new("/data").join("tracked-codex.json")
        );
    }
}
