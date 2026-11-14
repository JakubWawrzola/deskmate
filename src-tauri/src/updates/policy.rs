//! What may be installed: the rules an update has to pass before Deskmate
//! downloads or runs it.
//!
//! They apply on top of the signature check in the updater plugin and do no
//! I/O, so each of them is covered by tests (docs/SECURITY.md, "Updates").

use url::Url;

/// Release assets live under this address and nowhere else. A manifest that
/// points anywhere else is refused before anything is downloaded.
const RELEASE_HOST: &str = "github.com";
const RELEASE_PATH: &str = "/JakubWawrzola/deskmate/releases/download/";
const MAX_ADDRESS_CHARS: usize = 512;
/// The installer is a few megabytes. A download past this size is not one.
pub const MAX_INSTALLER_BYTES: u64 = 64 * 1024 * 1024;
/// Longest run of digits accepted in one part of a version.
const MAX_VERSION_DIGITS: usize = 9;

/// A release version: `major.minor.patch`, with or without a pre-release tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
    pre_release: bool,
}

impl ReleaseVersion {
    /// "v1.2.3", "1.2" or "1.2.3-beta.1+build". Anything else is refused.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        let (text, build) = match text.split_once('+') {
            Some((version, build)) => (version, Some(build)),
            None => (text, None),
        };
        let (core, tag) = match text.split_once('-') {
            Some((core, tag)) => (core, Some(tag)),
            None => (text, None),
        };
        if !build.is_none_or(is_label) || !tag.is_none_or(is_label) {
            return None;
        }
        let mut parts = core.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        let patch = match parts.next() {
            Some(part) => number(part)?,
            None => 0,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre_release: tag.is_some(),
        })
    }

    /// "1.2.3", rebuilt from the numbers: nothing from a response ends up in
    /// the window, the log or a link as it was received.
    pub fn plain(self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    fn numbers(self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

/// Digits only: `str::parse` alone would also take "+1".
fn number(part: &str) -> Option<u64> {
    if part.is_empty()
        || part.len() > MAX_VERSION_DIGITS
        || !part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    part.parse().ok()
}

/// A pre-release tag or build label such as "beta.1".
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

/// Whether `offered` may replace `running`: only a strictly higher version.
///
/// The address Deskmate reads never carries a pre-release, so an offered
/// pre-release is refused whatever its number. A running pre-release counts as
/// lower than the release it leads to.
pub fn is_newer(running: &str, offered: &str) -> bool {
    let (Some(running), Some(offered)) = (
        ReleaseVersion::parse(running),
        ReleaseVersion::parse(offered),
    ) else {
        return false;
    };
    if offered.pre_release {
        return false;
    }
    match offered.numbers().cmp(&running.numbers()) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Equal => running.pre_release,
        std::cmp::Ordering::Less => false,
    }
}

/// Whether two version texts name the same release.
pub fn same_version(left: &str, right: &str) -> bool {
    match (ReleaseVersion::parse(left), ReleaseVersion::parse(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// Whether an installer may be downloaded from `address`:
/// `https://github.com/JakubWawrzola/deskmate/releases/download/<tag>/<file>`
/// and nothing else. No other host, repository, scheme or port, no user name,
/// query or fragment, and a tag and file name made of plain characters.
pub fn download_address_allowed(address: &str) -> bool {
    if address.len() > MAX_ADDRESS_CHARS {
        return false;
    }
    let Ok(url) = Url::parse(address) else {
        return false;
    };
    // The parser resolves "..", "%2e%2e" and backslashes. An address it had to
    // rewrite is not one Deskmate's release tooling wrote.
    if url.as_str() != address {
        return false;
    }
    if url.scheme() != "https"
        || url.host_str() != Some(RELEASE_HOST)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let Some(asset) = url.path().strip_prefix(RELEASE_PATH) else {
        return false;
    };
    let mut segments = asset.split('/');
    let (Some(tag), Some(file), None) = (segments.next(), segments.next(), segments.next()) else {
        return false;
    };
    is_plain_name(tag) && is_plain_name(file)
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Whether a download may go on. Neither the bytes received so far nor the
/// size the server announced may pass the limit.
pub fn download_size_allowed(received: u64, announced: Option<u64>) -> bool {
    received <= MAX_INSTALLER_BYTES && announced.is_none_or(|total| total <= MAX_INSTALLER_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNING: &str = "0.8.5";
    const X64: &str = "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/Deskmate_0.9.0_x64-setup.exe";

    #[test]
    fn parses_release_versions() {
        assert_eq!(
            ReleaseVersion::parse("v0.7.0").map(ReleaseVersion::plain),
            Some("0.7.0".into())
        );
        assert_eq!(
            ReleaseVersion::parse("0.8").map(ReleaseVersion::plain),
            Some("0.8.0".into())
        );
        assert_eq!(
            ReleaseVersion::parse(" V1.2.3-beta.1+build.7 ").map(ReleaseVersion::plain),
            Some("1.2.3".into())
        );
        for refused in [
            "",
            "latest",
            "1",
            "1.2.3.4",
            "vv1.2.3",
            "+1.2.3",
            "1.+2.3",
            "1.-2.3",
            "1.2.3-",
            "1.2.3+",
            "1.2.3-beta 1",
            "1.2.x",
            "1..3",
            "1.2.3\n4",
            "1.2.\u{0663}",
            "1.2.99999999999999999999",
        ] {
            assert_eq!(ReleaseVersion::parse(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn only_a_higher_version_is_newer() {
        for higher in ["0.8.6", "0.9.0", "1.0.0", "v0.8.6", "0.10.0", "0.9"] {
            assert!(is_newer(RUNNING, higher), "{higher}");
        }
        assert!(is_newer("0.9.9", "0.10.0"));
        for not_higher in ["0.8.5", "v0.8.5", "0.8.4", "0.7.9", "0.8", "0.0.0"] {
            assert!(!is_newer(RUNNING, not_higher), "{not_higher}");
        }
    }

    #[test]
    fn build_labels_do_not_make_a_version_newer() {
        assert!(!is_newer(RUNNING, "0.8.5+rebuilt"));
        assert!(is_newer(RUNNING, "0.8.6+rebuilt"));
    }

    #[test]
    fn a_pre_release_is_never_installed() {
        assert!(!is_newer(RUNNING, "0.9.0-beta.1"));
        assert!(!is_newer(RUNNING, "1.0.0-rc.1"));
        assert!(!is_newer(RUNNING, "0.8.5-beta"));
        assert!(!is_newer("0.9.0-beta.1", "0.9.0-beta.2"));
    }

    #[test]
    fn a_running_pre_release_moves_to_its_release() {
        assert!(is_newer("0.9.0-beta.1", "0.9.0"));
        assert!(is_newer("0.9.0-beta.1", "0.9.1"));
        assert!(!is_newer("0.9.0-beta.1", "0.8.9"));
    }

    #[test]
    fn a_version_that_cannot_be_read_is_never_newer() {
        assert!(!is_newer(RUNNING, "latest"));
        assert!(!is_newer(RUNNING, ""));
        assert!(!is_newer(RUNNING, "9.9.9.9"));
        assert!(!is_newer("unknown", "9.9.9"));
    }

    #[test]
    fn same_version_ignores_spelling_only() {
        assert!(same_version("0.9.0", "v0.9.0"));
        assert!(same_version("0.9", "0.9.0"));
        assert!(!same_version("0.9.0", "0.9.1"));
        assert!(!same_version("0.9.0-beta", "0.9.0"));
        assert!(!same_version("0.9.0", "latest"));
    }

    #[test]
    fn release_assets_are_allowed() {
        assert!(download_address_allowed(X64));
        assert!(download_address_allowed(
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/Deskmate_0.9.0_arm64-setup.exe"
        ));
    }

    #[test]
    fn other_hosts_are_refused() {
        for address in [
            "https://example.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://github.com.example.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://examplegithub.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://www.github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://api.github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://github.com./JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://GitHub.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://192.168.1.10/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://github.com:8443/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://github.com:443/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
        ] {
            assert!(!download_address_allowed(address), "{address}");
        }
    }

    #[test]
    fn other_repositories_are_refused() {
        for address in [
            "https://github.com/someone-else/deskmate/releases/download/v0.9.0/setup.exe",
            "https://github.com/JakubWawrzola/another/releases/download/v0.9.0/setup.exe",
            "https://github.com/JakubWawrzola/deskmate-fork/releases/download/v0.9.0/setup.exe",
            "https://github.com/jakubwawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/tag/v0.9.0",
            "https://github.com/JakubWawrzola/deskmate/archive/refs/tags/v0.9.0.zip",
            "https://github.com/JakubWawrzola/deskmate/releases/download/",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/",
            "https://github.com/JakubWawrzola/deskmate/releases/download//setup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/extra/setup.exe",
        ] {
            assert!(!download_address_allowed(address), "{address}");
        }
    }

    #[test]
    fn other_schemes_are_refused() {
        for address in [
            "http://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "ftp://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "file:///C:/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "//github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
        ] {
            assert!(!download_address_allowed(address), "{address}");
        }
    }

    #[test]
    fn user_names_in_the_address_are_refused() {
        for address in [
            "https://github.com@example.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://github.com:443@example.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://user@github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://user:secret@github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://example.com\\@github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
        ] {
            assert!(!download_address_allowed(address), "{address}");
        }
    }

    #[test]
    fn path_tricks_are_refused() {
        for address in [
            "https://github.com/JakubWawrzola/deskmate/releases/download/../../../../someone-else/tool/releases/download/v1/setup.exe",
            "https://github.com/someone-else/tool/../../JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/../setup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/%2e%2e/setup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/%2E%2E/%2E%2E/setup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/..%2Fsetup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0%2Fsetup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0\\setup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe?x=1",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe#x",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/set up.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe\n",
            " https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe",
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/setup.exe;x",
        ] {
            assert!(!download_address_allowed(address), "{address}");
        }
    }

    #[test]
    fn an_overlong_address_is_refused() {
        let address = format!(
            "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/{}.exe",
            "a".repeat(MAX_ADDRESS_CHARS)
        );
        assert!(!download_address_allowed(&address));
    }

    #[test]
    fn the_download_stops_at_the_size_limit() {
        assert!(download_size_allowed(0, None));
        assert!(download_size_allowed(
            3 * 1024 * 1024,
            Some(3 * 1024 * 1024)
        ));
        assert!(download_size_allowed(
            MAX_INSTALLER_BYTES,
            Some(MAX_INSTALLER_BYTES)
        ));
        assert!(!download_size_allowed(MAX_INSTALLER_BYTES + 1, None));
        assert!(!download_size_allowed(0, Some(MAX_INSTALLER_BYTES + 1)));
        assert!(!download_size_allowed(u64::MAX, Some(1)));
    }
}
