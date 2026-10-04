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

/// The plural form of a count for a key with "_one", "_few" and "_many" variants: Russian has three forms
/// ("21 день", "22 дня", "25 дней"), English uses the same text for all of them.
pub fn plural_key(lang: &str, key: &str, n: i64) -> String {
    let form = match lang {
        "ru" => match (n % 10, n % 100) {
            (1, r) if r != 11 => "one",
            (2..=4, r) if !(12..=14).contains(&r) => "few",
            _ => "many",
        },
        _ if n == 1 => "one",
        _ => "many",
    };
    format!("{key}_{form}")
}

pub fn duration(lang: &str, secs: i64) -> String {
    let s = secs.max(0);
    let (key, n) = match s {
        0..=119 => ("dur.s".to_string(), s),
        120..=7199 => ("dur.min".to_string(), s / 60),
        7200..=172_799 => ("dur.h".to_string(), s / 3600),
        _ => (plural_key(lang, "dur.d", s / 86400), s / 86400),
    };
    trf(lang, &key, &[("n", n.to_string())])
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
    fn says_days_in_full() {
        assert_eq!(duration("en", 1903 * 86400), "1903 days");
        assert_eq!(duration("ru", 1903 * 86400), "1903 дня");
        assert_eq!(duration("ru", 1901 * 86400), "1901 день");
        assert_eq!(duration("ru", 1911 * 86400), "1911 дней");
        assert_eq!(duration("ru", 1905 * 86400), "1905 дней");
    }

    #[test]
    fn fills_placeholders_and_falls_back() {
        assert_eq!(trf("ru", "status.synced", &[("block", "5".into())]), "Синхронизирована · блок 5");
        assert_eq!(tr("xx", "btn.start"), "Start");
        assert_eq!(duration("en", 7200), "2 h");
    }
}
