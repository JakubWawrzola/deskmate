//! HA notifications -> Windows toast (image + action buttons).
//! HA publishes JSON to `deskmate/<node>/notify`:
//! {"title":"Dishwasher","message":"Ready to unload","image":"https://...",
//!  "actions":[{"title":"OK","action":"ok"},{"title":"Snooze","action":"snooze"}]}
//! A clicked button publishes `{action}` to `deskmate/<node>/notify/action`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Whether the branded AUMID (Start Menu shortcut) was set up successfully. If not,
/// toasts carry the PowerShell AUMID, which always renders (only the label differs).
static BRANDED: AtomicBool = AtomicBool::new(false);
static TEMP_IMAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Prefix of downloaded toast images in %TEMP%.
const TEMP_IMAGE_PREFIX: &str = "deskmate_toast_";
/// Toast images (often camera snapshots) are removed this long after display.
const TEMP_IMAGE_TTL: Duration = Duration::from_secs(10 * 60);

/// Toast button tokens: token -> (action name, issued at). Any web page or
/// program can launch `deskmate:action?name=...`, so a click only counts when
/// it carries a token this process put on a toast it actually showed.
static ACTION_TOKENS: OnceLock<Mutex<HashMap<String, (String, Instant)>>> = OnceLock::new();
const ACTION_TOKEN_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_ACTION_TOKENS: usize = 256;

fn action_tokens() -> &'static Mutex<HashMap<String, (String, Instant)>> {
    ACTION_TOKENS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Issues a single-use token for one toast button.
#[cfg(windows)]
fn issue_action_token(action: &str) -> String {
    use rand::RngCore;
    let mut raw = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    if let Ok(mut tokens) = action_tokens().lock() {
        tokens.retain(|_, (_, issued)| issued.elapsed() < ACTION_TOKEN_TTL);
        if tokens.len() >= MAX_ACTION_TOKENS {
            if let Some(oldest) = tokens
                .iter()
                .min_by_key(|(_, (_, issued))| *issued)
                .map(|(key, _)| key.clone())
            {
                tokens.remove(&oldest);
            }
        }
        tokens.insert(token.clone(), (action.to_string(), Instant::now()));
    }
    token
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotifyAction {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub action: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotifyPayload {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub actions: Vec<NotifyAction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotifyRecord {
    pub title: String,
    pub message: String,
    pub image: Option<String>,
    /// local time hh:mm:ss (for the UI list)
    pub received_at: String,
}

/// Parses the payload; tolerates plain text (message without JSON).
pub fn parse(payload: &str) -> NotifyPayload {
    match serde_json::from_str::<NotifyPayload>(payload) {
        Ok(mut p) => {
            // Windows toasts only have room for a small number of actions. Limits
            // also keep an untrusted MQTT payload from creating an oversized XML/
            // PowerShell command line in the fallback renderer.
            p.title = p.title.chars().take(120).collect();
            p.message = p.message.chars().take(500).collect();
            p.actions.truncate(5);
            for action in &mut p.actions {
                action.title = action.title.chars().take(80).collect();
                action.action = action.action.chars().take(128).collect();
            }
            p
        }
        Err(_) => NotifyPayload {
            title: crate::consts::APP_NAME.into(),
            message: payload.trim().chars().take(500).collect(),
            image: None,
            actions: Vec::new(),
        },
    }
}

/// Downloads an image to a temp file (the toast API needs a local path).
fn fetch_image(url: &str) -> Option<std::path::PathBuf> {
    let parsed = crate::sys_commands::parse_web_url(url).ok()?;
    use std::io::Read;
    let connector = native_tls::TlsConnector::new().ok()?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(std::sync::Arc::new(connector))
        .timeout(std::time::Duration::from_secs(10))
        // Redirects would bypass the origin allowlist checked before this call.
        .redirects(0)
        .build();
    let resp = agent.get(parsed.as_str()).call().ok()?;
    if (300..400).contains(&resp.status()) {
        return None;
    }
    let mut buf = Vec::with_capacity(256 * 1024);
    resp.into_reader()
        .take(5 * 1024 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.is_empty() {
        return None;
    }
    let ext = if parsed.path().to_ascii_lowercase().ends_with(".jpg") || parsed.path().to_ascii_lowercase().ends_with(".jpeg") {
        "jpg"
    } else {
        "png"
    };
    let sequence = TEMP_IMAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "{TEMP_IMAGE_PREFIX}{}_{}.{}",
        std::process::id(),
        sequence,
        ext
    ));
    std::fs::write(&path, &buf).ok()?;
    Some(path)
}

/// Deletes a downloaded toast image once the toast no longer needs it.
fn schedule_image_removal(path: std::path::PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(TEMP_IMAGE_TTL);
        let _ = std::fs::remove_file(path);
    });
}

/// Removes toast images left behind by an earlier run (crash, shutdown before
/// the timer fired). They used to accumulate in %TEMP% indefinitely.
pub fn cleanup_temp_images() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(TEMP_IMAGE_PREFIX)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Registers the AUMID in HKCU: DisplayName (the toast source label) + IconUri.
#[cfg(windows)]
pub fn ensure_aumid_registered() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = format!("Software\\Classes\\AppUserModelId\\{}", crate::consts::TOAST_AUMID);
    let exe = std::env::current_exe().ok();
    if let Ok((key, _)) = hkcu.create_subkey(&path) {
        let _ = key.set_value("DisplayName", &crate::consts::TOAST_DISPLAY_NAME);
        if let Some(exe) = &exe {
            let _ = key.set_value("IconUri", &exe.to_string_lossy().to_string());
        }
        // Without CustomActivator Windows silently drops <actions> from the toast:
        // the notification appears, the buttons never do.
        let _ = key.set_value("CustomActivator", &crate::consts::TOAST_ACTIVATOR_CLSID);
    }
    // The CLSID has to resolve to a local server, otherwise the activator counts
    // as unregistered. Clicks are handled over the deskmate: protocol, so this
    // server is never actually driven - see TOAST_ACTIVATED_ARG.
    if let Some(exe) = &exe {
        let clsid_path = format!(
            "Software\\Classes\\CLSID\\{}\\LocalServer32",
            crate::consts::TOAST_ACTIVATOR_CLSID
        );
        if let Ok((key, _)) = hkcu.create_subkey(&clsid_path) {
            let command = format!(
                "\"{}\" {}",
                exe.to_string_lossy(),
                crate::consts::TOAST_ACTIVATED_ARG
            );
            let _ = key.set_value("", &command);
        }
    }
}
#[cfg(not(windows))]
pub fn ensure_aumid_registered() {}

/// Turns toast branding on/off. enabled=true: registers the AUMID in HKCU and
/// makes sure a Start Menu shortcut carries it (an unpackaged app needs one
/// before Windows shows toasts under its own name) -> toasts show "Deskmate".
/// If that fails, or enabled=false -> BRANDED=false and toasts go out under the
/// PowerShell AUMID, which is always visible and only changes the label.
///
/// Nothing here starts another process. Up to 0.7.0 the shortcut was written
/// by PowerShell compiling C# through Add-Type, launched with -EncodedCommand,
/// and Bitdefender quarantined deskmate.exe for it (GitHub issue #2).
#[cfg(windows)]
pub fn apply_branding(enabled: bool) {
    remove_legacy_shortcut();
    if !enabled {
        BRANDED.store(false, Ordering::Relaxed);
        return;
    }
    ensure_aumid_registered();
    match ensure_start_menu_shortcut() {
        Ok(()) => BRANDED.store(true, Ordering::Relaxed),
        Err(e) => {
            log::warn!("branding shortcut failed ({e}); toasts use the PowerShell AUMID");
            BRANDED.store(false, Ordering::Relaxed);
        }
    }
}
#[cfg(not(windows))]
pub fn apply_branding(_enabled: bool) {}

#[cfg(windows)]
fn start_menu_programs(base_var: &str) -> Option<std::path::PathBuf> {
    std::env::var_os(base_var).map(|base| {
        std::path::PathBuf::from(base).join("Microsoft\\Windows\\Start Menu\\Programs")
    })
}

/// Makes sure `Deskmate.lnk` in the user's Start Menu carries the AUMID and the
/// toast activator CLSID. With a per-user install this is the installer's own
/// shortcut, updated in place (same target), so Start keeps a single entry.
#[cfg(windows)]
fn ensure_start_menu_shortcut() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let file_name = format!("{}.lnk", crate::consts::TOAST_DISPLAY_NAME);
    // Installed for all users: the installer already put a shortcut with this
    // AUMID into the common Start Menu. A per-user copy would only make
    // Deskmate show up twice in Start; the registry part covers the activator.
    if start_menu_programs("ProgramData").is_some_and(|dir| dir.join(&file_name).exists()) {
        return Ok(());
    }
    let lnk = start_menu_programs("APPDATA")
        .ok_or_else(|| "APPDATA is not set".to_string())?
        .join(&file_name);

    // Rewritten whenever the identifiers it carries change, so a shortcut from
    // an older build cannot keep pointing at a stale AUMID or executable.
    let stamp = format!(
        "3|{}|{}|{}",
        crate::consts::TOAST_AUMID,
        crate::consts::TOAST_ACTIVATOR_CLSID,
        exe.to_string_lossy()
    );
    if lnk.exists() && shortcut_stamp().as_deref() == Some(stamp.as_str()) {
        return Ok(());
    }
    // A debug build must not repoint the installed app's shortcut at target\debug.
    if cfg!(debug_assertions) && lnk.exists() {
        return Ok(());
    }
    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    shell::write_shortcut(
        &lnk,
        &exe,
        crate::consts::TOAST_AUMID,
        crate::consts::TOAST_ACTIVATOR_CLSID,
        crate::consts::TOAST_DISPLAY_NAME,
    )?;
    set_shortcut_stamp(&stamp);
    Ok(())
}

/// Versions up to 0.7.0 created a second Start Menu entry, `HomeOS.lnk`, and
/// recorded that with a "2|..." stamp. Removed only when that stamp proves
/// Deskmate wrote it.
#[cfg(windows)]
fn remove_legacy_shortcut() {
    if !shortcut_stamp().is_some_and(|stamp| stamp.starts_with("2|")) {
        return;
    }
    if let Some(dir) = start_menu_programs("APPDATA") {
        let legacy = dir.join("HomeOS.lnk");
        if legacy.exists() {
            match std::fs::remove_file(&legacy) {
                Ok(()) => log::info!("removed the old HomeOS Start Menu shortcut"),
                Err(e) => log::warn!("cannot remove the old HomeOS shortcut: {e}"),
            }
        }
    }
    set_shortcut_stamp("");
}

/// Identifiers baked into the Start Menu shortcut the last time it was written.
#[cfg(windows)]
fn shortcut_stamp() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!(
            "Software\\Classes\\AppUserModelId\\{}",
            crate::consts::TOAST_AUMID
        ))
        .ok()?
        .get_value("DeskmateShortcutStamp")
        .ok()
}

#[cfg(windows)]
fn set_shortcut_stamp(stamp: &str) {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(format!(
        "Software\\Classes\\AppUserModelId\\{}",
        crate::consts::TOAST_AUMID
    )) {
        let _ = key.set_value("DeskmateShortcutStamp", &stamp.to_string());
    }
}

/// Shell link through COM, in process (IShellLinkW + IPropertyStore).
#[cfg(windows)]
mod shell {
    use std::path::Path;
    use windows::core::{Interface, GUID, HSTRING};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::StructuredStorage::{
        InitPropVariantFromCLSID, PropVariantClear, PROPVARIANT,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{IShellLinkW, SHStrDupW, ShellLink};

    const APP_MODEL: GUID = GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3);
    /// System.AppUserModel.ID - ties the shortcut to the AUMID used for toasts.
    const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY { fmtid: APP_MODEL, pid: 5 };
    /// System.AppUserModel.ToastActivatorCLSID - without it Windows drops the
    /// action buttons of an unpackaged app's toast.
    const PKEY_TOAST_ACTIVATOR: PROPERTYKEY = PROPERTYKEY { fmtid: APP_MODEL, pid: 26 };

    pub fn write_shortcut(
        lnk: &Path,
        exe: &Path,
        aumid: &str,
        activator: &str,
        description: &str,
    ) -> Result<(), String> {
        let clsid = GUID::try_from(activator.trim_matches(|c| c == '{' || c == '}'))
            .map_err(|e| e.to_string())?;
        let lnk = HSTRING::from(lnk.as_os_str());
        let target = HSTRING::from(exe.as_os_str());
        let work_dir = exe.parent().map(|dir| HSTRING::from(dir.as_os_str()));
        let aumid = HSTRING::from(aumid);
        let description = HSTRING::from(description);
        // Own thread and apartment: the caller may be a Tokio worker or the
        // UI thread, and neither should have its COM state changed.
        std::thread::spawn(move || unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let result = (|| -> windows::core::Result<()> {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                link.SetPath(&target)?;
                if let Some(dir) = &work_dir {
                    link.SetWorkingDirectory(dir)?;
                }
                link.SetIconLocation(&target, 0)?;
                link.SetDescription(&description)?;

                let store: IPropertyStore = link.cast()?;
                let mut id = PROPVARIANT::default();
                (*id.Anonymous.Anonymous).vt = VT_LPWSTR;
                (*id.Anonymous.Anonymous).Anonymous.pwszVal = SHStrDupW(&aumid)?;
                let set_id = store.SetValue(&PKEY_APP_USER_MODEL_ID, &id);
                let _ = PropVariantClear(&mut id);
                set_id?;
                let mut activator = InitPropVariantFromCLSID(&clsid)?;
                let set_activator = store.SetValue(&PKEY_TOAST_ACTIVATOR, &activator);
                let _ = PropVariantClear(&mut activator);
                set_activator?;
                store.Commit()?;

                link.cast::<IPersistFile>()?.Save(&lnk, true)
            })();
            if init.is_ok() {
                CoUninitialize();
            }
            result.map_err(|e| e.to_string())
        })
        .join()
        .map_err(|_| "shortcut thread panicked".to_string())?
    }
}

#[cfg(windows)]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Percent-encodes a value for the protocol URL argument (deskmate:action?name=...).
#[cfg(windows)]
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// The inverse of pct_encode - decodes the name from the protocol URL (no external deps).
pub fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Extracts the action name and token from the toast activation URL, e.g.
/// "deskmate:action?name=test_ok&t=ab12" -> ("test_ok", "ab12"). Returns None
/// if the URL doesn't match the scheme.
fn parse_action_parts(url: &str) -> Option<(String, String)> {
    let prefix = format!("{}:action?name=", crate::consts::PROTOCOL_SCHEME);
    let rest = url.trim().strip_prefix(&prefix)?;
    let rest = rest.split('#').next().unwrap_or(rest);
    let mut parts = rest.split('&');
    let name = pct_decode(parts.next().unwrap_or(""));
    let token = parts
        .find_map(|part| part.strip_prefix("t="))
        .map(pct_decode)
        .unwrap_or_default();
    if name.is_empty() {
        None
    } else {
        Some((name, token))
    }
}

/// Whether an argument looks like a toast activation URL (valid or not).
pub fn is_action_url(url: &str) -> bool {
    parse_action_parts(url).is_some()
}

/// Accepts a toast button click only with a token this process issued for
/// exactly that action. Tokens are single-use and live for 24 hours.
pub fn redeem_action_url(url: &str) -> Option<String> {
    let (name, token) = parse_action_parts(url)?;
    let issued = action_tokens()
        .lock()
        .ok()
        .and_then(|mut tokens| tokens.remove(&token));
    match issued {
        Some((action, at)) if action == name && at.elapsed() < ACTION_TOKEN_TTL => Some(name),
        _ => {
            crate::security::audit("toast_action", "blocked_unknown_token");
            log::warn!("toast action without a valid token ignored");
            None
        }
    }
}

/// Registers the `deskmate:` URL scheme in HKCU, so clicking a toast button launches
/// the app with the argument `deskmate:action?name=...` (single-instance hands it off
/// to the already-running instance).
#[cfg(windows)]
pub fn register_protocol() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let exe = match std::env::current_exe() {
        Ok(e) => e.to_string_lossy().to_string(),
        Err(_) => return,
    };
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base = format!("Software\\Classes\\{}", crate::consts::PROTOCOL_SCHEME);
    if let Ok((k, _)) = hkcu.create_subkey(&base) {
        let _ = k.set_value("", &format!("URL:{} protocol", crate::consts::TOAST_DISPLAY_NAME));
        let _ = k.set_value("URL Protocol", &"");
    }
    if let Ok((k, _)) = hkcu.create_subkey(format!("{}\\shell\\open\\command", base)) {
        let _ = k.set_value("", &format!("\"{}\" \"%1\"", exe));
    }
}
#[cfg(not(windows))]
pub fn register_protocol() {}

/// Toast XML. Buttons use protocol activation: a click launches
/// `deskmate:action?name=...&t=...`, single-instance hands the URL to the
/// running app, and the app only accepts tokens it put on a toast itself.
#[cfg(windows)]
fn toast_xml(p: &NotifyPayload, img: Option<&std::path::Path>) -> String {
    let image_xml = match img {
        Some(path) => format!(
            "<image placement=\"appLogoOverride\" src=\"{}\"/>",
            xml_escape(&path.to_string_lossy())
        ),
        None => String::new(),
    };
    let mut actions_xml = String::new();
    for a in &p.actions {
        if a.title.is_empty() || a.action.is_empty() {
            continue;
        }
        actions_xml.push_str(&format!(
            "<action content=\"{}\" arguments=\"{}\" activationType=\"protocol\"/>",
            xml_escape(&a.title),
            xml_escape(&format!(
                "{}:action?name={}&t={}",
                crate::consts::PROTOCOL_SCHEME,
                pct_encode(&a.action),
                issue_action_token(&a.action),
            )),
        ));
    }
    if !actions_xml.is_empty() {
        actions_xml = format!("<actions>{actions_xml}</actions>");
    }
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text>{}</binding></visual>{}</toast>",
        xml_escape(&p.title),
        xml_escape(&p.message),
        image_xml,
        actions_xml
    )
}

/// AUMID of Windows PowerShell. Only used as the toast's source label when
/// branding is off or failed; no PowerShell process is ever started for it.
#[cfg(windows)]
const POWERSHELL_AUMID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

/// Shows the toast through WinRT in this process. windows-rs joins the
/// process-wide multithreaded apartment on its own when the calling thread has
/// no COM apartment yet.
#[cfg(windows)]
pub fn show_toast(p: &NotifyPayload) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let aumid = if BRANDED.load(Ordering::Relaxed) {
        crate::consts::TOAST_AUMID
    } else {
        POWERSHELL_AUMID
    };
    let img = p.image.as_deref().and_then(fetch_image);
    if let Some(path) = &img {
        schedule_image_removal(path.clone());
    }
    let xml = toast_xml(p, img.as_deref());
    let show = || -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml.as_str()))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(aumid))?.Show(&toast)
    };
    show().map_err(|e| format!("toast: {e}"))
}

#[cfg(not(windows))]
pub fn show_toast(_p: &NotifyPayload) -> Result<(), String> {
    Err("windows only".into())
}
