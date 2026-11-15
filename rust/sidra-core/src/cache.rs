//! A disk cache for Apple Music API responses, and how long each kind stays fresh.
//!
//! A stale entry is still served: the page opens at once from disk and the caller
//! refetches in the background. Only a missing entry makes a page wait.

use md5::{Digest, Md5};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;

/// What a cached response is, which decides how long it stays fresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A released album's track list almost never changes.
    CatalogAlbum,
    Artist,
    /// Playlists refresh on their own `lastModifiedDate` (Unix seconds), when known.
    Playlist {
        modified: Option<u64>,
    },
    /// Recommendations and recently played.
    Home,
    /// The user's own library, which changes whenever they add or remove something.
    Library,
    Lyrics,
}

/// Seconds an entry of `kind` stays fresh at `now`.
pub fn max_age(kind: Kind, now: u64) -> u64 {
    match kind {
        Kind::CatalogAlbum => 7 * DAY,
        Kind::Artist => DAY,
        Kind::Home => 6 * HOUR,
        Kind::Library => HOUR,
        Kind::Lyrics => 30 * DAY,
        // A playlist edited recently is likely to be edited again soon; one untouched
        // for a week is likely to stay that way.
        Kind::Playlist { modified: None } => 6 * HOUR,
        Kind::Playlist { modified: Some(at) } => match now.saturating_sub(at) {
            age if age < DAY => 2 * HOUR,
            age if age < 7 * DAY => 6 * HOUR,
            _ => DAY,
        },
    }
}

/// Unix seconds from an ISO 8601 UTC timestamp such as `2024-05-01T12:30:00Z`.
pub fn parse_iso8601(text: &str) -> Option<u64> {
    let num = |r: std::ops::Range<usize>| text.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = if text.len() >= 19 {
        (num(11..13)?, num(14..16)?, num(17..19)?)
    } else {
        (0, 0, 0)
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * DAY as i64 + hh * 3600 + mm * 60 + ss).ok()
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub struct Hit {
    pub body: Value,
    pub fresh: bool,
}

/// Responses on disk, one JSON file per request. Library responses live in their own
/// folder so a change to the library can drop all of them at once.
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, key: &str) -> PathBuf {
        let hash = Md5::digest(key.as_bytes());
        let name: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        let folder = if key.contains("/me/") {
            "library"
        } else {
            "catalog"
        };
        self.dir.join(folder).join(format!("{name}.json"))
    }

    pub fn get(&self, key: &str) -> Option<Hit> {
        self.get_at(key, now())
    }

    fn get_at(&self, key: &str, now: u64) -> Option<Hit> {
        let text = std::fs::read_to_string(self.path(key)).ok()?;
        let mut entry: Value = serde_json::from_str(&text).ok()?;
        // A hash collision would otherwise serve another request's page.
        if entry.get("key").and_then(Value::as_str) != Some(key) {
            return None;
        }
        let saved = entry.get("saved").and_then(Value::as_u64)?;
        let age = entry.get("maxAge").and_then(Value::as_u64)?;
        Some(Hit {
            body: entry.get_mut("body")?.take(),
            fresh: now < saved + age,
        })
    }

    pub fn put(&self, key: &str, body: &Value, kind: Kind) -> std::io::Result<()> {
        self.put_at(key, body, kind, now())
    }

    fn put_at(&self, key: &str, body: &Value, kind: Kind, now: u64) -> std::io::Result<()> {
        let path = self.path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let entry = serde_json::json!({
            "key": key, "saved": now, "maxAge": max_age(kind, now), "body": body,
        });
        // Write then rename, so a crash mid-write never leaves a truncated entry behind.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, entry.to_string())?;
        std::fs::rename(tmp, path)
    }

    pub fn remove(&self, key: &str) {
        let _ = std::fs::remove_file(self.path(key));
    }

    /// Drops every library response, after Sidra changes the library itself.
    pub fn clear_library(&self) {
        let _ = std::fs::remove_dir_all(self.dir.join("library"));
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates() {
        assert_eq!(parse_iso8601("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso8601("2024-05-01T12:30:00Z"), Some(1_714_566_600));
        assert_eq!(parse_iso8601("2024-05-01"), Some(1_714_521_600));
        assert_eq!(parse_iso8601("2024-13-01"), None);
        assert_eq!(parse_iso8601("soon"), None);
    }

    #[test]
    fn playlists_refresh_by_how_recently_they_changed() {
        let now = 100 * DAY;
        let p = |ago| {
            max_age(
                Kind::Playlist {
                    modified: Some(now - ago),
                },
                now,
            )
        };
        assert_eq!(p(HOUR), 2 * HOUR);
        assert_eq!(p(3 * DAY), 6 * HOUR);
        assert_eq!(p(30 * DAY), DAY);
        // A clock behind the server's timestamp counts as just modified.
        assert_eq!(
            max_age(
                Kind::Playlist {
                    modified: Some(now + 50)
                },
                now
            ),
            2 * HOUR
        );
    }

    #[test]
    fn entries_go_stale_but_are_still_served() {
        let dir = std::env::temp_dir().join(format!("sidra-cache-test-{}", std::process::id()));
        let cache = Cache::new(&dir);
        let body = serde_json::json!({"data": [1]});
        cache
            .put_at("/v1/me/library/albums", &body, Kind::Library, 1000)
            .unwrap();
        let hit = cache
            .get_at("/v1/me/library/albums", 1000 + HOUR - 1)
            .unwrap();
        assert!(hit.fresh);
        assert_eq!(hit.body, body);
        assert!(
            !cache
                .get_at("/v1/me/library/albums", 1000 + HOUR)
                .unwrap()
                .fresh
        );
        cache.clear_library();
        assert!(cache.get_at("/v1/me/library/albums", 1000).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
