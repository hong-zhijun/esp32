//! Random identifiers kept in `device.json`: the token the board presents on `/device`, and the
//! computer id that is advertised over mDNS and handed to the board during USB provisioning.
use crate::store;
use anyhow::Result;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::Path;

const TOKEN_BYTES: usize = 24;
const COMPUTER_ID_BYTES: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentity {
    /// 48 hex digits.
    pub token: String,
    /// 12 hex digits.
    pub computer_id: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Stored {
    token: Option<String>,
    computer_id: Option<String>,
}

/// Loads `device.json`. Valid values are kept (the Node version's token carries over); missing or
/// malformed ones are generated. The file is rewritten only when something changed.
pub fn load_or_create(path: &Path) -> Result<DeviceIdentity> {
    let stored: Stored = store::read_json(path).unwrap_or_default();
    let identity = DeviceIdentity {
        token: keep_hex(stored.token, TOKEN_BYTES).unwrap_or_else(|| random_hex(TOKEN_BYTES)),
        computer_id: keep_hex(stored.computer_id, COMPUTER_ID_BYTES)
            .unwrap_or_else(|| random_hex(COMPUTER_ID_BYTES)),
    };
    if store::read_json::<DeviceIdentity>(path).as_ref() != Some(&identity) {
        store::write_json(path, &identity)?;
    }
    Ok(identity)
}

/// `value` when it is hex text of `bytes` bytes, otherwise `None`.
fn keep_hex(value: Option<String>, bytes: usize) -> Option<String> {
    value
        .filter(|text| text.len() == bytes * 2 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_TOKEN: &str = "3ab02225949748ffce9b99c1ab00d28f5631f1e0504fc4da";

    #[test]
    fn creates_both_ids_on_first_run_and_reuses_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device.json");
        let first = load_or_create(&path).unwrap();
        assert_eq!(first.token.len(), 48);
        assert_eq!(first.computer_id.len(), 12);
        assert_eq!(load_or_create(&path).unwrap(), first);
    }

    #[test]
    fn keeps_a_valid_token_from_older_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device.json");
        std::fs::write(&path, format!(r#"{{"token":"{OLD_TOKEN}"}}"#)).unwrap();
        let identity = load_or_create(&path).unwrap();
        assert_eq!(identity.token, OLD_TOKEN);
        assert_eq!(identity.computer_id.len(), 12);
    }

    #[test]
    fn replaces_malformed_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device.json");
        std::fs::write(&path, r#"{"token":"xyz","computerId":"ABC"}"#).unwrap();
        let identity = load_or_create(&path).unwrap();
        assert_eq!(identity.token.len(), 48);
        assert_eq!(identity.computer_id.len(), 12);
        assert!(identity.token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
