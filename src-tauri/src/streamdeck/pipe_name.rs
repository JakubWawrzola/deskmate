//! The name of the channel's pipe: `\\.\pipe\deskmate-streamdeck-<user>`.
//! The plugin derives the same name on its side
//! (`streamdeck-plugin/src/channel/pipe-name.ts`); both are tested against
//! one table, `streamdeck-plugin/test/pipe-names.json`.

/// Every pipe of the channel starts with this; the sanitized user name follows.
pub const PIPE_NAME_PREFIX: &str = "deskmate-streamdeck-";

const WINDOWS_PIPE_ROOT: &str = r"\\.\pipe\";

/// The user name as it appears in the pipe name: lowercased, then every
/// character outside a-z and 0-9 written as its code point in lowercase hex
/// between two underscores (`é` is `_e9_`). A character is a Unicode code
/// point. Lowercasing comes first and is the full Unicode one, so a letter
/// whose lowercase form is two code points gives two. Every underscore opens
/// or closes such an escape, so two names that differ after lowercasing never
/// share a pipe.
pub fn sanitize_user_name(user_name: &str) -> String {
    let mut sanitized = String::new();
    for character in user_name.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            sanitized.push(character);
        } else {
            sanitized.push_str(&format!("_{:x}_", u32::from(character)));
        }
    }
    sanitized
}

/// Full path of the channel's pipe for a Windows user.
pub fn pipe_path_for_user(user_name: &str) -> String {
    format!(
        "{WINDOWS_PIPE_ROOT}{PIPE_NAME_PREFIX}{}",
        sanitize_user_name(user_name)
    )
}

/// The name of the user this process runs as, from `GetUserNameW`: the call
/// behind `os.userInfo().username`, which the plugin uses.
#[cfg(windows)]
pub fn current_user_name() -> std::io::Result<String> {
    use windows::core::PWSTR;
    use windows::Win32::System::WindowsProgramming::GetUserNameW;

    /// Longest user name Windows allows (UNLEN), plus the terminating NUL.
    const BUFFER_CHARS: usize = 256 + 1;

    let mut buffer = [0u16; BUFFER_CHARS];
    let mut length = BUFFER_CHARS as u32;
    // SAFETY: `buffer` is writable for `length` UTF-16 units, and `length`
    // is a valid pointer for the duration of the call.
    unsafe { GetUserNameW(Some(PWSTR(buffer.as_mut_ptr())), &mut length) }?;
    // On success `length` counts the units written, the NUL included.
    let written = (length as usize).min(BUFFER_CHARS).saturating_sub(1);
    Ok(String::from_utf16_lossy(&buffer[..written]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        user: String,
        name: String,
    }

    /// The table the plugin's own test reads as well.
    fn shared_table() -> Vec<Case> {
        serde_json::from_str(include_str!(
            "../../../streamdeck-plugin/test/pipe-names.json"
        ))
        .unwrap()
    }

    #[test]
    fn every_name_of_the_shared_table_is_derived_like_in_the_plugin() {
        let table = shared_table();
        assert!(table.iter().any(|case| !case.user.is_ascii()));
        for case in table {
            assert_eq!(sanitize_user_name(&case.user), case.name, "{:?}", case.user);
        }
    }

    #[test]
    fn names_that_differ_get_different_pipes() {
        let table = shared_table();
        let mut names = std::collections::HashSet::new();
        for case in &table {
            assert!(names.insert(case.name.as_str()), "{:?}", case.user);
        }
        for (one, other) in [
            ("Иван", "Петр"),
            ("张伟", "李娜"),
            ("Zoë", "Zoé"),
            ("jan.kowalski", "jan kowalski"),
            ("a_b", "a b"),
            ("_5f_", "_"),
        ] {
            assert_ne!(sanitize_user_name(one), sanitize_user_name(other), "{one}");
        }
    }

    #[test]
    fn lowercasing_happens_before_the_replacement() {
        // The Kelvin sign lowercases to an ASCII k.
        assert_eq!(sanitize_user_name("\u{212a}"), "k");
        // A capital I with a dot lowercases to "i" plus a combining dot.
        assert_eq!(sanitize_user_name("\u{130}"), "i_307_");
        // One emoji is one code point, so one escape.
        assert_eq!(sanitize_user_name("pc\u{1f600}"), "pc_1f600_");
    }

    #[test]
    fn the_pipe_path_is_the_fixed_prefix_plus_the_sanitized_name() {
        assert_eq!(
            pipe_path_for_user("My-PC User"),
            r"\\.\pipe\deskmate-streamdeck-my_2d_pc_20_user"
        );
        assert_eq!(pipe_path_for_user(""), r"\\.\pipe\deskmate-streamdeck-");
    }

    #[cfg(windows)]
    #[test]
    fn the_current_user_has_a_name() {
        let name = current_user_name().unwrap();
        assert!(!name.is_empty());
        assert!(!name.contains('\0'));
    }
}
