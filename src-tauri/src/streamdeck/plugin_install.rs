//! Copies the Stream Deck plugin that ships with the app into Stream Deck's
//! plugin folder, and removes it again.
//!
//! Every path is built from two known folders: the app's resources and
//! `%AppData%\Elgato\StreamDeck`. Nothing here takes a path from the window.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The plugin's folder inside Stream Deck's `Plugins` folder.
pub const PLUGIN_FOLDER: &str = "com.deskmate.streamdeck.sdPlugin";
/// The folder of the plugin inside the app's resources.
pub const BUNDLE_FOLDER: &str = "streamdeck-plugin";

/// The folders of the plugin that are copied. `logs`, which Stream Deck
/// creates next to them, is not among them.
const COPIED_FOLDERS: [&str; 3] = ["bin", "imgs", "ui"];
/// The plugin's code. Without it the resources hold no built plugin.
const CODE_FILE: &str = "bin/plugin.js";
/// The manifest travels in the resources under its repository name and gets
/// the name Stream Deck expects when it is installed.
const BUNDLED_MANIFEST: &str = "manifest.template.json";
const INSTALLED_MANIFEST: &str = "manifest.json";
/// What the running plugin writes. Kept across an update.
const LOGS_FOLDER: &str = "logs";

/// Work folders next to the plugin. Neither name ends in `.sdPlugin`, so
/// Stream Deck does not take them for plugins.
const STAGING_FOLDER: &str = "com.deskmate.streamdeck.installing";
const PREVIOUS_FOLDER: &str = "com.deskmate.streamdeck.previous";

/// A manifest larger than this is not read.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Parts of a version, and digits of one part, that count as a version.
const MAX_VERSION_PARTS: usize = 4;
const MAX_VERSION_PART_DIGITS: usize = 5;

const QUIT_STREAM_DECK: &str = "Stream Deck is using the installed plugin. Quit Stream Deck \
     (right-click its icon next to the clock, then Quit Stream Deck) and try again.";

/// Stream Deck's own folder in the user's profile.
#[derive(Debug, Clone)]
pub struct StreamDeckFolders {
    root: PathBuf,
}

impl StreamDeckFolders {
    /// `%AppData%\Elgato\StreamDeck` of the current user.
    pub fn of_current_user() -> Option<Self> {
        std::env::var_os("APPDATA")
            .filter(|appdata| !appdata.is_empty())
            .map(|appdata| Self::under(Path::new(&appdata)))
    }

    /// Stream Deck's folder below the given roaming application data folder.
    pub fn under(appdata: &Path) -> Self {
        Self {
            root: appdata.join("Elgato").join("StreamDeck"),
        }
    }

    /// The folder exists: Stream Deck has been installed and run.
    pub fn found(&self) -> bool {
        self.root.is_dir()
    }

    fn plugins(&self) -> PathBuf {
        self.root.join("Plugins")
    }

    pub fn plugin(&self) -> PathBuf {
        self.plugins().join(PLUGIN_FOLDER)
    }

    fn staging(&self) -> PathBuf {
        self.plugins().join(STAGING_FOLDER)
    }

    fn previous(&self) -> PathBuf {
        self.plugins().join(PREVIOUS_FOLDER)
    }
}

/// What the Settings panel shows about the plugin.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PluginStatus {
    /// Stream Deck's folder exists on this computer.
    pub stream_deck_found: bool,
    /// Version of the plugin that ships with this app. None: this build of
    /// the app carries no plugin.
    pub bundled_version: Option<String>,
    pub installed: bool,
    /// None also when the installed copy has no readable version.
    pub installed_version: Option<String>,
    /// Installing now would replace the installed copy with a newer one, or
    /// repair one whose version cannot be read.
    pub update_available: bool,
}

/// Whether an install put the plugin there for the first time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    New,
    Replaced,
}

pub fn status(bundle: &Path, folders: Option<&StreamDeckFolders>) -> PluginStatus {
    let bundled = bundled_version(bundle);
    let folders = folders.filter(|folders| folders.found());
    let plugin = folders.map(StreamDeckFolders::plugin);
    let installed = plugin.as_deref().is_some_and(Path::is_dir);
    let installed_version = plugin
        .filter(|_| installed)
        .and_then(|plugin| read_version(&plugin.join(INSTALLED_MANIFEST)));
    let update_available = installed
        && match (&bundled, &installed_version) {
            (Some(bundled), Some(installed)) => is_newer(bundled, installed),
            (Some(_), None) => true,
            (None, _) => false,
        };
    PluginStatus {
        stream_deck_found: folders.is_some(),
        bundled_version: bundled.as_deref().map(display_version),
        installed,
        installed_version: installed_version.as_deref().map(display_version),
        update_available,
    }
}

/// The version of the plugin in the app's resources, when it is complete.
fn bundled_version(bundle: &Path) -> Option<String> {
    if !bundle.join(CODE_FILE).is_file() {
        return None;
    }
    read_version(&bundle.join(BUNDLED_MANIFEST))
}

/// The `Version` of a plugin manifest. The file is data like any other: a
/// value that is not a plain dotted number is not passed on.
fn read_version(manifest: &Path) -> Option<String> {
    if fs::metadata(manifest).ok()?.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    let text = fs::read_to_string(manifest).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&text).ok()?;
    let version = parsed.get("Version")?.as_str()?;
    version_parts(version).map(|_| version.to_string())
}

fn version_parts(version: &str) -> Option<Vec<u32>> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() < 2 || parts.len() > MAX_VERSION_PARTS {
        return None;
    }
    parts
        .into_iter()
        .map(|part| {
            let is_number = !part.is_empty()
                && part.len() <= MAX_VERSION_PART_DIGITS
                && part.chars().all(|c| c.is_ascii_digit());
            is_number.then(|| part.parse().ok()).flatten()
        })
        .collect()
}

fn is_newer(candidate: &str, than: &str) -> bool {
    let padded = |version: &str| {
        let mut parts = version_parts(version).unwrap_or_default();
        parts.resize(MAX_VERSION_PARTS, 0);
        parts
    };
    padded(candidate) > padded(than)
}

/// The manifest carries four parts; the fourth is a build number that is 0
/// for every release, so it is not shown.
fn display_version(version: &str) -> String {
    match version.rsplit_once('.') {
        Some((release, "0")) if release.split('.').count() == MAX_VERSION_PARTS - 1 => {
            release.to_string()
        }
        _ => version.to_string(),
    }
}

/// Installs the plugin from the app's resources, replacing an installed
/// copy. The new copy is written next to the old one first; only a complete
/// copy takes its place, so a copy that fails leaves the old plugin as it
/// was.
pub fn install(bundle: &Path, folders: &StreamDeckFolders) -> Result<Installed, String> {
    if bundled_version(bundle).is_none() {
        return Err("This build of Deskmate does not contain the Stream Deck plugin.".into());
    }
    if !folders.found() {
        return Err(
            "Stream Deck is not installed for this Windows account, or has never been \
             started. Install and start Stream Deck, then try again."
                .into(),
        );
    }
    let plugin = folders.plugin();
    let staging = folders.staging();
    let previous = folders.previous();
    fs::create_dir_all(folders.plugins()).map_err(|e| cannot("prepare the plugin folder", &e))?;
    recover_interrupted_install(&plugin, &previous)?;
    remove_folder(&staging).map_err(|e| cannot("clear an earlier attempt", &e))?;

    if let Err(error) = stage(bundle, &staging) {
        let _ = remove_folder(&staging);
        return Err(cannot("copy the plugin", &error));
    }

    let replaced = plugin.exists();
    if replaced {
        if fs::rename(&plugin, &previous).is_err() {
            let _ = remove_folder(&staging);
            return Err(QUIT_STREAM_DECK.into());
        }
        // The log of the installed plugin stays with it.
        let _ = fs::rename(previous.join(LOGS_FOLDER), staging.join(LOGS_FOLDER));
    }
    if let Err(error) = fs::rename(&staging, &plugin) {
        if replaced {
            let _ = fs::rename(staging.join(LOGS_FOLDER), previous.join(LOGS_FOLDER));
            let _ = fs::rename(&previous, &plugin);
        }
        let _ = remove_folder(&staging);
        return Err(cannot("put the plugin in place", &error));
    }
    // The old copy is no longer needed; a leftover is cleared next time.
    let _ = remove_folder(&previous);
    Ok(if replaced {
        Installed::Replaced
    } else {
        Installed::New
    })
}

/// An install that was cut off after it moved the old copy aside, and before
/// the new one took its place, left no plugin. The old copy goes back.
fn recover_interrupted_install(plugin: &Path, previous: &Path) -> Result<(), String> {
    if !previous.exists() {
        return Ok(());
    }
    if plugin.exists() {
        remove_folder(previous).map_err(|e| cannot("clear an earlier attempt", &e))
    } else {
        fs::rename(previous, plugin).map_err(|e| cannot("restore the installed plugin", &e))
    }
}

/// Writes a complete copy of the plugin into `staging`.
fn stage(bundle: &Path, staging: &Path) -> io::Result<()> {
    fs::create_dir(staging)?;
    for folder in COPIED_FOLDERS {
        copy_folder(&bundle.join(folder), &staging.join(folder))?;
    }
    fs::copy(
        bundle.join(BUNDLED_MANIFEST),
        staging.join(INSTALLED_MANIFEST),
    )?;
    Ok(())
}

/// Copies the files and folders below `from`. Anything else, a link for
/// one, is not followed and not copied.
fn copy_folder(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Removes the installed plugin. `false`: there was none.
pub fn remove(folders: &StreamDeckFolders) -> Result<bool, String> {
    let plugin = folders.plugin();
    if !plugin.exists() {
        return Ok(false);
    }
    let previous = folders.previous();
    remove_folder(&previous).map_err(|e| cannot("clear an earlier attempt", &e))?;
    // Moved aside first: that either works as a whole or, while Stream Deck
    // runs the plugin, not at all. Deleting in place could stop halfway.
    if fs::rename(&plugin, &previous).is_err() {
        return Err(QUIT_STREAM_DECK.into());
    }
    remove_folder(&previous).map_err(|e| cannot("delete the plugin", &e))?;
    Ok(true)
}

fn remove_folder(folder: &Path) -> io::Result<()> {
    match fs::remove_dir_all(folder) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn cannot(step: &str, error: &io::Error) -> String {
    format!("Deskmate could not {step}: {error}")
}
