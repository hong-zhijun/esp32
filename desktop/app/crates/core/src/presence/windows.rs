//! Electron uses the same executable for its main process and helpers on Windows.
//! Inspect only candidate arguments; never log them or send them to the UI/device.
use super::{executable_provider, Provider, RunningProviders};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

struct Identity {
    parent: Option<u32>,
    started_at: u64,
    provider: Option<Provider>,
    eligible: bool,
}

pub(super) fn poll(system: &mut System) -> RunningProviders {
    // A separate targeted refresh avoids collecting command lines of unrelated apps.
    let candidates: Vec<_> = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let exe = normalize(&process.exe()?.to_string_lossy());
            (executable_provider(&exe).is_some() || basename(&exe) == "node.exe").then_some(*pid)
        })
        .collect();
    if !candidates.is_empty() {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&candidates),
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::Always)
                .with_cmd(UpdateKind::Always),
        );
    }
    running_providers(&identities(system))
}

fn identities(system: &System) -> HashMap<u32, Identity> {
    system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let exe = process.exe().map(|path| path.to_string_lossy());
            let (provider, eligible) = identify(exe.as_deref().unwrap_or(""), process.cmd());
            (
                pid.as_u32(),
                Identity {
                    parent: process.parent().map(|pid| pid.as_u32()),
                    started_at: process.start_time(),
                    provider,
                    eligible,
                },
            )
        })
        .collect()
}

fn running_providers(processes: &HashMap<u32, Identity>) -> RunningProviders {
    let mut running = RunningProviders::default();
    for (pid, process) in processes {
        if let Some(provider) = process.provider {
            if process.eligible && !has_provider_ancestor(*pid, provider, processes) {
                running.include(provider);
            }
        }
    }
    running
}

fn has_provider_ancestor(pid: u32, provider: Provider, processes: &HashMap<u32, Identity>) -> bool {
    let mut child = &processes[&pid];
    let mut seen = HashSet::from([pid]);
    while let Some(parent_pid) = child.parent {
        if !seen.insert(parent_pid) {
            break;
        }
        let Some(parent) = processes.get(&parent_pid) else {
            break;
        };
        // Windows retains the original parent PID after exit; it may now be reused.
        if parent.started_at > child.started_at {
            break;
        }
        if parent.provider == Some(provider) {
            return true;
        }
        child = parent;
    }
    false
}

fn identify(executable: &str, args: &[OsString]) -> (Option<Provider>, bool) {
    let exe = normalize(executable);
    let node = basename(&exe) == "node.exe";
    let script = node
        .then(|| node_entry_script(args))
        .flatten()
        .map(normalize);
    let provider = if node {
        script.as_deref().and_then(node_script_provider)
    } else {
        executable_provider(&exe)
    };
    let helper = !node
        && args
            .iter()
            .skip(1)
            .take_while(|arg| *arg != "--")
            .any(|arg| {
                let arg = arg.to_string_lossy();
                arg == "--type"
                    || arg.starts_with("--type=")
                    || arg == "--crashpad-handler"
                    || arg.starts_with("--crashpad-handler=")
            });
    // Managed CLI copies can outlive the desktop process, so ancestry alone is insufficient.
    let embedded = is_embedded(&exe) || script.as_deref().is_some_and(is_embedded);
    (provider, provider.is_some() && !helper && !embedded)
}

fn is_embedded(path: &str) -> bool {
    path.contains("/resources/")
        || path.contains("/claude/claude-code/")
        || path.contains("/openai/codex/bin/")
}

fn node_entry_script(args: &[OsString]) -> Option<&str> {
    let mut args = args.iter().skip(1);
    loop {
        let arg = args.next()?.to_str()?;
        match arg {
            // Only flags with no value are skipped. In particular, -e/-p/--eval,
            // --require and arbitrary prompt arguments must never become scripts.
            "--no-warnings" | "--enable-source-maps" | "--no-deprecation" => continue,
            "--" => return args.next()?.to_str(),
            _ if arg.starts_with('-') => return None,
            _ => return Some(arg),
        }
    }
}

fn node_script_provider(script: &str) -> Option<Provider> {
    // Prefixing a separator also handles a relative node_modules/... entry path.
    let script = format!("/{script}");
    if script.ends_with("/node_modules/@anthropic-ai/claude-code/cli.js") {
        Some(Provider::Claude)
    } else if script.ends_with("/node_modules/@openai/codex/bin/codex.js") {
        Some(Provider::Codex)
    } else {
        None
    }
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn recognizes_store_desktops_native_clis_and_npm_entry_points() {
        for (exe, command, provider) in [
            (
                r"C:\Program Files\WindowsApps\OpenAI.Codex_1_x64__id\app\ChatGPT.exe",
                vec!["ChatGPT.exe"],
                Provider::Codex,
            ),
            (
                r"C:\Program Files\WindowsApps\Claude_1_x64__id\app\claude.exe",
                vec!["claude.exe"],
                Provider::Claude,
            ),
            (
                r"C:\Users\me\.dsh\bin\dsh.exe",
                vec!["dsh.exe"],
                Provider::Deepseek,
            ),
            (
                r"C:\Users\me\.local\bin\claude.exe",
                vec!["claude.exe", "--resume"],
                Provider::Claude,
            ),
            (
                r"C:\npm\node_modules\@openai\codex-win32-x64\vendor\codex.exe",
                vec!["codex.exe"],
                Provider::Codex,
            ),
            (
                r"C:\Program Files\nodejs\node.exe",
                vec![
                    "node.exe",
                    r"C:\Users\用户\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\cli.js",
                    "--resume",
                ],
                Provider::Claude,
            ),
            (
                r"D:\Node\NODE.EXE",
                vec![
                    "node.exe",
                    "--no-warnings",
                    "--enable-source-maps",
                    r"C:\Program Files\npm\node_modules\@openai\codex\bin\codex.js",
                ],
                Provider::Codex,
            ),
            (
                "C:/node/node.exe",
                vec!["node.exe", "--", "node_modules/@openai/codex/bin/codex.js"],
                Provider::Codex,
            ),
            (
                "C:/node/node.exe",
                vec![
                    "node.exe",
                    "C:/npm/node_modules/@openai/codex/bin/codex.js",
                    "--type=renderer",
                ],
                Provider::Codex,
            ),
            (
                "C:/cli/claude.exe",
                vec!["claude.exe", "--", "--type=renderer"],
                Provider::Claude,
            ),
        ] {
            assert_eq!(
                identify(exe, &args(&command)),
                (Some(provider), true),
                "{exe}"
            );
        }
    }

    #[test]
    fn orphan_helpers_and_managed_cli_copies_do_not_count() {
        for (exe, command) in [
            (
                r"C:\Claude\claude.exe",
                vec!["claude.exe", "--type=renderer"],
            ),
            (
                r"C:\Codex\ChatGPT.exe",
                vec!["ChatGPT.exe", "--type", "utility"],
            ),
            (
                r"C:\DeepSeek\DeepSeek.exe",
                vec!["DeepSeek.exe", "--type=gpu-process"],
            ),
            (
                r"C:\Claude\claude.exe",
                vec!["claude.exe", "--crashpad-handler"],
            ),
            (r"C:\Codex\resources\codex.exe", vec!["codex.exe"]),
            (
                r"C:\Claude\resources\app.asar.unpacked\node_modules\cli\claude.exe",
                vec!["claude.exe"],
            ),
            (
                r"C:\Users\me\AppData\Roaming\Claude\claude-code\2.1\hash\claude.exe",
                vec!["claude.exe"],
            ),
            (
                r"C:\Users\me\AppData\Local\OpenAI\Codex\bin\hash\codex.exe",
                vec!["codex.exe"],
            ),
            (
                "C:/node/node.exe",
                vec![
                    "node.exe",
                    "C:/Claude/resources/app/node_modules/@anthropic-ai/claude-code/cli.js",
                ],
            ),
        ] {
            let (provider, eligible) = identify(exe, &args(&command));
            assert!(provider.is_some(), "{exe}");
            assert!(!eligible, "{exe}: {command:?}");
        }
    }

    #[test]
    fn node_only_matches_the_actual_entry_script() {
        let cli = r"C:\npm\node_modules\@anthropic-ai\claude-code\cli.js";
        for command in [
            vec!["node.exe", "server.js", cli],
            vec!["node.exe", "--eval", cli],
            vec!["node.exe", "-p", cli],
            vec!["node.exe", "--require", cli, "server.js"],
            vec!["node.exe", "--title", cli],
            vec!["node.exe", "C:/projects/claude/cli.js"],
            vec![
                "node.exe",
                "C:/npm/node_modules/@openai/codex/bin/codex.js.bak",
            ],
            vec!["node.exe"],
            vec![],
        ] {
            assert_eq!(identify("C:/node/node.exe", &args(&command)), (None, false));
        }
        assert_eq!(
            identify("C:/Windows/cmd.exe", &args(&["cmd.exe", cli])),
            (None, false)
        );
    }

    fn identity(
        parent: Option<u32>,
        started_at: u64,
        provider: Option<Provider>,
        eligible: bool,
    ) -> Identity {
        Identity {
            parent,
            started_at,
            provider,
            eligible,
        }
    }

    #[test]
    fn ancestry_ignores_wrappers_handles_reused_pids_and_terminates_on_cycles() {
        let mut processes = HashMap::from([
            (1, identity(None, 1, Some(Provider::Claude), true)),
            (2, identity(Some(1), 2, None, false)),
            (3, identity(Some(2), 3, Some(Provider::Claude), true)),
            (4, identity(None, 1, Some(Provider::Codex), true)),
            (5, identity(None, 1, Some(Provider::Deepseek), true)),
        ]);
        assert!(has_provider_ancestor(3, Provider::Claude, &processes));
        assert_eq!(
            running_providers(&processes),
            RunningProviders {
                claude: true,
                codex: true,
                deepseek: true
            }
        );
        processes.get_mut(&1).unwrap().started_at = 4;
        assert!(!has_provider_ancestor(3, Provider::Claude, &processes));
        processes.remove(&1);
        assert!(!has_provider_ancestor(3, Provider::Claude, &processes));
        processes.get_mut(&2).unwrap().parent = Some(3);
        assert!(!has_provider_ancestor(3, Provider::Claude, &processes));
    }

    // A separate executable accepts arbitrary Electron/Node arguments. It is built with
    // the same Rust toolchain as these tests and never launches or closes a real AI app.
    #[test]
    fn live_windows_processes_track_start_exit_and_orphan_helpers() {
        use std::os::windows::process::CommandExt;
        use std::process::{Child, Command};
        struct Fixture(Child);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        fn spawn(exe: &std::path::Path, args: &[&str]) -> Fixture {
            Fixture(
                Command::new(exe)
                    .args(args)
                    .creation_flags(0x08000000)
                    .spawn()
                    .unwrap(),
            )
        }
        fn fixture_running(
            monitor: &mut super::super::ProcessMonitor,
            dir: &std::path::Path,
        ) -> RunningProviders {
            monitor.poll();
            let identities = identities(&monitor.system);
            let fixtures = identities
                .into_iter()
                .filter(|(pid, _)| {
                    monitor
                        .system
                        .process(sysinfo::Pid::from_u32(*pid))
                        .and_then(|process| process.exe())
                        .is_some_and(|exe| exe.starts_with(dir))
                })
                .collect();
            running_providers(&fixtures)
        }
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dsh.exe");
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/presence-process.rs");
        let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg(source)
            .arg("--crate-name")
            .arg("presence_fixture")
            .arg("-o")
            .arg(&exe)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut monitor = super::super::ProcessMonitor::default();
        let mut main = spawn(&exe, &[]);
        let pid = sysinfo::Pid::from_u32(main.0.id());
        assert!(fixture_running(&mut monitor, dir.path()).deepseek);
        let _helper = spawn(&exe, &["--type=renderer"]);
        main.0.kill().unwrap();
        main.0.wait().unwrap();
        assert_eq!(
            fixture_running(&mut monitor, dir.path()),
            RunningProviders::default()
        );
        assert!(
            monitor.system.process(pid).is_none(),
            "exited PID must be removed"
        );

        let node = dir.path().join("node.exe");
        std::fs::copy(&exe, &node).unwrap();
        let _unrelated = spawn(
            &node,
            &[
                "server.js",
                "C:/npm/node_modules/@openai/codex/bin/codex.js",
            ],
        );
        assert_eq!(
            fixture_running(&mut monitor, dir.path()),
            RunningProviders::default()
        );
        let mut cli = spawn(
            &node,
            &["C:/npm/node_modules/@anthropic-ai/claude-code/cli.js"],
        );
        assert!(fixture_running(&mut monitor, dir.path()).claude);
        cli.0.kill().unwrap();
        cli.0.wait().unwrap();
        assert_eq!(
            fixture_running(&mut monitor, dir.path()),
            RunningProviders::default()
        );

        let resources = dir.path().join("resources");
        std::fs::create_dir(&resources).unwrap();
        std::fs::copy(&exe, resources.join("codex.exe")).unwrap();
        let _embedded = spawn(&resources.join("codex.exe"), &[]);
        assert_eq!(
            fixture_running(&mut monitor, dir.path()),
            RunningProviders::default()
        );
    }
}
