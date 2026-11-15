//! Starts a program with administrator rights: one UAC prompt, then a
//! process the caller can watch.

use std::path::Path;
use std::time::Instant;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{GetExitCodeProcess, GetProcessId, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

use super::handle::Owned;

/// Why an elevated program did not run to the end.
#[derive(Debug, PartialEq, Eq)]
pub enum Elevation {
    /// The user said no at the UAC prompt.
    Declined,
    /// Windows could not start the program.
    NotStarted(String),
}

impl Elevation {
    pub fn message(&self) -> String {
        match self {
            Self::Declined => "Administrator approval was declined. Nothing was changed.".into(),
            Self::NotStarted(detail) => format!(
                "Windows could not start the step that needs administrator rights ({detail})."
            ),
        }
    }
}

/// A running elevated process.
pub struct ElevatedProcess {
    process: Owned,
    id: u32,
}

impl ElevatedProcess {
    /// Asks for approval and starts `program` with `parameters`. Returns once
    /// the user has answered the prompt.
    pub fn start(program: &Path, parameters: &str) -> Result<Self, Elevation> {
        let file = HSTRING::from(program.as_os_str());
        let parameters = HSTRING::from(parameters);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // SAFETY: `info` is fully initialised, its size is set, and the
        // strings it points at live until the call returns. With
        // SEE_MASK_NOCLOSEPROCESS the process handle becomes ours.
        let started = unsafe { ShellExecuteExW(&mut info) };
        if let Err(error) = started {
            return Err(if error.code() == ERROR_CANCELLED.to_hresult() {
                Elevation::Declined
            } else {
                Elevation::NotStarted(error.message())
            });
        }
        let process = Owned::new(info.hProcess);
        if process.raw().is_invalid() {
            return Err(Elevation::NotStarted("no process was created".into()));
        }
        // SAFETY: the handle is open; a failure returns 0, which no process has.
        let id = unsafe { GetProcessId(process.raw()) };
        Ok(Self { process, id })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    /// The process handle, signalled when the process ends.
    pub fn handle(&self) -> HANDLE {
        self.process.raw()
    }

    /// Waits for the end until `deadline`. Some(exit code), or None when the
    /// process still runs.
    pub fn wait(&self, deadline: Instant) -> Option<u32> {
        let left = deadline.saturating_duration_since(Instant::now());
        let millis = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the handle is open for the length of the call.
        if unsafe { WaitForSingleObject(self.process.raw(), millis) } != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 1u32;
        // SAFETY: the handle is open and `code` is a valid out-pointer.
        unsafe { GetExitCodeProcess(self.process.raw(), &mut code) }.ok()?;
        Some(code)
    }
}
