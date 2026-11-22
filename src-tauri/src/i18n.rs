//! The texts Deskmate shows outside its window: the tray menu and the
//! notifications it raises itself (screen time, updates). The window has its
//! own translations in `src/i18n`; both follow the `language` setting.
//!
//! As in the window, the English text is the key. `t("Quit")` gives the
//! translation of exactly that text, or the text itself in English and where
//! a translation is missing. The translations are the JSON files in
//! `src-tauri/i18n`, built into the program. Entity names stay English: Home
//! Assistant shows them, not Deskmate.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Every text that is translated here. The tests hold each translation to
/// this list, and the code to using nothing outside it.
#[cfg(test)]
pub const TEXTS: &[&str] = &[
    // The tray menu.
    "Show/hide widgets",
    "Open Home Assistant",
    "Open Deskmate",
    "Connect to Home Assistant",
    "Disconnect from Home Assistant",
    "Quit",
    "Update available: Deskmate {version}",
    // Screen time.
    "Ask for more time",
    "{minutes} min with a password",
    "Bedtime in 1 minute",
    "Bedtime in {minutes} minutes",
    "1 minute of screen time left",
    "{minutes} minutes of screen time left",
    "Save your work. The computer locks when the time is up.",
    "Screen time is up",
    "Outside the allowed hours",
    "This computer locks in {seconds} seconds.",
    "This computer signs you out in {seconds} seconds.",
    "{name} is blocked",
    "The family rules on this computer block it.",
    "{minutes} extra minutes",
    "An administrator added them on this computer: {added} of {max} minutes today.",
    "{minutes} extra minutes today",
    "Home Assistant gave this computer more screen time.",
    "Message from home",
    // Updates.
    "Deskmate {version} is available",
    "Install it now, or later from the Status page. Settings and pairing are kept.",
    "Download it from the tray menu or the Status page. Settings and pairing are kept.",
    "Update the Deskmate integration in Home Assistant (HACS) first.",
    "Install now",
    "Later",
    "Deskmate was updated to {version}",
    "Settings and pairing are unchanged.",
];

/// The translations, by the language code of the configuration.
const TRANSLATIONS: [(&str, &str); 6] = [
    ("de", include_str!("../i18n/de.json")),
    ("es", include_str!("../i18n/es.json")),
    ("fr", include_str!("../i18n/fr.json")),
    ("pl", include_str!("../i18n/pl.json")),
    ("pt-BR", include_str!("../i18n/pt-BR.json")),
    ("zh-Hans", include_str!("../i18n/zh-Hans.json")),
];

type Dictionary = HashMap<String, String>;

/// Each translation, read once. A file that cannot be read gives an empty
/// dictionary, so its language falls back to English; the tests fail on it.
fn dictionaries() -> &'static HashMap<&'static str, Dictionary> {
    static DICTIONARIES: OnceLock<HashMap<&'static str, Dictionary>> = OnceLock::new();
    DICTIONARIES.get_or_init(|| {
        TRANSLATIONS
            .iter()
            .map(|(code, json)| (*code, serde_json::from_str(json).unwrap_or_default()))
            .collect()
    })
}

/// The language in use; "en" until `set_language` ran.
static LANGUAGE: RwLock<&'static str> = RwLock::new("en");

/// The language for a setting: the setting itself when it names one, else
/// the one of `system`, a tag such as "pl-PL" or "pt_BR.UTF-8". Every
/// Portuguese is the Brazilian one and every Chinese the simplified one.
/// Anything else is English.
pub fn language_for(setting: &str, system: &str) -> &'static str {
    let known = |code: &str| {
        TRANSLATIONS
            .iter()
            .map(|(known, _)| *known)
            .find(|known| *known == code)
    };
    if let Some(code) = known(setting) {
        return code;
    }
    if setting != crate::config::SYSTEM_LANGUAGE {
        return "en";
    }
    let primary = system
        .split(['-', '_', '.'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match primary.as_str() {
        "pt" => "pt-BR",
        "zh" => "zh-Hans",
        other => known(other).unwrap_or("en"),
    }
}

/// Switches the texts to the language of `setting` (the `language` of the
/// configuration).
pub fn set_language(setting: &str) {
    let code = language_for(setting, &system_language());
    *LANGUAGE.write().unwrap_or_else(|e| e.into_inner()) = code;
}

/// The text in the current language.
pub fn t(source: &str) -> String {
    let language = *LANGUAGE.read().unwrap_or_else(|e| e.into_inner());
    translated(language, source)
}

/// The text in the current language, with each `{name}` replaced by its
/// value: `tf("{name} is blocked", &[("name", "steam.exe")])`.
pub fn tf(source: &str, values: &[(&str, &str)]) -> String {
    filled(&t(source), values)
}

fn translated(language: &str, source: &str) -> String {
    dictionaries()
        .get(language)
        .and_then(|dictionary| dictionary.get(source))
        .cloned()
        .unwrap_or_else(|| source.to_string())
}

fn filled(text: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(text.to_string(), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), value)
    })
}

/// The language of the user's Windows, as a tag such as "pl-PL".
#[cfg(windows)]
fn system_language() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    // LOCALE_NAME_MAX_LENGTH
    let mut name = [0u16; 85];
    // SAFETY: the call writes at most the length of the buffer it is given
    // and returns how many characters it wrote, the final NUL included.
    let written = unsafe { GetUserDefaultLocaleName(&mut name) };
    let length = usize::try_from(written).unwrap_or(0).saturating_sub(1);
    String::from_utf16_lossy(&name[..length.min(name.len())])
}

/// The language of the session, from the usual variables.
#[cfg(not(windows))]
fn system_language() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(text: &str) -> Vec<String> {
        let mut found: Vec<String> = text
            .split('{')
            .skip(1)
            .filter_map(|rest| rest.split_once('}'))
            .map(|(name, _)| name.to_string())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn every_translation_has_exactly_the_texts_and_their_placeholders() {
        for (code, json) in TRANSLATIONS {
            let dictionary: Dictionary =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("{code}.json: {e}"));
            for text in TEXTS {
                let translation = dictionary
                    .get(*text)
                    .unwrap_or_else(|| panic!("{code}.json has no translation of {text:?}"));
                assert!(!translation.trim().is_empty(), "{code}: {text:?}");
                assert_eq!(
                    placeholders(translation),
                    placeholders(text),
                    "{code}: {text:?}"
                );
            }
            for key in dictionary.keys() {
                assert!(
                    TEXTS.contains(&key.as_str()),
                    "{code}.json translates {key:?}, which is not a text of the program"
                );
            }
        }
    }

    /// Every `i18n::t("...")` and `i18n::tf("...", ...)` in the program
    /// names a text of the list, so none stays untranslated unnoticed.
    #[test]
    fn the_code_only_asks_for_listed_texts() {
        fn visit(dir: &std::path::Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, found);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && path.file_name().is_some_and(|name| name != "i18n.rs")
                {
                    let code = std::fs::read_to_string(&path).unwrap();
                    for call in ["i18n::t(", "i18n::tf("] {
                        for rest in code.split(call).skip(1) {
                            let rest = rest.trim_start();
                            let literal = rest
                                .strip_prefix('"')
                                .and_then(|rest| rest.split_once('"'))
                                .map(|(text, _)| text.to_string())
                                .unwrap_or_else(|| {
                                    panic!("{}: {call} without a literal text", path.display())
                                });
                            found.push(literal);
                        }
                    }
                }
            }
        }
        let mut used = Vec::new();
        visit(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut used,
        );
        assert!(!used.is_empty());
        for text in &used {
            assert!(TEXTS.contains(&text.as_str()), "not in TEXTS: {text:?}");
        }
        for text in TEXTS {
            assert!(used.iter().any(|used| used == text), "never used: {text:?}");
        }
    }

    #[test]
    fn the_setting_or_the_system_picks_the_language() {
        assert_eq!(language_for("de", "pl-PL"), "de");
        assert_eq!(language_for("zh-Hans", ""), "zh-Hans");
        assert_eq!(language_for("en", "pl-PL"), "en");
        assert_eq!(language_for("system", "pl-PL"), "pl");
        assert_eq!(language_for("system", "pt-PT"), "pt-BR");
        assert_eq!(language_for("system", "pt_BR.UTF-8"), "pt-BR");
        assert_eq!(language_for("system", "zh-TW"), "zh-Hans");
        assert_eq!(language_for("system", "fr_CA.UTF-8"), "fr");
        assert_eq!(language_for("system", "ja-JP"), "en");
        assert_eq!(language_for("system", ""), "en");
        // A setting that names no language is not a reason to guess.
        assert_eq!(language_for("klingon", "pl-PL"), "en");
    }

    #[test]
    fn a_text_is_translated_and_its_placeholders_are_filled() {
        assert_eq!(translated("pl", "Quit"), "Zakończ");
        assert_eq!(translated("en", "Quit"), "Quit");
        assert_eq!(
            translated("pl", "not a text of the program"),
            "not a text of the program"
        );
        assert_eq!(
            filled("{name} is blocked", &[("name", "steam.exe")]),
            "steam.exe is blocked"
        );
        assert_eq!(
            filled("{added} of {max}", &[("added", "15"), ("max", "60")]),
            "15 of 60"
        );
    }
}
