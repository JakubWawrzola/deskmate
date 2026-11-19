//! The one-time pipe on which the app hands the service its settings and keys.
//!
//! - The name is `deskmate-provision-` and 128 random bits in hex, so nobody
//!   can create it in advance.
//! - It is created as the first and only instance of that name, refuses
//!   remote clients, and lets only the signed-in user and Administrators
//!   read it (Administrators for an administrator who approves the step
//!   with their own account). Nobody may write to it or add an instance.
//! - It serves one reader: the process the app started. Any other client is
//!   disconnected unread. After that one reader it never connects again.
//! - It is open only until a deadline, and the caller drops it as soon as the
//!   elevated step has ended.
//!
//! The frame is written once from a buffer that the caller wipes.

use rand::RngCore;
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_OUTBOUND,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForMultipleObjects};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use super::account::current_user_sid;
use super::handle::Owned;
use deskmate_service::win::pipes::{full_name, is_plain_name};
use deskmate_service::win::security::Descriptor;

const NAME_PREFIX: &str = "deskmate-provision-";
const NAME_RANDOM_BYTES: usize = 16;
/// The frame is far shorter, so a write never waits for the reader.
const OUT_BUFFER_BYTES: u32 = 16 * 1024;
/// Remote users are refused, the user and Administrators may read.
fn sddl(user_sid: &str) -> String {
    format!("D:P(D;;GA;;;NU)(A;;GR;;;{user_sid})(A;;GR;;;BA)")
}

/// A fresh name: the prefix and 32 lowercase hex digits.
pub fn random_name() -> String {
    let mut bytes = [0u8; NAME_RANDOM_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut name = String::with_capacity(NAME_PREFIX.len() + 2 * NAME_RANDOM_BYTES);
    name.push_str(NAME_PREFIX);
    for byte in bytes {
        name.push_str(&format!("{byte:02x}"));
    }
    name
}

/// Why an offer ended without handing the frame over.
#[derive(Debug, PartialEq, Eq)]
pub enum OfferError {
    /// Nobody read the frame before the deadline.
    TimedOut,
    /// The process the frame was meant for ended first.
    ReaderGone,
    /// Windows refused an operation on the pipe.
    Failed(String),
}

impl OfferError {
    pub fn message(&self) -> String {
        match self {
            Self::TimedOut => "the service installer did not ask for the settings in time".into(),
            Self::ReaderGone => "the service installer ended before it read the settings".into(),
            Self::Failed(detail) => format!("the settings could not be handed over: {detail}"),
        }
    }
}

/// An OVERLAPPED with its own event, so a pending operation can be waited
/// for with a deadline.
struct Pending {
    overlapped: OVERLAPPED,
    event: Owned,
}

impl Pending {
    fn new() -> Result<Self, OfferError> {
        // SAFETY: no security attributes and no name; manual reset, not set.
        let event = unsafe { CreateEventW(None, true, false, None) }
            .map_err(|e| OfferError::Failed(e.to_string()))?;
        let event = Owned::new(event);
        let overlapped = OVERLAPPED {
            hEvent: event.raw(),
            ..Default::default()
        };
        Ok(Self { overlapped, event })
    }

    fn reset(&mut self) {
        // SAFETY: the event belongs to `self` and is open.
        let _ = unsafe { ResetEvent(self.event.raw()) };
        self.overlapped = OVERLAPPED {
            hEvent: self.event.raw(),
            ..Default::default()
        };
    }
}

/// The pipe, from its creation to its drop.
pub struct OneTimePipe {
    name: String,
    pipe: Owned,
}

/// What waiting for a pending operation came to.
enum Waited {
    Done,
    TimedOut,
    ReaderGone,
}

impl OneTimePipe {
    /// Creates the pipe under a fresh random name.
    pub fn create() -> Result<Self, String> {
        Self::create_named(&random_name())
    }

    fn create_named(name: &str) -> Result<Self, String> {
        if !is_plain_name(name) {
            return Err("the name of the provisioning pipe is not valid".into());
        }
        let descriptor = Descriptor::from_sddl(&sddl(&current_user_sid()?))?;
        let attributes = descriptor.attributes();
        // SAFETY: the name is a valid HSTRING and `attributes` points at a
        // descriptor; both live until the call returns. A failure comes back
        // as an invalid handle.
        let pipe = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(full_name(name)),
                PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                OUT_BUFFER_BYTES,
                0,
                0,
                Some(&attributes),
            )
        };
        if pipe.is_invalid() {
            return Err(format!(
                "cannot create the provisioning pipe: {}",
                windows::core::Error::from_thread()
            ));
        }
        Ok(Self {
            name: name.to_string(),
            pipe: Owned::new(pipe),
        })
    }

    /// The last part of the pipe's name, for `--provision-pipe`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Waits for `pending` until the deadline or until `reader` ends. A
    /// pending operation is always settled before this returns, because the
    /// system writes into `pending` until then.
    fn wait(
        &self,
        pending: &mut Pending,
        deadline: Instant,
        reader: Option<HANDLE>,
    ) -> Result<Waited, OfferError> {
        let mut handles = vec![pending.event.raw()];
        handles.extend(reader);
        let left = deadline.saturating_duration_since(Instant::now());
        let millis = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: every handle in `handles` is open for the whole call.
        let woke = unsafe { WaitForMultipleObjects(&handles, false, millis) };
        let outcome = if woke == WAIT_OBJECT_0 {
            Waited::Done
        } else if woke.0 == WAIT_OBJECT_0.0 + 1 {
            Waited::ReaderGone
        } else {
            // WAIT_TIMEOUT, or a failed wait: either way nothing was read.
            Waited::TimedOut
        };
        let mut transferred = 0u32;
        if !matches!(outcome, Waited::Done) {
            // SAFETY: the pipe is open and `pending.overlapped` is the one
            // the operation was started with.
            let _ = unsafe { CancelIoEx(self.pipe.raw(), Some(&pending.overlapped)) };
        }
        // SAFETY: as above; `true` waits until the operation has settled,
        // so nothing writes into `pending` afterwards.
        let settled = unsafe {
            GetOverlappedResult(self.pipe.raw(), &pending.overlapped, &mut transferred, true)
        };
        match (outcome, settled) {
            (Waited::Done, Err(error)) => Err(OfferError::Failed(error.to_string())),
            (outcome, _) => Ok(outcome),
        }
    }

    /// Waits for one client and connects it. Ok(true) when a client is there.
    fn connect(
        &self,
        pending: &mut Pending,
        deadline: Instant,
        reader: Option<HANDLE>,
    ) -> Result<(), OfferError> {
        pending.reset();
        // SAFETY: the pipe is open and `pending.overlapped` stays in place
        // until `wait` has settled the operation.
        let started = unsafe { ConnectNamedPipe(self.pipe.raw(), Some(&mut pending.overlapped)) };
        match started {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => Ok(()),
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
                match self.wait(pending, deadline, reader)? {
                    Waited::Done => Ok(()),
                    Waited::TimedOut => Err(OfferError::TimedOut),
                    Waited::ReaderGone => Err(OfferError::ReaderGone),
                }
            }
            Err(error) => Err(OfferError::Failed(error.to_string())),
        }
    }

    fn client_process(&self) -> Result<u32, OfferError> {
        let mut process_id = 0u32;
        // SAFETY: the pipe is open and connected; `process_id` is a valid
        // out-pointer.
        unsafe { GetNamedPipeClientProcessId(self.pipe.raw(), &mut process_id) }
            .map_err(|e| OfferError::Failed(e.to_string()))?;
        Ok(process_id)
    }

    /// Hands `frame` to the process `reader_id` and to nobody else. Gives up
    /// at `deadline`, or as soon as `reader` (that process) has ended.
    pub fn offer(
        &self,
        frame: &[u8],
        reader_id: u32,
        deadline: Instant,
        reader: Option<HANDLE>,
    ) -> Result<(), OfferError> {
        let mut pending = Pending::new()?;
        loop {
            self.connect(&mut pending, deadline, reader)?;
            if self.client_process()? == reader_id {
                break;
            }
            // Another program found the name first; it gets nothing.
            log::warn!("service setup: a program other than the installer opened the pipe");
            // SAFETY: the pipe is open; this only ends the current client.
            let _ = unsafe { DisconnectNamedPipe(self.pipe.raw()) };
            if Instant::now() >= deadline {
                return Err(OfferError::TimedOut);
            }
        }
        let length =
            u32::try_from(frame.len()).map_err(|_| OfferError::Failed("too long".into()))?;
        pending.reset();
        // SAFETY: `frame` and `pending.overlapped` stay valid until `wait`
        // has settled the write.
        let started = unsafe {
            WriteFile(
                self.pipe.raw(),
                Some(frame),
                None,
                Some(&mut pending.overlapped),
            )
        };
        match started {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {}
            Err(error) => return Err(OfferError::Failed(error.to_string())),
        }
        match self.wait(&mut pending, deadline, reader)? {
            Waited::Done => {}
            Waited::TimedOut => return Err(OfferError::TimedOut),
            Waited::ReaderGone => return Err(OfferError::ReaderGone),
        }
        let mut written = 0u32;
        // SAFETY: the write has settled; this reads its result.
        unsafe { GetOverlappedResult(self.pipe.raw(), &pending.overlapped, &mut written, false) }
            .map_err(|e| OfferError::Failed(e.to_string()))?;
        if written == length {
            Ok(())
        } else {
            Err(OfferError::Failed(
                "the settings were written only in part".into(),
            ))
        }
    }
}

/// How long the caller lets the elevated step take, from the approval to
/// the end: it stops a running service, copies it, starts it and only then
/// reads the pipe, and each of those may take a while.
pub const OFFER_LIMIT: Duration = Duration::from_secs(120);

#[cfg(test)]
mod tests {
    use super::*;
    use deskmate_service::win::pipes::read_provisioning_frame;
    use std::fs::OpenOptions;

    const FRAME: &[u8] = b"{\"v\":1,\"op\":\"reload\"}\n";

    /// The test-only reader: the service's own client, which checks that the
    /// pipe is served by the expected program, here the test binary.
    fn read(name: String) -> std::thread::JoinHandle<Result<Vec<u8>, String>> {
        let program = std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        std::thread::spawn(move || {
            read_provisioning_frame(&name, &program).map(|frame| frame.to_vec())
        })
    }

    fn soon(seconds: u64) -> Instant {
        Instant::now() + Duration::from_secs(seconds)
    }

    #[test]
    fn names_are_random_and_plain() {
        let first = random_name();
        let second = random_name();
        assert_ne!(first, second);
        let digits = first.strip_prefix(NAME_PREFIX).unwrap();
        assert_eq!(digits.len(), 32);
        assert!(digits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert!(is_plain_name(&first));
        assert!(OneTimePipe::create_named("..\\other").is_err());
    }

    #[test]
    fn hands_the_frame_to_its_reader_once() {
        let pipe = OneTimePipe::create().unwrap();
        let reader = read(pipe.name().to_string());
        pipe.offer(FRAME, std::process::id(), soon(10), None)
            .unwrap();
        assert_eq!(reader.join().unwrap().unwrap(), FRAME);
        // Nobody gets a second turn: the one instance is used up.
        assert!(read(pipe.name().to_string()).join().unwrap().is_err());
    }

    #[test]
    fn the_name_cannot_be_served_twice() {
        let pipe = OneTimePipe::create().unwrap();
        assert!(OneTimePipe::create_named(pipe.name()).is_err());
    }

    #[test]
    fn a_client_may_read_but_not_write() {
        let pipe = OneTimePipe::create().unwrap();
        let denied = OpenOptions::new()
            .read(true)
            .write(true)
            .open(full_name(pipe.name()))
            .unwrap_err();
        assert_eq!(denied.raw_os_error(), Some(5), "access denied");
    }

    #[test]
    fn gives_up_at_the_deadline() {
        let pipe = OneTimePipe::create().unwrap();
        let started = Instant::now();
        let refused = pipe.offer(
            FRAME,
            std::process::id(),
            started + Duration::from_millis(300),
            None,
        );
        assert_eq!(refused, Err(OfferError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn another_client_gets_nothing() {
        let pipe = OneTimePipe::create().unwrap();
        let reader = read(pipe.name().to_string());
        let other_process = std::process::id().wrapping_add(4);
        let refused = pipe.offer(
            FRAME,
            other_process,
            Instant::now() + Duration::from_secs(2),
            None,
        );
        assert_eq!(refused, Err(OfferError::TimedOut));
        assert!(reader.join().unwrap().is_err());
    }

    #[test]
    fn the_access_list_is_valid() {
        let sid = current_user_sid().unwrap();
        Descriptor::from_sddl(&sddl(&sid)).unwrap();
    }
}
