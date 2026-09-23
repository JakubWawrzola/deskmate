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
/// we use the PowerShell AUMID, which ALWAYS renders the toast (at the cost of the label).
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
/// creates a Start Menu shortcut with AppUserModelID (Windows requires a shortcut
/// before an UNpackaged app can send toasts under its own AUMID) -> the toast
/// shows "HomeOS". If the shortcut creation fails, or enabled=false -> BRANDED=false,
/// show_toast falls back to the PowerShell AUMID (always visible). A branding
/// failure never breaks the toast itself, it just loses the custom label.
#[cfg(windows)]
pub fn apply_branding(enabled: bool) {
    if !enabled {
        BRANDED.store(false, Ordering::Relaxed);
        return;
    }
    ensure_aumid_registered();
    match ensure_start_menu_shortcut() {
        Ok(()) => BRANDED.store(true, Ordering::Relaxed),
        Err(e) => {
            log::warn!("branding shortcut failed ({e}); fallback to PowerShell AUMID");
            BRANDED.store(false, Ordering::Relaxed);
        }
    }
}
#[cfg(not(windows))]
pub fn apply_branding(_enabled: bool) {}

/// Creates `%AppData%\...\Start Menu\Programs\HomeOS.lnk` with System.AppUserModel.ID =
/// TOAST_AUMID, via PowerShell with embedded C# (IShellLink + IPropertyStore) - the
/// windows crate 0.61 didn't expose these COM interfaces. If the shortcut already
/// exists, returns Ok immediately (no PowerShell spawn on every startup).
#[cfg(windows)]
fn ensure_start_menu_shortcut() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let appdata = std::env::var("APPDATA").map_err(|e| e.to_string())?;
    let lnk = std::path::Path::new(&appdata)
        .join("Microsoft\\Windows\\Start Menu\\Programs")
        .join(format!("{}.lnk", crate::consts::TOAST_DISPLAY_NAME));

    // The shortcut is rewritten whenever the identifiers it carries change.
    // A plain "file exists" check used to make this a one-shot: a shortcut
    // written by an older build kept pointing at a stale AUMID and no later
    // version could ever repair it.
    let stamp = format!(
        "2|{}|{}|{}",
        crate::consts::TOAST_AUMID,
        crate::consts::TOAST_ACTIVATOR_CLSID,
        exe.to_string_lossy()
    );
    if lnk.exists() && shortcut_stamp() == Some(stamp.clone()) {
        return Ok(());
    }

    let ps_quote = |s: &str| s.replace('\'', "''");
    let header = format!(
        "$Exe='{}'; $Lnk='{}'; $Aumid='{}'; $Name='{}'; $Clsid='{}';\n",
        ps_quote(&exe.to_string_lossy()),
        ps_quote(&lnk.to_string_lossy()),
        ps_quote(crate::consts::TOAST_AUMID),
        ps_quote(crate::consts::TOAST_DISPLAY_NAME),
        ps_quote(crate::consts::TOAST_ACTIVATOR_CLSID),
    );
    let script = format!("{}{}", header, SHORTCUT_PS);
    // Passed inline as -EncodedCommand (UTF-16LE base64). A script file with a
    // fixed name in %TEMP% could be swapped by another process between the
    // write and the launch.
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-EncodedCommand"])
        .arg(&encoded)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() && lnk.exists() {
        set_shortcut_stamp(&stamp);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim().chars().take(500).collect::<String>();
        Err(format!(
            "powershell exit {}, lnk exists {}: {stderr}",
            output.status,
            lnk.exists()
        ))
    }
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

/// C# (IShellLink+IPropertyStore) that creates the shortcut with the AUMID and the
/// toast activator CLSID. The $Exe/$Lnk/$Aumid/$Name/$Clsid variables come from the
/// header prepended before this block.
#[cfg(windows)]
const SHORTCUT_PS: &str = r#"
$ErrorActionPreference='Stop'
$code=@"
using System;
using System.Runtime.InteropServices;
namespace ShLnk {
  [ComImport, Guid("00021401-0000-0000-C000-000000000046")] internal class CShellLink {}
  [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("000214F9-0000-0000-C000-000000000046")]
  internal interface IShellLinkW {
    void GetPath([MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder f, int c, IntPtr d, uint fl);
    void GetIDList(out IntPtr ppidl);
    void SetIDList(IntPtr pidl);
    void GetDescription([MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder n, int c);
    void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string n);
    void GetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder d, int c);
    void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string d);
    void GetArguments([MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder a, int c);
    void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string a);
    void GetHotkey(out short w);
    void SetHotkey(short w);
    void GetShowCmd(out int i);
    void SetShowCmd(int i);
    void GetIconLocation([MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder p, int c, out int i);
    void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string p, int i);
    void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string p, uint r);
    void Resolve(IntPtr h, uint fl);
    void SetPath([MarshalAs(UnmanagedType.LPWStr)] string f);
  }
  [StructLayout(LayoutKind.Sequential)] internal struct PROPERTYKEY { public Guid fmtid; public uint pid; }
  [StructLayout(LayoutKind.Explicit)] internal struct PROPVARIANT {
    [FieldOffset(0)] public ushort vt;
    [FieldOffset(8)] public IntPtr p;
  }
  [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99")]
  internal interface IPropertyStore {
    void GetCount(out uint c);
    void GetAt(uint i, out PROPERTYKEY k);
    void GetValue(ref PROPERTYKEY k, out PROPVARIANT pv);
    void SetValue(ref PROPERTYKEY k, ref PROPVARIANT pv);
    void Commit();
  }
  [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("0000010b-0000-0000-C000-000000000046")]
  internal interface IPersistFile {
    void GetClassID(out Guid c);
    [PreserveSig] int IsDirty();
    void Load([MarshalAs(UnmanagedType.LPWStr)] string f, int m);
    void Save([MarshalAs(UnmanagedType.LPWStr)] string f, [MarshalAs(UnmanagedType.Bool)] bool remember);
    void SaveCompleted([MarshalAs(UnmanagedType.LPWStr)] string f);
    void GetCurFile([MarshalAs(UnmanagedType.LPWStr)] out string f);
  }
  internal static class Native {
    [DllImport("ole32.dll")] public static extern int PropVariantClear(ref PROPVARIANT pv);
    [DllImport("shlwapi.dll", CharSet=CharSet.Unicode)] public static extern int SHStrDupW(string psz, out IntPtr ppwsz);
  }
  public static class Creator {
    public static void Create(string exe, string lnk, string aumid, string name, string clsid) {
      IShellLinkW link = (IShellLinkW)new CShellLink();
      link.SetPath(exe);
      link.SetDescription(name);
      string wd = System.IO.Path.GetDirectoryName(exe);
      if (wd != null) link.SetWorkingDirectory(wd);
      link.SetIconLocation(exe, 0);
      IPropertyStore store = (IPropertyStore)link;
      Guid fmtid = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3");

      // System.AppUserModel.ID - ties the shortcut to the AUMID used for toasts.
      PROPERTYKEY key = new PROPERTYKEY();
      key.fmtid = fmtid;
      key.pid = 5;
      PROPVARIANT pv = new PROPVARIANT();
      IntPtr strPtr;
      int hr = Native.SHStrDupW(aumid, out strPtr);
      if (hr != 0) { throw new System.Runtime.InteropServices.COMException("SHStrDupW failed", hr); }
      pv.vt = 31; // VT_LPWSTR
      pv.p = strPtr;
      store.SetValue(ref key, ref pv);
      Native.PropVariantClear(ref pv);

      // System.AppUserModel.ToastActivatorCLSID - required before Windows will
      // render action buttons for an unpackaged app. VT_CLSID points at a GUID.
      PROPERTYKEY actKey = new PROPERTYKEY();
      actKey.fmtid = fmtid;
      actKey.pid = 26;
      PROPVARIANT actPv = new PROPVARIANT();
      IntPtr guidPtr = Marshal.AllocCoTaskMem(16);
      Marshal.StructureToPtr(new Guid(clsid), guidPtr, false);
      actPv.vt = 72; // VT_CLSID
      actPv.p = guidPtr;
      store.SetValue(ref actKey, ref actPv);
      Native.PropVariantClear(ref actPv);

      store.Commit();
      IPersistFile pf = (IPersistFile)link;
      pf.Save(lnk, true);
    }
  }
}
"@
Add-Type -TypeDefinition $code -Language CSharp | Out-Null
$dir = Split-Path $Lnk -Parent
if (!(Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
[ShLnk.Creator]::Create($Exe, $Lnk, $Aumid, $Name, $Clsid)
"#;

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

/// Fallback: show the toast through a FRESH PowerShell process (a clean WinRT
/// environment). Used when the in-process `.show()` fails - typically a bad
/// COM/WinRT apartment in the unpackaged Tauri process's thread. PowerShell.exe
/// has a registered AUMID, so the toast always renders. Action buttons are built
/// here via protocol activation (`deskmate:action?name=...`, see pct_encode above)
/// so they route back into the running app through the single-instance handoff -
/// but as of 2026-07, they still don't render on Kuba's machine even though the
/// toast itself shows correctly with the right branding. Root cause not yet found.
#[cfg(windows)]
fn show_toast_powershell(p: &NotifyPayload, img: Option<&std::path::Path>) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use tauri_winrt_notification::Toast;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let image_xml = match img {
        Some(path) => format!(
            "<image placement=\"appLogoOverride\" src=\"{}\"/>",
            xml_escape(&path.to_string_lossy())
        ),
        None => String::new(),
    };
    // action buttons: activationType="protocol" -> a click launches deskmate:action?name=...
    // (works even though the PS process is already gone; single-instance hands the URL to the app)
    let actions_xml = if p.actions.is_empty() {
        String::new()
    } else {
        let mut s = String::from("<actions>");
        for a in &p.actions {
            if a.title.is_empty() || a.action.is_empty() {
                continue;
            }
            s.push_str(&format!(
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
        s.push_str("</actions>");
        s
    };
    // source label: if branding succeeded -> our own AUMID (toast shows "HomeOS"),
    // otherwise the PowerShell AUMID (always visible, shows "Windows PowerShell").
    let aumid = if BRANDED.load(Ordering::Relaxed) {
        crate::consts::TOAST_AUMID.to_string()
    } else {
        Toast::POWERSHELL_APP_ID.to_string()
    };
    // The toast XML sits inside single quotes ($x.LoadXml('...')); title/message
    // escape ' -> &apos;, attributes use ", so nothing breaks inside the PS '...' string.
    let toast_xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text>{}</binding></visual>{}</toast>",
        xml_escape(&p.title),
        xml_escape(&p.message),
        image_xml,
        actions_xml
    );
    let script = format!(
        "$ErrorActionPreference='Stop';\
         $null=[Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime];\
         $null=[Windows.Data.Xml.Dom.XmlDocument,Windows.Data.Xml.Dom.XmlDocument,ContentType=WindowsRuntime];\
         $x=New-Object Windows.Data.Xml.Dom.XmlDocument;\
         $x.LoadXml('{toast_xml}');\
         $t=New-Object Windows.UI.Notifications.ToastNotification $x;\
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{aumid}').Show($t);",
        toast_xml = toast_xml,
        aumid = aumid
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("powershell exited with {status}"))
    }
}

#[cfg(windows)]
pub fn show_toast(p: &NotifyPayload, action_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>) -> Result<(), String> {
    use tauri_winrt_notification::Toast;
    let aumid = if BRANDED.load(Ordering::Relaxed) {
        crate::consts::TOAST_AUMID
    } else {
        Toast::POWERSHELL_APP_ID
    };
    let img = p.image.as_deref().and_then(fetch_image);

    // tauri-winrt-notification 0.8 builds each <action> element, sets its
    // attributes and then never appends it to <actions>, so a toast sent through
    // the crate arrives with an empty actions list and Windows renders no
    // buttons. Our own XML is correct and verified, so anything with buttons
    // takes that path directly instead.
    if let Some(path) = &img {
        schedule_image_removal(path.clone());
    }
    if !p.actions.is_empty() {
        return show_toast_powershell(p, img.as_deref());
    }

    let mut toast = Toast::new(aumid).title(&p.title).text1(&p.message);
    if let Some(path) = &img {
        toast = toast.image(path, "");
    }
    if let Some(tx) = action_tx {
        toast = toast.on_activated(move |arg| {
            if let Some(a) = arg {
                if !a.is_empty() {
                    let _ = tx.send(a);
                }
            }
            Ok(())
        });
    }
    // In-process WinRT can be unreliable in an unpackaged process (COM apartment
    // issues). If it fails, a fresh PowerShell process renders the toast instead.
    match toast.show().map_err(|e| e.to_string()) {
        Ok(()) => Ok(()),
        Err(winrt_err) => match show_toast_powershell(p, img.as_deref()) {
            Ok(()) => Ok(()),
            Err(ps_err) => Err(format!("winrt: {winrt_err} | powershell: {ps_err}")),
        },
    }
}

#[cfg(not(windows))]
pub fn show_toast(_p: &NotifyPayload, _action_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>) -> Result<(), String> {
    Err("windows only".into())
}
