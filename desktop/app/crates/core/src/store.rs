//! Small JSON files: missing or unreadable files read as `None`, and writes go
//! through a temporary file so a reader never sees a half-written document.
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_string_pretty(value)?;
    write_atomic(path, text.as_bytes())
}

/// Writes `<path>.tmp` and renames it over `path`. The rename replaces an existing file on
/// Windows as well.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建目录失败：{}", parent.display()))?;
    }
    let tmp = temp_path(path);
    fs::write(&tmp, bytes).with_context(|| format!("写入失败：{}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("保存失败：{}", path.display()))?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_replaces_existing_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("value.json");
        write_json(&path, &vec!["a", "b"]).unwrap();
        write_json(&path, &vec!["c"]).unwrap();
        assert_eq!(read_json::<Vec<String>>(&path), Some(vec!["c".to_string()]));
        assert!(!temp_path(&path).exists());
    }

    #[test]
    fn missing_or_corrupt_files_read_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        assert_eq!(read_json::<Vec<String>>(&path), None);
        fs::write(&path, "{not json").unwrap();
        assert_eq!(read_json::<Vec<String>>(&path), None);
    }
}
