//! Process list and stopping programs by name, for the "process running"
//! sensor and the "stop a program" control. Toolhelp snapshot: one cheap
//! system call per publish interval, no PowerShell.

/// (process id, executable name in lower case) of every running process.
#[cfg(windows)]
pub fn snapshot() -> Vec<(u32, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                out.push((entry.th32ProcessID, name));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

#[cfg(not(windows))]
pub fn snapshot() -> Vec<(u32, String)> {
    Vec::new()
}

/// "notepad", "Notepad.exe" -> "notepad.exe". None for anything that is not
/// a plain executable name (no folders: a name is matched, not a path).
pub fn normalize_name(name: &str) -> Option<String> {
    let name = name.trim().to_lowercase();
    if name.is_empty() || name.contains(['\\', '/', ':']) || name.len() > 100 {
        return None;
    }
    Some(if name.ends_with(".exe") {
        name
    } else {
        format!("{name}.exe")
    })
}

/// Whether a process with this executable name runs.
pub fn is_running(list: &[(u32, String)], name: &str) -> bool {
    normalize_name(name).is_some_and(|name| list.iter().any(|(_, exe)| *exe == name))
}

/// Stops every process with this executable name that this Windows account may
/// stop. Never Deskmate itself. Returns how many were stopped.
#[cfg(windows)]
pub fn stop_by_name(name: &str) -> Result<usize, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    let name = normalize_name(name).ok_or("not a program name")?;
    let own = std::process::id();
    let mut stopped = 0;
    for (pid, exe) in snapshot() {
        if exe != name || pid == own || pid == 0 {
            continue;
        }
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                if TerminateProcess(handle, 1).is_ok() {
                    stopped += 1;
                }
                let _ = CloseHandle(handle);
            }
        }
    }
    Ok(stopped)
}

#[cfg(not(windows))]
pub fn stop_by_name(_name: &str) -> Result<usize, String> {
    Err("windows only".into())
}

#[cfg(test)]
mod tests {
    use super::{is_running, normalize_name};

    #[test]
    fn normalizes_program_names() {
        assert_eq!(normalize_name("Notepad").as_deref(), Some("notepad.exe"));
        assert_eq!(normalize_name(" steam.EXE ").as_deref(), Some("steam.exe"));
        assert_eq!(normalize_name("C:\\Windows\\notepad.exe"), None);
        assert_eq!(normalize_name(""), None);
        let list = vec![(10, "steam.exe".to_string())];
        assert!(is_running(&list, "Steam"));
        assert!(!is_running(&list, "discord"));
    }
}
