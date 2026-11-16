use std::path::Path;

/// The Stream Deck plugin's folder, as `npm run build` in `streamdeck-plugin/`
/// fills it. `tauri.conf.json` ships its `bin`, `imgs` and `ui` folders as
/// resources, next to the plugin's manifest template.
const PLUGIN_FOLDER: &str = "../streamdeck-plugin/com.deskmate.streamdeck.sdPlugin";
/// What the plugin's build writes into that folder. The rest of the folder is
/// in the repository.
const PLUGIN_BUILD_OUTPUT: [&str; 2] = ["bin/plugin.js", "bin/package.json"];
/// The folder tauri.conf.json lists as a resource. A release build fills it
/// with deskmate-service.exe (scripts/build-service.mjs) before this runs.
const SERVICE_BUNDLE_DIR: &str = "service-bin";

fn main() {
    // Only the Windows installer carries the plugin (tauri.linux.conf.json
    // leaves it out of the Linux packages).
    let windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    if windows {
        if let Err(message) = check_stream_deck_plugin() {
            // Fails the build with this message.
            println!("cargo::error={message}");
            return;
        }
    }
    // Tauri refuses a resource folder that does not exist and skips an empty
    // one, so `cargo check`, `cargo test` and `tauri dev` work without a
    // built service.
    if let Err(error) = std::fs::create_dir_all(SERVICE_BUNDLE_DIR) {
        println!("cargo:warning=cannot create {SERVICE_BUNDLE_DIR}: {error}");
    }
    tauri_build::build()
}

/// An installer has to carry the built plugin: `tauri build` fails without
/// it. `tauri dev`, `cargo check` and `cargo test` do not need the plugin and
/// must work in a tree where it was never built; the app then says that it
/// carries no plugin.
fn check_stream_deck_plugin() -> Result<(), String> {
    let plugin = Path::new(PLUGIN_FOLDER);
    let code = plugin.join("bin");
    println!("cargo:rerun-if-changed={}", code.display());
    if PLUGIN_BUILD_OUTPUT
        .iter()
        .all(|file| plugin.join(file).is_file())
    {
        return Ok(());
    }
    // False only under `tauri build`, which compiles the app as it is shipped.
    if !tauri_build::is_dev() {
        return Err(
            "the Stream Deck plugin is not built, so the installer would ship without it. \
             Run `npm run build:streamdeck` in the repository root, or `npm ci` and \
             `npm run build` in streamdeck-plugin/, then build again"
                .into(),
        );
    }
    // Tauri refuses a resource folder that does not exist; an empty one is
    // fine and ships nothing.
    std::fs::create_dir_all(&code)
        .map_err(|error| format!("cannot create {}: {error}", code.display()))
}
