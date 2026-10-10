//! `deskmate.exe --forget-this-computer`, run by the uninstaller when "Delete
//! the application data" is ticked (GitHub issue #2: a full uninstall left
//! settings and keys behind, so a reinstall picked them up again).
//!
//! Removes what Deskmate keeps outside its install folder: settings and the
//! security log in %APPDATA%\Deskmate, and the pairing keys, cascade keys, MQTT
//! passwords and Home Assistant tokens in Windows Credential Manager. The
//! uninstaller itself removes the registry entries. Files received in the inbox
//! folder belong to the user and stay.

pub const FORGET_ARG: &str = "--forget-this-computer";

/// Credential Manager entries are "<account>.<service>" (keyring crate).
const CREDENTIAL_SUFFIXES: [&str; 4] = [
    crate::consts::KEYRING_SERVICE,
    crate::consts::LINK_KEYRING_SERVICE,
    crate::consts::CASCADE_KEYRING_SERVICE,
    crate::consts::HA_TOKEN_KEYRING_SERVICE,
];

pub fn forget_this_computer() {
    delete_credentials();
    if let Some(dir) = crate::config::config_path().parent() {
        if dir.ends_with(crate::consts::CONFIG_DIR) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Whether a Credential Manager target belongs to Deskmate.
fn is_deskmate_target(target: &str) -> bool {
    CREDENTIAL_SUFFIXES
        .iter()
        .any(|service| target.ends_with(&format!(".{service}")))
}

#[cfg(windows)]
fn delete_credentials() {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredEnumerateW, CredFree, CREDENTIALW, CRED_TYPE_GENERIC,
    };
    let mut targets = Vec::new();
    unsafe {
        let mut count = 0u32;
        let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
        // The filter only takes a "prefix*" form and these targets start with the
        // node name, so list everything and match the suffix here.
        if CredEnumerateW(PCWSTR::null(), None, &mut count, &mut list).is_ok() {
            for i in 0..count as usize {
                let credential = &**list.add(i);
                if credential.Type == CRED_TYPE_GENERIC {
                    if let Ok(target) = credential.TargetName.to_string() {
                        if is_deskmate_target(&target) {
                            targets.push(target);
                        }
                    }
                }
            }
            CredFree(list as *const _);
        }
        for target in targets {
            let _ = CredDeleteW(&HSTRING::from(target), CRED_TYPE_GENERIC, None);
        }
    }
}

#[cfg(not(windows))]
fn delete_credentials() {}

#[cfg(test)]
mod tests {
    use super::is_deskmate_target;

    #[test]
    fn matches_only_deskmate_credentials() {
        assert!(is_deskmate_target("laptop.Deskmate Link"));
        assert!(is_deskmate_target("laptop.Deskmate Link Cascade"));
        assert!(is_deskmate_target("laptop.Deskmate HA Token"));
        assert!(is_deskmate_target("kuba@192.168.1.5.Deskmate MQTT"));
        assert!(!is_deskmate_target("Deskmate Link"));
        assert!(!is_deskmate_target("laptop.Deskmate Link backup"));
        assert!(!is_deskmate_target("git:https://github.com"));
    }
}
