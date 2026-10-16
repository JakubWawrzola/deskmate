//! "Press keys" controls: a key combination such as Ctrl+F5, set on the PC,
//! pressed in the window in front when Home Assistant presses the button.
//! Home Assistant only triggers the control; it cannot choose the keys.

/// One key press: modifiers held while the key goes down and up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    pub modifiers: Vec<u16>,
    pub key: u16,
}

const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12;
const VK_LWIN: u16 = 0x5B;
const VK_DELETE: u16 = 0x2E;

/// Longest text accepted for one control.
const MAX_LEN: usize = 200;
/// Combos pressed one after another, e.g. "Ctrl+L, Ctrl+C".
const MAX_COMBOS: usize = 8;

fn modifier(name: &str) -> Option<u16> {
    Some(match name {
        "ctrl" | "control" | "ctl" => VK_CONTROL,
        "shift" => VK_SHIFT,
        "alt" => VK_MENU,
        "win" | "windows" | "meta" | "super" | "cmd" => VK_LWIN,
        _ => return None,
    })
}

fn key(name: &str) -> Option<u16> {
    let bytes = name.as_bytes();
    if bytes.len() == 1 {
        let c = bytes[0].to_ascii_uppercase();
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(u16::from(c));
        }
    }
    if let Some(number) = name.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
        if (1..=24).contains(&number) {
            return Some(0x70 + number - 1);
        }
    }
    if let Some(number) = name
        .strip_prefix("numpad")
        .or_else(|| name.strip_prefix("num"))
        .and_then(|n| n.parse::<u16>().ok())
    {
        if number <= 9 {
            return Some(0x60 + number);
        }
    }
    Some(match name {
        "enter" | "return" => 0x0D,
        "esc" | "escape" => 0x1B,
        "tab" => 0x09,
        "space" | "spacebar" => 0x20,
        "backspace" | "bksp" => 0x08,
        "delete" | "del" => VK_DELETE,
        "insert" | "ins" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "printscreen" | "prtsc" | "print" => 0x2C,
        "pause" => 0x13,
        "capslock" => 0x14,
        "numlock" => 0x90,
        "scrolllock" => 0x91,
        "menu" | "apps" | "contextmenu" => 0x5D,
        "plus" | "=" => 0xBB,
        "minus" | "-" => 0xBD,
        "comma" | "," => 0xBC,
        "period" | "." => 0xBE,
        "/" | "slash" => 0xBF,
        ";" | "semicolon" => 0xBA,
        "[" => 0xDB,
        "]" => 0xDD,
        "\\" | "backslash" => 0xDC,
        "'" | "quote" => 0xDE,
        "`" | "backquote" => 0xC0,
        "volumeup" => 0xAF,
        "volumedown" => 0xAE,
        "volumemute" | "mute" => 0xAD,
        "playpause" | "mediaplaypause" => 0xB3,
        "nexttrack" | "medianext" => 0xB0,
        "prevtrack" | "previoustrack" | "mediaprev" => 0xB1,
        "stop" | "mediastop" => 0xB2,
        "browserback" => 0xA6,
        "browserforward" => 0xA7,
        "browserrefresh" => 0xA8,
        "browserhome" => 0xAC,
        _ => return None,
    })
}

/// Keys Windows wants flagged as extended, or they act like the numpad ones.
fn extended(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2C | 0x2D | VK_DELETE | 0x5B | 0x5D | 0x90 | 0xA6..=0xB3)
}

fn parse_combo(text: &str) -> Result<Combo, String> {
    let parts: Vec<String> = text
        .split('+')
        .map(|part| part.trim().to_lowercase())
        .collect();
    // "Ctrl++" means Ctrl and the plus key
    let parts: Vec<String> = if text.trim_end().ends_with("++") {
        let mut parts: Vec<String> = parts.into_iter().filter(|p| !p.is_empty()).collect();
        parts.push("plus".into());
        parts
    } else {
        parts
    };
    if parts.iter().any(String::is_empty) {
        return Err(format!("\"{}\" is not a key combination", text.trim()));
    }
    let (last, rest) = parts.split_last().ok_or("no key given")?;
    let mut modifiers = Vec::new();
    for name in rest {
        let vk = modifier(name).ok_or_else(|| format!("\"{name}\" is not Ctrl, Shift, Alt or Win"))?;
        if !modifiers.contains(&vk) {
            modifiers.push(vk);
        }
    }
    let key = key(last)
        .or_else(|| modifier(last))
        .ok_or_else(|| format!("unknown key \"{last}\" (examples: F5, R, Enter, Tab, Up, PageDown)"))?;
    if modifiers.contains(&VK_CONTROL) && modifiers.contains(&VK_MENU) && key == VK_DELETE {
        return Err("Windows does not let programs press Ctrl+Alt+Delete".into());
    }
    Ok(Combo { modifiers, key })
}

/// "Ctrl+F5" or "Ctrl+L, Ctrl+C": one or more combinations, pressed in order.
pub fn parse(text: &str) -> Result<Vec<Combo>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("give a key combination, e.g. Ctrl+F5".into());
    }
    if text.len() > MAX_LEN {
        return Err("too many keys".into());
    }
    // A comma is a separator, unless it is the key itself ("Ctrl+,").
    let mut combos = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c == ',' && !current.trim_end().ends_with('+') {
            combos.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    combos.push(current);
    let combos: Vec<Combo> = combos
        .iter()
        .filter(|combo| !combo.trim().is_empty())
        .map(|combo| parse_combo(combo))
        .collect::<Result<_, _>>()?;
    if combos.is_empty() {
        return Err("give a key combination, e.g. Ctrl+F5".into());
    }
    if combos.len() > MAX_COMBOS {
        return Err(format!("at most {MAX_COMBOS} combinations in one control"));
    }
    Ok(combos)
}

/// Presses the combinations in the window in front.
#[cfg(windows)]
pub fn press(text: &str) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    let combos = parse(text)?;
    if crate::sensors::session_locked() {
        return Err("Windows is locked, so there is no window to press keys in".into());
    }
    let event = |vk: u16, up: bool| {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        if extended(vk) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    };
    for (index, combo) in combos.iter().enumerate() {
        if index > 0 {
            // Some programs miss a combination that follows the previous one at once.
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        let mut inputs = Vec::new();
        for &vk in &combo.modifiers {
            inputs.push(event(vk, false));
        }
        inputs.push(event(combo.key, false));
        inputs.push(event(combo.key, true));
        for &vk in combo.modifiers.iter().rev() {
            inputs.push(event(vk, true));
        }
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            return Err("Windows refused the key presses (is the window in front running as administrator?)".into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn press(_text: &str) -> Result<(), String> {
    Err("windows only".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_combinations() {
        assert_eq!(parse("Ctrl+F5").unwrap(), vec![Combo { modifiers: vec![VK_CONTROL], key: 0x74 }]);
        assert_eq!(parse(" ctrl + shift + r ").unwrap()[0].modifiers, vec![VK_CONTROL, VK_SHIFT]);
        assert_eq!(parse("Win+D").unwrap()[0], Combo { modifiers: vec![VK_LWIN], key: u16::from(b'D') });
        assert_eq!(parse("F5").unwrap()[0].modifiers, Vec::<u16>::new());
        assert_eq!(parse("Ctrl+L, Ctrl+C").unwrap().len(), 2);
        assert_eq!(parse("Ctrl++").unwrap()[0].key, 0xBB);
        assert_eq!(parse("Ctrl+,").unwrap()[0].key, 0xBC);
        assert_eq!(parse("Alt+Tab").unwrap()[0].key, 0x09);
    }

    #[test]
    fn rejects_what_cannot_work() {
        assert!(parse("").is_err());
        assert!(parse("Ctrl+").is_err());
        assert!(parse("Hyper+F5").is_err());
        assert!(parse("Ctrl+Banana").is_err());
        assert!(parse("Ctrl+Alt+Del").is_err());
    }
}
