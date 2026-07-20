//! Deskmate Link Files v1: encrypted, allowlisted and strictly read-only.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde_json::{json, Map, Value};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

const MAX_CHUNK_BYTES: u64 = 256 * 1024;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const RATE_BYTES_PER_SEC: f64 = 4.0 * 1024.0 * 1024.0;
const MAX_LIST_ENTRIES: usize = 4096;

pub fn normalize_roots(roots: &[String]) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for raw in roots {
        let raw = raw.trim();
        if raw.is_empty() { continue; }
        let path = validate_windows_path(raw).map_err(|error| format!("invalid Link Files root '{raw}': {error}"))?;
        reject_reparse_components(&path).map_err(|error| format!("invalid Link Files root '{raw}': {error}"))?;
        let canonical = fs::canonicalize(&path).map_err(|_| format!("Link Files root does not exist: {raw}"))?;
        if !canonical.is_dir() {
            return Err(format!("Link Files root is not a directory: {raw}"));
        }
        let display = friendly_canonical_path(&canonical);
        if !normalized.iter().any(|existing: &String| existing.eq_ignore_ascii_case(&display)) {
            normalized.push(display);
        }
    }
    Ok(normalized)
}

pub fn handle_request(cfg: &crate::config::AppConfig, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let op = request.get("op").and_then(Value::as_str).unwrap_or("");
    let raw_path = request.get("path").and_then(Value::as_str).unwrap_or("");
    let result = execute(cfg, request, op, raw_path);
    match result {
        Ok(fields) => {
            crate::security::audit_file(op, raw_path, "ok");
            let mut response = Map::from_iter([
                ("t".into(), json!("fs_res")),
                ("id".into(), id),
                ("ok".into(), json!(true)),
            ]);
            response.extend(fields);
            Value::Object(response)
        }
        Err(error) => {
            crate::security::audit_file(op, raw_path, &format!("denied: {error}"));
            json!({"t": "fs_res", "id": id, "ok": false, "error": error})
        }
    }
}

fn execute(
    cfg: &crate::config::AppConfig,
    request: &Value,
    op: &str,
    raw_path: &str,
) -> Result<Map<String, Value>, String> {
    if cfg.link_file_roots.is_empty() {
        return Err("file access disabled: no allowed roots".into());
    }
    if !matches!(op, "list" | "stat" | "read") {
        return Err("unsupported read-only file operation".into());
    }
    let path = authorized_path(raw_path, &cfg.link_file_roots)?;
    match op {
        "list" => list(&path),
        "stat" => stat(&path),
        "read" => read(&path, request),
        _ => unreachable!(),
    }
}

fn list(path: &Path) -> Result<Map<String, Value>, String> {
    if !path.is_dir() { return Err("path is not a directory".into()); }
    let mut entries = Vec::new();
    for entry in fs::read_dir(path).map_err(|_| "cannot list directory")? {
        let entry = entry.map_err(|_| "cannot read directory entry")?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| "cannot stat directory entry")?;
        if is_reparse(&metadata) { continue; }
        entries.push(metadata_json(&entry.file_name().to_string_lossy(), &metadata));
        if entries.len() > MAX_LIST_ENTRIES {
            return Err("directory contains too many entries".into());
        }
    }
    entries.sort_by(|left, right| {
        left["name"].as_str().unwrap_or("").to_ascii_lowercase()
            .cmp(&right["name"].as_str().unwrap_or("").to_ascii_lowercase())
    });
    Ok(Map::from_iter([("entries".into(), Value::Array(entries))]))
}

fn stat(path: &Path) -> Result<Map<String, Value>, String> {
    let metadata = fs::metadata(path).map_err(|_| "cannot stat path")?;
    let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
    Ok(Map::from_iter([("stat".into(), metadata_json(&name, &metadata))]))
}

fn read(path: &Path, request: &Value) -> Result<Map<String, Value>, String> {
    let metadata = fs::metadata(path).map_err(|_| "cannot stat file")?;
    if !metadata.is_file() { return Err("path is not a file".into()); }
    if metadata.len() > MAX_FILE_BYTES { return Err("file exceeds 16 MiB limit".into()); }
    let offset = request.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let len = request.get("len").and_then(Value::as_u64).unwrap_or(MAX_CHUNK_BYTES);
    if len > MAX_CHUNK_BYTES { return Err("read chunk exceeds 256 KiB limit".into()); }
    if offset > metadata.len() { return Err("read offset is past end of file".into()); }

    let mut file = File::open(path).map_err(|_| "cannot open file")?;
    file.seek(SeekFrom::Start(offset)).map_err(|_| "cannot seek file")?;
    let mut data = Vec::with_capacity(len as usize);
    file.take(len).read_to_end(&mut data).map_err(|_| "cannot read file")?;
    throttle(data.len());
    let eof = offset.saturating_add(data.len() as u64) >= metadata.len();
    Ok(Map::from_iter([
        ("data".into(), Value::String(B64.encode(data))),
        ("eof".into(), Value::Bool(eof)),
    ]))
}

fn metadata_json(name: &str, metadata: &fs::Metadata) -> Value {
    let mtime = metadata.modified().ok().and_then(|value| value.duration_since(UNIX_EPOCH).ok()).map(|value| value.as_secs()).unwrap_or(0);
    json!({
        "name": name,
        "dir": metadata.is_dir(),
        "size": if metadata.is_file() { metadata.len() } else { 0 },
        "mtime": mtime,
    })
}

fn authorized_path(raw: &str, roots: &[String]) -> Result<PathBuf, String> {
    let requested = validate_windows_path(raw)?;
    reject_reparse_components(&requested)?;
    let canonical = fs::canonicalize(&requested).map_err(|_| "path does not exist")?;
    for raw_root in roots {
        let root_path = validate_windows_path(raw_root).map_err(|_| "configured root is invalid")?;
        reject_reparse_components(&root_path).map_err(|_| "configured root contains a reparse point")?;
        let root = fs::canonicalize(root_path).map_err(|_| "configured root is unavailable")?;
        if root.is_dir() && path_is_within(&root, &canonical) {
            return Ok(canonical);
        }
    }
    Err("path is outside allowed roots".into())
}

fn validate_windows_path(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() || raw.len() > 32_767 || raw.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err("path is empty or malformed".into());
    }
    let bytes = raw.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
    {
        return Err("path must be an absolute local drive path".into());
    }
    if raw[2..].contains(':') { return Err("alternate data streams are not allowed".into()); }
    if raw.split(['\\', '/']).any(|component| matches!(component, "." | "..")) {
        return Err("relative path components are not allowed".into());
    }
    Ok(PathBuf::from(raw))
}

fn path_is_within(root: &Path, target: &Path) -> bool {
    let mut target_components = target.components();
    root.components().all(|root_component| {
        target_components.next().map(|target_component| component_eq(root_component, target_component)).unwrap_or(false)
    })
}

fn component_eq(left: Component<'_>, right: Component<'_>) -> bool {
    left.as_os_str().to_string_lossy().eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

fn reject_reparse_components(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if !current.is_absolute() { continue; }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_reparse(&metadata) => return Err("symbolic links and reparse points are not allowed".into()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("cannot validate path metadata".into()),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse(metadata: &fs::Metadata) -> bool { metadata.file_type().is_symlink() }

fn friendly_canonical_path(path: &Path) -> String {
    let display = path.to_string_lossy();
    let display = display.strip_prefix(r"\\?\").unwrap_or(&display);
    if display.len() == 3 && display.as_bytes()[1] == b':' && matches!(display.as_bytes()[2], b'\\' | b'/') {
        display.to_string()
    } else {
        display.trim_end_matches(['\\', '/']).to_string()
    }
}

struct RateGate { next: Instant }
static RATE_GATE: OnceLock<Mutex<RateGate>> = OnceLock::new();

fn throttle(bytes: usize) {
    if bytes == 0 { return; }
    let duration = Duration::from_secs_f64(bytes as f64 / RATE_BYTES_PER_SEC);
    let gate = RATE_GATE.get_or_init(|| Mutex::new(RateGate { next: Instant::now() }));
    let Ok(mut gate) = gate.lock() else { return };
    let now = Instant::now();
    if gate.next > now { std::thread::sleep(gate.next - now); }
    gate.next = std::cmp::max(gate.next, Instant::now()) + duration;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_absolute_local_drive_path() {
        assert!(validate_windows_path(r"C:\Users\Kuba\file.txt").is_ok());
        assert!(validate_windows_path(r"d:/Data/report.pdf").is_ok());
    }

    #[test]
    fn rejects_parent_and_current_components() {
        assert!(validate_windows_path(r"C:\allowed\..\secret.txt").is_err());
        assert!(validate_windows_path(r"C:\allowed\.\file.txt").is_err());
    }

    #[test]
    fn rejects_unc_and_device_paths() {
        assert!(validate_windows_path(r"\\server\share\file.txt").is_err());
        assert!(validate_windows_path(r"\\?\C:\allowed\file.txt").is_err());
        assert!(validate_windows_path(r"\\.\PhysicalDrive0").is_err());
    }

    #[test]
    fn rejects_alternate_data_streams_and_drive_relative_paths() {
        assert!(validate_windows_path(r"C:\allowed\file.txt:secret").is_err());
        assert!(validate_windows_path(r"C:allowed\file.txt").is_err());
    }

    #[test]
    fn containment_is_case_insensitive_and_component_aware() {
        assert!(path_is_within(Path::new(r"C:\Allowed"), Path::new(r"c:\allowed\child\file.txt")));
        assert!(!path_is_within(Path::new(r"C:\Allowed"), Path::new(r"C:\AllowedElsewhere\file.txt")));
    }

    #[test]
    fn preserves_drive_root_when_formatting_canonical_path() {
        assert_eq!(friendly_canonical_path(Path::new(r"\\?\C:\")), r"C:\");
        assert_eq!(friendly_canonical_path(Path::new(r"\\?\C:\Data\")), r"C:\Data");
    }

    #[test]
    fn file_allowlist_is_empty_by_default() {
        assert!(crate::config::AppConfig::default().link_file_roots.is_empty());
    }
}
