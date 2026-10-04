// Tray and window titles. The webview translates itself; this covers the bits
// drawn by the OS before any page exists. Catalogs are the same JSON files.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn parse(json: &str) -> HashMap<String, String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// "system", "en" or "fr". Empty means the preference has not been loaded yet.
static PREFERENCE: Mutex<String> = Mutex::new(String::new());

pub fn set_preference(pref: &str) {
    if let Ok(mut slot) = PREFERENCE.lock() {
        *slot = pref.to_string();
    }
}

fn resolved() -> String {
    let pref = PREFERENCE.lock().map(|slot| slot.clone()).unwrap_or_default();
    match pref.as_str() {
        "en" | "fr" => pref,
        _ => detect_language(),
    }
}

fn detect_language() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::GetUserDefaultUILanguage;
        let primary = unsafe { GetUserDefaultUILanguage() } & 0x3FF;
        // LANG_FRENCH
        if primary == 0x0C {
            return "fr".to_string();
        }
        return "en".to_string();
    }
    #[cfg(not(windows))]
    {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
            if let Ok(value) = std::env::var(key) {
                if value.to_ascii_lowercase().starts_with("fr") {
                    return "fr".to_string();
                }
            }
        }
        "en".to_string()
    }
}

pub fn t(key: &str) -> String {
    static EN: OnceLock<HashMap<String, String>> = OnceLock::new();
    static FR: OnceLock<HashMap<String, String>> = OnceLock::new();
    let en = EN.get_or_init(|| parse(include_str!("../../../locales/en.json")));
    let fr = FR.get_or_init(|| parse(include_str!("../../../locales/fr.json")));
    let active = if resolved() == "fr" { fr } else { en };
    active
        .get(key)
        .or_else(|| en.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn french_catalog_covers_english() {
        let en = parse(include_str!("../../../locales/en.json"));
        let fr = parse(include_str!("../../../locales/fr.json"));
        let missing: Vec<_> = en.keys().filter(|key| !fr.contains_key(*key)).collect();
        let extra: Vec<_> = fr.keys().filter(|key| !en.contains_key(*key)).collect();
        assert!(missing.is_empty(), "missing fr keys: {missing:?}");
        assert!(extra.is_empty(), "extra fr keys: {extra:?}");
    }
}
