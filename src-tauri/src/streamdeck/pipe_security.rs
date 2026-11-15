//! Who may open the channel's pipe, and the WinAPI calls that say so. Each
//! call sits in a small wrapper that owns what Windows allocated and frees
//! it when dropped.

use std::ffi::c_void;
use std::io;

use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// The access token of this process, closed when dropped.
struct ProcessToken(HANDLE);

impl ProcessToken {
    fn open() -> io::Result<Self> {
        let mut token = HANDLE::default();
        // SAFETY: the pseudo handle of the current process is always valid,
        // and `token` is a valid place for the opened handle.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }?;
        Ok(Self(token))
    }
}

impl Drop for ProcessToken {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `open` and is closed only here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Memory that Windows allocated with LocalAlloc, freed when dropped.
struct LocalMemory(*mut c_void);

impl Drop for LocalMemory {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a call documented to return
        // LocalAlloc memory and is freed only here.
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0))) };
    }
}

/// The SID of the user this process runs as, in its string form.
pub fn current_user_sid() -> io::Result<String> {
    let token = ProcessToken::open()?;
    let mut needed = 0u32;
    // SAFETY: asking with no buffer is the documented way to learn the size;
    // `needed` is a valid place for it. The call fails by design.
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 elements: TOKEN_USER holds a pointer and needs its alignment.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
    let buffer_bytes = (buffer.len() * size_of::<u64>()) as u32;
    // SAFETY: `buffer` is writable for `buffer_bytes` bytes.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            buffer_bytes,
            &mut needed,
        )
    }?;
    // SAFETY: the call succeeded, so the buffer starts with a TOKEN_USER. It
    // is aligned for one, and its SID pointer points into the same buffer,
    // which lives until the end of this function.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    // SAFETY: the SID is valid as explained above; `text` receives a string
    // that the guard below frees.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }?;
    let _guard = LocalMemory(text.0.cast());
    // SAFETY: `text` is the NUL-terminated string written by the call above
    // and is alive until `_guard` is dropped.
    unsafe { text.to_string() }.map_err(|_| io::Error::other("the user's SID is not valid text"))
}

/// The access rules of the pipe, in SDDL:
/// - network logons are denied everything, whatever else they match;
/// - the system account has full access;
/// - the user has read and write access, which includes opening further
///   instances of the pipe, as the app itself has to.
///
/// Nobody else is listed, so nobody else gets in. The rules are protected:
/// nothing is inherited.
pub fn channel_sddl(user_sid: &str) -> io::Result<String> {
    let is_sid = user_sid.starts_with("S-1-")
        && user_sid
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == 'S');
    if !is_sid {
        return Err(io::Error::other("the user's SID has an unexpected form"));
    }
    Ok(format!("D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;GRGW;;;{user_sid})"))
}

/// A security descriptor built from SDDL, freed when dropped.
struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let text = HSTRING::from(sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `text` is a NUL-terminated string that outlives the call,
        // and `descriptor` receives memory that `Drop` frees.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &text,
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }?;
        Ok(Self(descriptor))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by Windows in `from_sddl` and
        // is freed only here.
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0 .0))) };
    }
}

/// Opens one instance of the pipe with the given access rules, for local
/// clients only.
///
/// `first` asks Windows to refuse when a pipe of this name already exists.
/// That is how a program that took the name before the app is noticed: the
/// access rules of a pipe are those of its first instance, so joining a pipe
/// that someone else created would mean serving under their rules.
///
/// Has to be called inside the Tokio runtime.
pub fn open_instance(pipe_path: &str, sddl: &str, first: bool) -> io::Result<NamedPipeServer> {
    let descriptor = SecurityDescriptor::from_sddl(sddl)?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0 .0,
        bInheritHandle: false.into(),
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .pipe_mode(PipeMode::Byte);
    // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose descriptor
    // lives until the end of this function; Windows copies the descriptor
    // while the pipe is created and keeps no pointer to it.
    unsafe { options.create_with_security_attributes_raw(pipe_path, (&raw mut attributes).cast()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_user_has_a_sid() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
    }

    #[test]
    fn the_rules_name_the_system_the_user_and_nobody_else() {
        assert_eq!(
            channel_sddl("S-1-5-21-1-2-3-1001").unwrap(),
            "D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;GRGW;;;S-1-5-21-1-2-3-1001)"
        );
    }

    #[test]
    fn text_that_is_not_a_sid_does_not_reach_the_rules() {
        for text in ["", "WD", "S-1-5-21)(A;;GA;;;WD", "s-1-5-21", "S-2-5"] {
            assert!(channel_sddl(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn windows_accepts_the_rules() {
        let sddl = channel_sddl(&current_user_sid().unwrap()).unwrap();
        assert!(SecurityDescriptor::from_sddl(&sddl).is_ok());
        assert!(SecurityDescriptor::from_sddl("not sddl").is_err());
    }
}
