//! Translations shared with the window: the same JSON files from dist/locales, built into the binary.
//! A language is one more file there and one more line in LOCALES. Missing keys fall back to English.

use std::collections::HashMap;
use std::sync::OnceLock;

pub const DEFAULT_LANG: &str = "en";

const LOCALES: &[(&str, &str)] = &[
    ("en", include_str!("../dist/locales/en.json")),
    ("ru", include_str!("../dist/locales/ru.json")),
];

fn tables() -> &'static HashMap<&'static str, HashMap<String, String>> {
    static T: OnceLock<HashMap<&'static str, HashMap<String, String>>> = OnceLock::new();
    T.get_or_init(|| {
        LOCALES
            .iter()
            .map(|(lang, json)| (*lang, serde_json::from_str(json).expect("locale file is valid JSON")))
            .collect()
    })
}

/// (code, name in that language) for the language picker
pub fn languages() -> Vec<(String, String)> {
    LOCALES.iter().map(|(code, _)| (code.to_string(), tr(code, "_name"))).collect()
}

pub fn tr(lang: &str, key: &str) -> String {
    let t = tables();
    t.get(lang)
        .and_then(|m| m.get(key))
        .or_else(|| t.get(DEFAULT_LANG).and_then(|m| m.get(key)))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// tr() with {name} placeholders filled in
pub fn trf(lang: &str, key: &str, args: &[(&str, String)]) -> String {
    let mut s = tr(lang, key);
    for (name, value) in args {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

pub fn duration(lang: &str, secs: i64) -> String {
    let s = secs.max(0);
    let (key, n) = match s {
        0..=119 => ("dur.s", s),
        120..=7199 => ("dur.min", s / 60),
        7200..=172_799 => ("dur.h", s / 3600),
        _ => ("dur.d", s / 86400),
    };
    trf(lang, key, &[("n", n.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_locale_has_every_english_key() {
        let t = tables();
        for (lang, table) in t {
            for key in t[DEFAULT_LANG].keys() {
                assert!(table.contains_key(key), "{lang} lacks {key}");
            }
        }
    }

    #[test]
    fn fills_placeholders_and_falls_back() {
        assert_eq!(trf("ru", "status.synced", &[("block", "5".into())]), "Синхронизирована · блок 5");
        assert_eq!(tr("xx", "btn.start"), "Start");
        assert_eq!(duration("en", 7200), "2 h");
    }
}
