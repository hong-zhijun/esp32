//! Running applications are independent of task history. Poll executable identities only;
//! never infer presence from old logs or search arbitrary command-line text.
use crate::state::Provider;
use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RunningProviders {
    pub claude: bool,
    pub codex: bool,
    pub deepseek: bool,
}

impl RunningProviders {
    fn include(&mut self, provider: Provider) {
        match provider {
            Provider::Claude => self.claude = true,
            Provider::Codex => self.codex = true,
            Provider::Deepseek => self.deepseek = true,
        }
    }
}

pub struct ProcessMonitor {
    system: System,
    running: RunningProviders,
}

impl Default for ProcessMonitor {
    fn default() -> Self {
        Self {
            system: System::new(),
            running: RunningProviders::default(),
        }
    }
}

impl ProcessMonitor {
    /// Refresh membership and executable paths every tick, including exec() under an
    /// existing PID. Do not collect CPU, memory, environment or arguments.
    pub fn poll(&mut self) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
        );
        let mut running = RunningProviders::default();
        for process in self.system.processes().values() {
            if process.status() == ProcessStatus::Zombie {
                continue;
            }
            let Some(exe) = process.exe() else { continue };
            if let Some(provider) = executable_provider(&exe.to_string_lossy()) {
                running.include(provider);
            }
        }
        self.running = running;
    }

    pub fn running(&self) -> RunningProviders {
        self.running
    }
}

/// Exact executable names and main .app executables only. An Electron helper, crash reporter,
/// a similarly named editor/document or a shell mentioning an app must never keep a card alive.
fn executable_provider(executable: &str) -> Option<Provider> {
    let normalized = executable.replace('\\', "/").to_lowercase();
    if let Some((_, inner)) = normalized.split_once(".app/contents/") {
        // Only the outer bundle's main executable counts, not nested helper/CLI bundles.
        let name = inner.strip_prefix("macos/")?;
        if name.contains('/') {
            return None;
        }
        return executable_name(name);
    }
    executable_name(normalized.rsplit('/').next()?)
}

fn executable_name(name: &str) -> Option<Provider> {
    match name.strip_suffix(".exe").unwrap_or(name) {
        "claude" | "claude code" => Some(Provider::Claude),
        "codex" | "chatgpt" => Some(Provider::Codex),
        "dsh" | "deepseek" | "deepseek harness" | "deepseek-harness" => Some(Provider::Deepseek),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_desktop_and_native_cli_executables_on_both_platforms() {
        for (path, provider) in [
            (
                "/Applications/Claude.app/Contents/MacOS/Claude",
                Provider::Claude,
            ),
            (
                "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
                Provider::Codex,
            ),
            (
                "/Applications/Codex.app/Contents/MacOS/Codex",
                Provider::Codex,
            ),
            (
                "/Applications/DeepSeek Harness.app/Contents/MacOS/dsh",
                Provider::Deepseek,
            ),
            (
                "/Applications/DeepSeek Harness.app/Contents/MacOS/DeepSeek Harness",
                Provider::Deepseek,
            ),
            ("/opt/homebrew/bin/dsh", Provider::Deepseek),
            ("/Users/me/.local/bin/claude", Provider::Claude),
            ("/usr/local/bin/codex", Provider::Codex),
            (
                r"C:\Users\me\AppData\Local\Claude\Claude.exe",
                Provider::Claude,
            ),
            (r"C:\Program Files\ChatGPT\ChatGPT.exe", Provider::Codex),
            (r"C:\Users\me\.dsh\bin\dsh.exe", Provider::Deepseek),
        ] {
            assert_eq!(executable_provider(path), Some(provider), "{path}");
        }
    }

    #[test]
    fn helpers_nested_bundles_and_similar_names_do_not_count() {
        for path in [
            "/Applications/ChatGPT.app/Contents/Frameworks/Codex (Renderer).app/Contents/MacOS/Codex (Renderer)",
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
            "/Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper",
            "/Applications/DeepSeek Harness.app/Contents/Frameworks/crashpad_handler",
            "/usr/bin/node", "/bin/zsh", "/usr/local/bin/codex-code-mode-host",
            "/usr/bin/not-claude", "C:/Tools/dsh-monitor.exe",
            "/tmp/claude.txt", "/tmp/ChatGPT Helper.exe",
        ] {
            assert_eq!(executable_provider(path), None, "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn live_process_is_discovered_and_removed_after_exit() {
        use std::process::{Child, Command};
        struct Fixture(Child);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dsh");
        std::fs::copy("/bin/sleep", &exe).unwrap();
        let mut fixture = Fixture(Command::new(&exe).arg("60").spawn().unwrap());
        let pid = sysinfo::Pid::from_u32(fixture.0.id());
        let mut monitor = ProcessMonitor::default();
        monitor.poll();
        assert!(monitor.running().deepseek);
        assert_eq!(
            monitor.system.process(pid).unwrap().exe(),
            Some(exe.as_path())
        );
        fixture.0.kill().unwrap();
        fixture.0.wait().unwrap();
        monitor.poll();
        assert!(
            monitor.system.process(pid).is_none(),
            "exited PID must not remain cached"
        );
    }
}
