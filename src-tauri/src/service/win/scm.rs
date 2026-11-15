//! What the service manager knows about the Deskmate service, and its
//! status line, read only when the service's own process serves the pipe.

use std::fs::File;
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
    SERVICE_STATUS_PROCESS,
};

use deskmate_service::win::pipes::{server_process_id, PipeNames};
use deskmate_service::win::service::SERVICE_NAME;

/// A status line is far shorter.
const MAX_STATUS_BYTES: u64 = 4096;
/// How long to wait for a status pipe whose instance is busy.
const BUSY_WAIT: Duration = Duration::from_secs(1);
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PIPE_BUSY: i32 = 231;
/// The pipe's server learns nothing about this client.
const SECURITY_ANONYMOUS: u32 = 0;

/// A service manager handle, closed on drop.
struct Sc(SC_HANDLE);

impl Drop for Sc {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenSCManagerW or OpenServiceW and is
        // closed exactly once.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

/// The service as the service manager sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registered {
    No,
    /// Registered; the process id while it runs.
    Yes {
        process_id: Option<u32>,
    },
}

pub fn registration() -> Result<Registered, String> {
    // SAFETY: no pointers are passed; a failure comes back as an error.
    let manager = unsafe { OpenSCManagerW(None, None, SC_MANAGER_CONNECT) }
        .map_err(|e| format!("cannot reach the service manager: {e}"))?;
    let manager = Sc(manager);
    // SAFETY: the manager handle is open and the name a valid HSTRING.
    let service = match unsafe {
        OpenServiceW(
            manager.0,
            &HSTRING::from(SERVICE_NAME),
            SERVICE_QUERY_STATUS,
        )
    } {
        Ok(service) => Sc(service),
        Err(error) if error.code() == ERROR_SERVICE_DOES_NOT_EXIST.to_hresult() => {
            return Ok(Registered::No)
        }
        Err(error) => return Err(format!("cannot look at the service: {error}")),
    };
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0u32;
    // SAFETY: the buffer is one SERVICE_STATUS_PROCESS, its size is passed,
    // and both it and `needed` outlive the call.
    let buffer = unsafe {
        std::slice::from_raw_parts_mut(
            std::ptr::addr_of_mut!(status).cast::<u8>(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
        )
    };
    // SAFETY: the service handle is open with the right to query its state.
    unsafe { QueryServiceStatusEx(service.0, SC_STATUS_PROCESS_INFO, Some(buffer), &mut needed) }
        .map_err(|e| format!("cannot ask for the service's state: {e}"))?;
    let running = status.dwCurrentState == SERVICE_RUNNING && status.dwProcessId != 0;
    Ok(Registered::Yes {
        process_id: running.then_some(status.dwProcessId),
    })
}

/// Whether the service is registered. A failed query counts as not: nothing
/// that depends on this may act on a guess.
pub fn is_registered() -> bool {
    matches!(registration(), Ok(Registered::Yes { .. }))
}

fn open_status_pipe() -> std::io::Result<Option<File>> {
    let name = PipeNames::standard().status;
    let deadline = Instant::now() + BUSY_WAIT;
    loop {
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .security_qos_flags(SECURITY_ANONYMOUS)
            .open(&name);
        match opened {
            Ok(file) => return Ok(Some(file)),
            Err(error) if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => return Ok(None),
            Err(error)
                if error.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => return Ok(None),
            Err(error) => return Err(error),
        }
    }
}

/// The status line of the service running as `process_id`. Ok(None) when
/// nothing answers. Err when the pipe is served by another process, which
/// then may have taken the name.
pub fn read_status_line(process_id: u32) -> Result<Option<String>, String> {
    let Some(pipe) = open_status_pipe().map_err(|e| format!("cannot reach the service: {e}"))?
    else {
        return Ok(None);
    };
    if server_process_id(&pipe)? != process_id {
        return Err("A program other than the Deskmate service answers on its status pipe.".into());
    }
    let mut line = String::new();
    pipe.take(MAX_STATUS_BYTES)
        .read_to_string(&mut line)
        .map_err(|_| "The service's answer could not be read.".to_string())?;
    Ok(Some(line))
}
