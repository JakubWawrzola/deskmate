//! The two named pipes of the service, and the client side of them.
//!
//! `deskmate-service-admin` takes the frames of `crate::admin` from SYSTEM
//! and administrators. `deskmate-service-status` gives one status line to
//! any local user and never reads. Both refuse remote clients, and both are
//! created as the first instance of their name: if another program already
//! owns the name, the service does not start.

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
use tokio::sync::watch;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use zeroize::Zeroizing;

use super::security::{Descriptor, ADMIN_PIPE_SDDL, STATUS_PIPE_SDDL};
use crate::admin::{self, Refusal, MAX_FRAME_BYTES};
use crate::provisioning::AdminActions;
use crate::status::Status;

const ADMIN_PIPE: &str = "deskmate-service-admin";
const STATUS_PIPE: &str = "deskmate-service-status";
/// A status line is far shorter; a write that fits the buffer never waits
/// for the reader.
const STATUS_BUFFER_BYTES: u32 = 4096;
/// How long a client of the admin pipe may take to send its next line.
const ADMIN_IDLE: Duration = Duration::from_secs(10);
/// How long the service waits for one write to a pipe.
const WRITE_LIMIT: Duration = Duration::from_secs(2);
/// How long a client waits for a pipe whose instances are all busy.
const BUSY_WAIT: Duration = Duration::from_secs(3);
const ERROR_PIPE_BUSY: i32 = 231;
/// A server learns nothing about the client and cannot act as it.
const SECURITY_ANONYMOUS: u32 = 0;
/// A server may learn who the client is, and cannot act as it.
const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;
/// Full access for the owner of the object.
const OWNER_ACCESS_ACE: &str = "(A;;GA;;;OW)";

/// The full names of the service's pipes.
#[derive(Debug, Clone)]
pub struct PipeNames {
    pub admin: String,
    pub status: String,
    /// Also give the account that creates the pipes full access to them. A
    /// test run is not SYSTEM, and without this it could not create the
    /// second instance of its own pipes.
    owner_access: bool,
}

impl PipeNames {
    /// The names every installed service uses.
    pub fn standard() -> Self {
        Self {
            admin: full_name(ADMIN_PIPE),
            status: full_name(STATUS_PIPE),
            owner_access: false,
        }
    }

    /// Names with a suffix, so a test run does not meet an installed service,
    /// and pipes the account of the test run can serve.
    #[cfg(feature = "test-hooks")]
    pub fn with_suffix(suffix: &str) -> Self {
        Self {
            admin: full_name(&format!("{ADMIN_PIPE}-{suffix}")),
            status: full_name(&format!("{STATUS_PIPE}-{suffix}")),
            owner_access: true,
        }
    }

    /// The descriptor of a pipe: `sddl`, plus full access for the creating
    /// account in a test run.
    fn sddl(&self, sddl: &str) -> String {
        if self.owner_access {
            format!("{sddl}{OWNER_ACCESS_ACE}")
        } else {
            sddl.to_string()
        }
    }
}

/// `\\.\pipe\<name>`.
pub fn full_name(name: &str) -> String {
    format!(r"\\.\pipe\{name}")
}

/// Whether text can be the last part of a pipe name given on a command line:
/// letters, digits, `.`, `_` and `-`, 1 to 64 characters.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Which way a pipe carries data.
#[derive(Clone, Copy)]
enum Flow {
    /// Requests in, answers out.
    Both,
    /// Server to client only.
    Out,
}

fn create(name: &str, sddl: &str, flow: Flow, first: bool) -> Result<NamedPipeServer, String> {
    let descriptor = Descriptor::from_sddl(sddl)?;
    let mut attributes = descriptor.attributes();
    let mut options = ServerOptions::new();
    options
        .pipe_mode(PipeMode::Byte)
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .out_buffer_size(STATUS_BUFFER_BYTES);
    if matches!(flow, Flow::Out) {
        options.access_inbound(false);
    }
    // SAFETY: the pointer is to a SECURITY_ATTRIBUTES whose descriptor lives
    // until the end of this function; the call only reads it.
    unsafe {
        options.create_with_security_attributes_raw(
            name,
            std::ptr::addr_of_mut!(attributes).cast::<c_void>(),
        )
    }
    .map_err(|e| format!("cannot create the pipe {name}: {e}"))
}

/// A pipe that is open and has not served anyone yet.
pub struct Listener {
    name: String,
    sddl: String,
    flow: Flow,
    server: NamedPipeServer,
}

impl Listener {
    /// Opens the admin pipe. Must run inside the tokio runtime.
    pub fn admin(names: &PipeNames) -> Result<Self, String> {
        Self::open(&names.admin, names.sddl(ADMIN_PIPE_SDDL), Flow::Both)
    }

    /// Opens the status pipe. Must run inside the tokio runtime.
    pub fn status(names: &PipeNames) -> Result<Self, String> {
        Self::open(&names.status, names.sddl(STATUS_PIPE_SDDL), Flow::Out)
    }

    fn open(name: &str, sddl: String, flow: Flow) -> Result<Self, String> {
        let server = create(name, &sddl, flow, true)?;
        Ok(Self {
            name: name.to_string(),
            sddl,
            flow,
            server,
        })
    }

    /// Waits for the next client. The instance for the client after that is
    /// created before this one is handed out, so the name is never free for
    /// another program to take.
    async fn accept(&mut self) -> NamedPipeServer {
        loop {
            if let Err(error) = self.server.connect().await {
                log::warn!("pipe {}: a client could not connect: {error}", self.name);
                // The instance is still ours; make it ready for the next one.
                let _ = self.server.disconnect();
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            match create(&self.name, &self.sddl, self.flow, false) {
                Ok(next) => return std::mem::replace(&mut self.server, next),
                Err(error) => {
                    // Without a second instance this client is turned away
                    // and the one instance serves the next.
                    log::error!("pipe {}: {error}", self.name);
                    let _ = self.server.disconnect();
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

/// Serves the status pipe: every client gets the current status line, then
/// the connection closes. Nothing is ever read.
pub async fn serve_status(mut listener: Listener, status: watch::Receiver<Status>) {
    loop {
        let mut client = listener.accept().await;
        let line = status.borrow().line();
        tokio::spawn(async move {
            // The line fits the pipe's buffer, so this does not wait for the
            // reader. Closing the handle afterwards leaves the line readable.
            let _ = tokio::time::timeout(WRITE_LIMIT, client.write_all(line.as_bytes())).await;
        });
    }
}

/// Serves the admin pipe: one frame per line, one answer per frame. A frame
/// that is refused gets its answer and the connection closes.
pub async fn serve_admin(mut listener: Listener, actions: Arc<AdminActions>) {
    loop {
        let client = listener.accept().await;
        tokio::spawn(admin_client(client, Arc::clone(&actions)));
    }
}

async fn answer(client: &mut NamedPipeServer, line: String) -> bool {
    matches!(
        tokio::time::timeout(WRITE_LIMIT, client.write_all(line.as_bytes())).await,
        Ok(Ok(()))
    )
}

async fn admin_client(mut client: NamedPipeServer, actions: Arc<AdminActions>) {
    // Sized up front and wiped on drop: a frame may carry the keys.
    let mut pending = Zeroizing::new(Vec::<u8>::with_capacity(2 * MAX_FRAME_BYTES + 2));
    let mut chunk = Zeroizing::new([0u8; 1024]);
    loop {
        let line_end = loop {
            if let Some(position) = pending.iter().position(|byte| *byte == b'\n') {
                break position;
            }
            if pending.len() > MAX_FRAME_BYTES {
                let refusal = Refusal::TooLong;
                answer(
                    &mut client,
                    admin::refused(refusal.code(), refusal.detail()),
                )
                .await;
                return;
            }
            match tokio::time::timeout(ADMIN_IDLE, client.read(&mut chunk[..])).await {
                Ok(Ok(read)) if read > 0 => pending.extend_from_slice(&chunk[..read]),
                // Closed by the client, failed or idle for too long.
                _ => return,
            }
        };
        let request = {
            let line = &pending[..line_end];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            admin::parse(line)
        };
        pending.drain(..=line_end);
        let request = match request {
            Ok(request) => request,
            Err(refusal) => {
                // Only the kind of refusal is logged, never the frame.
                log::warn!("admin: a frame was refused ({})", refusal.code());
                answer(
                    &mut client,
                    admin::refused(refusal.code(), refusal.detail()),
                )
                .await;
                return;
            }
        };
        let actions = Arc::clone(&actions);
        // The vault and the settings file are blocking calls.
        let result = tokio::task::spawn_blocking(move || actions.carry_out(&request)).await;
        let line = match result {
            Ok(Ok(())) => admin::accepted(),
            Ok(Err(detail)) => admin::refused("failed", &detail),
            Err(_) => admin::refused("failed", "the request could not be carried out"),
        };
        if !answer(&mut client, line).await {
            return;
        }
    }
}

// ---- the client side ----

fn open_client(name: &str, write: bool, qos: u32) -> std::io::Result<File> {
    let deadline = Instant::now() + BUSY_WAIT;
    loop {
        let opened = OpenOptions::new()
            .read(true)
            .write(write)
            // Without this a pipe server could act with the rights of an
            // elevated client.
            .security_qos_flags(qos)
            .open(name);
        match opened {
            Err(error)
                if error.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            other => return other,
        }
    }
}

/// Reads the status line of a running service. Err when no service answers.
pub fn read_status(names: &PipeNames) -> Result<String, String> {
    let pipe = open_client(&names.status, false, SECURITY_ANONYMOUS)
        .map_err(|e| format!("the Deskmate service does not answer: {e}"))?;
    let mut line = String::new();
    pipe.take(u64::from(STATUS_BUFFER_BYTES))
        .read_to_string(&mut line)
        .map_err(|e| format!("cannot read the status: {e}"))?;
    Ok(line.trim_end().to_string())
}

/// A process handle, closed on drop.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcess and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The process on the server end of an open pipe.
pub fn server_process_id(pipe: &File) -> Result<u32, String> {
    let mut process_id = 0u32;
    // SAFETY: the handle belongs to `pipe`, which is open for the length of
    // the call, and `process_id` is a valid out-pointer.
    unsafe { GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &mut process_id) }
        .map_err(|e| format!("cannot tell which process serves the pipe: {e}"))?;
    Ok(process_id)
}

/// The path of the executable a process runs.
pub fn process_image(process_id: u32) -> Result<PathBuf, String> {
    // SAFETY: no pointers are passed; a failure comes back as an error.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
        .map_err(|e| format!("cannot open the process {process_id}: {e}"))?;
    let process = Process(handle);
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    // SAFETY: `buffer` holds `length` UTF-16 units and both outlive the call,
    // which writes at most that many and stores the count in `length`.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .map_err(|e| format!("cannot read the path of the process {process_id}: {e}"))?;
    Ok(PathBuf::from(String::from_utf16_lossy(
        &buffer[..length as usize],
    )))
}

/// An open connection to the admin pipe.
pub struct AdminClient(File);

impl AdminClient {
    /// Connects to the admin pipe. With `expected_server` the connection is
    /// kept only when that process serves the pipe: a frame with the keys
    /// must not go to a program that took the pipe's name before the service
    /// could.
    pub fn connect(names: &PipeNames, expected_server: Option<u32>) -> Result<Self, String> {
        let pipe = open_client(&names.admin, true, SECURITY_IDENTIFICATION)
            .map_err(|e| format!("cannot reach the service's admin pipe: {e}"))?;
        if let Some(expected) = expected_server {
            if server_process_id(&pipe)? != expected {
                return Err("the admin pipe is not served by the Deskmate service".into());
            }
        }
        Ok(Self(pipe))
    }

    /// Sends one frame, a line that ends in a line break, and returns the
    /// answer as (accepted, detail).
    pub fn request(&mut self, frame: &[u8]) -> Result<(bool, String), String> {
        self.0
            .write_all(frame)
            .map_err(|e| format!("cannot send to the service: {e}"))?;
        let mut answer = String::new();
        BufReader::new((&self.0).take(MAX_FRAME_BYTES as u64))
            .read_line(&mut answer)
            .map_err(|e| format!("the service did not answer: {e}"))?;
        let answer: serde_json::Value = serde_json::from_str(answer.trim())
            .map_err(|_| "the service gave no answer".to_string())?;
        let accepted = answer.get("ok").and_then(serde_json::Value::as_bool) == Some(true);
        let detail = answer
            .get("detail")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok((accepted, detail))
    }
}

/// Reads the one provisioning frame an app offers on its one-time pipe,
/// after checking that the program serving the pipe is `expected_program`
/// (a file name such as `deskmate.exe`). The frame comes back as one line.
pub fn read_provisioning_frame(
    pipe_name: &str,
    expected_program: &str,
) -> Result<Zeroizing<Vec<u8>>, String> {
    if !is_plain_name(pipe_name) {
        return Err("the name of the provisioning pipe is not valid".into());
    }
    let pipe = open_client(&full_name(pipe_name), false, SECURITY_ANONYMOUS)
        .map_err(|e| format!("cannot open the provisioning pipe: {e}"))?;
    let image = process_image(server_process_id(&pipe)?)?;
    let program = image
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !program.eq_ignore_ascii_case(expected_program) {
        return Err(format!(
            "the provisioning pipe is not served by {expected_program}"
        ));
    }
    // Sized up front and wiped on drop: the frame carries the keys.
    let mut frame = Zeroizing::new(Vec::<u8>::with_capacity(MAX_FRAME_BYTES + 2));
    let mut byte = [0u8; 1];
    let mut pipe = pipe;
    loop {
        // One byte at a time: nothing of the frame sits in a buffer that is
        // not wiped.
        match pipe.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => break,
            Ok(1) if frame.len() < MAX_FRAME_BYTES => frame.push(byte[0]),
            Ok(1) => return Err("the provisioning frame is too long".into()),
            Ok(_) if frame.is_empty() => return Err("the provisioning pipe sent nothing".into()),
            Ok(_) => break,
            Err(error) => return Err(format!("cannot read the provisioning frame: {error}")),
        }
    }
    frame.push(b'\n');
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_names_from_a_command_line_are_plain() {
        for name in ["deskmate-provision-4f2a", "a", "A.b_c-1", &"n".repeat(64)] {
            assert!(is_plain_name(name), "{name}");
        }
        for name in [
            "",
            r"..\other",
            r"\\server\pipe\x",
            "a b",
            "a/b",
            "pipe\0",
            &"n".repeat(65),
        ] {
            assert!(!is_plain_name(name), "{name}");
        }
        assert_eq!(
            full_name("deskmate-service-status"),
            r"\\.\pipe\deskmate-service-status"
        );
        assert_eq!(
            PipeNames::standard().admin,
            r"\\.\pipe\deskmate-service-admin"
        );
    }

    #[test]
    fn finds_the_program_of_a_process() {
        let image = process_image(std::process::id()).unwrap();
        assert_eq!(
            image.file_name(),
            std::env::current_exe().unwrap().file_name()
        );
        assert!(process_image(0).is_err());
    }
}
