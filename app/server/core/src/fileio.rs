//! JSON / text file helpers.
//!
//! Port of `backend_python/app/utils/file_utils.py`. Semantics match the
//! Python originals: read helpers return `None` on missing/invalid rather
//! than erroring; write helpers return `bool` success. Writes are atomic
//! (temp file + rename) — observably identical to the Python direct write,
//! but crash-safe, which matters for a tool that edits live config.

use serde_json::Value;
use std::path::Path;

/// Read a JSON file. Returns `None` if the file is missing or unparseable.
pub fn read_json_file(file_path: &Path) -> Option<Value> {
    if !file_path.exists() {
        return None;
    }
    let bytes = std::fs::read(file_path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Write `data` as pretty JSON (2-space indent), creating parent dirs.
/// Atomic: writes to a sibling temp file then renames over the target.
pub async fn write_json_file(file_path: &Path, data: &Value) -> bool {
    let serialized = match serde_json::to_string_pretty(data) {
        Ok(s) => s,
        Err(_) => return false,
    };
    write_atomic(file_path, serialized.as_bytes())
}

/// Read a text file. Returns `None` if missing or unreadable.
pub async fn read_text_file(file_path: &Path) -> Option<String> {
    if !file_path.exists() {
        return None;
    }
    std::fs::read_to_string(file_path).ok()
}

/// Write text content, creating parent dirs. Atomic (temp + rename).
pub async fn write_text_file(file_path: &Path, content: &str) -> bool {
    write_atomic(file_path, content.as_bytes())
}

/// `true` if the path exists and is a regular file.
pub fn file_exists(file_path: &Path) -> bool {
    file_path.is_file()
}

/// `true` if the path exists and is a directory.
pub fn directory_exists(dir_path: &Path) -> bool {
    dir_path.is_dir()
}

/// Shared atomic-write primitive: ensure parent, write to `<file>.tmp.<pid>`,
/// rename over the target. Returns `false` on any failure.
fn write_atomic(file_path: &Path, bytes: &[u8]) -> bool {
    if let Some(parent) = file_path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let tmp = match file_path.file_name() {
        Some(name) => {
            let mut t = name.to_os_string();
            t.push(format!(".tmp.{}", std::process::id()));
            file_path.with_file_name(t)
        }
        None => return false,
    };
    if std::fs::write(&tmp, bytes).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    if std::fs::rename(&tmp, file_path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}
