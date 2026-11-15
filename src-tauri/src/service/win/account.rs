//! The account this process runs as, in the text form an access list takes.

use windows::core::PWSTR;
use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::handle::Owned;

/// A string the system allocated with LocalAlloc, released on drop.
struct LocalString(PWSTR);

impl Drop for LocalString {
    fn drop(&mut self) {
        // SAFETY: the pointer came from ConvertSidToStringSidW, which
        // allocates with LocalAlloc, and it is released exactly once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0 .0.cast())));
        }
    }
}

/// `S-1-` followed by numbers separated by hyphens.
pub fn is_sid(text: &str) -> bool {
    text.len() <= 184
        && text.strip_prefix("S-1-").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .split('-')
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// The SID of the user this process runs as, such as `S-1-5-21-...`.
pub fn current_user_sid() -> Result<String, String> {
    let mut raw = HANDLE::default();
    // SAFETY: the pseudo handle of the current process is always valid and
    // `raw` is a valid out-pointer.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) }
        .map_err(|e| format!("cannot read this Windows account: {e}"))?;
    let token = Owned::new(raw);
    let mut needed = 0u32;
    // SAFETY: asks only for the size; no buffer is passed. The call fails
    // with "insufficient buffer" by design and fills `needed`.
    let _ = unsafe { GetTokenInformation(token.raw(), TokenUser, None, 0, &mut needed) };
    // u64 elements: TOKEN_USER holds a pointer and needs its alignment.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    let size = u32::try_from(buffer.len() * 8).map_err(|_| "the account is too large")?;
    // SAFETY: `buffer` holds `size` bytes, is aligned for TOKEN_USER and
    // outlives the call; `needed` is a valid out-pointer.
    unsafe {
        GetTokenInformation(
            token.raw(),
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut needed,
        )
    }
    .map_err(|e| format!("cannot read this Windows account: {e}"))?;
    // SAFETY: the call above filled the buffer with a TOKEN_USER whose SID
    // points into the same buffer, which lives until the end of this
    // function.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text = PWSTR::null();
    // SAFETY: `sid` is valid (see above) and `text` is a valid out-pointer;
    // the string it receives is released by LocalString.
    unsafe { ConvertSidToStringSidW(sid, &mut text) }
        .map_err(|e| format!("cannot read this Windows account: {e}"))?;
    let text = LocalString(text);
    // SAFETY: the string is NUL-terminated and valid until `text` drops.
    let sid = unsafe { text.0.to_string() }.map_err(|_| "the account name is not valid text")?;
    if is_sid(&sid) {
        Ok(sid)
    } else {
        Err("this Windows account has an unexpected form".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_account_of_this_process() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-5-"), "{sid}");
    }

    #[test]
    fn only_a_sid_is_a_sid() {
        assert!(is_sid("S-1-5-21-1-2-3-1001"));
        assert!(is_sid("S-1-5-18"));
        for text in [
            "",
            "S-1-",
            "S-1-5-",
            "S-1-5-x",
            "BA",
            "S-1-5-21)(A;;GA;;;WD",
        ] {
            assert!(!is_sid(text), "{text}");
        }
    }
}
