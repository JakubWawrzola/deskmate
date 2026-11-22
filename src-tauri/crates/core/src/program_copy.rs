//! Copies of the program that an elevated step put into Program Files: the
//! CPU temperature helper and the family watchdog. An update replaces the
//! program but not these copies, because writing there needs an
//! administrator. This tells whether a copy is still the program that runs
//! now, so the window can offer to replace it.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// What was found for a copy, and the state of the file it was found for.
type Known = (u64, Option<SystemTime>, bool);

/// The answer per copy. The comparison reads both files in full, and the
/// window asks every few seconds, so it is done again only when the copy
/// itself changed.
static KNOWN: Mutex<Option<HashMap<PathBuf, Known>>> = Mutex::new(None);

/// Whether `copy` exists and is not the program that runs now. False when
/// there is no copy, and when either file cannot be read: an offer to
/// replace something should not rest on a guess.
pub fn is_outdated(copy: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(copy) else {
        return false;
    };
    let state = (metadata.len(), metadata.modified().ok());
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    let known = known.get_or_insert_with(HashMap::new);
    if let Some((len, modified, outdated)) = known.get(copy) {
        if (*len, *modified) == state {
            return *outdated;
        }
    }
    let outdated = std::env::current_exe()
        .and_then(|running| differ(copy, &running))
        .unwrap_or(false);
    known.insert(copy.to_path_buf(), (state.0, state.1, outdated));
    outdated
}

/// Whether two files have different contents.
fn differ(left: &Path, right: &Path) -> std::io::Result<bool> {
    if std::fs::metadata(left)?.len() != std::fs::metadata(right)?.len() {
        return Ok(true);
    }
    let (mut left, mut right) = (File::open(left)?, File::open(right)?);
    let (mut left_chunk, mut right_chunk) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let read = read_full(&mut left, &mut left_chunk)?;
        if read != read_full(&mut right, &mut right_chunk)? {
            return Ok(true);
        }
        if left_chunk[..read] != right_chunk[..read] {
            return Ok(true);
        }
        if read == 0 {
            return Ok(false);
        }
    }
}

/// Fills `chunk` as far as the file goes; a short count means the end.
fn read_full(file: &mut File, chunk: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < chunk.len() {
        match file.read(&mut chunk[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of its own in the temp folder, removed again when dropped.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str, contents: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "deskmate-program-copy-{}-{name}",
                std::process::id()
            ));
            std::fs::write(&path, contents).unwrap();
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn files_with_the_same_contents_do_not_differ() {
        let big = vec![7u8; 200_000];
        let left = TempFile::new("same-left", &big);
        let right = TempFile::new("same-right", &big);
        assert!(!differ(&left.0, &right.0).unwrap());
        let empty_left = TempFile::new("empty-left", b"");
        let empty_right = TempFile::new("empty-right", b"");
        assert!(!differ(&empty_left.0, &empty_right.0).unwrap());
    }

    #[test]
    fn a_different_length_or_a_single_different_byte_differs() {
        let mut bytes = vec![7u8; 200_000];
        let left = TempFile::new("diff-left", &bytes);
        let longer = TempFile::new("diff-longer", &[bytes.as_slice(), b"x"].concat());
        assert!(differ(&left.0, &longer.0).unwrap());
        // Same length, the last byte of the last chunk changed.
        *bytes.last_mut().unwrap() = 8;
        let changed = TempFile::new("diff-changed", &bytes);
        assert!(differ(&left.0, &changed.0).unwrap());
    }

    #[test]
    fn a_copy_that_does_not_exist_is_not_outdated() {
        let missing = std::env::temp_dir().join("deskmate-program-copy-no-such-file");
        assert!(!is_outdated(&missing));
        assert!(differ(&missing, &missing).is_err());
    }

    #[test]
    fn a_copy_of_another_program_is_outdated_and_a_true_copy_is_not() {
        let other = TempFile::new("other", b"not this program");
        assert!(is_outdated(&other.0));
        // Asked again: the remembered answer.
        assert!(is_outdated(&other.0));

        let running = std::env::current_exe().unwrap();
        let copy = TempFile::new("true-copy", &std::fs::read(&running).unwrap());
        assert!(!is_outdated(&copy.0));
    }
}
