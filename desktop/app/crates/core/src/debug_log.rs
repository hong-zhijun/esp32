//! Bounded in-memory diagnostics shown in the UI. Never written to disk.
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const LIMIT: usize = 300;

type Listener = Box<dyn Fn(&Value) + Send + Sync>;

#[derive(Default)]
struct Inner {
    entries: VecDeque<Value>,
    next: u64,
    listeners: Vec<Listener>,
}

/// Cheap to clone; all clones share the same log.
#[derive(Clone, Default)]
pub struct DebugLog(Arc<Mutex<Inner>>);

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl DebugLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// `event` must be a JSON object with an `"event"` name, e.g.
    /// `json!({"event":"voice-error","stage":"wetype","message":"..."})`.
    /// `id` and `time` (ms) are added here.
    pub fn add(&self, event: Value) -> Value {
        let mut inner = self.0.lock();
        inner.next += 1;
        let mut row: Map<String, Value> = match event {
            Value::Object(map) => map,
            other => {
                let mut m = Map::new();
                m.insert("event".into(), other);
                m
            }
        };
        row.insert("id".into(), inner.next.into());
        row.insert("time".into(), now_ms().into());
        let row = Value::Object(row);
        inner.entries.push_back(row.clone());
        while inner.entries.len() > LIMIT {
            inner.entries.pop_front();
        }
        for listener in &inner.listeners {
            listener(&row);
        }
        row
    }

    pub fn entries(&self) -> Vec<Value> {
        self.0.lock().entries.iter().cloned().collect()
    }

    /// Listeners run while the log lock is held: keep them short and never call `add` from one.
    pub fn subscribe(&self, listener: impl Fn(&Value) + Send + Sync + 'static) {
        self.0.lock().listeners.push(Box::new(listener));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_latest_entries_with_ids() {
        let log = DebugLog::new();
        for i in 0..(LIMIT + 5) {
            log.add(json!({"event": "x", "n": i}));
        }
        let entries = log.entries();
        assert_eq!(entries.len(), LIMIT);
        assert_eq!(entries.last().unwrap()["id"], json!((LIMIT + 5) as u64));
    }

    #[test]
    fn subscribers_see_every_entry_and_text_is_kept_verbatim() {
        let log = DebugLog::new();
        let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&seen);
        log.subscribe(move |entry| sink.lock().push(entry.clone()));
        log.add(json!({"event": "button"}));
        log.add(json!({"event": "recognized", "text": "<script>测试</script>"}));
        log.add(json!({"event": "done"}));
        assert_eq!(seen.lock().len(), 3);
        let entries = log.entries();
        assert_eq!(entries[1]["text"], "<script>测试</script>");
        assert!(entries[0]["time"].as_u64().unwrap() > 0);
        assert_eq!(
            entries
                .iter()
                .map(|e| e["id"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }
}
