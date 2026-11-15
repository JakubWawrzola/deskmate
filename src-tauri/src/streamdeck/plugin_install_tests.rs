//! Installing and removing the plugin, in a temporary folder that stands in
//! for the user's application data. The real Stream Deck folder is never
//! touched.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::plugin_install::{install, remove, status, Installed, StreamDeckFolders, PLUGIN_FOLDER};

static NEXT_FOLDER: AtomicUsize = AtomicUsize::new(0);

/// A folder of its own for one test, deleted afterwards.
struct TestFolder(PathBuf);

impl TestFolder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "deskmate-streamdeck-test-{}-{}",
            std::process::id(),
            NEXT_FOLDER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// The plugin as it ships in the app's resources.
    fn bundle(&self, version: &str) -> PathBuf {
        let bundle = self.0.join("resources").join("streamdeck-plugin");
        write(
            &bundle.join("bin/plugin.js"),
            &format!("// plugin {version}"),
        );
        write(&bundle.join("bin/package.json"), "{ \"type\": \"module\" }");
        write(&bundle.join("imgs/plugin/marketplace.png"), "png");
        write(&bundle.join("imgs/actions/toggle/on.png"), "png");
        write(&bundle.join("ui/toggle-entity.html"), "<!doctype html>");
        write(&bundle.join("manifest.template.json"), &manifest(version));
        bundle
    }

    /// The application data folder, with or without Stream Deck in it.
    fn appdata(&self, with_stream_deck: bool) -> StreamDeckFolders {
        let appdata = self.0.join("AppData").join("Roaming");
        fs::create_dir_all(&appdata).unwrap();
        if with_stream_deck {
            fs::create_dir_all(appdata.join("Elgato/StreamDeck/Plugins")).unwrap();
        }
        StreamDeckFolders::under(&appdata)
    }

    fn plugins(&self) -> PathBuf {
        self.0.join("AppData/Roaming/Elgato/StreamDeck/Plugins")
    }
}

impl Drop for TestFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn manifest(version: &str) -> String {
    format!("{{\"Name\": \"Deskmate\", \"Version\": \"{version}\", \"UUID\": \"x\"}}")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// The names in a folder, sorted.
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_plugin_goes_into_stream_decks_plugin_folder() {
    let folders = StreamDeckFolders::under(Path::new(r"C:\Users\me\AppData\Roaming"));
    assert_eq!(
        folders.plugin(),
        Path::new(
            r"C:\Users\me\AppData\Roaming\Elgato\StreamDeck\Plugins\com.deskmate.streamdeck.sdPlugin"
        )
    );
}

#[test]
fn a_first_install_copies_the_plugin_and_names_the_manifest() {
    let test = TestFolder::new();
    let bundle = test.bundle("0.2.0.0");
    // What a development build may leave next to the plugin's folders.
    write(&bundle.join("logs/plugin.0.log"), "a log");
    write(&bundle.join("manifest.json"), "not the shipped manifest");
    let folders = test.appdata(true);

    assert_eq!(install(&bundle, &folders), Ok(Installed::New));

    let plugin = folders.plugin();
    assert_eq!(names(&plugin), ["bin", "imgs", "manifest.json", "ui"]);
    assert_eq!(read(&plugin.join("manifest.json")), manifest("0.2.0.0"));
    assert_eq!(read(&plugin.join("bin/plugin.js")), "// plugin 0.2.0.0");
    assert!(plugin.join("bin/package.json").is_file());
    assert!(plugin.join("imgs/actions/toggle/on.png").is_file());
    assert!(plugin.join("ui/toggle-entity.html").is_file());
    // Nothing but the plugin is left in the folder.
    assert_eq!(names(&test.plugins()), [PLUGIN_FOLDER]);
}

#[test]
fn nothing_is_copied_when_stream_deck_is_not_there() {
    let test = TestFolder::new();
    let bundle = test.bundle("0.2.0.0");
    let folders = test.appdata(false);

    let error = install(&bundle, &folders).unwrap_err();
    assert!(error.contains("Stream Deck is not installed"), "{error}");
    assert!(names(&test.0.join("AppData/Roaming")).is_empty());
    assert_eq!(remove(&folders), Ok(false));
}

#[test]
fn a_build_without_the_plugin_installs_nothing() {
    let test = TestFolder::new();
    let bundle = test.bundle("0.2.0.0");
    fs::remove_file(bundle.join("bin/plugin.js")).unwrap();
    let folders = test.appdata(true);

    let error = install(&bundle, &folders).unwrap_err();
    assert!(error.contains("does not contain"), "{error}");
    assert!(names(&test.plugins()).is_empty());

    let missing = test.0.join("no-such-resources");
    assert!(install(&missing, &folders).is_err());
    assert!(names(&test.plugins()).is_empty());
}

#[test]
fn an_update_replaces_the_old_copy_and_keeps_its_log() {
    let test = TestFolder::new();
    let folders = test.appdata(true);
    assert_eq!(
        install(&test.bundle("0.1.0.0"), &folders),
        Ok(Installed::New)
    );
    let plugin = folders.plugin();
    write(&plugin.join("bin/left-over.js"), "from the old version");
    write(&plugin.join("logs/plugin.0.log"), "the old log");

    assert_eq!(
        install(&test.bundle("0.2.0.0"), &folders),
        Ok(Installed::Replaced)
    );

    assert_eq!(read(&plugin.join("bin/plugin.js")), "// plugin 0.2.0.0");
    assert_eq!(read(&plugin.join("manifest.json")), manifest("0.2.0.0"));
    assert!(!plugin.join("bin/left-over.js").exists());
    assert_eq!(read(&plugin.join("logs/plugin.0.log")), "the old log");
    assert_eq!(names(&test.plugins()), [PLUGIN_FOLDER]);
}

#[test]
fn a_copy_that_fails_leaves_the_old_plugin_as_it_was() {
    let test = TestFolder::new();
    let folders = test.appdata(true);
    assert_eq!(
        install(&test.bundle("0.1.0.0"), &folders),
        Ok(Installed::New)
    );

    // The new plugin lacks a folder: the copy stops partway.
    let bundle = test.bundle("0.2.0.0");
    fs::remove_dir_all(bundle.join("ui")).unwrap();
    let error = install(&bundle, &folders).unwrap_err();
    assert!(error.contains("could not copy the plugin"), "{error}");

    let plugin = folders.plugin();
    assert_eq!(read(&plugin.join("bin/plugin.js")), "// plugin 0.1.0.0");
    assert_eq!(read(&plugin.join("manifest.json")), manifest("0.1.0.0"));
    assert!(plugin.join("ui/toggle-entity.html").is_file());
    assert_eq!(names(&test.plugins()), [PLUGIN_FOLDER]);
}

/// Stream Deck runs the plugin with files of its folder open, which on
/// Windows keeps the folder from being moved.
#[cfg(windows)]
#[test]
fn a_plugin_in_use_is_neither_replaced_nor_removed() {
    let test = TestFolder::new();
    let folders = test.appdata(true);
    assert_eq!(
        install(&test.bundle("0.1.0.0"), &folders),
        Ok(Installed::New)
    );
    let plugin = folders.plugin();
    write(&plugin.join("logs/plugin.0.log"), "the log");
    let in_use = fs::File::open(plugin.join("logs/plugin.0.log")).unwrap();

    let error = install(&test.bundle("0.2.0.0"), &folders).unwrap_err();
    assert!(error.contains("Quit Stream Deck"), "{error}");
    let error = remove(&folders).unwrap_err();
    assert!(error.contains("Quit Stream Deck"), "{error}");

    assert_eq!(read(&plugin.join("bin/plugin.js")), "// plugin 0.1.0.0");
    assert_eq!(read(&plugin.join("logs/plugin.0.log")), "the log");
    assert_eq!(names(&test.plugins()), [PLUGIN_FOLDER]);

    drop(in_use);
    assert_eq!(
        install(&test.bundle("0.2.0.0"), &folders),
        Ok(Installed::Replaced)
    );
    assert_eq!(read(&plugin.join("bin/plugin.js")), "// plugin 0.2.0.0");
}

#[test]
fn an_install_that_was_cut_off_is_put_right_by_the_next_one() {
    let test = TestFolder::new();
    let folders = test.appdata(true);
    assert_eq!(
        install(&test.bundle("0.1.0.0"), &folders),
        Ok(Installed::New)
    );
    let plugin = folders.plugin();
    // As after a crash between moving the old copy aside and placing the new
    // one, with a half-written new copy lying around.
    fs::rename(
        &plugin,
        test.plugins().join("com.deskmate.streamdeck.previous"),
    )
    .unwrap();
    write(
        &test
            .plugins()
            .join("com.deskmate.streamdeck.installing/bin/plugin.js"),
        "half",
    );

    assert_eq!(
        install(&test.bundle("0.2.0.0"), &folders),
        Ok(Installed::Replaced)
    );
    assert_eq!(read(&plugin.join("bin/plugin.js")), "// plugin 0.2.0.0");
    assert_eq!(names(&test.plugins()), [PLUGIN_FOLDER]);
}

#[test]
fn remove_deletes_the_plugin_and_only_the_plugin() {
    let test = TestFolder::new();
    let folders = test.appdata(true);
    assert_eq!(
        install(&test.bundle("0.2.0.0"), &folders),
        Ok(Installed::New)
    );
    write(
        &test
            .plugins()
            .join("com.other.plugin.sdPlugin/manifest.json"),
        "{}",
    );

    assert_eq!(remove(&folders), Ok(true));
    assert_eq!(names(&test.plugins()), ["com.other.plugin.sdPlugin"]);
    assert_eq!(remove(&folders), Ok(false));
}

#[test]
fn the_status_names_what_is_bundled_and_what_is_installed() {
    let test = TestFolder::new();
    let bundle = test.bundle("0.2.0.0");

    let without_stream_deck = status(&bundle, Some(&test.appdata(false)));
    assert!(!without_stream_deck.stream_deck_found);
    assert!(!without_stream_deck.installed);
    assert_eq!(
        without_stream_deck.bundled_version.as_deref(),
        Some("0.2.0")
    );
    assert!(!status(&bundle, None).stream_deck_found);

    let folders = test.appdata(true);
    let not_installed = status(&bundle, Some(&folders));
    assert!(not_installed.stream_deck_found);
    assert!(!not_installed.installed);
    assert_eq!(not_installed.installed_version, None);
    assert!(!not_installed.update_available);

    install(&bundle, &folders).unwrap();
    let current = status(&bundle, Some(&folders));
    assert!(current.installed);
    assert_eq!(current.installed_version.as_deref(), Some("0.2.0"));
    assert!(!current.update_available);
}

#[test]
fn an_update_is_offered_for_an_older_or_unreadable_copy_only() {
    let test = TestFolder::new();
    let bundle = test.bundle("0.2.0.0");
    let folders = test.appdata(true);
    install(&bundle, &folders).unwrap();
    let installed_manifest = folders.plugin().join("manifest.json");
    let offered = |installed: &str| {
        fs::write(&installed_manifest, installed).unwrap();
        status(&bundle, Some(&folders)).update_available
    };

    assert!(offered(&manifest("0.1.9.0")));
    assert!(offered(&manifest("0.1.0")));
    assert!(!offered(&manifest("0.2.0.0")));
    assert!(!offered(&manifest("0.2.0")));
    assert!(!offered(&manifest("0.10.0.0")));
    assert!(!offered(&manifest("1.0.0.0")));
    // A copy whose version cannot be read can be repaired.
    assert!(offered("not json"));
    assert!(offered("{}"));

    // A build that carries no plugin has nothing to offer.
    fs::remove_file(bundle.join("bin/plugin.js")).unwrap();
    let nothing_bundled = status(&bundle, Some(&folders));
    assert_eq!(nothing_bundled.bundled_version, None);
    assert!(nothing_bundled.installed);
    assert!(!nothing_bundled.update_available);
}

#[test]
fn a_version_that_is_not_a_dotted_number_is_not_shown() {
    let test = TestFolder::new();
    let bundle = test.bundle("0.2.0.0");
    let folders = test.appdata(true);
    install(&bundle, &folders).unwrap();
    let installed_manifest = folders.plugin().join("manifest.json");
    let shown = |version: &str| {
        fs::write(&installed_manifest, manifest(version)).unwrap();
        status(&bundle, Some(&folders)).installed_version
    };

    assert_eq!(shown("1.2.3.4").as_deref(), Some("1.2.3.4"));
    assert_eq!(shown("1.2.3").as_deref(), Some("1.2.3"));
    assert_eq!(shown("1.2").as_deref(), Some("1.2"));
    for version in [
        "",
        "1",
        "v1.2.3",
        "1.2.3.4.5",
        "1..2",
        "1.2.x",
        "1.2.3 <b>",
        "1.2.-3",
        "1.2.1234567",
    ] {
        assert_eq!(shown(version), None, "{version:?}");
    }
    fs::write(&installed_manifest, "{\"Version\": 2}").unwrap();
    assert_eq!(status(&bundle, Some(&folders)).installed_version, None);
}
