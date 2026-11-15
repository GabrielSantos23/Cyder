//! The registry of the two Apple web services, ported from `src/musicService.ts`.
//! Every host, origin and start page comes from here, never from a literal at the call site.

use url::Url;

/// The two Apple web services Sidra wraps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MusicServiceId {
    Music,
    Classical,
}

impl MusicServiceId {
    /// The id as persisted in config.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Music => "music",
            Self::Classical => "classical",
        }
    }

    /// Narrows a stored or externally supplied string to a registry id.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "music" => Some(Self::Music),
            "classical" => Some(Self::Classical),
            _ => None,
        }
    }
}

/// Service used when nothing is persisted, and when a stored id turns out not to be one.
pub const DEFAULT_SERVICE_ID: MusicServiceId = MusicServiceId::Music;

/// One start page choice: the id persisted in config, and its path under the storefront.
#[derive(Debug, PartialEq, Eq)]
pub struct StartPage {
    pub id: &'static str,
    pub path: &'static str,
}

/// A wrapped web service and everything Sidra needs to address and drive it.
#[derive(Debug)]
pub struct MusicService {
    pub id: MusicServiceId,
    pub host: &'static str,
    pub origin: &'static str,
    pub display_name: &'static str,
    /// Authentication iframe hostnames the sign-in frame fix may run in.
    pub auth_frame_hosts: &'static [&'static str],
    /// CSS selector probed to detect when the web app is interactive.
    pub content_ready_selector: &'static str,
    /// Ordered start pages for the Start Page menu. Never empty, so `[0]` is total.
    pub start_pages: &'static [StartPage],
    /// Start page id used when no persisted value exists.
    pub default_start_page: &'static str,
}

impl MusicService {
    /// The start page with this id, if the service offers one.
    pub fn start_page(&self, id: &str) -> Option<&'static StartPage> {
        self.start_pages.iter().find(|p| p.id == id)
    }
}

const SHARED_AUTH_FRAME_HOSTS: &[&str] = &["auth.music.apple.com", "idmsa.apple.com"];

/// Mirrors `CONTENT_READY_SELECTOR` in `src/contentReady.ts`.
pub const CONTENT_READY_SELECTOR: &str =
    r#"[data-testid="app-container"] amp-playback-controls-play[hydrated]"#;

pub static MUSIC: MusicService = MusicService {
    id: MusicServiceId::Music,
    host: "music.apple.com",
    origin: "https://music.apple.com",
    display_name: "Apple Music",
    auth_frame_hosts: SHARED_AUTH_FRAME_HOSTS,
    content_ready_selector: CONTENT_READY_SELECTOR,
    start_pages: &[
        StartPage {
            id: "home",
            path: "home",
        },
        StartPage {
            id: "new",
            path: "new",
        },
        StartPage {
            id: "radio",
            path: "radio",
        },
        StartPage {
            id: "all-playlists",
            path: "library/all-playlists/",
        },
    ],
    default_start_page: "new",
};

pub static CLASSICAL: MusicService = MusicService {
    id: MusicServiceId::Classical,
    host: "classical.music.apple.com",
    origin: "https://classical.music.apple.com",
    display_name: "Apple Music Classical",
    auth_frame_hosts: SHARED_AUTH_FRAME_HOSTS,
    content_ready_selector: CONTENT_READY_SELECTOR,
    start_pages: &[
        StartPage {
            id: "home",
            path: "",
        },
        StartPage {
            id: "browse",
            path: "browse/catalog",
        },
        StartPage {
            id: "playlists",
            path: "browse/playlists",
        },
        StartPage {
            id: "search",
            path: "search",
        },
    ],
    default_start_page: "home",
};

/// The registered service for an id.
pub fn get_service(id: MusicServiceId) -> &'static MusicService {
    match id {
        MusicServiceId::Music => &MUSIC,
        MusicServiceId::Classical => &CLASSICAL,
    }
}

/// Every registered service, for callers that iterate rather than look one up.
pub fn all_services() -> [&'static MusicService; 2] {
    [&MUSIC, &CLASSICAL]
}

/// The service serving a hostname, or `None` when the host is not one of ours.
pub fn service_by_host(host: &str) -> Option<&'static MusicService> {
    all_services().into_iter().find(|s| s.host == host)
}

/// True for a service host or one of its sign-in frame hosts.
pub fn is_allowed_navigation_host(host: &str) -> bool {
    all_services()
        .iter()
        .any(|s| s.host == host || s.auth_frame_hosts.contains(&host))
}

/// Accepts HTTPS URLs whose normalised hostname is registered exactly. A subdomain,
/// a suffix or a userinfo prefix of an allowed host is rejected; ports and
/// credentials are ignored, as in `isAllowedNavigationUrl()`.
pub fn is_allowed_navigation_url(url: &str) -> bool {
    match Url::parse(url) {
        Ok(parsed) => {
            parsed.scheme() == "https" && parsed.host_str().is_some_and(is_allowed_navigation_host)
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_reject_unknown() {
        for s in all_services() {
            assert_eq!(MusicServiceId::parse(s.id.as_str()), Some(s.id));
            assert!(std::ptr::eq(get_service(s.id), s));
        }
        assert_eq!(MusicServiceId::parse("spotify"), None);
        assert_eq!(MusicServiceId::parse("Music"), None);
    }

    #[test]
    fn default_start_page_exists_in_every_service() {
        for s in all_services() {
            assert!(!s.start_pages.is_empty());
            assert!(s.start_page(s.default_start_page).is_some(), "{}", s.host);
        }
    }

    #[test]
    fn finds_services_by_exact_host_only() {
        assert_eq!(
            service_by_host("music.apple.com").map(|s| s.id),
            Some(MusicServiceId::Music)
        );
        assert_eq!(
            service_by_host("classical.music.apple.com").map(|s| s.id),
            Some(MusicServiceId::Classical)
        );
        assert!(service_by_host("apple.com").is_none());
        assert!(service_by_host("evil.music.apple.com").is_none());
    }

    #[test]
    fn navigation_allows_services_and_auth_frames_over_https() {
        for url in [
            "https://music.apple.com/gb/new",
            "https://classical.music.apple.com/gb",
            "https://idmsa.apple.com/appleauth/auth",
            "https://auth.music.apple.com/x",
            "https://MUSIC.apple.com/gb",
            "https://music.apple.com:443/gb",
            "https://user:pass@music.apple.com/gb",
        ] {
            assert!(is_allowed_navigation_url(url), "{url}");
        }
    }

    #[test]
    fn navigation_rejects_other_hosts_schemes_and_garbage() {
        for url in [
            "http://music.apple.com/gb",
            "https://evil.music.apple.com/",
            "https://music.apple.com.evil.com/",
            "https://evilmusic.apple.com/",
            "https://music.apple.com@evil.com/",
            "https://apple.com/",
            "javascript://music.apple.com/%0aalert(1)",
            "file://music.apple.com/etc/passwd",
            "not a url",
            "",
        ] {
            assert!(!is_allowed_navigation_url(url), "{url}");
        }
    }
}
