//! Launch URLs and storefront bookkeeping, ported from `src/storefront.ts`.
//!
//! The TypeScript reads and writes config inline. Here every function takes the
//! stored values as arguments and returns what should change, so config stays a
//! separate layer and each rule is tested without a store.
//!
//! The storefront matters for playback, not only for links: Apple's player sets
//! `previewOnly` whenever the page storefront differs from the account's, so a
//! page on the wrong storefront plays 30 to 90 second previews to a subscriber.

use crate::itms::ItmsRouteToken;
use crate::music_service::{get_service, service_by_host, MusicServiceId, MUSIC};
use icu_locale_core::Locale;
use url::Url;

/// The region Apple Music shops in when neither config nor the OS names one.
pub const FALLBACK_STOREFRONT: &str = "us";

/// The persisted storefront, else the OS region lowercased, else `us`. The region
/// comes from the locale's country, not its language: a Welsh UI in the UK still shops in `gb`.
pub fn resolve_storefront(persisted: Option<&str>, locale_country: &str) -> String {
    match persisted {
        Some(s) => s.to_owned(),
        None if !locale_country.is_empty() => locale_country.to_lowercase(),
        None => FALLBACK_STOREFRONT.to_owned(),
    }
}

fn is_storefront_code(segment: &str) -> bool {
    segment.len() == 2 && segment.bytes().all(|b| b.is_ascii_lowercase())
}

/// Sets, replaces or removes the `l` query parameter. With no language and no
/// `l` present the input comes back untouched, as `applyLanguage()` does.
fn apply_language(url: &str, language: Option<&str>) -> String {
    let Ok(mut parsed) = Url::parse(url) else {
        return url.to_owned();
    };
    let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
    let has_l = pairs.iter().any(|(k, _)| k == "l");
    if language.is_none() && !has_l {
        return url.to_owned();
    }
    // URLSearchParams.set() replaces the first `l` in place and drops the rest.
    let mut out: Vec<(String, String)> = Vec::with_capacity(pairs.len() + 1);
    let mut placed = false;
    for (k, v) in pairs {
        if k != "l" {
            out.push((k, v));
        } else if let (Some(lang), false) = (language, placed) {
            out.push((k, lang.to_owned()));
            placed = true;
        }
    }
    if let (Some(lang), false) = (language, placed) {
        out.push(("l".into(), lang.to_owned()));
    }
    if out.is_empty() {
        parsed.set_query(None);
    } else {
        parsed.query_pairs_mut().clear().extend_pairs(out);
    }
    parsed.into()
}

/// What `buildAppleMusicURL()` reads from config, passed in.
#[derive(Clone, Copy, Debug)]
pub struct LaunchInputs<'a> {
    pub service: MusicServiceId,
    pub storefront: &'a str,
    pub language: Option<&'a str>,
    /// The stored start page id, `last`, or `None` when nothing is stored.
    pub start_page: Option<&'a str>,
    /// The stored last page, storefront stripped; used when `start_page` is `last`.
    pub last_page_path: Option<&'a str>,
}

/// The launch URL: the stored start page on the given storefront, falling back to
/// the service default when the stored id names nothing valid.
pub fn build_launch_url(i: &LaunchInputs) -> String {
    let service = get_service(i.service);
    if i.start_page == Some("last") {
        // Classical's Home is stored as the empty root, possibly with a query, and
        // must not gain a trailing slash; only an absent value falls through.
        if let Some(last) = i.last_page_path {
            let sep = if last.is_empty() || last.starts_with('?') {
                ""
            } else {
                "/"
            };
            return apply_language(
                &format!("{}/{}{sep}{last}", service.origin, i.storefront),
                i.language,
            );
        }
    }
    let page = i
        .start_page
        .and_then(|id| service.start_page(id))
        .or_else(|| service.start_page(service.default_start_page))
        .unwrap_or(&service.start_pages[0]);
    let path = if page.path.is_empty() {
        String::new()
    } else {
        format!("/{}", page.path)
    };
    apply_language(
        &format!("{}/{}{path}", service.origin, i.storefront),
        i.language,
    )
}

fn itms_route_path(token: ItmsRouteToken) -> &'static str {
    match token {
        ItmsRouteToken::Library => "library",
        ItmsRouteToken::Browse => "browse",
        ItmsRouteToken::Radio => "radio",
        ItmsRouteToken::ListenNow => "listen-now",
        ItmsRouteToken::Subscribe => "subscribe",
    }
}

/// In-app URL for an `itms://` route token. Always the music service, whatever is active.
pub fn build_itms_route_url(
    token: ItmsRouteToken,
    storefront: &str,
    music_language: Option<&str>,
) -> String {
    apply_language(
        &format!("{}/{storefront}/{}", MUSIC.origin, itms_route_path(token)),
        music_language,
    )
}

/// The service, storefront and explicit `l` language of a service URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorefrontInfo {
    pub service: MusicServiceId,
    pub storefront: String,
    pub language: Option<String>,
}

/// `None` when the URL is unparseable, the host is not a service, or the first
/// path segment is not a two-letter lowercase storefront code.
pub fn extract_storefront_from_url(url: &str) -> Option<StorefrontInfo> {
    let parsed = Url::parse(url).ok()?;
    let service = service_by_host(parsed.host_str()?)?;
    let storefront = parsed.path_segments()?.find(|s| !s.is_empty())?;
    if !is_storefront_code(storefront) {
        return None;
    }
    let language = parsed
        .query_pairs()
        .find(|(k, _)| k == "l")
        .map(|(_, v)| v.into_owned());
    Some(StorefrontInfo {
        service: service.id,
        storefront: storefront.to_owned(),
        language,
    })
}

/// What to persist after Apple's own region or language controls navigate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorefrontUpdate {
    pub storefront: Option<String>,
    /// The language to store for `service`. Absence of `l` in the URL means no change.
    pub language: Option<String>,
}

/// Compares a navigated URL with the stored storefront and the service's stored
/// language. `None` when the URL is not a service URL or nothing changed.
pub fn storefront_update(
    url: &str,
    current_storefront: Option<&str>,
    current_language_for: impl Fn(MusicServiceId) -> Option<String>,
) -> Option<(MusicServiceId, StorefrontUpdate)> {
    let info = extract_storefront_from_url(url)?;
    let current_language = current_language_for(info.service);
    let update = StorefrontUpdate {
        storefront: (Some(info.storefront.as_str()) != current_storefront)
            .then(|| info.storefront.clone()),
        language: info
            .language
            .filter(|l| Some(l) != current_language.as_ref()),
    };
    (update != StorefrontUpdate::default()).then_some((info.service, update))
}

/// The page to remember for the `last` start page: path and query, storefront
/// segment and fragment stripped. A bare root is a page only on a service that
/// declares a root start page (Classical's Home); elsewhere it is a redirect stop.
pub fn last_page_path(url: &str) -> Option<(MusicServiceId, String)> {
    let parsed = Url::parse(url).ok()?;
    let service = service_by_host(parsed.host_str()?)?;
    let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
    let page = match segments.first() {
        Some(first) if is_storefront_code(first) => &segments[1..],
        _ => &segments[..],
    }
    .join("/");
    if page.is_empty() && !service.start_pages.iter().any(|p| p.path.is_empty()) {
        return None;
    }
    let query = parsed.query().map(|q| format!("?{q}")).unwrap_or_default();
    Some((service.id, format!("{page}{query}")))
}

/// Normalises `document.documentElement.lang` for storage, as `new Intl.Locale(lang)`
/// does. `None` for an empty, malformed or undetermined (`und`) tag.
pub fn normalise_page_language(tag: &str) -> Option<String> {
    let locale = Locale::try_from_str(tag).ok()?;
    if locale.id.language.is_unknown() {
        return None;
    }
    Some(locale.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_service::{CLASSICAL, MUSIC};

    fn launch(
        service: MusicServiceId,
        storefront: &str,
        start: Option<&str>,
        last: Option<&str>,
        lang: Option<&str>,
    ) -> String {
        build_launch_url(&LaunchInputs {
            service,
            storefront,
            language: lang,
            start_page: start,
            last_page_path: last,
        })
    }
    use MusicServiceId::{Classical, Music};

    #[test]
    fn storefront_prefers_persisted_then_locale_then_us() {
        assert_eq!(resolve_storefront(Some("gb"), "DE"), "gb");
        assert_eq!(resolve_storefront(None, "DE"), "de");
        assert_eq!(resolve_storefront(None, ""), "us");
    }

    #[test]
    fn music_start_pages() {
        assert_eq!(
            launch(Music, "us", Some("home"), None, None),
            "https://music.apple.com/us/home"
        );
        assert_eq!(
            launch(Music, "us", Some("new"), None, None),
            "https://music.apple.com/us/new"
        );
        assert_eq!(
            launch(Music, "us", Some("radio"), None, None),
            "https://music.apple.com/us/radio"
        );
        assert_eq!(
            launch(Music, "us", Some("all-playlists"), None, None),
            "https://music.apple.com/us/library/all-playlists/"
        );
        for page in MUSIC.start_pages {
            assert_eq!(
                launch(Music, "gb", Some(page.id), None, None),
                format!("https://music.apple.com/gb/{}", page.path)
            );
        }
    }

    #[test]
    fn music_last_page_and_fallbacks() {
        assert_eq!(
            launch(Music, "us", Some("last"), Some("album/foo/123"), None),
            "https://music.apple.com/us/album/foo/123"
        );
        assert_eq!(
            launch(Music, "us", Some("last"), None, None),
            "https://music.apple.com/us/new"
        );
        assert_eq!(
            launch(Music, "us", None, None, None),
            "https://music.apple.com/us/new"
        );
        assert_eq!(
            launch(Music, "us", Some("removed-page"), None, None),
            "https://music.apple.com/us/new"
        );
        assert_eq!(
            launch(Music, "us", Some("last"), Some("search?term=jazz"), None),
            "https://music.apple.com/us/search?term=jazz"
        );
    }

    #[test]
    fn language_is_added_replaced_and_removed() {
        assert_eq!(
            launch(
                Music,
                "us",
                Some("last"),
                Some("search?term=jazz"),
                Some("en-GB")
            ),
            "https://music.apple.com/us/search?term=jazz&l=en-GB"
        );
        assert_eq!(
            launch(
                Music,
                "us",
                Some("last"),
                Some("search?term=jazz&l=fr"),
                Some("en-GB")
            ),
            "https://music.apple.com/us/search?term=jazz&l=en-GB"
        );
        assert_eq!(
            launch(Classical, "gb", Some("last"), Some("?l=fr"), None),
            "https://classical.music.apple.com/gb"
        );
        assert_eq!(
            launch(Music, "us", Some("new"), None, Some("en-GB")),
            "https://music.apple.com/us/new?l=en-GB"
        );
    }

    #[test]
    fn classical_start_pages() {
        assert_eq!(
            launch(Classical, "gb", Some("library"), None, None),
            "https://classical.music.apple.com/gb"
        );
        assert_eq!(
            launch(Classical, "gb", Some("home"), None, None),
            "https://classical.music.apple.com/gb"
        );
        assert_eq!(
            launch(Classical, "gb", Some("browse"), None, None),
            "https://classical.music.apple.com/gb/browse/catalog"
        );
        assert_eq!(
            launch(Classical, "gb", Some("playlists"), None, None),
            "https://classical.music.apple.com/gb/browse/playlists"
        );
        assert_eq!(
            launch(Classical, "gb", Some("search"), None, None),
            "https://classical.music.apple.com/gb/search"
        );
        assert_eq!(
            launch(Classical, "gb", Some("last"), Some("browse/albums"), None),
            "https://classical.music.apple.com/gb/browse/albums"
        );
        assert_eq!(
            launch(Classical, "gb", Some("last"), None, None),
            "https://classical.music.apple.com/gb"
        );
        assert_eq!(
            launch(Classical, "gb", Some("last"), Some(""), None),
            "https://classical.music.apple.com/gb"
        );
        assert_eq!(CLASSICAL.start_pages.len(), 4);
    }

    #[test]
    fn itms_routes_always_use_the_music_host() {
        assert_eq!(
            build_itms_route_url(ItmsRouteToken::Library, "gb", Some("en-GB")),
            "https://music.apple.com/gb/library?l=en-GB"
        );
        assert_eq!(
            build_itms_route_url(ItmsRouteToken::Browse, "de", None),
            "https://music.apple.com/de/browse"
        );
        assert_eq!(
            build_itms_route_url(ItmsRouteToken::ListenNow, "gb", None),
            "https://music.apple.com/gb/listen-now"
        );
    }

    #[test]
    fn extracts_storefront_and_language() {
        let info =
            extract_storefront_from_url("https://music.apple.com/gb/album/x/1?l=en-GB").unwrap();
        assert_eq!(
            info,
            StorefrontInfo {
                service: Music,
                storefront: "gb".into(),
                language: Some("en-GB".into())
            }
        );
        let info =
            extract_storefront_from_url("https://classical.music.apple.com/gb/browse").unwrap();
        assert_eq!(
            (info.service, info.storefront.as_str(), info.language),
            (Classical, "gb", None)
        );
        for bad in [
            "https://music.apple.com/",
            "https://music.apple.com/GB/new",
            "https://music.apple.com/gbr/new",
            "https://example.com/gb/new",
            "not a url",
        ] {
            assert_eq!(extract_storefront_from_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn storefront_update_reports_only_changes() {
        let none = |_| None;
        let en_gb = |_| Some("en-GB".to_string());
        assert_eq!(
            storefront_update("https://music.apple.com/gb/new?l=en-GB", Some("us"), none),
            Some((
                Music,
                StorefrontUpdate {
                    storefront: Some("gb".into()),
                    language: Some("en-GB".into())
                }
            ))
        );
        assert_eq!(
            storefront_update("https://music.apple.com/gb/new", Some("us"), none),
            Some((
                Music,
                StorefrontUpdate {
                    storefront: Some("gb".into()),
                    language: None
                }
            ))
        );
        assert_eq!(
            storefront_update("https://music.apple.com/gb/new?l=en-GB", Some("gb"), en_gb),
            None
        );
        assert_eq!(
            storefront_update("https://music.apple.com/gb/new", Some("gb"), en_gb),
            None
        );
        assert_eq!(
            storefront_update("https://music.apple.com/gb/new?l=cy", Some("gb"), en_gb),
            Some((
                Music,
                StorefrontUpdate {
                    storefront: None,
                    language: Some("cy".into())
                }
            ))
        );
        assert_eq!(
            storefront_update("https://example.com/gb/new", Some("us"), none),
            None
        );
        assert_eq!(storefront_update("::", Some("us"), none), None);
    }

    #[test]
    fn last_page_keeps_query_drops_storefront_and_fragment() {
        assert_eq!(
            last_page_path("https://music.apple.com/gb/album/foo/123"),
            Some((Music, "album/foo/123".into()))
        );
        assert_eq!(
            last_page_path("https://classical.music.apple.com/gb/browse/albums"),
            Some((Classical, "browse/albums".into()))
        );
        assert_eq!(
            last_page_path("https://music.apple.com/gb/search?term=jazz"),
            Some((Music, "search?term=jazz".into()))
        );
        assert_eq!(
            last_page_path("https://music.apple.com/gb/search?term=jazz#top"),
            Some((Music, "search?term=jazz".into()))
        );
        assert_eq!(
            last_page_path("https://music.apple.com/gb/browse"),
            Some((Music, "browse".into()))
        );
        assert_eq!(
            last_page_path("https://classical.music.apple.com/gb"),
            Some((Classical, "".into()))
        );
        assert_eq!(
            last_page_path("https://classical.music.apple.com/gb?l=en-GB"),
            Some((Classical, "?l=en-GB".into()))
        );
        assert_eq!(last_page_path("https://music.apple.com/gb"), None);
        assert_eq!(last_page_path("https://example.com/gb/browse"), None);
        assert_eq!(last_page_path("not a url"), None);
    }

    #[test]
    fn page_language_is_normalised_or_refused() {
        assert_eq!(normalise_page_language("ja").as_deref(), Some("ja"));
        assert_eq!(normalise_page_language("en-gb").as_deref(), Some("en-GB"));
        assert_eq!(normalise_page_language("pt-BR").as_deref(), Some("pt-BR"));
        assert_eq!(normalise_page_language("und"), None);
        assert_eq!(normalise_page_language(""), None);
        assert_eq!(normalise_page_language("not_a_tag"), None);
    }
}
