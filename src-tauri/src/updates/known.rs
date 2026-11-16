//! What a new check keeps of the release that was known before it. No I/O,
//! so every case has a test: `H` stands for the updater plugin's handle.

use super::policy::ReleaseVersion;
use super::{UpdateInfo, UpdatePhase};

/// The release and its installer after a check that found `found`.
/// `manifest_failed`: the check could not read the release manifest, or
/// refused it, and took the version from GitHub's release API instead.
pub(super) fn after_check<H>(
    known: Option<UpdateInfo>,
    known_installer: Option<H>,
    found: Option<(ReleaseVersion, Option<H>)>,
    manifest_failed: bool,
) -> (Option<UpdateInfo>, Option<H>) {
    // A download that is running keeps the release it started with.
    if known.as_ref().is_some_and(UpdateInfo::in_progress) {
        return (known, known_installer);
    }
    let Some((version, installer)) = found else {
        return (None, None);
    };
    let version_text = version.plain();
    let known = known.filter(|known| known.version == version_text);
    // A manifest that could not be read this time, after a timeout or an
    // error from GitHub, does not take back the installer an earlier check
    // found for the same version.
    let installer = match installer {
        None if manifest_failed && known.is_some() => known_installer,
        installer => installer,
    };
    let mut info = UpdateInfo::found(version, installer.is_some());
    // An install of this version that failed in this run stays failed, so
    // that the `auto` mode does not try it again by itself: that waits for a
    // click on "Try again".
    if let Some(known) = known.filter(|known| known.phase == UpdatePhase::Failed) {
        info.phase = UpdatePhase::Failed;
        info.error = known.error;
    }
    (Some(info), installer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> ReleaseVersion {
        ReleaseVersion::parse(text).unwrap()
    }

    fn failed(text: &str) -> UpdateInfo {
        let mut info = UpdateInfo::found(version(text), true);
        info.phase = UpdatePhase::Failed;
        info.error = Some("The download did not finish.".into());
        info
    }

    #[test]
    fn a_failed_install_stays_failed_when_the_check_finds_it_again() {
        let (info, installer) = after_check(
            Some(failed("0.9.1")),
            Some("yesterday"),
            Some((version("0.9.1"), Some("today"))),
            false,
        );
        let info = info.unwrap();
        assert_eq!(info.phase, UpdatePhase::Failed);
        assert_eq!(info.error.as_deref(), Some("The download did not finish."));
        assert!(info.installable);
        assert!(!info.auto_install);
        assert_eq!(installer, Some("today"));
    }

    #[test]
    fn a_newer_release_starts_without_the_old_failure() {
        let (info, installer) = after_check(
            Some(failed("0.9.1")),
            Some("old"),
            Some((version("0.9.2"), Some("new"))),
            false,
        );
        let info = info.unwrap();
        assert_eq!(info.version, "0.9.2");
        assert_eq!(info.phase, UpdatePhase::Available);
        assert_eq!(info.error, None);
        assert_eq!(installer, Some("new"));
    }

    #[test]
    fn a_running_install_keeps_its_release() {
        let mut running = UpdateInfo::found(version("0.9.1"), true);
        running.phase = UpdatePhase::Downloading;
        let (info, installer) = after_check(
            Some(running.clone()),
            Some("running"),
            Some((version("0.9.2"), Some("new"))),
            false,
        );
        assert_eq!(info, Some(running));
        assert_eq!(installer, Some("running"));
    }

    #[test]
    fn nothing_newer_forgets_the_release() {
        let (info, installer) = after_check(Some(failed("0.9.1")), Some("old"), None, false);
        assert_eq!(info, None);
        assert_eq!(installer, None);
    }

    #[test]
    fn a_manifest_that_cannot_be_read_keeps_the_installer_of_the_same_version() {
        let known = UpdateInfo::found(version("0.9.1"), true);
        let (info, installer) = after_check(
            Some(known),
            Some("yesterday"),
            Some((version("0.9.1"), None)),
            true,
        );
        assert!(info.unwrap().installable);
        assert_eq!(installer, Some("yesterday"));
    }

    #[test]
    fn a_new_version_without_a_manifest_is_only_announced() {
        let known = UpdateInfo::found(version("0.9.1"), true);
        let (info, installer) = after_check(
            Some(known),
            Some("yesterday"),
            Some((version("0.9.2"), None)),
            true,
        );
        let info = info.unwrap();
        assert_eq!(info.version, "0.9.2");
        assert!(!info.installable);
        assert_eq!(installer, None);
    }

    #[test]
    fn the_release_api_alone_offers_no_installer() {
        // `notify` mode reads no manifest on purpose.
        let known = UpdateInfo::found(version("0.9.1"), true);
        let (info, installer) = after_check(
            Some(known),
            Some("yesterday"),
            Some((version("0.9.1"), None)),
            false,
        );
        assert!(!info.unwrap().installable);
        assert_eq!(installer, None);
    }
}
