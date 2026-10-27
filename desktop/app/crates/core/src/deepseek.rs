//! Read-only Harness v4 session source. Zstd frames are appended independently;
//! an unfinished frame is kept until the next tick. No prompts, arguments or replies
//! are copied into the panel state, logs or persistence.
use crate::paths::DataPaths;
use crate::state::{Event, Provider, State};
use crate::store;
use chrono::DateTime;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const SCAN_MS: i64 = 5_000;
const DAY_MS: i64 = 86_400_000;
const READ_BUDGET: usize = 2 * 1024 * 1024;
const MAX_FRAME: usize = 16 * 1024 * 1024;
const MAX_DECODED: usize = 32 * 1024 * 1024;

pub struct DeepseekSource {
    home: PathBuf,
    tracked_path: PathBuf,
    tracked: BTreeSet<PathBuf>,
    files: BTreeMap<PathBuf, LogFile>,
    last_scan: Option<i64>,
    next_file: usize,
}

#[derive(Default)]
struct LogFile {
    offset: u64,
    pending: Vec<u8>,
    meta: Value,
    title: Option<String>,
    ignored: bool,
    approvals: BTreeSet<String>,
    tool: Option<String>,
    turn_open: bool,
    failed: bool,
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("DSH_HOME")
        .filter(|s| !s.to_string_lossy().trim().is_empty())
        .map(|s| {
            let text = s.to_string_lossy();
            if text == "~" {
                dirs::home_dir().unwrap_or_default()
            } else if text.starts_with("~/") || text.starts_with("~\\") {
                dirs::home_dir().unwrap_or_default().join(&text[2..])
            } else {
                PathBuf::from(s)
            }
        })
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".dsh"))
}

impl DeepseekSource {
    pub fn with_home(paths: &DataPaths, home: PathBuf) -> Self {
        let tracked_path = paths.root().join("tracked-deepseek.json");
        Self {
            home,
            tracked: store::read_json(&tracked_path).unwrap_or_default(),
            tracked_path,
            files: BTreeMap::new(),
            last_scan: None,
            next_file: 0,
        }
    }

    pub fn tick(&mut self, state: &mut State, now: i64) -> Result<(), String> {
        let mut error = None;
        if self.last_scan.is_none_or(|at| now - at >= SCAN_MS) {
            if let Err(problem) = self.scan(now) {
                error = Some(problem);
            }
            self.last_scan = Some(now);
        }
        // Round-robin and a shared byte budget keep startup catch-up from blocking
        // the one-second board updates when there are many large histories.
        let paths: Vec<_> = self.files.keys().cloned().collect();
        let mut budget = READ_BUDGET;
        for _ in 0..paths.len() {
            let index = self.next_file % paths.len();
            self.next_file = (index + 1) % paths.len();
            let path = &paths[index];
            let file = self.files.get_mut(path).unwrap();
            if let Err(problem) = file.poll(path, state, &mut budget) {
                error = Some(problem);
            }
            if budget == 0 {
                break;
            }
        }
        error.map_or(Ok(()), Err)
    }

    fn scan(&mut self, now: i64) -> Result<(), String> {
        let mut found = Vec::new();
        collect_logs(&self.home.join("sessions"), &mut found).map_err(|_| "deepseek_scan_error")?;
        let mut changed = false;
        for path in found {
            if !self.tracked.contains(&path) {
                let modified = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64);
                if !modified.is_some_and(|at| now - at < DAY_MS) {
                    continue;
                }
                self.tracked.insert(path.clone());
                changed = true;
            }
            self.files.entry(path).or_default();
        }
        if changed {
            store::write_json(&self.tracked_path, &self.tracked)
                .map_err(|_| "deepseek_save_error")?;
        }
        Ok(())
    }
}

fn collect_logs(dir: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_logs(&entry.path(), found)?;
        } else if kind.is_file()
            && matches!(
                entry.file_name().to_str(),
                Some("session.v4.jsonl.zstd" | "session.v4.jsonl")
            )
        {
            found.push(entry.path());
        }
    }
    Ok(())
}

impl LogFile {
    fn poll(&mut self, path: &Path, state: &mut State, budget: &mut usize) -> Result<(), String> {
        let metadata = match fs::metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("deepseek_read_error".into()),
        };
        if metadata.len() < self.offset {
            *self = Self::default();
        }
        if self.ignored {
            return Ok(());
        }
        if self.failed {
            return Err("deepseek_invalid_log".into());
        }
        if self.pending.len() > MAX_FRAME {
            return Err("deepseek_frame_too_large".into());
        }
        let count = (metadata.len() - self.offset).min(*budget as u64) as usize;
        if count > 0 {
            let mut file = File::open(path).map_err(|_| "deepseek_read_error")?;
            file.seek(SeekFrom::Start(self.offset))
                .map_err(|_| "deepseek_read_error")?;
            let old = self.pending.len();
            file.take(count as u64)
                .read_to_end(&mut self.pending)
                .map_err(|_| "deepseek_read_error")?;
            let read = self.pending.len() - old;
            self.offset += read as u64;
            *budget -= read;
        }
        let compressed = path.extension().is_some_and(|ext| ext == "zstd");
        let mut used = 0;
        let outcome = (|| -> Result<(), String> {
            while used < self.pending.len() {
                let input = &self.pending[used..];
                let (size, decoded) = if compressed {
                    let Some(size) = frame_length(input).map_err(|_| "deepseek_invalid_frame")?
                    else {
                        break;
                    };
                    let mut decoder = zstd::stream::read::Decoder::new(&input[..size])
                        .map_err(|_| "deepseek_decode_error")?;
                    decoder
                        .window_log_max(27)
                        .map_err(|_| "deepseek_decode_error")?;
                    let mut decoded = Vec::new();
                    decoder
                        .take(MAX_DECODED as u64 + 1)
                        .read_to_end(&mut decoded)
                        .map_err(|_| "deepseek_decode_error")?;
                    if decoded.len() > MAX_DECODED {
                        return Err("deepseek_frame_too_large".into());
                    }
                    (size, decoded)
                } else {
                    let Some(end) = input.iter().position(|&b| b == b'\n') else {
                        break;
                    };
                    (end + 1, input[..end].to_vec())
                };
                // Harness writes complete JSON rows in each independent frame, often
                // without a newline between adjacent frames.
                for line in decoded.split(|&byte| byte == b'\n') {
                    if let Ok(row) = serde_json::from_slice(line) {
                        self.apply(&row, state);
                    }
                }
                used += size;
                if self.ignored {
                    break;
                }
            }
            Ok(())
        })();
        self.pending.drain(..used);
        if let Err(error) = outcome {
            self.pending.clear();
            self.failed = true;
            return Err(error);
        }
        if self.ignored {
            self.pending.clear();
        }
        if self.pending.len() > MAX_FRAME {
            return Err("deepseek_frame_too_large".into());
        }
        Ok(())
    }

    fn apply(&mut self, row: &Value, state: &mut State) {
        let kind = row["type"].as_str().unwrap_or("");
        if kind == "session" {
            self.ignored = row["version"].as_u64() != Some(4)
                || row["origin"].as_str() == Some("subagent")
                || row["delegationDepth"].as_u64().unwrap_or(0) > 0
                || row
                    .get("parentSessionId")
                    .is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty()));
            self.meta = row.clone();
            return;
        }
        if self.ignored {
            return;
        }
        let Some(id) = self.meta["id"].as_str().filter(|id| !id.is_empty()) else {
            return;
        };
        let data = &row["data"];
        if kind == "session/title" {
            if let Some(title) = data["title"].as_str() {
                self.title = Some(crate::state::take_units(title, crate::state::TITLE_UNITS));
                state.set_title(Provider::Deepseek, id, title);
            }
            return;
        }
        let Some(ms) = row["time"].as_i64() else {
            return;
        };
        let Some(time) = DateTime::from_timestamp_millis(ms) else {
            return;
        };
        let event = match kind {
            "turn/start" => {
                self.turn_open = true;
                self.approvals.clear();
                self.tool = None;
                "UserPromptSubmit"
            }
            "turn/end" => {
                self.turn_open = false;
                self.approvals.clear();
                match data["reason"]["kind"].as_str() {
                    Some("completed") => "Stop",
                    Some("interrupted" | "aborted" | "cancelled" | "canceled") => "Interrupt",
                    _ => "Error",
                }
            }
            "approval/asked" if self.turn_open => {
                if let Some(request) = data["id"].as_str() {
                    self.approvals.insert(request.into());
                }
                "PermissionRequest"
            }
            "approval/decided" if self.turn_open => {
                if let Some(request) = data["id"].as_str() {
                    self.approvals.remove(request);
                }
                if self.approvals.is_empty() {
                    "PostToolUse"
                } else {
                    "PermissionRequest"
                }
            }
            "tool/call" if self.turn_open => {
                self.tool = data["name"]
                    .as_str()
                    .map(|s| crate::state::take_units(s, 70));
                if self.approvals.is_empty() {
                    "PreToolUse"
                } else {
                    "PermissionRequest"
                }
            }
            "step/start" | "step/end" | "assistant/attempt" | "tool/result" if self.turn_open => {
                if self.approvals.is_empty() {
                    "PostToolUse"
                } else {
                    "PermissionRequest"
                }
            }
            _ => return,
        };
        state.apply(&Event {
            provider: Provider::Deepseek,
            session: id.into(),
            time: time.to_rfc3339(),
            event: event.into(),
            cwd: self.meta["cwd"].as_str().map(str::to_owned),
            tool: self.tool.clone(),
            notification: None,
            title: self.title.clone(),
        });
    }
}

/// Locate one complete Zstandard frame without decompressing a partially appended
/// frame. Supports content size, dictionary IDs, RLE/raw/compressed blocks/checksum.
fn frame_length(input: &[u8]) -> Result<Option<usize>, ()> {
    if input.len() < 5 {
        return Ok(None);
    }
    if input[..4] != [0x28, 0xb5, 0x2f, 0xfd] {
        return Err(());
    }
    let descriptor = input[4];
    if descriptor & 0x18 != 0 {
        return Err(());
    }
    let single = descriptor & 0x20 != 0;
    let content = match descriptor >> 6 {
        0 => usize::from(single),
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let mut pos = 5 + usize::from(!single) + [0, 1, 2, 4][(descriptor & 3) as usize] + content;
    if input.len() < pos {
        return Ok(None);
    }
    loop {
        if input.len() < pos + 3 {
            return Ok(None);
        }
        let block = u32::from_le_bytes([input[pos], input[pos + 1], input[pos + 2], 0]);
        let block_type = (block >> 1) & 3;
        if block_type == 3 {
            return Err(());
        }
        pos += 3 + if block_type == 1 {
            1
        } else {
            (block >> 3) as usize
        };
        if pos > MAX_FRAME {
            return Err(());
        }
        if input.len() < pos {
            return Ok(None);
        }
        if block & 1 != 0 {
            break;
        }
    }
    if descriptor & 4 != 0 {
        pos += 4;
    }
    if input.len() < pos {
        Ok(None)
    } else {
        Ok(Some(pos))
    }
}

#[cfg(test)]
#[path = "deepseek_tests.rs"]
mod tests;
