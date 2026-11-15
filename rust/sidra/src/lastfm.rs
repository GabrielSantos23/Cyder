//! Last.fm: the desktop auth flow, now playing and scrobbling, on the rules in
//! `sidra_core::lastfm` (signing, the threshold, which errors are final, the retry queue).
//! Requests go out on ehttp's threads and come back through a channel the app drains each frame.

use crate::engine::Playback;
use crate::settings::LastfmSettings;
use serde_json::Value;
use sidra_core::lastfm::{
    self as rules, AUTH_POLL_INTERVAL_MS, AUTH_POLL_TIMEOUT_MS, DEFAULT_API_URL, Drain,
    DrainResult, LastfmError, PendingScrobble, ScrobbleQueue,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Error 14: the token exists but the user hasn't approved it yet.
const UNAUTHORIZED_TOKEN: i64 = 14;

enum Msg {
    Token(Result<Value, LastfmError>),
    Session(Result<Value, LastfmError>),
    /// A now-playing update or a single scrobble settled; `queued` is the play to hold on a failure.
    Sent {
        result: Result<Value, LastfmError>,
        generation: u64,
        queued: Option<PendingScrobble>,
    },
    Drained(Drain, Result<Value, LastfmError>),
}

enum Auth {
    Idle,
    Token,
    /// Waiting for the user to approve `token` in the browser.
    Waiting {
        token: String,
        until: f64,
        next_poll: f64,
    },
    Failed(&'static str),
}

/// The song being listened to, and how long it has actually played.
struct Play {
    id: String,
    artist: String,
    title: String,
    album: String,
    duration_ms: u64,
    started: i64,
    played_ms: f64,
    last_frame: f64,
    now_sent: bool,
    scrobbled: bool,
    radio: bool,
}

pub struct Lastfm {
    key: String,
    secret: String,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    auth: Auth,
    queue: ScrobbleQueue,
    play: Option<Play>,
    repaint: Option<egui::Context>,
}

/// The app's key: the build's, the environment's, or `lastfm-credentials.json` in the data folder.
fn credentials(dir: &Path) -> (String, String) {
    let pair = |k: Option<String>, s: Option<String>| match (k, s) {
        (Some(k), Some(s)) if !k.is_empty() && !s.is_empty() => Some((k, s)),
        _ => None,
    };
    let built = pair(
        option_env!("SIDRA_LASTFM_API_KEY").map(str::to_owned),
        option_env!("SIDRA_LASTFM_API_SECRET").map(str::to_owned),
    );
    let env = || {
        pair(
            std::env::var("SIDRA_LASTFM_API_KEY").ok(),
            std::env::var("SIDRA_LASTFM_API_SECRET").ok(),
        )
    };
    let file = || {
        let v: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("lastfm-credentials.json")).ok()?,
        )
        .ok()?;
        let get = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        pair(get("apiKey"), get("apiSecret"))
    };
    built.or_else(env).or_else(file).unwrap_or_default()
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Opens `url` in the default browser.
fn open_in_browser(url: &str) {
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        eprintln!("sidra: could not open the browser: {e}");
    }
}

/// Last.fm's answer: its own error code is read before the HTTP status, because it
/// sends several codes with a non-2xx status.
fn read_answer(r: Result<ehttp::Response, String>) -> Result<Value, LastfmError> {
    let r = r.map_err(LastfmError::Transport)?;
    let body: Value = serde_json::from_slice(&r.bytes)
        .map_err(|_| LastfmError::BadResponse(format!("HTTP {}", r.status)))?;
    if let Some(code) = body.get("error").and_then(Value::as_i64) {
        let message = body
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        return Err(LastfmError::Api { code, message });
    }
    if !r.ok {
        return Err(LastfmError::BadResponse(format!("HTTP {}", r.status)));
    }
    Ok(body)
}

impl Lastfm {
    pub fn new(dir: &Path, saved: &LastfmSettings) -> Self {
        let (key, secret) = credentials(dir);
        let (tx, rx) = channel();
        Self {
            key,
            secret,
            tx,
            rx,
            auth: Auth::Idle,
            queue: ScrobbleQueue::new(saved.pending_scrobbles.clone()),
            play: None,
            repaint: None,
        }
    }

    pub fn configured(&self) -> bool {
        !self.key.is_empty()
    }

    pub fn connecting(&self) -> bool {
        matches!(self.auth, Auth::Token | Auth::Waiting { .. })
    }

    /// A line for Settings while connecting or after a failed attempt.
    pub fn status(&self) -> Option<&'static str> {
        match self.auth {
            Auth::Token => Some("Opening Last.fm…"),
            Auth::Waiting { .. } => Some("Approve Sidra in your browser…"),
            Auth::Failed(why) => Some(why),
            Auth::Idle => None,
        }
    }

    /// Signs and posts `params`, sending the answer back as `wrap(answer)`.
    fn call(
        &self,
        mut params: BTreeMap<String, String>,
        wrap: impl FnOnce(Result<Value, LastfmError>) -> Msg + Send + 'static,
    ) {
        params.insert("api_key".into(), self.key.clone());
        let sig = rules::sign_params(&params, &self.secret);
        params.insert("api_sig".into(), sig);
        params.insert("format".into(), "json".into());
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&params)
            .finish();
        let mut req = ehttp::Request::post(DEFAULT_API_URL, body.into_bytes());
        req.headers = ehttp::Headers::new(&[("Content-Type", "application/x-www-form-urlencoded")]);
        let tx = self.tx.clone();
        let repaint = self.repaint.clone();
        // shortcut: ehttp 0.5 sets no timeout, so a request that never settles holds its drain; add one if that shows up.
        ehttp::fetch(req, move |r| {
            let _ = tx.send(wrap(read_answer(r)));
            if let Some(c) = repaint {
                c.request_repaint();
            }
        });
    }

    fn params(method: &str) -> BTreeMap<String, String> {
        BTreeMap::from([("method".to_owned(), method.to_owned())])
    }

    /// Starts the desktop auth flow: get a token, open Last.fm to approve it, then poll.
    pub fn connect(&mut self) {
        if !self.configured() || self.connecting() {
            return;
        }
        self.auth = Auth::Token;
        self.call(Self::params("auth.getToken"), Msg::Token);
    }

    pub fn disconnect(&mut self, s: &mut LastfmSettings) {
        s.session_key = None;
        s.username = None;
        s.enabled = false;
        // The queued plays belong to the account that left.
        self.queue.disconnect();
        s.pending_scrobbles.clear();
        self.auth = Auth::Idle;
        self.play = None;
    }

    /// Runs every frame: settles answers, polls the auth flow, follows playback.
    /// Returns true when `s` changed and should be saved.
    pub fn tick(
        &mut self,
        ctx: &egui::Context,
        s: &mut LastfmSettings,
        pb: &Playback,
        now: f64,
    ) -> bool {
        self.repaint.get_or_insert_with(|| ctx.clone());
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            changed |= self.settle(msg, s, now);
        }

        if let Auth::Waiting {
            token,
            until,
            next_poll,
        } = &mut self.auth
        {
            if now > *until {
                self.auth = Auth::Failed("Last.fm wasn't approved in time. Try again.");
            } else if now >= *next_poll {
                *next_poll = now + AUTH_POLL_INTERVAL_MS as f64 / 1000.0;
                let mut p = Self::params("auth.getSession");
                p.insert("token".into(), token.clone());
                self.call(p, Msg::Session);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(AUTH_POLL_INTERVAL_MS));
        }

        let Some(sk) = s.session_key.clone().filter(|_| s.enabled) else {
            self.play = None;
            return changed;
        };
        self.follow(pb, now);
        let generation = self.queue.session_generation();
        let Some(play) = &mut self.play else {
            return changed;
        };

        if pb.playing && !play.now_sent {
            play.now_sent = true;
            let mut p = Self::params("track.updateNowPlaying");
            p.insert("sk".into(), sk.clone());
            p.insert("artist".into(), play.artist.clone());
            p.insert("track".into(), play.title.clone());
            if !play.album.is_empty() {
                p.insert("album".into(), play.album.clone());
            }
            if play.duration_ms >= 1000 {
                p.insert("duration".into(), (play.duration_ms / 1000).to_string());
            }
            self.call(p, move |result| Msg::Sent {
                result,
                generation,
                queued: None,
            });
            return changed;
        }

        let threshold = rules::scrobble_threshold_ms(play.duration_ms as f64);
        if !play.scrobbled && threshold.is_some_and(|t| play.played_ms >= t) {
            play.scrobbled = true;
            let entry = PendingScrobble {
                artist: play.artist.clone(),
                track: play.title.clone(),
                timestamp: play.started,
                album: Some(play.album.clone()).filter(|a| !a.is_empty()),
                duration_sec: Some(play.duration_ms / 1000).filter(|d| *d > 0),
                chosen_by_user: play.radio.then_some(0),
            };
            let mut p = Self::params("track.scrobble");
            p.insert("sk".into(), sk);
            p.extend(rules::track_params(&entry, ""));
            self.call(p, move |result| Msg::Sent {
                result,
                generation,
                queued: Some(entry),
            });
        }
        changed
    }

    /// Tracks the playing song and adds up the time it actually plays.
    fn follow(&mut self, pb: &Playback, now: f64) {
        // Live radio has no song to scrobble, only the station's name.
        let Some(item) = pb.now.as_ref().filter(|_| !pb.live) else {
            self.play = None;
            return;
        };
        if self.play.as_ref().map(|p| &p.id) != Some(&item.id) {
            if item.title.is_empty() || item.artist.is_empty() {
                self.play = None;
                return;
            }
            self.play = Some(Play {
                id: item.id.clone(),
                artist: item.artist.clone(),
                title: item.title.clone(),
                album: item.album.clone(),
                duration_ms: item.duration_ms,
                started: unix_now(),
                played_ms: 0.0,
                last_frame: now,
                now_sent: false,
                scrobbled: false,
                radio: pb.radio,
            });
        }
        let play = self.play.as_mut().expect("set above");
        // Frames come at least every quarter second while playing; a longer gap was a stall.
        let dt = (now - play.last_frame).clamp(0.0, 5.0);
        play.last_frame = now;
        if pb.playing {
            play.played_ms += dt * 1000.0;
        }
        // Repeat-one replays the same id: a playhead back near the start is a new play.
        if play.scrobbled && pb.time < 2.0 && play.played_ms > 10_000.0 {
            self.play = None;
        }
    }

    fn settle(&mut self, msg: Msg, s: &mut LastfmSettings, now: f64) -> bool {
        match msg {
            Msg::Token(Ok(v)) => match v.get("token").and_then(Value::as_str) {
                Some(token) => {
                    open_in_browser(&format!(
                        "{}?api_key={}&token={token}",
                        rules::AUTH_URL,
                        self.key
                    ));
                    self.auth = Auth::Waiting {
                        token: token.to_owned(),
                        until: now + AUTH_POLL_TIMEOUT_MS as f64 / 1000.0,
                        next_poll: now + AUTH_POLL_INTERVAL_MS as f64 / 1000.0,
                    };
                }
                None => self.auth = Auth::Failed("Couldn't connect to Last.fm. Try again."),
            },
            Msg::Token(Err(e)) => {
                eprintln!("sidra: lastfm: getToken failed: {e:?}");
                self.auth = Auth::Failed("Couldn't connect to Last.fm. Try again.");
            }
            Msg::Session(Ok(v)) => {
                let key = v.pointer("/session/key").and_then(Value::as_str);
                let name = v.pointer("/session/name").and_then(Value::as_str);
                if let (Some(key), Some(name)) = (key, name)
                    && matches!(self.auth, Auth::Waiting { .. })
                {
                    s.session_key = Some(key.to_owned());
                    s.username = Some(name.to_owned());
                    s.enabled = true;
                    self.queue.advance_session();
                    self.auth = Auth::Idle;
                    return true;
                }
            }
            Msg::Session(Err(LastfmError::Api {
                code: UNAUTHORIZED_TOKEN,
                ..
            })) => {}
            Msg::Session(Err(e)) => {
                eprintln!("sidra: lastfm: getSession failed: {e:?}");
                if matches!(self.auth, Auth::Waiting { .. }) {
                    self.auth = Auth::Failed("Couldn't connect to Last.fm. Try again.");
                }
            }
            Msg::Sent {
                result,
                generation,
                queued,
            } => match result {
                Ok(_) => return self.drain(s),
                Err(e) => {
                    eprintln!("sidra: lastfm: request failed: {e:?}");
                    if e.is_invalid_session() && generation == self.queue.session_generation() {
                        self.disconnect(s);
                        return true;
                    }
                    if let Some(entry) = queued
                        && !e.is_final_refusal()
                        && self.queue.queue(entry, generation)
                    {
                        s.pending_scrobbles = self.queue.entries().to_vec();
                        return true;
                    }
                }
            },
            Msg::Drained(drain, result) => {
                let result = match result {
                    Ok(_) => DrainResult::Settled,
                    Err(e) => {
                        eprintln!("sidra: lastfm: queued scrobbles failed: {e:?}");
                        if e.is_invalid_session()
                            && drain.generation == self.queue.session_generation()
                        {
                            self.queue.finish_drain(&drain, &DrainResult::Failed(e));
                            self.disconnect(s);
                            return true;
                        }
                        DrainResult::Failed(e)
                    }
                };
                self.queue.finish_drain(&drain, &result);
                s.pending_scrobbles = self.queue.entries().to_vec();
                return true;
            }
        }
        false
    }

    /// Sends held plays, riding on a request that just succeeded so no timer retries.
    fn drain(&mut self, s: &LastfmSettings) -> bool {
        let (Some(sk), true) = (&s.session_key, s.enabled) else {
            return false;
        };
        let Some(drain) = self.queue.start_drain() else {
            return false;
        };
        let mut p = rules::batch_params(&drain.batch, &self.key, sk);
        p.remove("api_key");
        let d = drain.clone();
        self.call(p, move |r| Msg::Drained(d, r));
        false
    }
}
