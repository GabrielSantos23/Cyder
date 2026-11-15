//! Apple Music API responses reduced to what Sidra's pages draw.
//!
//! Every parser takes the raw JSON:API document and tolerates missing fields:
//! Apple omits attributes freely (no artwork on a new playlist, no track number on
//! a library upload), and a page with a gap beats a page that fails to open.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Artwork {
    /// Apple's template, with `{w}`, `{h}` and sometimes `{c}` and `{f}` placeholders.
    pub template: String,
    /// The dominant colour Apple computes for the image, used to tint page headers.
    pub bg: Option<[u8; 3]>,
}

impl Artwork {
    /// The image URL at `px` square, or `None` when the template is empty.
    pub fn url(&self, px: u32) -> Option<String> {
        if self.template.is_empty() {
            return None;
        }
        let px = px.to_string();
        Some(
            self.template
                .replace("{w}", &px)
                .replace("{h}", &px)
                .replace("{c}", "bb")
                .replace("{f}", "jpg"),
        )
    }

    /// The image URL at an exact size, for wide editorial banners.
    pub fn url_wh(&self, w: u32, h: u32) -> Option<String> {
        if self.template.is_empty() {
            return None;
        }
        Some(
            self.template
                .replace("{w}", &w.to_string())
                .replace("{h}", &h.to_string())
                .replace("{c}", "sr")
                .replace("{f}", "jpg"),
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// The id MusicKit queues by: a catalogue id, or `i.` for a library song.
    pub id: String,
    pub catalog_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Catalogue album id, from the song's share URL.
    pub album_id: Option<String>,
    pub duration_ms: u64,
    pub number: Option<u32>,
    pub disc: Option<u32>,
    pub explicit: bool,
    pub artwork: Option<Artwork>,
    pub url: Option<String>,
    /// False for an item the account cannot play, such as one withdrawn from the catalogue.
    pub playable: bool,
}

impl Track {
    /// The catalogue id when there is one, which is what ratings and share links need.
    pub fn catalog(&self) -> Option<&str> {
        self.catalog_id
            .as_deref()
            .or_else(|| (!self.id.starts_with("i.")).then_some(self.id.as_str()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardKind {
    Album,
    Playlist,
    Artist,
    Station,
    Song,
}

/// One tile on a shelf: enough to draw it and to open or play what it points at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub kind: CardKind,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artwork: Option<Artwork>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Shelf {
    pub title: String,
    pub cards: Vec<Card>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub artist_artwork: Option<Artwork>,
    pub artwork: Option<Artwork>,
    pub genre: Option<String>,
    pub year: Option<String>,
    pub track_count: usize,
    pub tracks: Vec<Track>,
    pub url: Option<String>,
    pub copyright: Option<String>,
    pub library: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub curator: String,
    pub description: Option<String>,
    pub artwork: Option<Artwork>,
    pub last_modified: Option<String>,
    pub tracks: Vec<Track>,
    pub url: Option<String>,
    pub library: bool,
    pub editable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub artwork: Option<Artwork>,
    /// The wide editorial image Apple puts behind an artist's name.
    pub hero: Option<Artwork>,
    pub genre: Option<String>,
    pub top_songs: Vec<Track>,
    pub latest: Option<Card>,
    pub shelves: Vec<Shelf>,
    pub url: Option<String>,
}

/// An entry in the library's playlist tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub id: String,
    pub name: String,
    pub folder: bool,
    pub artwork: Option<Artwork>,
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

fn attrs(v: &Value) -> &Value {
    v.get("attributes").unwrap_or(&Value::Null)
}

/// The first resource of a document, or the document itself when it is already one.
fn first(doc: &Value) -> &Value {
    doc.get("data").and_then(|d| d.get(0)).unwrap_or(doc)
}

fn rel<'a>(v: &'a Value, name: &str) -> &'a [Value] {
    v.pointer(&format!("/relationships/{name}/data"))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn hex_colour(text: &str) -> Option<[u8; 3]> {
    let n = u32::from_str_radix(text.trim_start_matches('#'), 16).ok()?;
    (text.trim_start_matches('#').len() == 6).then_some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

pub fn artwork(v: Option<&Value>) -> Option<Artwork> {
    let v = v?;
    let template = s(v, "url")?;
    Some(Artwork {
        template,
        bg: v
            .get("bgColor")
            .and_then(Value::as_str)
            .and_then(hex_colour),
    })
}

/// A catalogue album id taken from a song's share URL (`…/album/name/123?i=456`).
fn album_id_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next()?;
    let mut parts = path.rsplit('/');
    let id = parts.next()?;
    (path.contains("/album/") && id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty())
        .then(|| id.to_owned())
}

pub fn track(v: &Value) -> Track {
    let a = attrs(v);
    let id = s(v, "id").unwrap_or_default();
    let catalog_id = a
        .pointer("/playParams/catalogId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            a.pointer("/playParams/id")
                .and_then(Value::as_str)
                .filter(|p| !p.starts_with("i."))
                .map(str::to_owned)
        });
    let url = s(a, "url");
    Track {
        album_id: url.as_deref().and_then(album_id_from_url),
        id,
        catalog_id,
        kind: s(v, "type").unwrap_or_default(),
        title: s(a, "name").unwrap_or_default(),
        artist: s(a, "artistName").unwrap_or_default(),
        album: s(a, "albumName").unwrap_or_default(),
        duration_ms: a
            .get("durationInMillis")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        number: a
            .get("trackNumber")
            .and_then(Value::as_u64)
            .map(|n| n as u32),
        disc: a
            .get("discNumber")
            .and_then(Value::as_u64)
            .map(|n| n as u32),
        explicit: a.get("contentRating").and_then(Value::as_str) == Some("explicit"),
        artwork: artwork(a.get("artwork")),
        url,
        playable: a.get("playParams").is_some(),
    }
}

fn year(a: &Value) -> Option<String> {
    s(a, "releaseDate").map(|d| d.chars().take(4).collect())
}

pub fn card(v: &Value) -> Option<Card> {
    let a = attrs(v);
    let kind = match s(v, "type")?.as_str() {
        "albums" | "library-albums" => CardKind::Album,
        "playlists" | "library-playlists" => CardKind::Playlist,
        "artists" | "library-artists" => CardKind::Artist,
        "stations" => CardKind::Station,
        "songs" | "library-songs" | "music-videos" => CardKind::Song,
        _ => return None,
    };
    let title = s(a, "name")?;
    let subtitle = match kind {
        CardKind::Album => {
            let artist = s(a, "artistName").unwrap_or_default();
            match year(a) {
                Some(y) if !artist.is_empty() => format!("{artist} · {y}"),
                _ => artist,
            }
        }
        CardKind::Playlist => s(a, "curatorName").unwrap_or_else(|| "Playlist".into()),
        CardKind::Artist => "Artist".into(),
        CardKind::Station => "Station".into(),
        CardKind::Song => s(a, "artistName").unwrap_or_default(),
    };
    Some(Card {
        kind,
        id: s(v, "id")?,
        title,
        subtitle,
        artwork: artwork(a.get("artwork")),
    })
}

pub fn cards(list: &[Value]) -> Vec<Card> {
    list.iter().filter_map(card).collect()
}

/// `GET /v1/catalog/{sf}/albums/{id}?include=tracks,artists` or the library equivalent.
pub fn album(doc: &Value) -> Option<Album> {
    let v = first(doc);
    let a = attrs(v);
    let tracks: Vec<Track> = rel(v, "tracks").iter().map(track).collect();
    let artist = rel(v, "artists").first();
    Some(Album {
        id: s(v, "id")?,
        title: s(a, "name").unwrap_or_default(),
        artist: s(a, "artistName").unwrap_or_default(),
        artist_id: artist
            .and_then(|r| s(r, "id"))
            .filter(|id| !id.starts_with("r.")),
        artist_artwork: artist.and_then(|r| artwork(attrs(r).get("artwork"))),
        artwork: artwork(a.get("artwork")),
        genre: a
            .get("genreNames")
            .and_then(|g| g.get(0))
            .and_then(Value::as_str)
            .map(str::to_owned),
        year: year(a),
        track_count: a
            .get("trackCount")
            .and_then(Value::as_u64)
            .map_or(tracks.len(), |n| n as usize),
        tracks,
        url: s(a, "url"),
        copyright: s(a, "copyright"),
        library: s(v, "type").as_deref() == Some("library-albums"),
    })
}

/// `GET /v1/catalog/{sf}/playlists/{id}?include=tracks` or the library equivalent.
pub fn playlist(doc: &Value) -> Option<Playlist> {
    let v = first(doc);
    let a = attrs(v);
    let library = s(v, "type").as_deref() == Some("library-playlists");
    Some(Playlist {
        id: s(v, "id")?,
        name: s(a, "name").unwrap_or_default(),
        curator: s(a, "curatorName").unwrap_or_else(|| {
            if library {
                "Your playlist".into()
            } else {
                "Apple Music".into()
            }
        }),
        description: a
            .pointer("/description/standard")
            .or_else(|| a.pointer("/description/short"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        artwork: artwork(a.get("artwork")),
        last_modified: s(a, "lastModifiedDate"),
        tracks: rel(v, "tracks").iter().map(track).collect(),
        url: s(a, "url"),
        library,
        editable: a.get("canEdit").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn view<'a>(v: &'a Value, name: &str) -> (&'a [Value], Option<String>) {
    let view = v.pointer(&format!("/views/{name}"));
    let data = view
        .and_then(|x| x.get("data"))
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let title = view
        .and_then(|x| x.pointer("/attributes/title"))
        .and_then(Value::as_str);
    (data, title.map(str::to_owned))
}

/// The artist views Sidra asks for, in the order its page shows them.
pub const ARTIST_VIEWS: &str =
    "top-songs,latest-release,full-albums,singles,appears-on,similar-artists";

/// `GET /v1/catalog/{sf}/artists/{id}?views=…&extend=editorialArtwork`.
pub fn artist(doc: &Value) -> Option<Artist> {
    let v = first(doc);
    let a = attrs(v);
    let hero = a.get("editorialArtwork").and_then(|e| {
        [
            "centeredFullscreenBackground",
            "bannerUber",
            "superHeroWide",
            "storeFlowcase",
        ]
        .iter()
        .find_map(|k| artwork(e.get(*k)))
    });
    let shelves = ["full-albums", "singles", "appears-on", "similar-artists"]
        .iter()
        .filter_map(|name| {
            let (data, title) = view(v, name);
            let cards = cards(data);
            (!cards.is_empty()).then(|| Shelf {
                title: title.unwrap_or_else(|| (*name).into()),
                cards,
            })
        })
        .collect();
    Some(Artist {
        id: s(v, "id")?,
        name: s(a, "name").unwrap_or_default(),
        artwork: artwork(a.get("artwork")),
        hero,
        genre: a
            .get("genreNames")
            .and_then(|g| g.get(0))
            .and_then(Value::as_str)
            .map(str::to_owned),
        top_songs: view(v, "top-songs").0.iter().map(track).collect(),
        latest: view(v, "latest-release").0.first().and_then(card),
        shelves,
        url: s(a, "url"),
    })
}

/// `GET /v1/me/recommendations`: one shelf per recommendation group.
pub fn recommendations(doc: &Value) -> Vec<Shelf> {
    doc.get("data")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|g| {
            let title = g
                .pointer("/attributes/title/stringForDisplay")
                .and_then(Value::as_str)?
                .to_owned();
            let cards = cards(rel(g, "contents"));
            (!cards.is_empty()).then_some(Shelf { title, cards })
        })
        .collect()
}

/// Any list endpoint (`/v1/me/recent/played`, library albums, search results) as cards.
pub fn card_list(doc: &Value) -> Vec<Card> {
    cards(
        doc.get("data")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice),
    )
}

pub fn track_list(doc: &Value) -> Vec<Track> {
    doc.get("data")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(track)
        .collect()
}

/// `GET /v1/me/library/playlist-folders/{id}/children`.
pub fn library_entries(doc: &Value) -> Vec<LibraryEntry> {
    doc.get("data")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|v| {
            let folder = match s(v, "type")?.as_str() {
                "library-playlist-folders" => true,
                "library-playlists" => false,
                _ => return None,
            };
            Some(LibraryEntry {
                id: s(v, "id")?,
                name: s(attrs(v), "name").unwrap_or_default(),
                folder,
                artwork: artwork(attrs(v).get("artwork")),
            })
        })
        .collect()
}

/// `GET /v1/me/ratings/songs?ids=…`: the ids rated as loved.
pub fn loved_ids(doc: &Value) -> Vec<String> {
    doc.get("data")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|r| r.pointer("/attributes/value").and_then(Value::as_i64) == Some(1))
        .filter_map(|r| s(r, "id"))
        .collect()
}

/// `GET /v1/catalog/{sf}/songs/{id}/syllable-lyrics` (or `/lyrics`): the TTML document,
/// which the app's lyrics renderer parses itself.
pub fn lyrics_ttml(doc: &Value) -> Option<&str> {
    first(doc)
        .pointer("/attributes/ttml")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn artwork_fills_the_template() {
        let a = Artwork {
            template: "https://x/{w}x{h}{c}.{f}".into(),
            bg: None,
        };
        assert_eq!(a.url(300).unwrap(), "https://x/300x300bb.jpg");
        assert_eq!(hex_colour("1a2b3c"), Some([0x1a, 0x2b, 0x3c]));
        assert_eq!(hex_colour("zz"), None);
    }

    #[test]
    fn album_reads_tracks_artist_and_counts() {
        let doc = json!({"data": [{
            "id": "123", "type": "albums",
            "attributes": {"name": "girl EDM", "artistName": "Nina", "genreNames": ["Dance", "Music"],
                "releaseDate": "2024-03-01", "trackCount": 2,
                "artwork": {"url": "u/{w}x{h}bb.jpg", "bgColor": "101010"}},
            "relationships": {
                "artists": {"data": [{"id": "77", "type": "artists"}]},
                "tracks": {"data": [
                    {"id": "1", "type": "songs", "attributes": {"name": "Angel", "artistName": "Nina",
                        "durationInMillis": 127000, "trackNumber": 1, "contentRating": "explicit",
                        "url": "https://music.apple.com/br/album/girl-edm/123?i=1", "playParams": {"id": "1"}}},
                    {"id": "2", "type": "songs", "attributes": {"name": "1x1"}}
                ]}
            }
        }]});
        let a = album(&doc).unwrap();
        assert_eq!(
            (a.title.as_str(), a.genre.as_deref(), a.year.as_deref()),
            ("girl EDM", Some("Dance"), Some("2024"))
        );
        assert_eq!(a.artist_id.as_deref(), Some("77"));
        assert_eq!(a.tracks.len(), 2);
        assert!(a.tracks[0].explicit && a.tracks[0].playable);
        assert_eq!(a.tracks[0].album_id.as_deref(), Some("123"));
        assert!(!a.tracks[1].playable);
        assert_eq!(a.artwork.unwrap().bg, Some([16, 16, 16]));
    }

    #[test]
    fn library_track_keeps_its_catalogue_id() {
        let t = track(&json!({"id": "i.abc", "type": "library-songs",
            "attributes": {"name": "x", "playParams": {"id": "i.abc", "catalogId": "999"}}}));
        assert_eq!(t.catalog(), Some("999"));
        let t = track(
            &json!({"id": "i.abc", "type": "library-songs", "attributes": {"playParams": {"id": "i.abc"}}}),
        );
        assert_eq!(t.catalog(), None);
    }

    #[test]
    fn artist_views_become_shelves() {
        let doc = json!({"data": [{"id": "5", "type": "artists",
        "attributes": {"name": "Chromeo", "editorialArtwork": {"bannerUber": {"url": "b/{w}x{h}{c}.{f}"}}},
        "views": {
            "top-songs": {"data": [{"id": "s1", "type": "songs", "attributes": {"name": "Fancy Footwork"}}]},
            "latest-release": {"data": [{"id": "a1", "type": "albums", "attributes": {"name": "Single", "artistName": "Chromeo"}}]},
            "full-albums": {"attributes": {"title": "Albums"}, "data": [{"id": "a2", "type": "albums", "attributes": {"name": "Adult Contemporary"}}]},
            "singles": {"data": []}
        }}]});
        let a = artist(&doc).unwrap();
        assert_eq!(a.top_songs[0].title, "Fancy Footwork");
        assert_eq!(a.latest.unwrap().id, "a1");
        assert_eq!(a.shelves.len(), 1);
        assert_eq!(a.shelves[0].title, "Albums");
        assert!(a.hero.is_some());
    }

    #[test]
    fn recommendations_and_folders() {
        let doc = json!({"data": [{"attributes": {"title": {"stringForDisplay": "Top Picks for You"}},
            "relationships": {"contents": {"data": [
                {"id": "pl.1", "type": "playlists", "attributes": {"name": "Chill", "curatorName": "Apple Music"}},
                {"id": "x", "type": "unknown-kind", "attributes": {"name": "skip"}}]}}}]});
        let shelves = recommendations(&doc);
        assert_eq!(shelves[0].cards.len(), 1);
        assert_eq!(shelves[0].cards[0].kind, CardKind::Playlist);

        let doc = json!({"data": [
            {"id": "p.f", "type": "library-playlist-folders", "attributes": {"name": "Sets"}},
            {"id": "p.1", "type": "library-playlists", "attributes": {"name": "Mine"}}]});
        let e = library_entries(&doc);
        assert!(e[0].folder && !e[1].folder);
    }

    #[test]
    fn lyrics_document() {
        let doc = json!({"data": [{"attributes": {"ttml": "<tt/>"}}]});
        assert_eq!(lyrics_ttml(&doc), Some("<tt/>"));
        assert_eq!(
            lyrics_ttml(&json!({"data": [{"attributes": {"ttml": ""}}]})),
            None
        );
    }

    #[test]
    fn loved_ratings() {
        let doc = json!({"data": [{"id": "1", "attributes": {"value": 1}}, {"id": "2", "attributes": {"value": -1}}]});
        assert_eq!(loved_ids(&doc), vec!["1".to_string()]);
    }
}
