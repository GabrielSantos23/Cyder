//! Translated strings for Sidra's own UI, ported from `src/i18n.ts`.
//!
//! The records are the same `assets/locales/*.json` files, compiled in with
//! `include_str!`, so there is no packaging step that can leave one behind.
//! Callers pass the OS's preferred-language list; nothing here asks the OS.

use icu_locale_core::LanguageIdentifier;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One translation record: BCP 47 tag to text.
pub type Record = HashMap<String, String>;

const LOCALE_FILES: [(&str, &str); 4] = [
    (
        "loading.json",
        include_str!("../../../assets/locales/loading.json"),
    ),
    (
        "tray.json",
        include_str!("../../../assets/locales/tray.json"),
    ),
    (
        "about.json",
        include_str!("../../../assets/locales/about.json"),
    ),
    (
        "update.json",
        include_str!("../../../assets/locales/update.json"),
    ),
];

fn records() -> &'static HashMap<String, Record> {
    static RECORDS: OnceLock<HashMap<String, Record>> = OnceLock::new();
    RECORDS.get_or_init(|| {
        let mut all = HashMap::new();
        for (file, json) in LOCALE_FILES {
            // The files are compiled in and covered by tests, so a parse failure is a build defect.
            let parsed: HashMap<String, Record> =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("assets/locales/{file}: {e}"));
            all.extend(parsed);
        }
        all
    })
}

/// A named record. Panics on an unknown name, which the tests rule out for every name used here.
pub fn record(name: &str) -> &'static Record {
    records()
        .get(name)
        .unwrap_or_else(|| panic!("no translation record named {name}"))
}

fn non_empty<'a>(record: &'a Record, tag: &str) -> Option<&'a str> {
    record
        .get(tag)
        .map(String::as_str)
        .filter(|v| !v.is_empty())
}

// shortcut: a two-line script table instead of CLDR likely-subtags, because only
// Chinese is ever maximised; switch to icu_locale's LocaleExpander if another
// language needs script resolution.
fn chinese_script(id: &LanguageIdentifier) -> &'static str {
    match (
        id.script.as_ref().map(|s| s.as_str()),
        id.region.as_ref().map(|r| r.as_str()),
    ) {
        (Some("Hant"), _) => "Hant",
        (Some(_), _) => "Hans",
        (None, Some("TW" | "HK" | "MO")) => "Hant",
        (None, _) => "Hans",
    }
}

fn region_script(region: &str) -> &'static str {
    if matches!(region, "TW" | "HK" | "MO") {
        "Hant"
    } else {
        "Hans"
    }
}

/// Resolves the preferred languages in order: the exact tag, its normalised base
/// name, the Chinese script, then the base language. English only when nothing
/// matches, so every record carries `en`. Returns the text and the tag it used.
pub fn localized_entry<'a, S: AsRef<str>>(record: &'a Record, langs: &[S]) -> (&'a str, String) {
    for lang in langs.iter().map(AsRef::as_ref) {
        if let Some(v) = non_empty(record, lang) {
            return (v, lang.to_owned());
        }
        // A malformed tag is skipped, as `new Intl.Locale()` throwing is.
        let Ok(id) = LanguageIdentifier::try_from_str(lang) else {
            continue;
        };
        let base_name = id.to_string();
        if let Some(v) = non_empty(record, &base_name) {
            return (v, base_name);
        }
        if id.language.as_str() == "zh" {
            let script = chinese_script(&id);
            let region = id.region.as_ref().map(|r| r.as_str());
            let regional = region.map(|r| format!("zh-{r}"));
            let tag = match (&regional, region) {
                (Some(t), Some(r))
                    if non_empty(record, t).is_some() && region_script(r) == script =>
                {
                    t.clone()
                }
                _ if script == "Hant" => "zh-TW".to_owned(),
                _ => "zh-CN".to_owned(),
            };
            if let Some(v) = non_empty(record, &tag) {
                return (v, tag);
            }
        }
        let base = id.language.as_str();
        if let Some(v) = non_empty(record, base) {
            return (v, base.to_owned());
        }
    }
    (
        record.get("en").map(String::as_str).unwrap_or(""),
        "en".to_owned(),
    )
}

/// The text for the preferred languages; see [`localized_entry`].
pub fn localized<S: AsRef<str>>(record: &Record, langs: &[S]) -> String {
    localized_entry(record, langs).0.to_owned()
}

fn t<S: AsRef<str>>(name: &str, langs: &[S]) -> String {
    localized(record(name), langs)
}

/// Replaces the first `{placeholder}` literally. A `$` in the value stays a `$`.
fn fill(template: String, placeholder: &str, value: &str) -> String {
    template.replacen(placeholder, value, 1)
}

/// The splash text and the tag it resolved to; Arabic and Hebrew switch the splash to right to left.
pub fn loading_text<S: AsRef<str>>(langs: &[S]) -> (String, String) {
    let (text, lang) = localized_entry(record("LOADING_TEXT"), langs);
    (text.to_owned(), lang)
}

/// Resolved labels shared by the tray and settings controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayStrings {
    pub settings: String,
    pub integrations: String,
    pub settings_error: String,
    pub lastfm: String,
    pub custom_scrobbler: String,
    pub lastfm_connected: String,
    pub about: String,
    pub quit: String,
    pub notifications: String,
    pub discord: String,
    pub player: String,
    pub lastfm_connect: String,
    pub lastfm_disconnect: String,
    pub start_page: String,
    pub start_page_home: String,
    pub start_page_new: String,
    pub start_page_radio: String,
    pub start_page_all_playlists: String,
    pub start_page_browse: String,
    pub start_page_library: String,
    pub start_page_playlists: String,
    pub start_page_search: String,
    pub start_page_last: String,
    pub on: String,
    pub off: String,
    pub style: String,
    pub style_apple_music: String,
    pub style_custom: String,
    pub zoom: String,
    pub zoom_100: String,
    pub zoom_125: String,
    pub zoom_150: String,
    pub zoom_175: String,
    pub zoom_200: String,
    pub previous: String,
    pub play: String,
    pub pause: String,
    pub not_playing: String,
    pub next: String,
    pub volume: String,
    pub mute: String,
    pub share: String,
    pub hide_window: String,
    pub show_window: String,
    pub close_to_tray: String,
}

/// Every tray label. A struct literal must name every field, so a label added to
/// [`TrayStrings`] and left out here fails to compile. `{name}` in About and the
/// window labels takes the product name, which stays out of every translation.
/// Brand names and zoom steps read the same in every language.
pub fn tray_strings<S: AsRef<str>>(langs: &[S], product_name: &str) -> TrayStrings {
    let named = |rec: &str| fill(t(rec, langs), "{name}", product_name);
    TrayStrings {
        settings: t("SETTINGS_TEXT", langs),
        integrations: t("INTEGRATIONS_TEXT", langs),
        settings_error: t("SETTINGS_ERROR_TEXT", langs),
        lastfm: "Last.fm".into(),
        custom_scrobbler: t("CUSTOM_SCROBBLER_TEXT", langs),
        lastfm_connected: t("LASTFM_CONNECTED_TEXT", langs),
        about: named("ABOUT_TEXT"),
        quit: t("QUIT_TEXT", langs),
        notifications: t("NOTIFICATIONS_TEXT", langs),
        discord: t("DISCORD_TEXT", langs),
        player: t("PLAYER_TEXT", langs),
        lastfm_connect: t("LASTFM_CONNECT_TEXT", langs),
        lastfm_disconnect: t("LASTFM_DISCONNECT_TEXT", langs),
        start_page: t("START_PAGE_TEXT", langs),
        start_page_home: t("START_PAGE_HOME_TEXT", langs),
        start_page_new: t("START_PAGE_NEW_TEXT", langs),
        start_page_radio: t("START_PAGE_RADIO_TEXT", langs),
        start_page_all_playlists: t("START_PAGE_ALL_PLAYLISTS_TEXT", langs),
        start_page_browse: t("START_PAGE_BROWSE_TEXT", langs),
        start_page_library: t("START_PAGE_LIBRARY_TEXT", langs),
        start_page_playlists: t("START_PAGE_PLAYLISTS_TEXT", langs),
        start_page_search: t("START_PAGE_SEARCH_TEXT", langs),
        start_page_last: t("START_PAGE_LAST_TEXT", langs),
        on: t("ON_TEXT", langs),
        off: t("OFF_TEXT", langs),
        style: t("STYLE_TEXT", langs),
        style_apple_music: "Apple Music".into(),
        style_custom: t("STYLE_CUSTOM_TEXT", langs),
        zoom: t("ZOOM_TEXT", langs),
        zoom_100: "100%".into(),
        zoom_125: "125%".into(),
        zoom_150: "150%".into(),
        zoom_175: "175%".into(),
        zoom_200: "200%".into(),
        previous: t("PREVIOUS_TEXT", langs),
        play: t("PLAY_TEXT", langs),
        pause: t("PAUSE_TEXT", langs),
        not_playing: t("NOT_PLAYING_TEXT", langs),
        next: t("NEXT_TEXT", langs),
        volume: t("VOLUME_TEXT", langs),
        mute: t("MUTE_TEXT", langs),
        share: t("SHARE_TEXT", langs),
        hide_window: named("HIDE_WINDOW_TEXT"),
        show_window: named("SHOW_WINDOW_TEXT"),
        close_to_tray: t("CLOSE_TO_TRAY_TEXT", langs),
    }
}

/// The Discord button label naming the service.
pub fn discord_play_on_text<S: AsRef<str>>(langs: &[S], service: &str) -> String {
    fill(t("DISCORD_PLAY_ON_TEXT", langs), "{service}", service)
}

/// The Discord artist line, with translated text for an unknown artist.
pub fn discord_artist_text<S: AsRef<str>>(langs: &[S], artist: Option<&str>) -> String {
    let unknown;
    let name = match artist {
        Some(a) => a,
        None => {
            unknown = t("UNKNOWN_ARTIST_TEXT", langs);
            &unknown
        }
    };
    fill(t("DISCORD_BY_ARTIST_TEXT", langs), "{artist}", name)
}

/// The connected Last.fm account label.
pub fn lastfm_connected_text<S: AsRef<str>>(langs: &[S], name: &str) -> String {
    fill(t("LASTFM_CONNECTED_TEXT", langs), "{name}", name)
}

/// The Last.fm connection failure message.
pub fn lastfm_connect_failed_text<S: AsRef<str>>(langs: &[S]) -> String {
    t("LASTFM_CONNECT_FAILED_TEXT", langs)
}

/// Labels for the injected navigation bar.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NavigationStrings {
    pub settings: String,
    pub back: String,
    pub forward: String,
    pub reload: String,
}

pub fn navigation_strings<S: AsRef<str>>(langs: &[S]) -> NavigationStrings {
    NavigationStrings {
        settings: t("SETTINGS_TEXT", langs),
        back: t("BACK_TEXT", langs),
        forward: t("FORWARD_TEXT", langs),
        reload: t("RELOAD_TEXT", langs),
    }
}

/// The About window text and metadata labels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AboutStrings {
    pub description: String,
    pub close: String,
    pub version_prefix: String,
    pub copyright_suffix: String,
    pub license_prefix: String,
}

pub fn about_strings<S: AsRef<str>>(langs: &[S]) -> AboutStrings {
    AboutStrings {
        description: t("ABOUT_DESCRIPTION_TEXT", langs),
        close: t("CLOSE_TEXT", langs),
        version_prefix: t("VERSION_PREFIX", langs),
        copyright_suffix: t("COPYRIGHT_SUFFIX", langs),
        license_prefix: t("LICENSE_PREFIX", langs),
    }
}

/// Update availability messages, and the downloaded-update prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateStrings {
    pub update_available: String,
    pub up_to_date: String,
    pub ready: String,
    pub restart_now: String,
    pub later: String,
}

pub fn update_strings<S: AsRef<str>>(langs: &[S]) -> UpdateStrings {
    UpdateStrings {
        update_available: t("UPDATE_AVAILABLE_TEXT", langs),
        up_to_date: t("UP_TO_DATE_TEXT", langs),
        ready: t("UPDATE_READY_TEXT", langs),
        restart_now: t("RESTART_NOW_TEXT", langs),
        later: t("LATER_TEXT", langs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_service::all_services;

    fn tag_record() -> Record {
        record("LOADING_TEXT")
            .keys()
            .map(|k| (k.clone(), k.clone()))
            .collect()
    }

    #[test]
    fn resolves_exact_base_and_english() {
        let r = record("LOADING_TEXT");
        assert_eq!(localized(r, &["zh-TW"]), "載入中…");
        assert_eq!(localized(r, &["fr-CA"]), "Chargement...");
        assert_eq!(localized(r, &["xx-YY"]), "Loading...");
        assert_eq!(localized(r, &["ja", "fr"]), "読み込み中…");
        assert_eq!(localized::<&str>(r, &[]), "Loading...");
        assert_eq!(localized(r, &["not_a_tag", "fr"]), r["fr"]);
    }

    #[test]
    fn resolves_chinese_scripts_before_base_language() {
        let r = tag_record();
        for (requested, resolved) in [
            ("zh-Hans-CN", "zh-CN"),
            ("zh-Hans", "zh-CN"),
            ("zh-Hant-TW", "zh-TW"),
            ("zh-Hant", "zh-TW"),
            ("zh-Hant-HK", "zh-HK"),
            ("zh-Hans-SG", "zh-SG"),
            ("zh-Hant-US", "zh-TW"),
            ("zh-Hans-HK", "zh-CN"),
            ("ZH-hant-hk", "zh-HK"),
        ] {
            assert_eq!(localized(&r, &[requested, "en"]), resolved, "{requested}");
        }
    }

    #[test]
    fn french_labels_and_literal_placeholders() {
        let fr = ["fr-CA"];
        assert_eq!(
            tray_strings(&fr, "Sidra").style_custom,
            "Thème personnalisé"
        );
        assert_eq!(
            discord_play_on_text(&fr, "Apple Music Classical"),
            "Lire sur Apple Music Classical"
        );
        assert_eq!(discord_artist_text(&fr, None), "par Artiste inconnu");
        assert_eq!(discord_artist_text(&fr, Some("$&")), "par $&");
        assert_eq!(
            about_strings(&fr).description,
            "Un client de bureau minimaliste pour Apple Music."
        );
        assert_eq!(loading_text(&fr).1, "fr");
    }

    #[test]
    fn tray_and_navigation_share_labels_and_fill_the_name() {
        let en = ["en"];
        let tray = tray_strings(&en, "Sidra");
        assert_eq!(navigation_strings(&en).settings, tray.settings);
        assert!(tray.lastfm_connected.contains("{name}"));
        assert!(tray.about.contains("Sidra") && !tray.about.contains("{name}"));
        assert_eq!(
            lastfm_connected_text(&en, "$1user"),
            tray.lastfm_connected.replacen("{name}", "$1user", 1)
        );
    }

    #[test]
    fn discord_buttons_fit_the_32_character_limit() {
        for (lang, template) in record("DISCORD_PLAY_ON_TEXT") {
            for s in all_services() {
                let label = template.replace("{service}", s.display_name);
                assert!(label.contains(s.display_name), "{lang}: {}", s.host);
                assert!(label.chars().count() <= 32, "{lang}: {}", s.host);
            }
        }
    }

    fn placeholders(v: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = v;
        while let Some(start) = rest.find('{') {
            let Some(len) = rest[start..].find('}') else {
                break;
            };
            let inner = &rest[start..start + len + 1];
            if !inner[1..].contains('{') {
                out.push(inner);
            }
            rest = &rest[start + 1..];
        }
        out.sort();
        out
    }

    #[test]
    fn every_record_is_complete_and_keeps_placeholders() {
        let mut reference: Vec<_> = record("LOADING_TEXT").keys().collect();
        reference.sort();
        for (name, rec) in records() {
            let mut keys: Vec<_> = rec.keys().collect();
            keys.sort();
            assert_eq!(
                keys, reference,
                "{name} has different languages from LOADING_TEXT"
            );
            let expected = placeholders(&rec["en"]);
            for (lang, value) in rec {
                assert!(!value.trim().is_empty(), "{name}[{lang}] is empty");
                assert_eq!(placeholders(value), expected, "{name}[{lang}] placeholders");
            }
        }
    }

    #[test]
    fn every_locale_record_is_used_by_this_module() {
        // Derived from the files and this source, so no hand-written list can drift.
        let source = include_str!("i18n.rs");
        for name in records().keys() {
            assert!(
                source.contains(&format!("\"{name}\"")),
                "{name} is in assets/locales but unused"
            );
        }
        // And every name used here exists: building every string panics on an unknown one.
        let langs = ["en"];
        tray_strings(&langs, "Sidra");
        navigation_strings(&langs);
        about_strings(&langs);
        update_strings(&langs);
        discord_artist_text(&langs, None);
        discord_play_on_text(&langs, "x");
        lastfm_connect_failed_text(&langs);
        loading_text(&langs);
    }
}
