//! Last.fm rules that need no network, ported from `src/integrations/lastfm/index.ts`:
//! the endpoint check, request signing, the scrobble threshold, which errors are
//! final, how a `track.scrobble` answer is read, and the persisted retry queue.
//! The HTTP calls, timers and auth flow stay with the integration that owns them.

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use url::Url;

/// The official API root.
pub const DEFAULT_API_URL: &str = "https://ws.audioscrobbler.com/2.0/";
/// Where the user approves the desktop auth token.
pub const AUTH_URL: &str = "https://www.last.fm/api/auth/";

/// Tracks of this length or less never scrobble.
pub const MIN_TRACK_LENGTH_MS: f64 = 30_000.0;
/// The play time that always scrobbles, whatever the track length.
pub const SCROBBLE_CAP_MS: f64 = 240_000.0;
/// Allowance for the playhead report arriving after the wall-clock timer fires.
pub const POSITION_TOLERANCE_MS: f64 = 2_000.0;
/// The most plays `track.scrobble` accepts in one request; it caps the queue and each drain.
pub const MAX_PENDING_SCROBBLES: usize = 50;
/// A request that never settles is abandoned after this long, never re-issued.
pub const REQUEST_TIMEOUT_MS: u64 = 30_000;
pub const AUTH_POLL_INTERVAL_MS: u64 = 4_000;
pub const AUTH_POLL_TIMEOUT_MS: u64 = 120_000;

/// Error 9, "Invalid session key": the session was revoked, so the integration disconnects.
pub const INVALID_SESSION_ERROR: i64 = 9;

/// Codes that say nothing about the play itself. Four report the service (8 operation
/// failed, 11 offline, 16 temporarily unavailable, 29 rate limit) and two report
/// Sidra's own application key (10 invalid, 26 suspended), which Last.fm can restore
/// server-side. Batch contents cannot provoke any of them, so a held play cannot
/// wedge the queue. Every other code is Last.fm's word on the play and is final.
pub const RETRIABLE_ERRORS: [i64; 6] = [8, 10, 11, 16, 26, 29];

/// The API root for a configured custom scrobbler endpoint, or the official root
/// when none is configured. `None` for a custom URL that is not plain HTTP(S) or
/// that carries credentials, a query or a fragment.
pub fn resolve_api_url(configured: Option<&str>) -> Option<String> {
    let Some(raw) = configured else {
        return Some(DEFAULT_API_URL.to_owned());
    };
    let url = Url::parse(raw).ok()?;
    let clean = matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().unwrap_or("").is_empty()
        && url.fragment().unwrap_or("").is_empty();
    clean.then(|| url.into())
}

/// The `api_sig` per the Last.fm spec: params sorted by name, each name and value
/// concatenated, the shared secret appended, then MD5 as lowercase hex. `format`
/// and `callback` are added to the request by the caller and never signed.
pub fn sign_params(params: &BTreeMap<String, String>, secret: &str) -> String {
    let mut hasher = Md5::new();
    for (k, v) in params {
        hasher.update(k.as_bytes());
        hasher.update(v.as_bytes());
    }
    hasher.update(secret.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The play time a track needs before it scrobbles: half its length or four minutes,
/// whichever comes first. `None` for a track of 30 seconds or less. A non-positive
/// duration is unknown and uses the four-minute fallback.
pub fn scrobble_threshold_ms(duration_ms: f64) -> Option<f64> {
    if duration_ms > 0.0 && duration_ms <= MIN_TRACK_LENGTH_MS {
        None
    } else if duration_ms <= 0.0 {
        Some(SCROBBLE_CAP_MS)
    } else {
        Some((duration_ms / 2.0).min(SCROBBLE_CAP_MS))
    }
}

/// How a request failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LastfmError {
    /// Last.fm answered with its own error code in the body. The body is read
    /// before the HTTP status, because Last.fm sends several codes with a non-2xx status.
    Api { code: i64, message: String },
    /// The request never got an answer: network failure or timeout.
    Transport(String),
    /// An answer that was not JSON, or a bad status with no code in the body.
    BadResponse(String),
}

impl LastfmError {
    /// True when this is Last.fm's final word on the play: an API code outside
    /// [`RETRIABLE_ERRORS`]. Transport and response failures say nothing about the play.
    pub fn is_final_refusal(&self) -> bool {
        matches!(self, Self::Api { code, .. } if !RETRIABLE_ERRORS.contains(code))
    }

    /// True for error 9, which disconnects.
    pub fn is_invalid_session(&self) -> bool {
        matches!(
            self,
            Self::Api {
                code: INVALID_SESSION_ERROR,
                ..
            }
        )
    }
}

/// Counts of a `track.scrobble` answer: accepted, filtered, and each distinct
/// filter code (1 artist, 2 track, 3 too old, 4 too new, 5 daily limit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrobbleOutcome {
    pub accepted: u64,
    pub ignored: u64,
    pub codes: Vec<String>,
}

fn count(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n
            .as_f64()
            .filter(|f| f.is_finite() && *f >= 0.0)
            .map_or(0, |f| f as u64),
        Some(Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite() && *f >= 0.0)
            .map_or(0, |f| f as u64),
        _ => 0,
    }
}

/// Reads a successful `track.scrobble` body. A filtered play still answers
/// `status: ok`, so only these counts reveal it. `None` when the body has no
/// `scrobbles` field: absent counts are not zero accepted plays. `scrobble` is a
/// bare object for one play and an array for several, and counts may be numbers
/// or strings. A custom endpoint's codes are kept only when they are 1 to 5.
/// Only counts and codes come out: an artist or title must never reach a log.
pub fn read_scrobble_outcome(body: &Value, custom_endpoint: bool) -> Option<ScrobbleOutcome> {
    let scrobbles = body.get("scrobbles")?;
    let entries: Vec<&Value> = match scrobbles.get("scrobble") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(v @ Value::Object(_)) => vec![v],
        _ => vec![],
    };
    let mut codes: Vec<String> = entries
        .iter()
        .filter_map(|e| e.get("ignoredMessage")?.get("code")?.as_str())
        .filter(|c| !c.is_empty() && *c != "0")
        .filter(|c| !custom_endpoint || matches!(*c, "1" | "2" | "3" | "4" | "5"))
        .map(str::to_owned)
        .collect();
    codes.sort();
    codes.dedup();
    let attr = scrobbles.get("@attr");
    Some(ScrobbleOutcome {
        accepted: count(attr.and_then(|a| a.get("accepted"))),
        ignored: count(attr.and_then(|a| a.get("ignored"))),
        codes,
    })
}

/// One play whose request never reached Last.fm, persisted as `lastfm.pendingScrobbles`.
/// Field names match the JSON the Electron build already stores.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingScrobble {
    pub artist: String,
    pub track: String,
    /// Unix seconds when the play started.
    pub timestamp: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<u64>,
    /// `Some(0)` for a radio song the user did not pick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen_by_user: Option<u8>,
}

/// One play as the fields `track.scrobble` names it by; `suffix` is the `[index]` of a batch.
pub fn track_params(entry: &PendingScrobble, suffix: &str) -> BTreeMap<String, String> {
    let mut p = BTreeMap::new();
    p.insert(format!("artist{suffix}"), entry.artist.clone());
    p.insert(format!("track{suffix}"), entry.track.clone());
    p.insert(format!("timestamp{suffix}"), entry.timestamp.to_string());
    if let Some(album) = entry.album.as_ref().filter(|a| !a.is_empty()) {
        p.insert(format!("album{suffix}"), album.clone());
    }
    if let Some(d) = entry.duration_sec.filter(|d| *d != 0) {
        p.insert(format!("duration{suffix}"), d.to_string());
    }
    if let Some(c) = entry.chosen_by_user {
        p.insert(format!("chosenByUser{suffix}"), c.to_string());
    }
    p
}

/// The unsigned params of one `track.scrobble` request carrying a batch.
pub fn batch_params(
    batch: &[PendingScrobble],
    api_key: &str,
    session_key: &str,
) -> BTreeMap<String, String> {
    let mut p = BTreeMap::from([
        ("method".to_owned(), "track.scrobble".to_owned()),
        ("api_key".to_owned(), api_key.to_owned()),
        ("sk".to_owned(), session_key.to_owned()),
    ]);
    for (i, entry) in batch.iter().enumerate() {
        p.extend(track_params(entry, &format!("[{i}]")));
    }
    p
}

/// A batch out for submission, tagged with the session it was signed for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drain {
    pub batch: Vec<PendingScrobble>,
    pub generation: u64,
}

/// How a drain request ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrainResult {
    /// Last.fm answered; filtered plays count as delivered, since a resend would be filtered again.
    Settled,
    /// The request failed with this error.
    Failed(LastfmError),
}

/// The persisted queue of undelivered plays and the bookkeeping that keeps a
/// drain from deleting plays it did not send.
///
/// Entries are only appended and only trimmed from the head. `session_generation`
/// moves on at every connect and disconnect; a request captures it when it goes
/// out, and an answer for an older generation never touches the queue, because
/// the plays now in it belong to the next account.
#[derive(Clone, Debug, Default)]
pub struct ScrobbleQueue {
    entries: Vec<PendingScrobble>,
    session_generation: u64,
    drain_generation: Option<u64>,
    trimmed_while_draining: usize,
}

impl ScrobbleQueue {
    /// A queue restored from config. A stored queue over the cap is trimmed on the next append.
    pub fn new(entries: Vec<PendingScrobble>) -> Self {
        Self {
            entries,
            ..Self::default()
        }
    }

    /// The entries to persist.
    pub fn entries(&self) -> &[PendingScrobble] {
        &self.entries
    }

    pub fn session_generation(&self) -> u64 {
        self.session_generation
    }

    /// A new session started (a successful connect). Requests already out go stale.
    pub fn advance_session(&mut self) {
        self.session_generation += 1;
    }

    /// The account went: requests go stale and its plays leave with it, because a
    /// queue that outlived the account would be submitted to the next one. The
    /// drain marker stays, so a second drain cannot start until the stale one settles.
    pub fn disconnect(&mut self) {
        self.advance_session();
        self.entries.clear();
    }

    /// Holds an undelivered play from a request signed under `generation`. Refused
    /// (false) when the session changed since, so a play is never handed to another account.
    pub fn queue(&mut self, entry: PendingScrobble, generation: u64) -> bool {
        if generation != self.session_generation {
            return false;
        }
        self.entries.push(entry);
        let trimmed = self.entries.len().saturating_sub(MAX_PENDING_SCROBBLES);
        if self.drain_generation == Some(self.session_generation) {
            self.trimmed_while_draining += trimmed;
        }
        self.entries.drain(..trimmed);
        true
    }

    /// Claims the next batch, or `None` when the queue is empty or a drain for this
    /// session is already out. The caller checks that the integration is enabled
    /// and the session key is the one it holds; a stale drain does not block.
    pub fn start_drain(&mut self) -> Option<Drain> {
        if self.drain_generation == Some(self.session_generation) || self.entries.is_empty() {
            return None;
        }
        let n = self.entries.len().min(MAX_PENDING_SCROBBLES);
        self.drain_generation = Some(self.session_generation);
        self.trimmed_while_draining = 0;
        Some(Drain {
            batch: self.entries[..n].to_vec(),
            generation: self.session_generation,
        })
    }

    /// Settles a drain. Its batch leaves the queue when Last.fm answered or refused
    /// it for good, unless the session changed meanwhile; a retriable failure keeps it.
    pub fn finish_drain(&mut self, drain: &Drain, result: &DrainResult) {
        let drop_batch = match result {
            DrainResult::Settled => true,
            DrainResult::Failed(e) => e.is_final_refusal(),
        };
        if drop_batch && drain.generation == self.session_generation {
            // On a full queue a play queued mid-drain trimmed an entry the drain had
            // sent, so the leading `len` are no longer the batch; subtract the trim.
            let n = drain
                .batch
                .len()
                .saturating_sub(self.trimmed_while_draining)
                .min(self.entries.len());
            self.entries.drain(..n);
        }
        // Only this drain's own marker is cleared, so a stale drain settling late
        // cannot let a second drain start beside the live one.
        if self.drain_generation == Some(drain.generation) {
            self.drain_generation = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn play(n: i64) -> PendingScrobble {
        PendingScrobble {
            artist: format!("a{n}"),
            track: format!("t{n}"),
            timestamp: n,
            album: None,
            duration_sec: None,
            chosen_by_user: None,
        }
    }

    fn api(code: i64) -> LastfmError {
        LastfmError::Api {
            code,
            message: String::new(),
        }
    }

    #[test]
    fn api_url_defaults_and_validates_custom_endpoints() {
        assert_eq!(resolve_api_url(None).as_deref(), Some(DEFAULT_API_URL));
        assert_eq!(
            resolve_api_url(Some("https://scrobbler.example/2.0/")).as_deref(),
            Some("https://scrobbler.example/2.0/")
        );
        assert_eq!(
            resolve_api_url(Some("http://localhost:42010/apis/web/v1/")).as_deref(),
            Some("http://localhost:42010/apis/web/v1/")
        );
        for bad in [
            "ftp://x/",
            "https://u:p@x/",
            "https://x/?a=1",
            "https://x/#f",
            "not a url",
            "javascript:alert(1)",
        ] {
            assert_eq!(resolve_api_url(Some(bad)), None, "{bad}");
        }
    }

    #[test]
    fn signature_matches_the_spec() {
        let params = BTreeMap::from([
            ("method".to_owned(), "auth.getSession".to_owned()),
            ("api_key".to_owned(), "key".to_owned()),
            ("token".to_owned(), "tok".to_owned()),
        ]);
        // md5("api_keykeymethodauth.getSessiontokentoksecret")
        let expected: String = Md5::digest(b"api_keykeymethodauth.getSessiontokentoksecret")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(sign_params(&params, "secret"), expected);
        assert_eq!(sign_params(&params, "secret").len(), 32);
    }

    #[test]
    fn threshold_follows_the_scrobbling_rules() {
        assert_eq!(scrobble_threshold_ms(30_000.0), None);
        assert_eq!(scrobble_threshold_ms(10_000.0), None);
        assert_eq!(scrobble_threshold_ms(30_001.0), Some(15_000.5));
        assert_eq!(scrobble_threshold_ms(200_000.0), Some(100_000.0));
        assert_eq!(scrobble_threshold_ms(600_000.0), Some(240_000.0));
        assert_eq!(scrobble_threshold_ms(0.0), Some(240_000.0));
        assert_eq!(scrobble_threshold_ms(-1.0), Some(240_000.0));
    }

    #[test]
    fn only_non_retriable_api_codes_are_final() {
        for code in RETRIABLE_ERRORS {
            assert!(!api(code).is_final_refusal(), "{code}");
        }
        for code in [4, 6, 9, 13, 14] {
            assert!(api(code).is_final_refusal(), "{code}");
        }
        assert!(api(9).is_invalid_session());
        assert!(!LastfmError::Transport("timeout".into()).is_final_refusal());
        assert!(!LastfmError::BadResponse("html".into()).is_final_refusal());
    }

    #[test]
    fn reads_counts_and_distinct_codes() {
        let body = json!({"scrobbles": {"@attr": {"accepted": 1, "ignored": "2"}, "scrobble": [
            {"ignoredMessage": {"code": "0"}}, {"ignoredMessage": {"code": "3"}}, {"ignoredMessage": {"code": "1"}}, {"ignoredMessage": {"code": "3"}}
        ]}});
        assert_eq!(
            read_scrobble_outcome(&body, false),
            Some(ScrobbleOutcome {
                accepted: 1,
                ignored: 2,
                codes: vec!["1".into(), "3".into()]
            })
        );
        let single = json!({"scrobbles": {"@attr": {"accepted": 0, "ignored": 1}, "scrobble": {"ignoredMessage": {"code": "5"}}}});
        assert_eq!(
            read_scrobble_outcome(&single, false).unwrap().codes,
            vec!["5"]
        );
        assert_eq!(read_scrobble_outcome(&json!({"status": "ok"}), false), None);
        let custom = json!({"scrobbles": {"scrobble": [{"ignoredMessage": {"code": "99"}}, {"ignoredMessage": {"code": "2"}}]}});
        assert_eq!(
            read_scrobble_outcome(&custom, true).unwrap().codes,
            vec!["2"]
        );
        assert_eq!(
            read_scrobble_outcome(&custom, false).unwrap().codes,
            vec!["2", "99"]
        );
    }

    #[test]
    fn params_encode_optional_fields_only_when_present() {
        let mut p = play(7);
        assert_eq!(
            track_params(&p, "").keys().collect::<Vec<_>>(),
            ["artist", "timestamp", "track"]
        );
        p.album = Some("LP".into());
        p.duration_sec = Some(0);
        p.chosen_by_user = Some(0);
        let params = track_params(&p, "[3]");
        assert_eq!(params["album[3]"], "LP");
        assert_eq!(params["chosenByUser[3]"], "0");
        assert!(!params.contains_key("duration[3]"));
        let batch = batch_params(&[play(1), play(2)], "key", "sk");
        assert_eq!(batch["method"], "track.scrobble");
        assert_eq!(batch["artist[1]"], "a2");
    }

    #[test]
    fn pending_scrobbles_round_trip_the_stored_json() {
        let stored =
            r#"[{"artist":"A","track":"T","timestamp":5,"durationSec":200,"chosenByUser":0}]"#;
        let parsed: Vec<PendingScrobble> = serde_json::from_str(stored).unwrap();
        assert_eq!(parsed[0].duration_sec, Some(200));
        assert_eq!(serde_json::to_string(&parsed).unwrap(), stored);
    }

    #[test]
    fn queue_caps_at_fifty_dropping_the_oldest() {
        let mut q = ScrobbleQueue::default();
        for n in 0..55 {
            assert!(q.queue(play(n), 0));
        }
        assert_eq!(q.entries().len(), 50);
        assert_eq!(q.entries()[0], play(5));
    }

    #[test]
    fn a_settled_drain_removes_exactly_its_batch() {
        let mut q = ScrobbleQueue::new((0..3).map(play).collect());
        let d = q.start_drain().unwrap();
        assert_eq!(
            q.start_drain(),
            None,
            "a second drain must not start beside the first"
        );
        q.queue(play(3), q.session_generation());
        q.finish_drain(&d, &DrainResult::Settled);
        assert_eq!(q.entries(), &[play(3)]);
        assert!(q.start_drain().is_some());
    }

    #[test]
    fn a_play_queued_into_a_full_queue_mid_drain_survives() {
        let mut q = ScrobbleQueue::new((0..50).map(play).collect());
        let d = q.start_drain().unwrap();
        assert_eq!(d.batch.len(), 50);
        q.queue(play(50), q.session_generation()); // trims play(0), which the drain sent
        q.finish_drain(&d, &DrainResult::Settled);
        assert_eq!(q.entries(), &[play(50)]);
    }

    #[test]
    fn retriable_failures_keep_the_batch_and_refusals_drop_it() {
        let mut q = ScrobbleQueue::new(vec![play(1)]);
        let d = q.start_drain().unwrap();
        q.finish_drain(&d, &DrainResult::Failed(api(16)));
        assert_eq!(q.entries().len(), 1);
        let d = q.start_drain().unwrap();
        q.finish_drain(
            &d,
            &DrainResult::Failed(LastfmError::Transport("timeout".into())),
        );
        assert_eq!(q.entries().len(), 1);
        let d = q.start_drain().unwrap();
        q.finish_drain(&d, &DrainResult::Failed(api(6)));
        assert!(q.entries().is_empty());
    }

    #[test]
    fn a_drain_that_settles_after_disconnect_leaves_the_next_account_alone() {
        let mut q = ScrobbleQueue::new(vec![play(1), play(2)]);
        let stale = q.start_drain().unwrap();
        q.disconnect();
        assert!(q.entries().is_empty());
        assert!(
            !q.queue(play(9), stale.generation),
            "a play from the old session must not queue"
        );
        assert!(q.queue(play(3), q.session_generation()));
        let fresh = q.start_drain();
        assert!(fresh.is_some(), "the stale drain does not block a new one");
        q.finish_drain(&stale, &DrainResult::Settled);
        assert_eq!(q.entries(), &[play(3)], "the stale answer removed nothing");
        assert_eq!(
            q.start_drain(),
            None,
            "the stale answer did not release the live drain's marker"
        );
    }
}
