//! Parser for `itms://` URLs delivered by the OS, ported from `src/itms.ts`.
//! This is a trust boundary: the URL comes from outside, so anything unexpected is `None`.

use crate::music_service::MUSIC;
use url::Url;

/// The named destinations Apple's `itms://music.apple.com/deeplink?p=` form carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItmsRouteToken {
    Library,
    Browse,
    Radio,
    ListenNow,
    Subscribe,
}

impl ItmsRouteToken {
    pub const ALL: [Self; 5] = [
        Self::Library,
        Self::Browse,
        Self::Radio,
        Self::ListenNow,
        Self::Subscribe,
    ];

    /// The `p` value Apple sends.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Browse => "browse",
            Self::Radio => "radio",
            Self::ListenNow => "listenNow",
            Self::Subscribe => "subscribe",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == value)
    }
}

/// Either a catalogue URL to open as-is, or a route the storefront resolves to one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItmsTarget {
    Url(String),
    Route(ItmsRouteToken),
}

/// Validates one `itms://` URL. A bad scheme or host is `None`, a `/deeplink` path
/// is `None` unless its `p` token is allowlisted, and any other path becomes an
/// HTTPS catalogue URL on the music origin with the `app` parameter stripped.
pub fn transform_itms_url(input: &str) -> Option<ItmsTarget> {
    let parsed = Url::parse(input).ok()?;
    // itms:// is permanently pinned to the music service.
    if parsed.scheme() != "itms" || parsed.host_str() != Some(MUSIC.host) {
        return None;
    }

    if parsed.path() == "/deeplink" {
        let token = parsed
            .query_pairs()
            .find(|(k, _)| k == "p")
            .map(|(_, v)| v)?;
        return ItmsRouteToken::parse(&token).map(ItmsTarget::Route);
    }

    // The registry owns the origin; input controls only the path and query.
    let mut rebuilt = Url::parse(&format!("{}/", MUSIC.origin)).ok()?;
    rebuilt.set_path(parsed.path());
    let kept: Vec<_> = parsed.query_pairs().filter(|(k, _)| k != "app").collect();
    if kept.is_empty() {
        rebuilt.set_query(None);
    } else {
        rebuilt.query_pairs_mut().clear().extend_pairs(kept);
    }
    Some(ItmsTarget::Url(rebuilt.into()))
}

/// The first valid `itms://` URL in a process argument list. The launch and
/// second-instance paths receive links as arguments rather than events.
pub fn extract_itms_url_from_argv<S: AsRef<str>>(argv: &[S]) -> Option<ItmsTarget> {
    argv.iter()
        .map(AsRef::as_ref)
        .filter(|a| a.starts_with("itms:"))
        .find_map(transform_itms_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Option<ItmsTarget> {
        Some(ItmsTarget::Url(s.into()))
    }

    #[test]
    fn rebuilds_catalogue_urls_as_https_without_app() {
        assert_eq!(
            transform_itms_url("itms://music.apple.com/gb/album/foo/123?app=music"),
            url("https://music.apple.com/gb/album/foo/123")
        );
        assert_eq!(
            transform_itms_url("itms://music.apple.com/gb/album/foo/123?app=music&i=789"),
            url("https://music.apple.com/gb/album/foo/123?i=789")
        );
    }

    #[test]
    fn maps_every_allowlisted_deeplink_token() {
        for token in ItmsRouteToken::ALL {
            let input = format!("itms://music.apple.com/deeplink?p={}", token.as_str());
            assert_eq!(transform_itms_url(&input), Some(ItmsTarget::Route(token)));
        }
    }

    #[test]
    fn rejects_unknown_or_missing_tokens() {
        assert_eq!(
            transform_itms_url("itms://music.apple.com/deeplink?p=bogus"),
            None
        );
        assert_eq!(transform_itms_url("itms://music.apple.com/deeplink"), None);
    }

    #[test]
    fn rejects_other_schemes_hosts_and_garbage() {
        for input in [
            "itmss://music.apple.com/gb/album/foo/123",
            "https://music.apple.com/gb/album/foo/123",
            "javascript:alert(1)",
            "data:text/html,<script>",
            "file:///etc/passwd",
            "itms://evil.example.com/gb/album/foo/123",
            "itms://attacker.music.apple.com/gb/album/foo/123",
            "not a url",
            "",
        ] {
            assert_eq!(transform_itms_url(input), None, "{input}");
        }
    }

    #[test]
    fn argv_returns_first_valid_link_anywhere() {
        let argv = [
            "/usr/bin/sidra",
            "--enable-features=Foo",
            "itms://music.apple.com/gb/album/foo/123?app=music",
        ];
        assert_eq!(
            extract_itms_url_from_argv(&argv),
            url("https://music.apple.com/gb/album/foo/123")
        );
        let argv = [
            "/usr/bin/sidra",
            "itms://music.apple.com/deeplink?p=radio",
            "--some-flag",
        ];
        assert_eq!(
            extract_itms_url_from_argv(&argv),
            Some(ItmsTarget::Route(ItmsRouteToken::Radio))
        );
        let argv = [
            "itms://evil.example.com/album/foo",
            "itms://music.apple.com/deeplink?p=browse",
            "itms://music.apple.com/deeplink?p=library",
        ];
        assert_eq!(
            extract_itms_url_from_argv(&argv),
            Some(ItmsTarget::Route(ItmsRouteToken::Browse))
        );
    }

    #[test]
    fn argv_without_a_valid_link_is_none() {
        assert_eq!(
            extract_itms_url_from_argv(&["/usr/bin/sidra", "--enable-features=Foo"]),
            None
        );
        assert_eq!(
            extract_itms_url_from_argv(&["/usr/bin/sidra", "itms://evil.example.com/album/foo"]),
            None
        );
        assert_eq!(extract_itms_url_from_argv::<&str>(&[]), None);
    }
}
