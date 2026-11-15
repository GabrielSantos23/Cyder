//! The playback engine: a WebView holding music.apple.com, kept at one pixel while
//! the user is signed in. MusicKit inside it plays (Widevine needs the browser) and
//! makes every Apple Music API call, so the tokens never leave Apple's page.
//! Sidra drives it with `window.__sidra` and hears back over `window.ipc`.

use serde_json::{Value, json};
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};
use wry::{Rect, WebContext, WebView, WebViewBuilder, dpi};

const BRIDGE: &str = r#"
(function () {
  if (window !== window.top || window.__sidraBridge) return;
  window.__sidraBridge = true;
  const send = o => { try { window.ipc.postMessage(JSON.stringify(o)); } catch (e) {} };
  const rmksa = navigator.requestMediaKeySystemAccess && navigator.requestMediaKeySystemAccess.bind(navigator);
  if (rmksa) navigator.requestMediaKeySystemAccess = (ks, cfg) => rmksa(ks, cfg).then(
    a => { send({ t: 'eme', ks, ok: true }); return a; },
    e => { send({ t: 'eme', ks, ok: false, error: e.name }); throw e; });
  const MKS = window.MediaKeySession && MediaKeySession.prototype;
  if (MKS) {
    const gen = MKS.generateRequest;
    MKS.generateRequest = function (type, data) { send({ t: 'licence-request', ks: type }); return gen.call(this, type, data); };
    const upd = MKS.update;
    MKS.update = function (r) { return upd.call(this, r).then(x => { send({ t: 'licence', ok: true }); return x; },
      e => { send({ t: 'licence', ok: false, error: e.name }); throw e; }); };
  }
  // Diagnostics: the status of Apple's playback and licence calls (codes only, no bodies).
  const origFetch = window.fetch.bind(window);
  window.fetch = async (input, init) => {
    const url = String((input && input.url) || input);
    const r = await origFetch(input, init);
    if (/webPlayback|Licen[cs]e|MZPlay/i.test(url)) {
      r.clone().json().then(j => send({ t: 'licence-http', ok: r.ok, error: r.status + ' status=' + (j && (j.status !== undefined ? j.status : j.errorCode)) + ' failure=' + (j && (j.failureType || j['failure-type'] || j.customerMessage || '')) }),
        () => send({ t: 'licence-http', ok: r.ok, error: String(r.status) + ' ' + url.split('?')[0].split('/').pop() }));
    }
    return r;
  };
  const API = 'https://amp-api.music.apple.com';
  let mk = null;
  const pp = it => (it && (it.playParams || (it.attributes && it.attributes.playParams))) || {};
  const item = it => it ? ({
    id: String(it.id || ''),
    catalogId: pp(it).catalogId || (pp(it).id && !String(pp(it).id).startsWith('i.') ? String(pp(it).id) : null),
    kind: it.type || '',
    title: (it.attributes && it.attributes.name) || it.title || '',
    artist: (it.attributes && it.attributes.artistName) || it.artistName || '',
    album: (it.attributes && it.attributes.albumName) || it.albumName || '',
    artwork: (it.attributes && it.attributes.artwork && it.attributes.artwork.url) || null,
    durationMs: (it.attributes && it.attributes.durationInMillis) || it.playbackDuration || 0,
    url: (it.attributes && it.attributes.url) || null
  }) : null;
  // MusicKit keeps more than one media element (the next song preloads for gapless play), so
  // the first in the page can belong to the wrong song. Its whole-second time picks the right one.
  const playhead = () => {
    const whole = mk.currentPlaybackTime || 0;
    const el = [...document.querySelectorAll('audio, video')]
      .find(e => !e.paused && Math.abs(e.currentTime - whole) < 1.5);
    return el ? el.currentTime : whole;
  };
  const state = () => mk && send({ t: 'state', playing: !!mk.isPlaying, ps: mk.playbackState,
    preview: !!mk.previewOnly, hasItem: !!mk.nowPlayingItem,
    // currentPlaybackTime is whole seconds, which pulls the bar back between reports.
    time: playhead(), duration: mk.currentPlaybackDuration || 0,
    volume: mk.volume, shuffle: mk.shuffleMode === 1, repeat: mk.repeatMode || 0,
    autoplay: !!mk.autoplayEnabled, ...caps() });
  // What the current item allows, as Apple's own player reads it: stations can't be
  // shuffled or rewound, and live radio can't be paused or seeked either.
  const caps = () => {
    const c = mk.capabilities || {}, ni = mk.nowPlayingItem;
    const live = !!ni && (!!ni.isLiveRadioStation || pp(ni).kind === 'radioStation');
    const radio = live || (!!ni && (ni.type === 'stations' || (ni.container && ni.container.type === 'stations')));
    return { radio, live, canPause: c.canPause !== false && !live, canSeek: c.canSeek !== false && !live,
      canNext: c.canSkipToNextItem !== false, canPrev: c.canSkipToPreviousItem !== false && !radio,
      canEdit: c.canEditPlaybackQueue !== false && !radio };
  };
  // Music videos play in a <video> MusicKit puts somewhere in Apple's page. Showing one moves
  // it to <body> (a move keeps it playing) and lets it fill the viewport over everything else.
  // Added on first use: this script runs before the page has a document to put it in.
  let style = null;
  const css = () => {
    if (style) return;
    style = document.createElement('style');
    style.textContent = 'html.sidra-video,html.sidra-video body{background:#000!important;overflow:hidden!important}' +
    'html.sidra-video video{position:fixed!important;inset:0!important;width:100vw!important;height:100vh!important;' +
    'max-width:none!important;max-height:none!important;z-index:2147483647!important;background:#000!important;' +
    'object-fit:contain!important;display:block!important;visibility:visible!important;opacity:1!important;transform:none!important}';
    document.head.appendChild(style);
  };
  const video = on => {
    css();
    document.documentElement.classList.toggle('sidra-video', on);
    const v = document.querySelector('video');
    if (on && v && v.parentElement !== document.body) document.body.appendChild(v);
  };
  const queue = () => mk && send({ t: 'queue', position: mk.queue.position,
    items: mk.queue.items.map(item) });
  const now = () => mk && send({ t: 'now', item: item(mk.nowPlayingItem) });
  let sentAuth = null;
  let checking = false;
  const auth = async () => {
    if (checking) return;
    checking = true;
    try {
      const page = (location.pathname.split('/')[1] || '').toLowerCase();
      const authorized = !!mk.isAuthorized;
      let account = '';
      if (authorized) {
        // mk.storefrontCountryCode follows the page until the next load, so ask the API for
        // the account's own storefront; catalogue ids from any other one fail to play.
        try { account = String((await call('/v1/me/storefront')).data[0].id).toLowerCase(); }
        catch (e) { account = (mk.storefrontCountryCode || '').toLowerCase(); }
      }
      // Apple also serves only previews when the page storefront differs from the account's.
      if (authorized && account && page && account !== page) { location.replace('/' + account + '/new'); return; }
      sentAuth = authorized;
      send({ t: 'auth', authorized, storefront: account || page });
    } finally { checking = false; }
  };
  const attach = () => {
    mk = MusicKit.getInstance();
    auth();
    mk.addEventListener('authorizationStatusDidChange', auth);
    mk.addEventListener('mediaPlaybackError', e => send({ t: 'error', what: 'playback',
      error: String((e && (e.errorCode || e.name || e.message)) || e) }));
    mk.addEventListener('nowPlayingItemDidChange', () => {
      now(); queue(); state();
      // A new video element can come with the next item.
      if (document.documentElement.classList.contains('sidra-video')) setTimeout(() => video(true), 300);
    });
    for (const e of ['playbackStateDidChange', 'repeatModeDidChange', 'playbackVolumeDidChange']) mk.addEventListener(e, state);
    for (const e of ['queueItemsDidChange', 'queuePositionDidChange', 'shuffleModeDidChange']) mk.addEventListener(e, () => { queue(); state(); });
    state(); now(); queue();
  };
  const wait = () => {
    try {
      if (window.MusicKit && MusicKit.getInstance && MusicKit.getInstance()) {
        attach();
        setInterval(() => { if (mk.isPlaying) state(); }, 500);
        // Apple's app can replace the MusicKit instance (it does around sign-in), and a sign-in
        // can land without an event; check both every second.
        setInterval(() => {
          let current = null;
          try { current = MusicKit.getInstance(); } catch (e) {}
          if (current && current !== mk) attach();
          else if (mk && !!mk.isAuthorized !== sentAuth) auth();
        }, 1000);
        return;
      }
    } catch (e) {}
    setTimeout(wait, 400);
  };
  wait();

  const call = async (path, method, body) => {
    const r = await fetch(API + path, { method: method || 'GET', body: body ? JSON.stringify(body) : undefined,
      headers: { 'Authorization': 'Bearer ' + mk.developerToken, 'Music-User-Token': mk.musicUserToken,
        'Content-Type': 'application/json' } });
    const text = await r.text();
    if (!r.ok) {
      // Apple's own reason (such as an invalid parameter) says what to fix; it carries no user data.
      let why = '';
      try { const e = JSON.parse(text).errors[0]; why = ': ' + (e.detail || e.title || e.code); } catch (e) {}
      throw new Error('HTTP ' + r.status + why);
    }
    return text ? JSON.parse(text) : {};
  };
  window.__sidra = {
    async req(id, path, o) {
      o = o || {};
      try {
        // `{tz}` is the local UTC offset as Apple's web app sends it (`-03:00`).
        const m = -new Date().getTimezoneOffset(), h = Math.abs(m);
        const tz = (m < 0 ? '-' : '+') + String(Math.floor(h / 60)).padStart(2, '0') + ':' + String(h % 60).padStart(2, '0');
        const doc = await call(path.replace('{tz}', encodeURIComponent(tz)), o.method, o.body);
        // Follow a relationship's pages (a playlist's tracks) up to `pageRel` limit.
        const res = o.pageRel && doc.data && doc.data[0];
        const rel = res && res.relationships && res.relationships[o.pageRel];
        let next = rel && rel.next, pages = 0;
        while (next && pages++ < 100) { const p = await call(next); rel.data.push(...(p.data || [])); next = p.next; }
        if (rel) delete rel.next;
        // Follow a list's pages up to `all` items.
        let more = o.all && doc.next;
        while (more && doc.data.length < o.all) { const p = await call(more); doc.data.push(...(p.data || [])); more = p.next; }
        if (o.all) delete doc.next;
        send({ t: 'res', id, ok: true, body: doc });
      } catch (e) { send({ t: 'res', id, ok: false, error: String((e && e.message) || e) }); }
    },
    async cmd(name, a) {
      a = a || {};
      try {
        switch (name) {
          case 'setQueue':
            // Set shuffle before the queue exists: writing it afterwards reshuffles on the main thread.
            mk.shuffleMode = a.shuffle ? 1 : 0;
            await mk.setQueue(Object.assign({ startPlaying: true }, a.opts));
            break;
          case 'toggle':
            if (!mk.isPlaying) await mk.play();
            else if (caps().canPause) mk.pause();
            else await mk.stop();
            break;
          case 'video': video(!!a.on); break;
          case 'next': await mk.skipToNextItem(); break;
          case 'prev': await mk.skipToPreviousItem(); break;
          case 'seek': await mk.seekToTime(a.t); break;
          case 'volume': mk.volume = a.v; break;
          case 'shuffle': mk.shuffleMode = a.on ? 1 : 0; break;
          case 'repeat': mk.repeatMode = a.mode; break;
          case 'autoplay': mk.autoplayEnabled = !!a.on; break;
          case 'playNext': await mk.playNext(a.opts); break;
          case 'playLater': await mk.playLater(a.opts); break;
          case 'jump': await mk.changeToMediaAtIndex(a.index); break;
          case 'signOut': await mk.unauthorize(); break;
          case 'authorize': {
            // Apple's own button, so its app records the session and keeps it across launches;
            // a bare mk.authorize() token is dropped on the next load.
            const b = document.querySelector('[data-testid="sign-in-button"]');
            if (b) b.click(); else await mk.authorize();
            break;
          }
        }
      } catch (e) { send({ t: 'error', what: name, error: String((e && e.message) || e) }); }
      send({ t: 'done', name });
      state(); queue();
    }
  };
})();
"#;

/// A queue entry or the playing item, as MusicKit reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub id: String,
    /// MusicKit's item type, such as `song` or `musicVideo`.
    pub kind: String,
    pub catalog_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<String>,
    pub duration_ms: u64,
    pub url: Option<String>,
}

impl Item {
    fn from(v: &Value) -> Option<Self> {
        let st = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        let id = st("id");
        if id.is_empty() {
            return None;
        }
        let opt = |k: &str| Some(st(k)).filter(|s| !s.is_empty());
        Some(Self {
            catalog_id: opt("catalogId"),
            title: st("title"),
            artist: st("artist"),
            album: st("album"),
            artwork: opt("artwork"),
            duration_ms: v.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0) as u64,
            url: opt("url"),
            kind: st("kind"),
            id,
        })
    }

    pub fn is_video(&self) -> bool {
        self.kind.to_ascii_lowercase().contains("video")
    }

    pub fn catalog(&self) -> Option<&str> {
        self.catalog_id
            .as_deref()
            .or_else(|| (!self.id.starts_with("i.")).then_some(self.id.as_str()))
    }

    /// The catalogue album id from the item's share URL.
    pub fn album_id(&self) -> Option<String> {
        let url = self.url.as_deref()?;
        let path = url.split('?').next()?;
        let id = path.rsplit('/').next()?;
        (path.contains("/album/") && id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty())
            .then(|| id.to_owned())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Playback {
    pub playing: bool,
    pub time: f64,
    pub duration: f64,
    pub volume: f32,
    pub shuffle: bool,
    /// 0 off, 1 one, 2 all: MusicKit's `repeatMode`.
    pub repeat: u8,
    pub autoplay: bool,
    /// Playing a station (live or algorithmic).
    pub radio: bool,
    /// Playing a live station, which has no position and no pause.
    pub live: bool,
    pub can_pause: bool,
    pub can_seek: bool,
    pub can_next: bool,
    pub can_prev: bool,
    /// Shuffle, repeat and the queue can be changed.
    pub can_edit: bool,
    pub now: Option<Item>,
    pub queue: Vec<Item>,
    pub position: usize,
    /// When `time` was reported, so the bar can move between reports.
    pub reported_at: f64,
}

/// A play on its way: what was clicked, so that row and button can show a spinner.
pub struct Loading {
    /// The album, playlist or station being queued.
    pub source: Option<String>,
    /// The song that will play first, when known.
    pub track: Option<String>,
    since: std::time::Instant,
    /// MusicKit has taken the queue; the spinner stays until audio actually plays.
    queued: bool,
}

pub enum Event {
    Auth {
        authorized: bool,
        storefront: String,
    },
    Response {
        id: u64,
        result: Result<Value, String>,
    },
    Error(String),
    Changed,
}

pub struct Engine {
    webview: Option<WebView>,
    context: WebContext,
    rx: Receiver<Value>,
    tx: std::sync::mpsc::Sender<Value>,
    next_id: u64,
    /// Calls made before MusicKit attached, sent once it has.
    waiting: Vec<String>,
    bounds: Option<egui::Rect>,
    pub authorized: Option<bool>,
    pub storefront: String,
    pub playback: Playback,
    /// Shows Apple's whole page for signing in, when its sign-in window can't open.
    pub show_page: bool,
    /// The user wants the playing music video on screen.
    pub video_open: bool,
    video_shown: bool,
    /// A play Sidra asked for that hasn't started yet.
    pub loading: Option<Loading>,
    /// The album or playlist the queue was started from, while it still is.
    pub queued_from: Option<String>,
    /// egui's zoom: the webview is placed in logical pixels, which are points times this.
    zoom: f32,
}

impl Engine {
    pub fn new(data_dir: &Path, storefront: String) -> Self {
        let (tx, rx) = channel();
        Self {
            webview: None,
            context: WebContext::new(Some(data_dir.to_path_buf())),
            rx,
            tx,
            next_id: 0,
            waiting: Vec::new(),
            bounds: None,
            video_open: false,
            video_shown: false,
            loading: None,
            queued_from: None,
            zoom: 1.0,
            authorized: None,
            storefront,
            playback: Playback {
                volume: 1.0,
                ..Default::default()
            },
            show_page: false,
        }
    }

    /// Creates the WebView on the first frame, when the window handle exists.
    pub fn start(&mut self, frame: &eframe::Frame, ctx: &egui::Context) {
        if self.webview.is_some() {
            return;
        }
        let tx = self.tx.clone();
        let repaint = ctx.clone();
        let url = format!("https://music.apple.com/{}/new", self.storefront);
        let builder = WebViewBuilder::new_with_web_context(&mut self.context);
        // The page is one pixel and never clicked, so Chromium's autoplay rule would refuse every play().
        // wry's own defaults are repeated because this argument replaces them.
        #[cfg(windows)]
        let builder = {
            use wry::WebViewBuilderExtWindows;
            builder.with_additional_browser_args(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection                  --autoplay-policy=no-user-gesture-required",
            )
        };
        let built = builder
            .with_url(url)
            .with_initialization_script_for_main_only(BRIDGE, true)
            .with_ipc_handler(move |req| {
                if let Ok(v) = serde_json::from_str(req.body()) {
                    let _ = tx.send(v);
                    repaint.request_repaint();
                }
            })
            // Popup blocking is off so Sidra's own button can open Apple's sign-in window, which
            // is a popup opened by script. Only Apple's sign-in hosts may open one.
            .with_new_window_req_handler(|url: String, _| {
                if is_sign_in_url(&url) {
                    wry::NewWindowResponse::Allow
                } else {
                    wry::NewWindowResponse::Deny
                }
            })
            .with_navigation_handler(|url: String| {
                sidra_core::music_service::is_allowed_navigation_url(&url)
            })
            .with_bounds(pixel())
            .build_as_child(frame);
        match built {
            Ok(w) => self.webview = Some(w),
            Err(e) => eprintln!("sidra: could not create the Apple Music web view: {e}"),
        }
    }

    /// Shows the page over `rect` (to sign in) or shrinks it to one pixel.
    /// Makes Apple's page show only the playing music video, or puts it back.
    pub fn set_video(&mut self, on: bool) {
        if self.video_shown != on {
            self.video_shown = on;
            self.command("video", serde_json::json!({ "on": on }));
        }
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        if self.zoom != zoom {
            self.zoom = zoom;
            let rect = self.bounds;
            self.bounds = Some(egui::Rect::NOTHING);
            self.place(rect);
        }
    }

    pub fn place(&mut self, rect: Option<egui::Rect>) {
        if self.bounds == rect {
            return;
        }
        self.bounds = rect;
        let Some(w) = &self.webview else { return };
        let z = self.zoom as f64;
        let bounds = rect.map_or_else(pixel, |r| Rect {
            position: dpi::LogicalPosition::new(r.left() as f64 * z, r.top() as f64 * z).into(),
            size: dpi::LogicalSize::new(r.width() as f64 * z, r.height() as f64 * z).into(),
        });
        let _ = w.set_bounds(bounds);
    }

    pub fn poll(&mut self, now: f64) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(v) = self.rx.try_recv() {
            if std::env::var_os("SIDRA_DEBUG").is_some() && v["t"] != "res" {
                // Types, states and errors only: no titles, artists or URLs.
                eprintln!(
                    "engine: t={} ps={} playing={} time={} preview={} item={} radio={} live={} pause={} next={} ks={} ok={} err={}",
                    v["t"],
                    v["ps"],
                    v["playing"],
                    v["time"],
                    v["preview"],
                    v["hasItem"],
                    v["radio"],
                    v["live"],
                    v["canPause"],
                    v["canNext"],
                    v["ks"],
                    v["ok"],
                    v["error"]
                );
            }
            match v.get("t").and_then(Value::as_str).unwrap_or("") {
                "auth" => {
                    let authorized = v["authorized"].as_bool().unwrap_or(false);
                    let storefront = v["storefront"].as_str().unwrap_or("").to_owned();
                    if !storefront.is_empty() {
                        self.storefront = storefront.clone();
                    }
                    self.authorized = Some(authorized);
                    if authorized {
                        self.show_page = false;
                        self.flush();
                    }
                    out.push(Event::Auth {
                        authorized,
                        storefront,
                    });
                }
                "res" => out.push(Event::Response {
                    id: v["id"].as_u64().unwrap_or(0),
                    result: if v["ok"].as_bool() == Some(true) {
                        Ok(v.get("body").cloned().unwrap_or(Value::Null))
                    } else {
                        Err(v["error"].as_str().unwrap_or("request failed").to_owned())
                    },
                }),
                "state" => {
                    let p = &mut self.playback;
                    p.playing = v["playing"].as_bool().unwrap_or(false);
                    p.time = v["time"].as_f64().unwrap_or(0.0);
                    p.duration = v["duration"].as_f64().unwrap_or(0.0);
                    p.volume = v["volume"].as_f64().unwrap_or(1.0) as f32;
                    p.shuffle = v["shuffle"].as_bool().unwrap_or(false);
                    p.repeat = v["repeat"].as_u64().unwrap_or(0) as u8;
                    p.autoplay = v["autoplay"].as_bool().unwrap_or(false);
                    let flag = |k: &str, d: bool| v[k].as_bool().unwrap_or(d);
                    p.radio = flag("radio", false);
                    p.live = flag("live", false);
                    p.can_pause = flag("canPause", true);
                    p.can_seek = flag("canSeek", true);
                    p.can_next = flag("canNext", true);
                    p.can_prev = flag("canPrev", true);
                    p.can_edit = flag("canEdit", true);
                    p.reported_at = now;
                    out.push(Event::Changed);
                }
                "now" => {
                    let item = Item::from(&v["item"]);
                    // Show a music video when one starts; the user can hide it from the bar.
                    if let Some(i) = &item
                        && i.is_video()
                        && self.playback.now.as_ref().map(|n| &n.id) != Some(&i.id)
                    {
                        self.video_open = true;
                    }
                    self.playback.now = item;
                    out.push(Event::Changed);
                }
                "queue" => {
                    let items = v["items"].as_array().map_or(&[][..], Vec::as_slice);
                    self.playback.queue = items.iter().filter_map(Item::from).collect();
                    self.playback.position = v["position"].as_u64().unwrap_or(0) as usize;
                    out.push(Event::Changed);
                }
                "done" if v["name"] == "setQueue" => {
                    if let Some(l) = &mut self.loading {
                        l.queued = true;
                    }
                }
                "error" => out.push(Event::Error(format!(
                    "{} failed: {}",
                    v["what"].as_str().unwrap_or("command"),
                    v["error"].as_str().unwrap_or("unknown error")
                ))),
                _ => {}
            }
        }
        // A play ends its spinner once audio runs, or when it failed or never answered.
        if let Some(l) = &self.loading {
            let failed = out
                .iter()
                .any(|e| matches!(e, Event::Error(m) if m.starts_with("setQueue")));
            if failed || (l.queued && self.playback.playing) || l.since.elapsed().as_secs() > 30 {
                self.loading = None;
                if failed {
                    self.queued_from = None;
                }
            }
        }
        out
    }

    fn run(&mut self, script: String) {
        match (&self.webview, self.authorized) {
            (Some(w), Some(true)) => {
                let _ = w.evaluate_script(&script);
            }
            _ => self.waiting.push(script),
        }
    }

    fn flush(&mut self) {
        if let Some(w) = &self.webview {
            for s in self.waiting.drain(..) {
                let _ = w.evaluate_script(&s);
            }
        }
    }

    /// An Apple Music API call; the answer arrives as `Event::Response` with this id.
    pub fn request(&mut self, path: &str, opts: Value) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.run(format!(
            "window.__sidra && window.__sidra.req({id}, {}, {opts})",
            json!(path)
        ));
        id
    }

    /// Opens Apple's sign-in box. Apple draws it inside its page, so the page is shown
    /// until the sign-in lands. Runs while signed out, unlike other commands.
    pub fn authorize(&mut self) {
        self.show_page = true;
        if let Some(w) = &self.webview {
            let _ = w.evaluate_script("window.__sidra && window.__sidra.cmd('authorize')");
        }
    }

    pub fn command(&mut self, name: &str, args: Value) {
        if name == "setQueue" {
            let o = &args["opts"];
            let id = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_owned);
            let source = id("album")
                .or_else(|| id("playlist"))
                .or_else(|| id("station"));
            self.queued_from = source.clone().filter(|_| o.get("station").is_none());
            self.loading = Some(Loading {
                source,
                track: id("song").or_else(|| {
                    o.get("songs")
                        .and_then(|s| s.get(0))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }),
                since: std::time::Instant::now(),
                queued: false,
            });
        }
        self.run(format!(
            "window.__sidra && window.__sidra.cmd({}, {args})",
            json!(name)
        ));
    }

    /// The playhead, moved on between reports while playing.
    pub fn position(&self, now: f64) -> f64 {
        let p = &self.playback;
        if p.playing {
            (p.time + (now - p.reported_at)).min(p.duration.max(p.time))
        } else {
            p.time
        }
    }
}

/// Apple's sign-in pages, the only popups the engine page may open.
fn is_sign_in_url(url: &str) -> bool {
    const HOSTS: [&str; 3] = [
        "authorize.music.apple.com",
        "idmsa.apple.com",
        "appleid.apple.com",
    ];
    url::Url::parse(url)
        .is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some_and(|h| HOSTS.contains(&h)))
}

fn pixel() -> Rect {
    Rect {
        position: dpi::LogicalPosition::new(0.0, 0.0).into(),
        size: dpi::LogicalSize::new(1.0, 1.0).into(),
    }
}
