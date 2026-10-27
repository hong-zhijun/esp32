//! Messages sent back to the board. `type` must stay the first key: the firmware
//! routes on the `{"type":"voice"` prefix. `serde_json` keeps insertion order
//! because the crate enables its `preserve_order` feature.

use serde_json::{json, Value};

pub(crate) fn voice(state: &str, text: &str) -> Value {
    json!({"type": "voice", "state": state, "text": text})
}
