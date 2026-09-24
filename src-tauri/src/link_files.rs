//! Deskmate Link Files: encrypted and allowlisted.
//!
//! Two separate capabilities with separate settings:
//! - read: list, stat and read inside the folders in `link_file_roots`;
//! - inbox: Home Assistant can write new files into one folder, and only there.
//!   It cannot overwrite, rename, delete or read anything through it.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

const MAX_CHUNK_BYTES: u64 = 256 * 1024;
const RATE_BYTES_PER_SEC: f64 = 4.0 * 1024.0 * 1024.0;
const MAX_LIST_ENTRIES: usize = 4096;
const MIB: u64 = 1024 * 1024;
/// An upload that has not received a chunk for this long is abandoned.
const UPLOAD_IDLE: Duration = Duration::from_secs(120);
const MAX_PARALLEL_UPLOADS: usize = 4;
const PART_PREFIX: &str = ".deskmate-";

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

/// Validates the configured inbox folder. Empty stays empty (the default
/// folder is resolved when a file arrives). A configured folder is created if
/// needed and must not contain symbolic links or junctions.
pub fn normalize_inbox_dir(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    let path = validate_windows_path(raw).map_err(|error| format!("invalid inbox folder: {error}"))?;
    fs::create_dir_all(&path).map_err(|_| format!("cannot create inbox folder: {raw}"))?;
    reject_reparse_components(&path).map_err(|error| format!("invalid inbox folder: {error}"))?;
    let canonical = fs::canonicalize(&path).map_err(|_| format!("inbox folder is unavailable: {raw}"))?;
    Ok(friendly_canonical_path(&canonical))
}

pub fn max_file_bytes(cfg: &crate::config::AppConfig) -> u64 {
    cfg.link_files_max_mb.clamp(1, 4096) * MIB
}

pub fn handle_request(cfg: &crate::config::AppConfig, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let op = request.get("op").and_then(Value::as_str).unwrap_or("");
    let raw_path = request
        .get("path")
        .or_else(|| request.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let result = execute(cfg, request, op);
    // Chunks are not logged one by one; begin, end and every failure are.
    let log = op != "put_chunk" || result.is_err();
    match result {
        Ok(fields) => {
            if log {
                let subject = fields.get("name").and_then(Value::as_str).unwrap_or(raw_path);
                crate::security::audit_file(op, subject, "ok");
            }
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
) -> Result<Map<String, Value>, String> {
    match op {
        "roots" => roots(cfg),
        "list" | "stat" | "read" => {
            if cfg.link_file_roots.is_empty() {
                return Err("file access disabled: no allowed roots".into());
            }
            let raw_path = request.get("path").and_then(Value::as_str).unwrap_or("");
            let path = authorized_path(raw_path, &cfg.link_file_roots)?;
            match op {
                "list" => list(&path),
                "stat" => stat(&path),
                _ => read(cfg, &path, request),
            }
        }
        "put_begin" => put_begin(cfg, request),
        "put_chunk" => put_chunk(request),
        "put_end" => put_end(request),
        "put_abort" => put_abort(request),
        _ => Err("unsupported file operation".into()),
    }
}

/// What Home Assistant may do here: the read-only folders and whether the
/// inbox accepts files. Answered even when both are off, so the Home Assistant
/// panel can explain why nothing is available.
fn roots(cfg: &crate::config::AppConfig) -> Result<Map<String, Value>, String> {
    let inbox_dir = if cfg.link_inbox_mode == "off" {
        String::new()
    } else {
        inbox_dir(cfg).map(|dir| friendly_canonical_path(&dir)).unwrap_or_default()
    };
    Ok(Map::from_iter([
        ("roots".into(), json!(cfg.link_file_roots)),
        (
            "inbox".into(),
            json!({
                "mode": cfg.link_inbox_mode,
                "dir": inbox_dir,
                "max_bytes": max_file_bytes(cfg),
            }),
        ),
        ("max_bytes".into(), json!(max_file_bytes(cfg))),
    ]))
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

fn read(cfg: &crate::config::AppConfig, path: &Path, request: &Value) -> Result<Map<String, Value>, String> {
    let metadata = fs::metadata(path).map_err(|_| "cannot stat file")?;
    if !metadata.is_file() { return Err("path is not a file".into()); }
    let max = max_file_bytes(cfg);
    if metadata.len() > max {
        return Err(format!("file exceeds the {} MiB limit set on the computer", max / MIB));
    }
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

// ---------- inbox (files sent from Home Assistant) ----------

struct Upload {
    dir: PathBuf,
    part: PathBuf,
    name: String,
    size: u64,
    written: u64,
    hasher: Sha256,
    file: File,
    last: Instant,
}

static UPLOADS: OnceLock<Mutex<HashMap<String, Upload>>> = OnceLock::new();

fn uploads() -> &'static Mutex<HashMap<String, Upload>> {
    UPLOADS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn discard(upload: Upload) {
    drop(upload.file);
    let _ = fs::remove_file(&upload.part);
}

/// Drops uploads that stopped sending chunks, together with their .part files.
fn sweep_stale(map: &mut HashMap<String, Upload>) {
    let stale: Vec<String> = map
        .iter()
        .filter(|(_, upload)| upload.last.elapsed() > UPLOAD_IDLE)
        .map(|(token, _)| token.clone())
        .collect();
    for token in stale {
        if let Some(upload) = map.remove(&token) {
            crate::security::audit_file("put_abort", &upload.name, "abandoned");
            discard(upload);
        }
    }
}

/// Called when the Link session ends: nothing half-written stays behind.
pub fn abort_all() {
    if let Ok(mut map) = uploads().lock() {
        for (_, upload) in map.drain() {
            crate::security::audit_file("put_abort", &upload.name, "session ended");
            discard(upload);
        }
    }
}

fn inbox_dir(cfg: &crate::config::AppConfig) -> Result<PathBuf, String> {
    let configured = cfg.link_inbox_dir.trim();
    let path = if configured.is_empty() {
        let profile = std::env::var("USERPROFILE").map_err(|_| "cannot resolve the Downloads folder")?;
        Path::new(&profile).join("Downloads").join("Deskmate")
    } else {
        validate_windows_path(configured)?
    };
    fs::create_dir_all(&path).map_err(|_| "cannot create the inbox folder")?;
    reject_reparse_components(&path)?;
    fs::canonicalize(&path).map_err(|_| "inbox folder is unavailable".into())
}

fn human_size(bytes: u64) -> String {
    if bytes >= MIB {
        format!("{:.1} MB", bytes as f64 / MIB as f64)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn put_begin(cfg: &crate::config::AppConfig, request: &Value) -> Result<Map<String, Value>, String> {
    let mode = cfg.link_inbox_mode.as_str();
    if !matches!(mode, "confirm" | "automatic") {
        return Err("receiving files is turned off on this computer".into());
    }
    let raw_name = request.get("name").and_then(Value::as_str).unwrap_or("");
    let size = request
        .get("size")
        .and_then(Value::as_u64)
        .ok_or("missing file size")?;
    let max = max_file_bytes(cfg);
    if size > max {
        return Err(format!("file is larger than the {} MiB limit set on the computer", max / MIB));
    }
    let name = sanitize_file_name(raw_name);
    let dir = inbox_dir(cfg)?;
    {
        let mut map = uploads().lock().map_err(|_| "upload state unavailable")?;
        sweep_stale(&mut map);
        if map.len() >= MAX_PARALLEL_UPLOADS {
            return Err("too many transfers in progress".into());
        }
    }
    if mode == "confirm" {
        if crate::sensors::session_locked() {
            return Err("the computer is locked, nobody can accept the file".into());
        }
        let by = crate::security::safe_preview(
            request.get("by").and_then(Value::as_str).unwrap_or("Home Assistant"),
            60,
        );
        let approved = crate::security::confirm(
            "Deskmate - incoming file",
            &format!(
                "{by} wants to save a file on this computer:\n\n{}\n{}\n\nFolder: {}\n\nAccept?",
                crate::security::safe_preview(&name, 150),
                human_size(size),
                friendly_canonical_path(&dir),
            ),
        );
        if !approved {
            return Err("declined on the computer".into());
        }
    }
    let token = random_token();
    let part = dir.join(format!("{PART_PREFIX}{token}.part"));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .map_err(|_| "cannot create the file in the inbox folder")?;
    let upload = Upload {
        dir,
        part,
        name: name.clone(),
        size,
        written: 0,
        hasher: Sha256::new(),
        file,
        last: Instant::now(),
    };
    uploads()
        .lock()
        .map_err(|_| "upload state unavailable")?
        .insert(token.clone(), upload);
    Ok(Map::from_iter([
        ("upload".into(), json!(token)),
        ("name".into(), json!(name)),
    ]))
}

fn upload_token(request: &Value) -> Result<String, String> {
    request
        .get("upload")
        .and_then(Value::as_str)
        .filter(|token| token.len() == 32 && token.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_string)
        .ok_or_else(|| "missing or invalid upload id".to_string())
}

fn put_chunk(request: &Value) -> Result<Map<String, Value>, String> {
    let token = upload_token(request)?;
    let offset = request.get("offset").and_then(Value::as_u64).ok_or("missing offset")?;
    let data = B64
        .decode(request.get("data").and_then(Value::as_str).ok_or("missing data")?)
        .map_err(|_| "invalid chunk data")?;
    if data.len() as u64 > MAX_CHUNK_BYTES {
        return Err("chunk exceeds 256 KiB limit".into());
    }
    let mut map = uploads().lock().map_err(|_| "upload state unavailable")?;
    let upload = map.get_mut(&token).ok_or("unknown or expired upload")?;
    let result = (|| {
        if offset != upload.written {
            return Err("unexpected chunk offset".to_string());
        }
        if upload.written + data.len() as u64 > upload.size {
            return Err("more data than announced".to_string());
        }
        upload.file.write_all(&data).map_err(|_| "cannot write to disk".to_string())?;
        upload.hasher.update(&data);
        upload.written += data.len() as u64;
        upload.last = Instant::now();
        Ok(upload.written)
    })();
    match result {
        Ok(written) => Ok(Map::from_iter([("written".into(), json!(written))])),
        Err(error) => {
            if let Some(upload) = map.remove(&token) {
                discard(upload);
            }
            Err(error)
        }
    }
}

fn put_end(request: &Value) -> Result<Map<String, Value>, String> {
    let token = upload_token(request)?;
    let upload = uploads()
        .lock()
        .map_err(|_| "upload state unavailable")?
        .remove(&token)
        .ok_or("unknown or expired upload")?;
    if upload.written != upload.size {
        let name = upload.name.clone();
        discard(upload);
        return Err(format!("{name}: transfer incomplete"));
    }
    let Upload { dir, part, name, size, hasher, file, .. } = upload;
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if let Some(expected) = request.get("sha256").and_then(Value::as_str) {
        if !expected.eq_ignore_ascii_case(&digest) {
            drop(file);
            let _ = fs::remove_file(&part);
            return Err("checksum mismatch, file discarded".into());
        }
    }
    file.sync_all().map_err(|_| "cannot flush file to disk")?;
    drop(file);
    let target = match unique_path(&dir, &name) {
        Ok(target) => target,
        Err(error) => {
            let _ = fs::remove_file(&part);
            return Err(error);
        }
    };
    if fs::rename(&part, &target).is_err() {
        let _ = fs::remove_file(&part);
        return Err("cannot move the file into place".into());
    }
    mark_of_the_web(&target);
    let final_name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or(name);
    Ok(Map::from_iter([
        ("name".into(), json!(final_name)),
        ("dir".into(), json!(friendly_canonical_path(&dir))),
        ("size".into(), json!(size)),
        ("sha256".into(), json!(digest)),
    ]))
}

fn put_abort(request: &Value) -> Result<Map<String, Value>, String> {
    let token = upload_token(request)?;
    if let Some(upload) = uploads()
        .lock()
        .map_err(|_| "upload state unavailable")?
        .remove(&token)
    {
        discard(upload);
    }
    Ok(Map::new())
}

fn random_token() -> String {
    use rand::RngCore;
    let mut raw = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

/// A file name that is safe to create on Windows: no path parts, no reserved
/// characters or device names, no trailing dots, not too long.
pub fn sanitize_file_name(raw: &str) -> String {
    let base = raw.rsplit(['\\', '/']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    let mut name = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if name.is_empty() || name.chars().all(|c| c == '.') {
        name = "file".into();
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved || name.starts_with(PART_PREFIX) {
        name = format!("_{name}");
    }
    if name.chars().count() > 150 {
        let (stem, ext) = split_extension(&name);
        let keep = 150usize.saturating_sub(ext.chars().count());
        name = format!("{}{}", stem.chars().take(keep).collect::<String>(), ext);
    }
    name
}

/// ("report", ".pdf") for "report.pdf"; extensions longer than 16 characters
/// are treated as part of the name.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(index) if index > 0 && name.len() - index <= 16 => name.split_at(index),
        _ => (name, ""),
    }
}

/// Never overwrites: "report.pdf" becomes "report (1).pdf" and so on.
fn unique_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let first = dir.join(name);
    if !first.exists() {
        return Ok(first);
    }
    let (stem, ext) = split_extension(name);
    for n in 1..1000 {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("too many files with this name in the inbox folder".into())
}

/// Marks the file as coming from another computer, the same way a browser
/// marks downloads, so SmartScreen and Office Protected View treat it as such.
fn mark_of_the_web(path: &Path) {
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    let _ = fs::write(PathBuf::from(stream), "[ZoneTransfer]\r\nZoneId=3\r\n");
}

// ---------- shared path checks ----------

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
    fn file_access_and_inbox_are_off_by_default() {
        let cfg = crate::config::AppConfig::default();
        assert!(cfg.link_file_roots.is_empty());
        assert_eq!(cfg.link_inbox_mode, "off");
    }

    #[test]
    fn sanitizes_incoming_file_names() {
        assert_eq!(sanitize_file_name(r"..\..\Windows\System32\evil.dll"), "evil.dll");
        assert_eq!(sanitize_file_name("/etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("report.pdf:hidden"), "report.pdf_hidden");
        assert_eq!(sanitize_file_name("a<b>c|d?.txt"), "a_b_c_d_.txt");
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_name("com1"), "_com1");
        assert_eq!(sanitize_file_name("name. . "), "name");
        assert_eq!(sanitize_file_name(".."), "file");
        assert_eq!(sanitize_file_name(""), "file");
        assert_eq!(sanitize_file_name(".deskmate-x.part"), "_.deskmate-x.part");
        let long = format!("{}.jpg", "x".repeat(300));
        let cut = sanitize_file_name(&long);
        assert_eq!(cut.chars().count(), 150);
        assert!(cut.ends_with(".jpg"));
    }

    fn test_config(dir: &Path, mode: &str) -> crate::config::AppConfig {
        crate::config::AppConfig {
            link_inbox_mode: mode.into(),
            link_inbox_dir: dir.to_string_lossy().to_string(),
            link_files_max_mb: 1,
            ..Default::default()
        }
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deskmate-test-{tag}-{}", random_token()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn inbox_receives_file_without_overwriting() {
        let dir = scratch_dir("inbox");
        fs::write(dir.join("photo.jpg"), b"old").unwrap();
        let cfg = test_config(&dir, "automatic");
        let body = b"hello from the phone";
        let begin = handle_request(&cfg, &json!({"t": "fs", "id": 1, "op": "put_begin", "name": "..\\photo.jpg", "size": body.len()}));
        assert_eq!(begin["ok"], true, "{begin}");
        let token = begin["upload"].as_str().unwrap().to_string();
        let chunk = handle_request(&cfg, &json!({"t": "fs", "id": 2, "op": "put_chunk", "upload": token, "offset": 0, "data": B64.encode(body)}));
        assert_eq!(chunk["ok"], true, "{chunk}");
        let digest: String = Sha256::digest(body).iter().map(|b| format!("{b:02x}")).collect();
        let end = handle_request(&cfg, &json!({"t": "fs", "id": 3, "op": "put_end", "upload": token, "sha256": digest}));
        assert_eq!(end["ok"], true, "{end}");
        assert_eq!(end["name"], "photo (1).jpg");
        assert_eq!(fs::read(dir.join("photo (1).jpg")).unwrap(), body);
        assert_eq!(fs::read(dir.join("photo.jpg")).unwrap(), b"old");
        let leftovers = fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".part")).count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_rejects_bad_transfers() {
        let dir = scratch_dir("reject");
        let off = test_config(&dir, "off");
        assert_eq!(handle_request(&off, &json!({"op": "put_begin", "name": "a.txt", "size": 3}))["ok"], false);

        let cfg = test_config(&dir, "automatic");
        let too_big = handle_request(&cfg, &json!({"op": "put_begin", "name": "a.txt", "size": 2 * MIB}));
        assert_eq!(too_big["ok"], false);

        let begin = handle_request(&cfg, &json!({"op": "put_begin", "name": "a.txt", "size": 3}));
        let token = begin["upload"].as_str().unwrap().to_string();
        let wrong_offset = handle_request(&cfg, &json!({"op": "put_chunk", "upload": token, "offset": 1, "data": B64.encode(b"abc")}));
        assert_eq!(wrong_offset["ok"], false);
        // the failed upload is gone, including its .part file
        assert_eq!(handle_request(&cfg, &json!({"op": "put_end", "upload": token}))["ok"], false);

        let begin = handle_request(&cfg, &json!({"op": "put_begin", "name": "b.txt", "size": 3}));
        let token = begin["upload"].as_str().unwrap().to_string();
        handle_request(&cfg, &json!({"op": "put_chunk", "upload": token, "offset": 0, "data": B64.encode(b"abc")}));
        let bad_hash = handle_request(&cfg, &json!({"op": "put_end", "upload": token, "sha256": "00"}));
        assert_eq!(bad_hash["ok"], false);
        assert!(!dir.join("b.txt").exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_ops_stay_disabled_without_roots() {
        let cfg = crate::config::AppConfig::default();
        assert_eq!(handle_request(&cfg, &json!({"op": "list", "path": r"C:\"}))["ok"], false);
        assert_eq!(handle_request(&cfg, &json!({"op": "roots"}))["ok"], true);
    }
}
