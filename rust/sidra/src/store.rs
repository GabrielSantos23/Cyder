//! What the pages read: Apple Music responses, served from the disk cache when it
//! has them and refetched through the engine when they are missing or stale.

use crate::engine::Engine;
use crate::pages::Route;
use serde_json::{Value, json};
use sidra_core::apple::{self, Album, Artist, Card, LibraryEntry, Playlist, Shelf, Track};
use sidra_core::cache::{self, Cache, Kind};
use std::collections::HashMap;
use std::sync::Arc;

/// One thing a page can ask for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Req {
    Album(String),
    Playlist(String),
    Artist(String),
    Recommendations,
    /// An editorial grouping: `"music"` is the New page, `"radio"` the Radio page.
    Browse(&'static str),
    /// The children of a library playlist folder.
    Folder(String),
    LibraryAlbums,
    RecentlyAdded,
    LibraryPlaylists,
    LibraryArtists,
    LibrarySongs,
    LibraryVideos,
    /// Library playlists the user can't edit, from the same response as `LibraryPlaylists`.
    CuratedPlaylists,
    /// Syllable-timed lyrics, where Apple has them.
    Lyrics(String),
    /// Line-timed lyrics, for songs without syllable timing.
    LineLyrics(String),
    /// Loved state of up to 100 catalogue songs, comma-separated.
    Ratings(String),
    Search(String),
    /// The account's Apple Music profile: name, handle and photo.
    Profile,
}

pub const ROOT_FOLDER: &str = "p.playlistsroot";

pub enum Res {
    Album(Album),
    Playlist(Playlist),
    Artist(Artist),
    Shelves(Vec<Shelf>),
    Cards(Vec<Card>),
    Tracks(Vec<Track>),
    Entries(Vec<LibraryEntry>),
    Lyrics(crate::lyrics::Lyrics),
    Loved(Vec<String>),
    Search {
        songs: Vec<Track>,
        cards: Vec<Card>,
    },
    Profile {
        name: String,
        handle: Option<String>,
        artwork: Option<sidra_core::apple::Artwork>,
    },
    /// A library item that is shown through its catalogue page.
    Redirect(Route),
    Missing,
}

#[derive(Default)]
pub struct Slot {
    pub res: Option<Arc<Res>>,
    pub loading: bool,
    pub error: Option<String>,
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

impl Req {
    fn path(&self, sf: &str) -> String {
        match self {
            Req::Album(id) if id.starts_with("l.") => {
                format!("/v1/me/library/albums/{id}?include=tracks,artists")
            }
            Req::Album(id) => {
                format!("/v1/catalog/{sf}/albums/{id}?include=tracks,artists&relate=library")
            }
            Req::Playlist(id) if id.starts_with("p.") => {
                format!("/v1/me/library/playlists/{id}?include=tracks")
            }
            Req::Playlist(id) => {
                format!("/v1/catalog/{sf}/playlists/{id}?include=tracks&relate=library")
            }
            Req::Artist(id) if id.starts_with("r.") => {
                format!("/v1/me/library/artists/{id}?include=catalog")
            }
            Req::Artist(id) => format!(
                "/v1/catalog/{sf}/artists/{id}?views={}&extend=editorialArtwork",
                apple::ARTIST_VIEWS
            ),
            // Apple's own Home ("listen-now"), which alone carries Top Picks; `{tz}` is filled in by the page.
            Req::Recommendations => "/v1/me/recommendations?name=listen-now&platform=web&timezone={tz}&with=marketingItems&extend=editorialArtwork,plainEditorialNotes".into(),
            Req::Browse(name) => format!(
                "/v1/editorial/{sf}/groupings?name={name}&platform=web&tabs=subscriber&omit[resource:artists]=autos&extend=editorialArtwork"
            ),
            Req::Folder(id) => format!("/v1/me/library/playlist-folders/{id}/children?limit=100"),
            Req::LibraryAlbums => "/v1/me/library/albums?limit=100&sort=-dateAdded".into(),
            Req::RecentlyAdded => "/v1/me/library/recently-added?limit=25".into(),
            Req::LibraryPlaylists | Req::CuratedPlaylists => {
                "/v1/me/library/playlists?limit=100".into()
            }
            Req::LibraryVideos => "/v1/me/library/music-videos?limit=100".into(),
            Req::LibraryArtists => "/v1/me/library/artists?limit=100".into(),
            Req::LibrarySongs => "/v1/me/library/songs?limit=100".into(),
            Req::Lyrics(id) => format!("/v1/catalog/{sf}/songs/{id}/syllable-lyrics"),
            Req::LineLyrics(id) => format!("/v1/catalog/{sf}/songs/{id}/lyrics"),
            Req::Profile => "/v1/me/social/profile?extend=artwork,handle".into(),
            Req::Ratings(ids) => format!("/v1/me/ratings/songs?ids={ids}"),
            Req::Search(term) => format!(
                "/v1/catalog/{sf}/search?term={}&types=songs,albums,artists,playlists&limit=15",
                enc(term)
            ),
        }
    }

    fn opts(&self) -> Value {
        match self {
            Req::Album(_) | Req::Playlist(_) => json!({"pageRel": "tracks"}),
            Req::Folder(_) => json!({"all": 500}),
            Req::RecentlyAdded => json!({"all": 100}),
            // shortcut: library lists stop at these sizes; page them on scroll if users hit the cap.
            Req::LibraryAlbums
            | Req::LibraryPlaylists
            | Req::CuratedPlaylists
            | Req::LibraryArtists
            | Req::LibraryVideos => {
                json!({"all": 2000})
            }
            Req::LibrarySongs => json!({"all": 3000}),
            _ => json!({}),
        }
    }

    fn parse(&self, doc: &Value) -> Res {
        let opt = |r: Option<Res>| r.unwrap_or(Res::Missing);
        match self {
            Req::Album(_) => opt(apple::album(doc).map(Res::Album)),
            Req::Playlist(_) => opt(apple::playlist(doc).map(Res::Playlist)),
            Req::Artist(id) if id.starts_with("r.") => opt(doc
                .pointer("/data/0/relationships/catalog/data/0/id")
                .and_then(Value::as_str)
                .map(|c| Res::Redirect(Route::Artist(c.to_owned())))),
            Req::Artist(_) => opt(apple::artist(doc).map(Res::Artist)),
            Req::Recommendations => Res::Shelves(apple::recommendations(doc)),
            Req::Browse(_) => Res::Shelves(apple::editorial(doc)),
            Req::CuratedPlaylists => Res::Cards(apple::curated_playlists(doc)),
            Req::Folder(_) => Res::Entries(apple::library_entries(doc)),
            Req::LibrarySongs => Res::Tracks(apple::track_list(doc)),
            Req::Lyrics(_) | Req::LineLyrics(_) => opt(apple::lyrics_ttml(doc)
                .and_then(|ttml| {
                    use crate::lyrics::{Lyrics, Provider, parse_ttml};
                    let t = parse_ttml(ttml, 0);
                    let provider = if t.word_synced {
                        Provider::BLyricsRich
                    } else {
                        Provider::BLyricsSynced
                    };
                    Lyrics::new(provider, t.lines, t.language, t.songwriters)
                })
                .map(Res::Lyrics)),
            Req::Ratings(_) => Res::Loved(apple::loved_ids(doc)),
            Req::Profile => {
                let a = doc.pointer("/data/0/attributes").unwrap_or(&Value::Null);
                let text = |k: &str| {
                    a.get(k)
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                };
                opt(text("name").map(|name| Res::Profile {
                    name,
                    // Without a handle attribute, the profile URL ends in it (`…/profile/<handle>`).
                    handle: text("handle").or_else(|| {
                        text("url")?
                            .split("/profile/")
                            .nth(1)
                            .map(|h| h.trim_matches('/').to_owned())
                            .filter(|h| !h.is_empty())
                    }),
                    artwork: apple::artwork(a.get("artwork")),
                }))
            }
            Req::Search(_) => Res::Search {
                songs: doc
                    .pointer("/results/songs/data")
                    .and_then(Value::as_array)
                    .map_or(Vec::new(), |d| d.iter().map(apple::track).collect()),
                cards: ["artists", "albums", "playlists"]
                    .iter()
                    .flat_map(|k| {
                        doc.pointer(&format!("/results/{k}/data"))
                            .and_then(Value::as_array)
                            .map_or(Vec::new(), |d| apple::cards(d))
                    })
                    .collect(),
            },
            _ => Res::Cards(apple::card_list(doc)),
        }
    }

    fn kind(&self, res: &Res) -> Kind {
        match self {
            Req::Album(id) if id.starts_with("l.") => Kind::Library,
            Req::Album(_) => Kind::CatalogAlbum,
            Req::Playlist(_) => Kind::Playlist {
                modified: match res {
                    Res::Playlist(p) => p.last_modified.as_deref().and_then(cache::parse_iso8601),
                    _ => None,
                },
            },
            Req::Artist(_) => Kind::Artist,
            Req::Recommendations | Req::Browse(_) | Req::Search(_) => Kind::Home,
            Req::Lyrics(_) | Req::LineLyrics(_) => Kind::Lyrics,
            _ => Kind::Library,
        }
    }
}

/// What to do when a change Sidra asked for has gone through.
pub enum Done {
    /// The library changed: drop library responses and say so.
    Library(String),
    /// An album or playlist was added or removed: as `Library`, then reload `page`
    /// so it learns the library id it now has, or that it has none.
    Collection {
        msg: String,
        page: Req,
    },
    Loved(String, bool),
}

enum Pending {
    Load(Req),
    Change(Done),
    /// Opens the album or artist of a song once its relationships arrive.
    Goto {
        artist: bool,
    },
}

pub struct Store {
    cache: Cache,
    slots: HashMap<Req, Slot>,
    pending: HashMap<u64, Pending>,
    /// Loved state set in this session, ahead of the cached ratings.
    loved: HashMap<String, bool>,
    pub toasts: Vec<(String, f64)>,
    pub goto: Option<Route>,
    /// Ids added to the library this session, ahead of the cached pages that predate it.
    pub added: std::collections::HashSet<String>,
    /// Pages to reload the next time they are read, without blanking them meanwhile.
    stale: Vec<Req>,
}

impl Store {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self {
            cache: Cache::new(dir),
            slots: HashMap::new(),
            pending: HashMap::new(),
            loved: HashMap::new(),
            toasts: Vec::new(),
            goto: None,
            added: Default::default(),
            stale: Vec::new(),
        }
    }

    fn key(req: &Req, sf: &str) -> String {
        req.path(sf)
    }

    /// The slot for `req`, loading it from disk or the network on first use.
    pub fn get(&mut self, engine: &mut Engine, req: &Req) -> &Slot {
        if let Some(i) = self.stale.iter().position(|r| r == req) {
            self.stale.swap_remove(i);
            self.fetch(engine, req);
        }
        if !self.slots.contains_key(req) {
            let mut slot = Slot::default();
            let key = Self::key(req, &engine.storefront);
            let stale = match self.cache.get(&key) {
                Some(hit) => {
                    slot.res = Some(Arc::new(req.parse(&hit.body)));
                    !hit.fresh
                }
                None => true,
            };
            self.slots.insert(req.clone(), slot);
            if stale {
                self.fetch(engine, req);
            }
        }
        &self.slots[req]
    }

    pub fn res(&mut self, engine: &mut Engine, req: &Req) -> Option<Arc<Res>> {
        self.get(engine, req).res.clone()
    }

    fn fetch(&mut self, engine: &mut Engine, req: &Req) {
        let slot = self.slots.entry(req.clone()).or_default();
        if slot.loading {
            return;
        }
        slot.loading = true;
        slot.error = None;
        let id = engine.request(&req.path(&engine.storefront), req.opts());
        self.pending.insert(id, Pending::Load(req.clone()));
    }

    pub fn refresh(&mut self, engine: &mut Engine, req: &Req) {
        self.fetch(engine, req);
    }

    pub fn is_loading(&self, req: &Req) -> bool {
        self.slots.get(req).is_some_and(|s| s.loading)
    }

    /// A change to the user's account: library, ratings or playlists.
    pub fn change(
        &mut self,
        engine: &mut Engine,
        method: &str,
        path: &str,
        body: Option<Value>,
        done: Done,
    ) {
        if let Done::Loved(id, on) = &done {
            self.loved.insert(id.clone(), *on);
        }
        let id = engine.request(path, json!({"method": method, "body": body}));
        self.pending.insert(id, Pending::Change(done));
    }

    /// Opens a song's album or artist page, looking the id up first.
    pub fn goto_from_song(&mut self, engine: &mut Engine, catalog_id: &str, artist: bool) {
        let path = format!(
            "/v1/catalog/{}/songs/{catalog_id}?include=artists,albums",
            engine.storefront
        );
        let id = engine.request(&path, json!({}));
        self.pending.insert(id, Pending::Goto { artist });
    }

    pub fn toast(&mut self, text: impl Into<String>, now: f64) {
        self.toasts.push((text.into(), now));
    }

    pub fn respond(&mut self, engine: &Engine, id: u64, result: Result<Value, String>, now: f64) {
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        match (pending, result) {
            (Pending::Load(req), Ok(body)) => {
                let res = req.parse(&body);
                let key = Self::key(&req, &engine.storefront);
                if !matches!(res, Res::Missing)
                    && let Err(e) = self.cache.put(&key, &body, req.kind(&res))
                {
                    eprintln!("sidra: could not cache a response: {e}");
                }
                let slot = self.slots.entry(req).or_default();
                slot.loading = false;
                slot.res = Some(Arc::new(res));
            }
            (Pending::Load(req), Err(e)) => {
                eprintln!("sidra: {req:?} failed: {e}");
                let slot = self.slots.entry(req).or_default();
                slot.loading = false;
                slot.error = Some(e);
            }
            (Pending::Change(done), Ok(_)) => match done {
                Done::Collection { msg, page } => {
                    self.clear_library();
                    self.cache.remove(&Self::key(&page, &engine.storefront));
                    self.stale.push(page);
                    self.toast(msg, now);
                }
                Done::Library(msg) => {
                    self.clear_library();
                    self.toast(msg, now);
                }
                Done::Loved(id, on) => {
                    self.loved.insert(id, on);
                    self.toast(
                        if on {
                            "Added to Favourites"
                        } else {
                            "Removed from Favourites"
                        },
                        now,
                    );
                }
            },
            (Pending::Change(done), Err(e)) => {
                if let Done::Loved(id, on) = done {
                    self.loved.insert(id, !on);
                }
                self.toast(format!("That didn't work: {e}"), now);
            }
            (Pending::Goto { artist }, Ok(body)) => {
                let rel = if artist { "artists" } else { "albums" };
                let id = body
                    .pointer(&format!("/data/0/relationships/{rel}/data/0/id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                match id {
                    Some(id) if artist => self.goto = Some(Route::Artist(id)),
                    Some(id) => self.goto = Some(Route::Album(id)),
                    None => self.toast("Apple Music has no page for that", now),
                }
            }
            (Pending::Goto { .. }, Err(e)) => {
                self.toast(format!("Couldn't open that page: {e}"), now)
            }
        }
    }

    /// Drops library responses on disk and in memory, so pages refetch them.
    pub fn clear_library(&mut self) {
        self.cache.clear_library();
        self.slots.retain(|req, _| {
            !matches!(
                req,
                Req::Folder(_)
                    | Req::LibraryAlbums
                    | Req::RecentlyAdded
                    | Req::LibraryPlaylists
                    | Req::LibraryArtists
                    | Req::LibrarySongs
                    | Req::Ratings(_)
            ) && !matches!(req, Req::Playlist(id) | Req::Album(id) if id.starts_with("p.") || id.starts_with("l."))
        });
    }

    /// Forgets loaded responses without touching the disk, after the storefront changes.
    pub fn forget(&mut self) {
        self.slots.clear();
    }

    /// Drops everything, for "Clear cache".
    pub fn clear_all(&mut self) {
        let _ = std::fs::remove_dir_all(self.cache.dir());
        self.slots.clear();
    }

    /// Whether a catalogue song is loved, from this session's changes or the ratings loaded for `ids`.
    pub fn loved(&mut self, engine: &mut Engine, ids: &[&str]) -> HashMap<String, bool> {
        let mut out: HashMap<String, bool> = HashMap::new();
        for chunk in ids.chunks(100) {
            let req = Req::Ratings(chunk.join(","));
            if let Some(res) = self.res(engine, &req)
                && let Res::Loved(list) = &*res
            {
                out.extend(list.iter().map(|id| (id.clone(), true)));
            }
        }
        out.extend(self.loved.iter().map(|(k, v)| (k.clone(), *v)));
        out
    }

    pub fn set_loved(&mut self, engine: &mut Engine, catalog_id: &str, on: bool) {
        let path = format!("/v1/me/ratings/songs/{catalog_id}");
        let (method, body) = if on {
            (
                "PUT",
                Some(json!({"type": "rating", "attributes": {"value": 1}})),
            )
        } else {
            ("DELETE", None)
        };
        self.change(
            engine,
            method,
            &path,
            body,
            Done::Loved(catalog_id.to_owned(), on),
        );
    }

    /// `kind` is the API's plural type: `songs`, `albums` or `playlists`.
    /// Adds an album or playlist and reloads its `page`, which then knows its library id.
    pub fn add_collection(&mut self, engine: &mut Engine, kind: &str, id: &str, page: Req) {
        self.added.insert(id.to_owned());
        let path = format!("/v1/me/library?ids[{kind}]={id}");
        let msg = "Added to your library".into();
        self.change(engine, "POST", &path, None, Done::Collection { msg, page });
    }

    /// Removes an album or playlist from the library by its library id.
    pub fn remove_collection(
        &mut self,
        engine: &mut Engine,
        kind: &str,
        id: &str,
        library_id: &str,
        page: Req,
    ) {
        self.added.remove(id);
        let path = format!("/v1/me/library/{kind}/{library_id}");
        let msg = "Removed from your library".into();
        self.change(
            engine,
            "DELETE",
            &path,
            None,
            Done::Collection { msg, page },
        );
    }

    pub fn add_to_library(&mut self, engine: &mut Engine, kind: &str, id: &str) {
        self.added.insert(id.to_owned());
        let path = format!("/v1/me/library?ids[{kind}]={id}");
        self.change(
            engine,
            "POST",
            &path,
            None,
            Done::Library("Added to your library".into()),
        );
    }

    pub fn add_to_playlist(&mut self, engine: &mut Engine, playlist: &str, song: &str, kind: &str) {
        let path = format!("/v1/me/library/playlists/{playlist}/tracks");
        let body = json!({"data": [{"id": song, "type": kind}]});
        self.change(
            engine,
            "POST",
            &path,
            Some(body),
            Done::Library("Added to the playlist".into()),
        );
    }

    pub fn create_playlist(&mut self, engine: &mut Engine, name: &str) {
        let body = json!({"attributes": {"name": name}});
        self.change(
            engine,
            "POST",
            "/v1/me/library/playlists",
            Some(body),
            Done::Library(format!("Created \u{201c}{name}\u{201d}")),
        );
    }
}
