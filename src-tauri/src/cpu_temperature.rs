//! CPU temperature through PawnIO (community feedback: "no CPU temperature,
//! it can be done with PawnIO").
//!
//! Reading CPU sensors needs a kernel driver and administrator rights, and
//! Deskmate runs as the user. So the work is split:
//! - `deskmate-cpu-temperature.exe --cpu-temperature-helper`: a copy of
//!   deskmate.exe in Program Files, started elevated at logon by the scheduled
//!   task "Deskmate CPU temperature". It loads the signed PawnIO module for the
//!   CPU (IntelMSR or AMDFamily17, `pawnio/`) and answers one JSON line on
//!   \\.\pipe\deskmate-cpu-temperature per connection. It reads nothing from
//!   the pipe.
//! - the app reads that pipe (hardware.rs); nothing else runs elevated.
//! - Settings > CPU temperature installs or removes the task with one UAC
//!   prompt. The copy lives in Program Files, which a non-elevated process
//!   cannot change: an elevated task pointing at the user-writable install
//!   folder would let any program the user runs gain administrator rights.
//!
//! x64 only: ARM processors have no such registers. The helper stops serving
//! once Deskmate is uninstalled, so a leftover task does nothing.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const HELPER_ARG: &str = "--cpu-temperature-helper";
pub const INSTALL_ARG: &str = "--install-cpu-temperature-helper";
pub const REMOVE_ARG: &str = "--remove-cpu-temperature-helper";
#[cfg(windows)]
const PIPE_NAME: &str = r"\\.\pipe\deskmate-cpu-temperature";
#[cfg(all(windows, target_arch = "x86_64"))]
const TASK_NAME: &str = "Deskmate CPU temperature";
const HELPER_DIR: &str = "Deskmate CPU temperature";
const HELPER_EXE: &str = "deskmate-cpu-temperature.exe";

#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    /// x64 build: the helper exists at all.
    pub supported: bool,
    pub pawnio_installed: bool,
    pub helper_installed: bool,
    /// The helper answered.
    pub helper_running: bool,
    pub celsius: Option<f64>,
    /// What the helper reported instead of a value, e.g. "pawnio_missing".
    pub error: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Answer {
    #[serde(skip_serializing_if = "Option::is_none")]
    celsius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn program_files() -> PathBuf {
    std::env::var_os("ProgramW6432")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
}

pub fn helper_path() -> PathBuf {
    program_files().join(HELPER_DIR).join(HELPER_EXE)
}

/// PawnIOLib.dll from the PawnIO installation (Program Files, admin-only).
fn pawnio_lib() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\PawnIO")
        {
            if let Ok(dir) = key.get_value::<String, _>("InstallLocation") {
                let path = PathBuf::from(dir.trim_matches('"')).join("PawnIOLib.dll");
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    let path = program_files().join("PawnIO").join("PawnIOLib.dll");
    path.is_file().then_some(path)
}

// ---------------------------------------------------------------- client

#[cfg(windows)]
fn query_helper() -> Option<Answer> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, FILE_SHARE_NONE, OPEN_EXISTING, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT,
    };
    use windows::Win32::System::Pipes::WaitNamedPipeW;
    let name = HSTRING::from(PIPE_NAME);
    unsafe {
        let mut handle = CreateFileW(
            &name,
            GENERIC_READ.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            None,
        );
        if handle.is_err() && WaitNamedPipeW(&name, 300).as_bool() {
            handle = CreateFileW(
                &name,
                GENERIC_READ.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            );
        }
        let handle = handle.ok()?;
        let mut buffer = [0u8; 512];
        let mut read = 0u32;
        let ok = ReadFile(handle, Some(&mut buffer), Some(&mut read), None).is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return None;
        }
        let text = std::str::from_utf8(&buffer[..read as usize]).ok()?;
        serde_json::from_str(text.trim()).ok()
    }
}

#[cfg(not(windows))]
fn query_helper() -> Option<Answer> {
    None
}

fn plausible(celsius: f64) -> bool {
    (1.0..=125.0).contains(&celsius)
}

/// CPU temperature from the helper, if it runs and could read one.
pub fn read_celsius() -> Option<f64> {
    query_helper()?.celsius.filter(|c| plausible(*c))
}

pub fn status() -> Status {
    let answer = query_helper();
    Status {
        supported: cfg!(target_arch = "x86_64"),
        pawnio_installed: pawnio_lib().is_some(),
        helper_installed: helper_path().is_file(),
        helper_running: answer.is_some(),
        celsius: answer
            .as_ref()
            .and_then(|a| a.celsius)
            .filter(|c| plausible(*c)),
        error: answer.and_then(|a| a.error),
    }
}

/// Starts this exe elevated with `arg` (UAC prompt) and waits for it.
#[cfg(windows)]
fn run_elevated(arg: &str) -> Result<(), String> {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(arg);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: windows::core::PCWSTR(file.as_ptr()),
        lpParameters: windows::core::PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info)
            .map_err(|_| "administrator approval was declined or failed".to_string())?;
        if info.hProcess.is_invalid() {
            return Err("the elevated step did not start".into());
        }
        let waited = WaitForSingleObject(info.hProcess, 120_000);
        let mut code = 1u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if waited != WAIT_OBJECT_0 {
            return Err("the elevated step did not finish in time".into());
        }
        if code == 0 {
            Ok(())
        } else {
            Err(format!("the elevated step failed (code {code})"))
        }
    }
}

#[cfg(not(windows))]
fn run_elevated(_arg: &str) -> Result<(), String> {
    Err("windows only".into())
}

/// From the app: install the helper (one UAC prompt).
pub fn request_install() -> Result<(), String> {
    if !cfg!(target_arch = "x86_64") {
        return Err("CPU temperature through PawnIO needs an x64 (Intel or AMD) computer".into());
    }
    if pawnio_lib().is_none() {
        return Err("install PawnIO first (pawnio.eu), then try again".into());
    }
    run_elevated(INSTALL_ARG)
}

/// From the app: remove the helper (one UAC prompt).
pub fn request_remove() -> Result<(), String> {
    run_elevated(REMOVE_ARG)
}

// ---------------------------------------------------------------- elevated parts

#[cfg(all(windows, target_arch = "x86_64"))]
pub use elevated::{install, remove, run_helper};

#[cfg(not(all(windows, target_arch = "x86_64")))]
pub fn install() -> Result<(), String> {
    Err("x64 only".into())
}
#[cfg(not(all(windows, target_arch = "x86_64")))]
pub fn remove() -> Result<(), String> {
    Ok(())
}
#[cfg(not(all(windows, target_arch = "x86_64")))]
pub fn run_helper() {}

#[cfg(all(windows, target_arch = "x86_64"))]
mod elevated {
    use super::{helper_path, pawnio_lib, plausible, Answer, PIPE_NAME, TASK_NAME};
    use std::time::{Duration, Instant};
    use windows::core::{s, BSTR, HRESULT, HSTRING, PCSTR};
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE, WAIT_ABANDONED,
        WAIT_OBJECT_0,
    };
    use windows::Win32::Storage::FileSystem::{
        FlushFileBuffers, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_OUTBOUND,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        LOAD_LIBRARY_SEARCH_SYSTEM32,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::Win32::System::TaskScheduler::{
        ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN,
    };
    use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
    use windows::Win32::System::Variant::VARIANT;

    static INTEL_MODULE: &[u8] = include_bytes!("../pawnio/IntelMSR.bin");
    static AMD_MODULE: &[u8] = include_bytes!("../pawnio/AMDFamily17.bin");

    type OpenFn = unsafe extern "system" fn(*mut HANDLE) -> HRESULT;
    type LoadFn = unsafe extern "system" fn(HANDLE, *const u8, usize) -> HRESULT;
    type ExecuteFn = unsafe extern "system" fn(
        HANDLE,
        PCSTR,
        *const u64,
        usize,
        *mut u64,
        usize,
        *mut usize,
    ) -> HRESULT;

    enum Cpu {
        Intel,
        Amd,
    }

    struct PawnIo {
        handle: HANDLE,
        execute: ExecuteFn,
        cpu: Cpu,
        /// Intel: TjMax from IA32_TEMPERATURE_TARGET.
        tjmax: f64,
    }

    fn cpu() -> Result<Cpu, String> {
        use std::arch::x86_64::__cpuid;
        let leaf0 = __cpuid(0);
        let mut vendor = Vec::with_capacity(12);
        for reg in [leaf0.ebx, leaf0.edx, leaf0.ecx] {
            vendor.extend_from_slice(&reg.to_le_bytes());
        }
        match vendor.as_slice() {
            b"GenuineIntel" => Ok(Cpu::Intel),
            b"AuthenticAMD" => {
                let eax = __cpuid(1).eax;
                let base = (eax >> 8) & 0xF;
                let family = if base == 0xF {
                    base + ((eax >> 20) & 0xFF)
                } else {
                    base
                };
                if (0x17..=0x1A).contains(&family) {
                    Ok(Cpu::Amd)
                } else {
                    Err("unsupported_cpu".into())
                }
            }
            _ => Err("unsupported_cpu".into()),
        }
    }

    impl PawnIo {
        fn load() -> Result<Self, String> {
            let cpu = cpu()?;
            let lib = pawnio_lib().ok_or("pawnio_missing")?;
            unsafe {
                let module = LoadLibraryExW(
                    &HSTRING::from(lib.as_os_str()),
                    None,
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
                .map_err(|_| "pawnio_missing")?;
                let open: OpenFn = std::mem::transmute(
                    GetProcAddress(module, s!("pawnio_open")).ok_or("pawnio_missing")?,
                );
                let load: LoadFn = std::mem::transmute(
                    GetProcAddress(module, s!("pawnio_load")).ok_or("pawnio_missing")?,
                );
                let execute: ExecuteFn = std::mem::transmute(
                    GetProcAddress(module, s!("pawnio_execute")).ok_or("pawnio_missing")?,
                );
                let mut handle = HANDLE::default();
                if open(&mut handle).is_err() {
                    return Err("pawnio_not_running".into());
                }
                let blob = match cpu {
                    Cpu::Intel => INTEL_MODULE,
                    Cpu::Amd => AMD_MODULE,
                };
                let loaded = load(handle, blob.as_ptr(), blob.len());
                if loaded.is_err() {
                    let _ = CloseHandle(handle);
                    return Err(format!("module_rejected_{:08x}", loaded.0));
                }
                let mut pawn = Self {
                    handle,
                    execute,
                    cpu,
                    tjmax: 100.0,
                };
                if let Cpu::Intel = pawn.cpu {
                    // IA32_TEMPERATURE_TARGET bits 23:16; 0 on some parts -> 100
                    if let Ok(target) = pawn.call(s!("ioctl_read_msr"), 0x1A2) {
                        let tj = ((target >> 16) & 0xFF) as f64;
                        if tj > 0.0 {
                            pawn.tjmax = tj;
                        }
                    }
                }
                Ok(pawn)
            }
        }

        fn call(&self, name: PCSTR, input: u64) -> Result<u64, String> {
            let mut out = 0u64;
            let mut returned = 0usize;
            let hr =
                unsafe { (self.execute)(self.handle, name, &input, 1, &mut out, 1, &mut returned) };
            if hr.is_ok() && returned >= 1 {
                Ok(out)
            } else {
                Err(format!("read_failed_{:08x}", hr.0))
            }
        }

        fn read(&self) -> Result<f64, String> {
            let celsius = match self.cpu {
                Cpu::Intel => {
                    // IA32_PACKAGE_THERM_STATUS bits 22:16: degrees below TjMax.
                    // Without package support, core 0's IA32_THERM_STATUS.
                    match self.call(s!("ioctl_read_msr"), 0x1B1) {
                        Ok(status) => self.tjmax - ((status >> 16) & 0x7F) as f64,
                        Err(_) => {
                            let status = self.call(s!("ioctl_read_msr"), 0x19C)?;
                            if status & (1 << 31) == 0 {
                                return Err("read_failed".into());
                            }
                            self.tjmax - ((status >> 16) & 0x7F) as f64
                        }
                    }
                }
                Cpu::Amd => {
                    // THM_TCON_CUR_TMP: bits 31:21 in 1/8 degree, bit 19
                    // selects the -49 range. The PCI mutex keeps other tools
                    // (HWiNFO, LibreHardwareMonitor) off the SMN index register.
                    let value = with_pci_mutex(|| self.call(s!("ioctl_read_smn"), 0x0005_9800))?;
                    let mut t = ((value >> 21) & 0x7FF) as f64 / 8.0;
                    if value & (1 << 19) != 0 {
                        t -= 49.0;
                    }
                    t
                }
            };
            if plausible(celsius) {
                Ok((celsius * 10.0).round() / 10.0)
            } else {
                Err("read_failed".into())
            }
        }
    }

    fn with_pci_mutex<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        unsafe {
            let mutex = CreateMutexW(None, false, &HSTRING::from("Global\\Access_PCI"))
                .map_err(|_| "read_failed".to_string())?;
            let wait = WaitForSingleObject(mutex, 200);
            let owned = wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED;
            let result = if owned { f() } else { Err("busy".into()) };
            if owned {
                let _ = ReleaseMutex(mutex);
            }
            let _ = CloseHandle(mutex);
            result
        }
    }

    /// Deskmate still installed for this user? Otherwise a leftover task idles.
    fn deskmate_installed() -> bool {
        std::env::var_os("LOCALAPPDATA")
            .map(|base| {
                std::path::PathBuf::from(base)
                    .join("Deskmate")
                    .join("deskmate.exe")
            })
            .is_some_and(|exe| exe.is_file())
    }

    /// The elevated loop. One pipe instance, reused; one JSON line per client.
    pub fn run_helper() {
        if !deskmate_installed() {
            return;
        }
        let pipe = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(PIPE_NAME),
                PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                512,
                0,
                0,
                None,
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            return; // another helper (another Windows account) already serves
        }
        let mut pawn: Option<PawnIo> = None;
        let mut load_error = String::new();
        let mut last_attempt: Option<Instant> = None;
        loop {
            let connected = unsafe { ConnectNamedPipe(pipe, None) };
            if let Err(e) = connected {
                if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
            }
            if pawn.is_none() && last_attempt.is_none_or(|t| t.elapsed() > Duration::from_secs(60))
            {
                last_attempt = Some(Instant::now());
                match PawnIo::load() {
                    Ok(p) => pawn = Some(p),
                    Err(e) => load_error = e,
                }
            }
            let answer = match &pawn {
                Some(p) => match p.read() {
                    Ok(celsius) => Answer {
                        celsius: Some(celsius),
                        error: None,
                    },
                    Err(e) => Answer {
                        celsius: None,
                        error: Some(e),
                    },
                },
                None => Answer {
                    celsius: None,
                    error: Some(load_error.clone()),
                },
            };
            let line = serde_json::to_string(&answer).unwrap_or_default() + "\n";
            unsafe {
                let _ = WriteFile(pipe, Some(line.as_bytes()), None, None);
                let _ = FlushFileBuffers(pipe);
                let _ = DisconnectNamedPipe(pipe);
            }
        }
    }

    fn xml_escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    fn task_folder() -> Result<windows::Win32::System::TaskScheduler::ITaskFolder, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| e.to_string())?;
            let empty = VARIANT::default();
            service
                .Connect(&empty, &empty, &empty, &empty)
                .map_err(|e| e.to_string())?;
            service
                .GetFolder(&BSTR::from("\\"))
                .map_err(|e| e.to_string())
        }
    }

    fn stop_task() {
        if let Ok(folder) = task_folder() {
            unsafe {
                if let Ok(task) = folder.GetTask(&BSTR::from(TASK_NAME)) {
                    let _ = task.Stop(0);
                }
            }
        }
    }

    /// Elevated: copy the helper to Program Files, register the logon task,
    /// start it now.
    pub fn install() -> Result<(), String> {
        let source = std::env::current_exe().map_err(|e| e.to_string())?;
        let target = helper_path();
        let dir = target.parent().ok_or("bad helper path")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        stop_task();
        let mut copied = std::fs::copy(&source, &target);
        for _ in 0..10 {
            if copied.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
            copied = std::fs::copy(&source, &target);
        }
        copied.map_err(|e| format!("cannot copy the helper: {e}"))?;

        let user = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").unwrap_or_default(),
            std::env::var("USERNAME").unwrap_or_default()
        );
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Reads the CPU temperature for Deskmate through PawnIO.</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <StartWhenAvailable>true</StartWhenAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>{arg}</Arguments></Exec></Actions>
</Task>"#,
            user = xml_escape(&user),
            exe = xml_escape(&target.to_string_lossy()),
            arg = super::HELPER_ARG,
        );
        let folder = task_folder()?;
        unsafe {
            let empty = VARIANT::default();
            let task = folder
                .RegisterTask(
                    &BSTR::from(TASK_NAME),
                    &BSTR::from(xml),
                    TASK_CREATE_OR_UPDATE.0,
                    &empty,
                    &empty,
                    TASK_LOGON_INTERACTIVE_TOKEN,
                    &empty,
                )
                .map_err(|e| format!("cannot register the task: {e}"))?;
            let _ = task.Run(&empty);
        }
        Ok(())
    }

    /// Elevated: stop and delete the task, delete the copy.
    pub fn remove() -> Result<(), String> {
        stop_task();
        if let Ok(folder) = task_folder() {
            unsafe {
                let _ = folder.DeleteTask(&BSTR::from(TASK_NAME), 0);
            }
        }
        let target = helper_path();
        for _ in 0..10 {
            if !target.exists() || std::fs::remove_file(&target).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        if let Some(dir) = target.parent() {
            let _ = std::fs::remove_dir(dir);
        }
        if target.exists() {
            Err("the helper is still running; sign out and remove it again".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{helper_path, plausible};

    #[test]
    fn helper_lives_in_program_files() {
        let path = helper_path();
        assert!(path.ends_with("Deskmate CPU temperature\\deskmate-cpu-temperature.exe"));
        assert!(plausible(45.5));
        assert!(!plausible(0.0));
        assert!(!plausible(180.0));
    }
}
