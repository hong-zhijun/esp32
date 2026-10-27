use super::*;
use crate::state::Phase;
use serde_json::json;
use std::io::Write;

const NOW: i64 = 1_800_000_000_000;

fn header() -> Value {
    json!({"type":"session","version":4,"id":"main","cwd":"D:\\work\\project","createdAt":NOW,"delegationDepth":0})
}

fn row(kind: &str, time: i64, data: Value) -> Value {
    json!({"type":kind,"seq":time-NOW,"time":time,"data":data})
}

fn fixture() -> (tempfile::TempDir, PathBuf, DeepseekSource) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("dsh");
    let dir = home.join("sessions/project/session-main");
    fs::create_dir_all(&dir).unwrap();
    let paths = DataPaths::new(temp.path().join("panel"));
    let source = DeepseekSource::with_home(&paths, home);
    (temp, dir.join("session.v4.jsonl.zstd"), source)
}

fn frame(rows: &[Value]) -> Vec<u8> {
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    zstd::encode_all(text.as_bytes(), 1).unwrap()
}

fn append(path: &Path, bytes: &[u8]) {
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn write_log(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    fs::write(path, bytes)?;
    fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .set_modified(UNIX_EPOCH + std::time::Duration::from_millis(NOW as u64))
}

#[test]
fn compressed_multiframe_log_streams_start_approval_resolution_and_completion_once() {
    let (_temp, path, mut source) = fixture();
    let first = frame(&[header()]);
    let start = frame(&[
        row("turn/start", NOW, json!({"turn":0})),
        row(
            "tool/call",
            NOW + 1,
            json!({"name":"pwsh","arguments":"PRIVATE"}),
        ),
    ]);
    write_log(&path, [first, start].concat()).unwrap();
    let mut state = State::default();
    source.tick(&mut state, NOW + 2).unwrap();
    let card = &state.snapshot(NOW + 2).cards[2];
    assert_eq!(card.session.state, Phase::Working);
    assert_eq!(card.session.tool, "pwsh");
    assert_eq!(card.session.project, "project");

    append(
        &path,
        &frame(&[
            row("approval/asked", NOW + 3, json!({"id":"a"})),
            row("approval/asked", NOW + 4, json!({"id":"b"})),
        ]),
    );
    source.tick(&mut state, NOW + 5).unwrap();
    assert_eq!(
        state.snapshot(NOW + 5).cards[2].session.state,
        Phase::Waiting
    );
    append(
        &path,
        &frame(&[
            row(
                "approval/decided",
                NOW + 6,
                json!({"id":"a","outcome":"allow"}),
            ),
            row(
                "tool/result",
                NOW + 7,
                json!({"message":{"content":"PRIVATE"}}),
            ),
        ]),
    );
    source.tick(&mut state, NOW + 8).unwrap();
    assert_eq!(
        state.snapshot(NOW + 8).cards[2].session.state,
        Phase::Waiting
    );
    append(
        &path,
        &frame(&[
            row("approval/decided", NOW + 9, json!({"id":"b"})),
            row("session/title", NOW + 10, json!({"title":"new title"})),
        ]),
    );
    source.tick(&mut state, NOW + 11).unwrap();
    assert_eq!(
        state.snapshot(NOW + 11).cards[2].session.state,
        Phase::Working
    );
    assert_eq!(
        state.snapshot(NOW + 11).cards[2].session.updated_at,
        NOW + 9
    );
    append(
        &path,
        &frame(&[row(
            "turn/end",
            NOW + 12,
            json!({"reason":{"kind":"completed"}}),
        )]),
    );
    source.tick(&mut state, NOW + 13).unwrap();
    let snapshot = state.snapshot(NOW + 13);
    assert_eq!(snapshot.cards[2].session.state, Phase::Done);
    assert_eq!(
        snapshot.cards[2].session.title.as_deref(),
        Some("new title")
    );
    assert_eq!(snapshot.cards[2].rows[0].project, "new title");
    assert!(!serde_json::to_string(&snapshot)
        .unwrap()
        .contains("PRIVATE"));
    state.acknowledge(Provider::Deepseek, "main");
    source.tick(&mut state, NOW + 14).unwrap();
    assert_eq!(state.snapshot(NOW + 14).cards[2].session.state, Phase::Idle);
}

#[test]
fn title_before_first_turn_is_used_on_screen_and_rename_keeps_completion_timestamp() {
    let mut file = LogFile::default();
    let mut state = State::default();
    file.apply(&header(), &mut state);
    file.apply(
        &row("session/title", NOW, json!({"title":"对话标题"})),
        &mut state,
    );
    file.apply(&row("turn/start", NOW + 1, json!({})), &mut state);
    assert_eq!(state.snapshot(NOW + 1).cards[2].rows[0].project, "对话标题");
    file.apply(
        &row("turn/end", NOW + 2, json!({"reason":{"kind":"completed"}})),
        &mut state,
    );
    state.acknowledge(Provider::Deepseek, "main");
    file.apply(
        &row(
            "session/title",
            NOW + 3,
            json!({"title":"修改后的对话标题"}),
        ),
        &mut state,
    );
    let snapshot = state.snapshot(NOW + 3);
    assert_eq!(snapshot.cards[2].rows[0].project, "修改后的对话标题");
    assert_eq!(snapshot.cards[2].session.updated_at, NOW + 2);
    assert_eq!(snapshot.cards[2].session.state, Phase::Idle);
    assert!(!snapshot.cards[2].session.unread);
}

#[test]
fn partial_frames_and_truncation_do_not_lose_or_duplicate_events() {
    let (_temp, path, mut source) = fixture();
    write_log(&path, frame(&[header(), row("turn/start", NOW, json!({}))])).unwrap();
    let mut state = State::default();
    source.tick(&mut state, NOW).unwrap();
    let end = frame(&[row(
        "turn/end",
        NOW + 1,
        json!({"reason":{"kind":"completed"}}),
    )]);
    append(&path, &end[..end.len() - 1]);
    source.tick(&mut state, NOW + 2).unwrap();
    assert_eq!(
        state.snapshot(NOW + 2).cards[2].session.state,
        Phase::Working
    );
    append(&path, &end[end.len() - 1..]);
    source.tick(&mut state, NOW + 3).unwrap();
    assert_eq!(state.snapshot(NOW + 3).cards[2].session.state, Phase::Done);
    write_log(&path, frame(&[header()])).unwrap();
    source.tick(&mut state, NOW + 4).unwrap();
    append(&path, &frame(&[row("turn/start", NOW + 5, json!({}))]));
    source.tick(&mut state, NOW + 6).unwrap();
    assert_eq!(
        state.snapshot(NOW + 6).cards[2].session.state,
        Phase::Working
    );
}

#[test]
fn subagents_are_excluded_and_unknown_end_reasons_never_claim_success() {
    let mut log = LogFile::default();
    let mut state = State::default();
    let mut child = header();
    child["delegationDepth"] = json!(1);
    log.apply(&child, &mut state);
    log.apply(&row("turn/start", NOW, json!({})), &mut state);
    assert!(state.snapshot(NOW).sessions.is_empty());
    log = LogFile::default();
    log.apply(&header(), &mut state);
    log.apply(&row("turn/start", NOW, json!({})), &mut state);
    log.apply(
        &row("turn/end", NOW + 1, json!({"reason":{"kind":"failed"}})),
        &mut state,
    );
    assert_eq!(
        state.snapshot(NOW + 1).cards[2].session.state,
        Phase::Unknown
    );
    log.apply(
        &row("session/title", NOW + 2, json!({"title":"renamed"})),
        &mut state,
    );
    assert_eq!(state.snapshot(NOW + 2).cards[2].session.updated_at, NOW + 1);
    log.apply(&row("turn/start", NOW + 3, json!({})), &mut state);
    log.apply(
        &row(
            "turn/end",
            NOW + 4,
            json!({"reason":{"kind":"interrupted"}}),
        ),
        &mut state,
    );
    assert_eq!(
        state.snapshot(NOW + 4).cards[2].session.state,
        Phase::Interrupted
    );
}

#[test]
fn tracked_logs_are_replayed_after_restart_and_plaintext_v4_is_supported() {
    let (temp, path, mut source) = fixture();
    let plain = path.with_file_name("session.v4.jsonl");
    let content = [
        header(),
        row("turn/start", NOW, json!({})),
        row("turn/end", NOW + 1, json!({"reason":{"kind":"completed"}})),
    ]
    .iter()
    .map(|v| format!("{v}\n"))
    .collect::<String>();
    write_log(&plain, content).unwrap();
    let mut state = State::default();
    source.tick(&mut state, NOW).unwrap();
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(5 * 86400);
    fs::OpenOptions::new()
        .write(true)
        .open(&plain)
        .unwrap()
        .set_modified(past)
        .unwrap();
    let paths = DataPaths::new(temp.path().join("panel"));
    let mut restarted = DeepseekSource::with_home(&paths, temp.path().join("dsh"));
    let mut restored = State::default();
    restarted.tick(&mut restored, NOW).unwrap();
    assert_eq!(
        restored.snapshot(NOW + 2).cards[2].session.state,
        Phase::Done
    );
}

#[test]
fn frame_boundaries_cover_every_partial_prefix_and_reject_corruption() {
    let compressed = frame(&[header()]);
    for end in 0..compressed.len() {
        assert_eq!(frame_length(&compressed[..end]), Ok(None));
    }
    assert_eq!(frame_length(&compressed), Ok(Some(compressed.len())));
    assert!(frame_length(b"not a frame").is_err());
}

#[test]
fn corrupt_append_does_not_replay_completion_after_user_acknowledges_it() {
    let (_temp, path, mut source) = fixture();
    let mut bytes = frame(&[
        header(),
        row("turn/start", NOW, json!({})),
        row("turn/end", NOW + 1, json!({"reason":{"kind":"completed"}})),
    ]);
    bytes.extend_from_slice(b"corrupt-frame");
    write_log(&path, bytes).unwrap();
    let mut state = State::default();
    assert!(source.tick(&mut state, NOW + 2).is_err());
    assert_eq!(state.snapshot(NOW + 2).cards[2].session.state, Phase::Done);
    state.acknowledge(Provider::Deepseek, "main");
    assert!(source.tick(&mut state, NOW + 3).is_err());
    assert_eq!(state.snapshot(NOW + 3).cards[2].session.state, Phase::Idle);
}
