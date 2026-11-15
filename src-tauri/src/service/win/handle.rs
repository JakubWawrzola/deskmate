//! A kernel handle that is closed when dropped.

use windows::Win32::Foundation::{CloseHandle, HANDLE};

pub struct Owned(HANDLE);

impl Owned {
    /// Takes over a handle that the caller opened and nobody else closes.
    pub fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.is_invalid() {
            return;
        }
        // SAFETY: the handle was handed over by `new`, which takes ownership,
        // and it is closed here exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// SAFETY: a kernel handle is a process-wide value; closing or using it from
// another thread is allowed, and `Owned` never hands out the same handle to
// two owners.
unsafe impl Send for Owned {}
