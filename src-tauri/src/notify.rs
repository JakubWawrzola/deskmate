//! Powiadomienia HA -> toast Windows (obraz + przyciski akcji).
//! HA publikuje JSON na `deskmate/<node>/notify`:
//! {"title":"Zmywarka","message":"Do rozpakowania","image":"https://...",
//!  "actions":[{"title":"OK","action":"ok"},{"title":"Drzemka","action":"snooze"}]}
//! Klikniety przycisk -> publikacja `{action}` na `deskmate/<node>/notify/action`.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

/// Czy udalo sie ustawic brandowany AUMID (skrot w Menu Start). Gdy nie -
/// uzywamy PowerShell AUMID, ktory renderuje toast ZAWSZE (kosztem etykiety).
static BRANDED: AtomicBool = AtomicBool::new(false);

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
    /// czas lokalny hh:mm:ss (do listy w UI)
    pub received_at: String,
}

/// Parsuje payload; toleruje czysty tekst (message bez JSON).
pub fn parse(payload: &str) -> NotifyPayload {
    match serde_json::from_str::<NotifyPayload>(payload) {
        Ok(p) => p,
        Err(_) => NotifyPayload {
            title: crate::consts::APP_NAME.into(),
            message: payload.trim().chars().take(500).collect(),
            image: None,
            actions: Vec::new(),
        },
    }
}

/// Pobiera obraz do pliku tymczasowego (toast wymaga sciezki lokalnej).
fn fetch_image(url: &str) -> Option<std::path::PathBuf> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    use std::io::Read;
    let connector = native_tls::TlsConnector::new().ok()?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(std::sync::Arc::new(connector))
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let resp = agent.get(url).call().ok()?;
    let mut buf = Vec::with_capacity(256 * 1024);
    resp.into_reader()
        .take(5 * 1024 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.is_empty() {
        return None;
    }
    let ext = if url.to_lowercase().contains(".jpg") || url.to_lowercase().contains(".jpeg") {
        "jpg"
    } else {
        "png"
    };
    let path = std::env::temp_dir().join(format!("deskmate_toast.{ext}"));
    std::fs::write(&path, &buf).ok()?;
    Some(path)
}

/// Rejestracja AUMID w HKCU (DisplayName) - nieszkodliwe uzupelnienie skrotu.
#[cfg(windows)]
pub fn ensure_aumid_registered() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = format!("Software\\Classes\\AppUserModelId\\{}", crate::consts::TOAST_AUMID);
    if let Ok((key, _)) = hkcu.create_subkey(&path) {
        let _ = key.set_value("DisplayName", &crate::consts::APP_NAME);
        if let Ok(exe) = std::env::current_exe() {
            let _ = key.set_value("IconUri", &exe.to_string_lossy().to_string());
        }
    }
}
#[cfg(not(windows))]
pub fn ensure_aumid_registered() {}

/// Branding toastow (skrot w Menu Start z AppUserModelID) = BACKLOG.
/// Wymaga COM IShellLink+IPropertyStore+PROPVARIANT (VT_LPWSTR) - do zrobienia
/// czysciej przez hook instalatora NSIS (plugin ApplicationID). Do tego czasu
/// toasty ida przez PowerShell AUMID (dzialaja ZAWSZE, kosztem etykiety).
pub fn ensure_branding() {
    let _ = &BRANDED; // BRANDED zostaje false -> show_toast uzywa PowerShell AUMID
}

#[cfg(windows)]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Fallback: pokaz toast przez SWIEZY proces PowerShell (czyste srodowisko WinRT).
/// Uzywane, gdy in-process `.show()` zawiedzie - typowo zly apartment COM/WinRT w
/// watku Tauri procesu niepakietowanego. PowerShell.exe ma zarejestrowany AUMID,
/// wiec toast renderuje sie zawsze. Przyciski akcji tu NIE dzialaja (brak callbacku
/// do naszego procesu) - to sciezka wizualna, gdy inaczej nie byloby nic.
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
    // XML toastu w apostrofach ($x.LoadXml('...')); title/message maja ' -> &apos;,
    // atrybuty uzywaja ", wiec wewnatrz PS-owego stringa w '...' nic sie nie lamie.
    let toast_xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text>{}</binding></visual></toast>",
        xml_escape(&p.title),
        xml_escape(&p.message),
        image_xml
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
        aumid = Toast::POWERSHELL_APP_ID
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
    let mut toast = Toast::new(aumid).title(&p.title).text1(&p.message);
    let img = p.image.as_deref().and_then(fetch_image);
    if let Some(path) = &img {
        toast = toast.image(path, "");
    }
    for a in &p.actions {
        if !a.title.is_empty() && !a.action.is_empty() {
            toast = toast.add_button(&a.title, &a.action);
        }
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
    // In-process WinRT bywa zawodne w niepakietowanym procesie (apartment COM).
    // Gdy zawiedzie - swiezy PowerShell renderuje toast (bez przyciskow akcji).
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
