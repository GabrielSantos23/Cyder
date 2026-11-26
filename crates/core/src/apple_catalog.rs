//! Catálogo do Apple Music (doc 13, fase 3): tela inicial, biblioteca da pessoa, busca e as páginas que eles abrem.
//! Vira `Shelf`, `Card`, `Page`, `Track` e `SearchResults` como os do Spotify, com URIs `apple:…`; as faixas carregam
//! o ID do catálogo da Apple, e o player do Apple Music toca direto, sem procurar pelo ISRC.
//! IDs da biblioteca da pessoa têm prefixo: `p.` (playlist), `l.` (álbum), `r.` (artista), `i.` (música).
use crate::i18n::t;
use std::sync::Mutex;

use serde_json::Value;

use crate::{ArtistInfo, Card, Kind, Library, Page, SearchKind, SearchResults, Shelf, Track, motion};

/// Prefixo das URIs do catálogo da Apple (`apple:playlist:pl.…`, `apple:album:…`, `apple:song:…`).
pub const URI: &str = "apple:";

/// Token da pessoa no Apple Music (`musicUserToken` do MusicKit do WebView2), para a biblioteca dela.
static USER_TOKEN: Mutex<Option<String>> = Mutex::new(None);

/// Guarda o token que veio do MusicKit. Verdadeiro se mudou (chegou agora ou é outro login).
pub fn set_user_token(token: &str) -> bool {
    let mut t = USER_TOKEN.lock().unwrap_or_else(|e| e.into_inner());
    if t.as_deref() == Some(token) {
        return false;
    }
    *t = Some(token.to_string());
    true
}

fn user_token() -> Result<String, String> {
    USER_TOKEN.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or_else(|| t("Entre no Apple Music para ver a sua biblioteca de lá.").to_string())
}

/// Imagem da Apple no tamanho pedido: a URL vem com `{w}`, `{h}` e `{f}` para preencher.
fn art(item: &Value, size: u32) -> Option<String> {
    let url = item["attributes"]["artwork"]["url"].as_str()?;
    Some(url.replace("{w}", &size.to_string()).replace("{h}", &size.to_string()).replace("{f}", "jpg"))
}

/// Playlist, álbum ou artista, do catálogo ou da biblioteca; o resto (músicas soltas, rádios, vídeos) fica de fora.
fn card(item: &Value) -> Option<Card> {
    let (kind, path, by) = match item["type"].as_str()? {
        "playlists" | "library-playlists" => (Kind::Playlist, "playlist", "curatorName"),
        "albums" | "library-albums" => (Kind::Album, "album", "artistName"),
        "artists" | "library-artists" => (Kind::Artist, "artist", ""),
        _ => return None,
    };
    let id = item["id"].as_str()?;
    let a = &item["attributes"];
    // Artista da biblioteca não tem foto: a do catálogo vem junto (`include=catalog`).
    let image = art(item, 300).or_else(|| art(&item["relationships"]["catalog"]["data"][0], 300));
    let subtitle = match kind {
        Kind::Artist => "Artista".to_string(),
        _ => a[by].as_str().unwrap_or_default().to_string(),
    };
    Some(Card { kind, id: id.into(), uri: format!("{URI}{path}:{id}"), title: a["name"].as_str()?.into(), subtitle, image })
}

fn cards(list: &Value) -> Vec<Card> {
    list.as_array().into_iter().flatten().filter_map(card).collect()
}

fn shelves(groupings: &Value) -> Vec<Shelf> {
    let tabs = &groupings["data"][0]["relationships"]["tabs"]["data"];
    let elements = tabs[0]["relationships"]["children"]["data"].as_array().into_iter().flatten();
    elements
        .filter_map(|e| {
            let title = e["attributes"]["name"].as_str()?;
            let cards = cards(&e["relationships"]["contents"]["data"]);
            (!cards.is_empty()).then(|| Shelf { title: title.into(), cards })
        })
        .collect()
}

/// As prateleiras da tela inicial do Apple Music na loja (`br`, `us`…). Não precisa de login.
pub async fn home(http: &reqwest::Client, storefront: &str) -> Result<Vec<Shelf>, String> {
    let path = format!("/v1/editorial/{storefront}/groupings?platform=web&name=music&l=pt-BR&art%5Burl%5D=f");
    Ok(shelves(&motion::amp(http, &path).await?))
}

fn track(item: &Value) -> Option<Track> {
    // Playlists também têm vídeos; o player só toca músicas.
    let a = &item["attributes"];
    let id = match item["type"].as_str()? {
        "songs" => item["id"].as_str()?,
        // Da biblioteca: toca pelo ID do catálogo (as enviadas pela pessoa não têm, e ficam de fora).
        "library-songs" => a["playParams"]["catalogId"].as_str()?,
        _ => return None,
    };
    Some(Track {
        id: id.into(),
        uri: format!("{URI}song:{id}"),
        name: a["name"].as_str()?.into(),
        artist: a["artistName"].as_str().unwrap_or_default().into(),
        album: a["albumName"].as_str().unwrap_or_default().into(),
        // Sem ID do Spotify: o álbum e os artistas não abrem página.
        album_id: String::new(),
        duration_ms: a["durationInMillis"].as_u64().unwrap_or(0) as u32,
        cover_small: art(item, 120),
        cover_large: art(item, 640),
        artists: Vec::new(),
    })
}

fn tracks(list: &Value) -> Vec<Track> {
    list.as_array().into_iter().flatten().filter_map(track).collect()
}

fn list_page(card: &Card, image: Option<String>, tracks: Vec<Track>) -> Page {
    Page {
        meta: crate::api::join_meta(&card.subtitle, tracks.len()),
        card: Card { image: image.or_else(|| card.image.clone()), ..card.clone() },
        tracks,
        albums: Vec::new(),
        note: None,
        artist: None,
        owner_image: None,
        more_from: None,
        total: 0,
    }
}

fn page_of(card: &Card, data: &Value) -> Page {
    let item = &data["data"][0];
    // shortcut: só as 100 primeiras faixas (o `include` da Apple para aí); paginar `tracks.next` se fizer falta.
    list_page(card, art(item, 600), tracks(&item["relationships"]["tracks"]["data"]))
}

fn artist_page(card: &Card, data: &Value) -> Page {
    let item = &data["data"][0];
    let views = &item["views"];
    let full = cards(&views["full-albums"]["data"]);
    let album_count = full.len();
    let albums: Vec<Card> = full.into_iter().chain(cards(&views["singles"]["data"])).collect();
    Page {
        meta: String::new(),
        card: Card { image: art(item, 300).or_else(|| card.image.clone()), title: item["attributes"]["name"].as_str().unwrap_or(&card.title).into(), ..card.clone() },
        tracks: tracks(&views["top-songs"]["data"]),
        albums,
        note: None,
        artist: Some(ArtistInfo { hero: art(item, 640), album_count, latest: None, related: cards(&views["similar-artists"]["data"]), featuring: Vec::new() }),
        owner_image: None,
        more_from: None,
        total: 0,
    }
}

async fn catalog_artist(http: &reqwest::Client, storefront: &str, card: &Card, id: &str) -> Result<Page, String> {
    let path = format!("/artists/{id}?views=top-songs,full-albums,singles,similar-artists&l=pt-BR");
    Ok(artist_page(card, &motion::catalog_in(http, storefront, &path).await?))
}

/// Página de uma playlist, álbum ou artista da Apple (do catálogo ou da biblioteca da pessoa).
pub async fn page(http: &reqwest::Client, storefront: &str, card: &Card) -> Result<Page, String> {
    let id = card.id.as_str();
    let mine = |path: String| async move { motion::amp_as(http, &path, Some(&user_token()?)).await };
    match card.kind {
        Kind::Playlist | Kind::Album if id.starts_with("p.") || id.starts_with("l.") => {
            let kind = if card.kind == Kind::Album { "albums" } else { "playlists" };
            let data = mine(format!("/v1/me/library/{kind}/{id}?include=tracks&l=pt-BR")).await?;
            Ok(page_of(card, &data))
        }
        // Artista da biblioteca: abre a página dele no catálogo.
        Kind::Artist if id.starts_with("r.") => {
            let data = mine(format!("/v1/me/library/artists/{id}?include=catalog")).await?;
            let catalog = data["data"][0]["relationships"]["catalog"]["data"][0]["id"].as_str().ok_or(t("Este artista não está no catálogo do Apple Music."))?.to_string();
            catalog_artist(http, storefront, card, &catalog).await
        }
        Kind::Artist => catalog_artist(http, storefront, card, id).await,
        _ => {
            let kind = if card.kind == Kind::Album { "albums" } else { "playlists" };
            let data = motion::catalog_in(http, storefront, &format!("/{kind}/{id}?include=tracks&l=pt-BR")).await?;
            Ok(page_of(card, &data))
        }
    }
}

/// Todas as páginas de uma lista da biblioteca (a Apple devolve até 100 por vez, com o caminho da próxima).
async fn library_list(http: &reqwest::Client, token: &str, mut path: String) -> Result<Vec<Card>, String> {
    let mut all = Vec::new();
    // shortcut: até 2.000 itens de cada tipo; continuar se alguém tiver mais.
    for _ in 0..20 {
        let data = motion::amp_as(http, &path, Some(token)).await?;
        all.extend(cards(&data["data"]));
        match data["next"].as_str() {
            Some(next) => path = format!("{next}{}l=pt-BR", if next.contains('?') { '&' } else { '?' }),
            None => break,
        }
    }
    Ok(all)
}

/// A biblioteca da pessoa no Apple Music: playlists, álbuns e artistas.
pub async fn library(http: &reqwest::Client) -> Result<Library, String> {
    let token = user_token()?;
    let (playlists, albums, artists) = tokio::try_join!(
        library_list(http, &token, "/v1/me/library/playlists?limit=100&l=pt-BR".into()),
        library_list(http, &token, "/v1/me/library/albums?limit=100&l=pt-BR".into()),
        library_list(http, &token, "/v1/me/library/artists?limit=100&include=catalog&l=pt-BR".into()),
    )?;
    Ok(Library { playlists, albums, artists, ..Default::default() })
}

/// Texto para a URL: letras e números como estão, o resto em `%XX`.
fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Busca no catálogo da Apple; sem podcasts (não estão nele).
pub async fn search(http: &reqwest::Client, storefront: &str, query: &str, kinds: &[SearchKind], offset: usize) -> Result<SearchResults, String> {
    let types: Vec<&str> = kinds.iter().filter_map(|k| match k {
        SearchKind::Tracks => Some("songs"),
        SearchKind::Artists => Some("artists"),
        SearchKind::Albums => Some("albums"),
        SearchKind::Playlists => Some("playlists"),
        SearchKind::Shows => None,
    }).collect();
    let mut found = SearchResults { query: query.into(), ..Default::default() };
    if types.is_empty() {
        return Ok(found);
    }
    let path = format!("/search?term={}&types={}&limit=10&offset={offset}&l=pt-BR", encode(query), types.join(","));
    let r = &motion::catalog_in(http, storefront, &path).await?["results"];
    found.tracks = tracks(&r["songs"]["data"]);
    found.artists = cards(&r["artists"]["data"]);
    found.albums = cards(&r["albums"]["data"]);
    found.playlists = cards(&r["playlists"]["data"]);
    Ok(found)
}

/// ID da Apple de uma faixa que veio do catálogo dela.
pub fn song_id(uri: &str) -> Option<&str> {
    uri.strip_prefix("apple:song:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_shelves_and_pages() {
        let art = json!({ "url": "https://x/{w}x{h}bb.{f}" });
        let groupings = json!({ "data": [{ "relationships": { "tabs": { "data": [{ "relationships": { "children": { "data": [
            { "attributes": { "name": "Top 100" }, "relationships": { "contents": { "data": [
                { "type": "playlists", "id": "pl.1", "attributes": { "name": "Top 100: mundo", "curatorName": "Apple Music", "artwork": art } },
                { "type": "stations", "id": "ra.1", "attributes": { "name": "Rádio" } },
            ] } } },
            { "attributes": { "name": "Ao vivo" }, "relationships": { "contents": { "data": [{ "type": "stations", "id": "ra.2", "attributes": { "name": "x" } }] } } },
            { "attributes": {}, "relationships": { "children": { "data": [] } } },
        ] } } }] } } }] });
        let s = shelves(&groupings);
        assert_eq!(s.len(), 1);
        let c = &s[0].cards[0];
        assert_eq!((s[0].title.as_str(), c.uri.as_str(), c.subtitle.as_str(), c.image.as_deref()), ("Top 100", "apple:playlist:pl.1", "Apple Music", Some("https://x/300x300bb.jpg")));

        let data = json!({ "data": [{ "attributes": { "artwork": art }, "relationships": { "tracks": { "data": [
            { "type": "songs", "id": "6822497774", "attributes": { "name": "Housewife", "artistName": "Tate McRae", "albumName": "Housewife - Single", "durationInMillis": 208810, "artwork": art } },
            { "type": "music-videos", "id": "9", "attributes": { "name": "clipe" } },
            { "type": "library-songs", "id": "i.1", "attributes": { "name": "Da biblioteca", "playParams": { "catalogId": "123" } } },
            { "type": "library-songs", "id": "i.2", "attributes": { "name": "Enviada", "playParams": {} } },
        ] } } }] });
        let p = page_of(c, &data);
        assert_eq!(p.tracks.iter().map(|t| t.uri.as_str()).collect::<Vec<_>>(), ["apple:song:6822497774", "apple:song:123"]);
        assert_eq!((p.tracks[0].duration_ms, song_id(&p.tracks[0].uri)), (208810, Some("6822497774")));
        assert_eq!(p.meta, "Apple Music · 2 faixas");
    }

    #[test]
    fn reads_library_artists_and_search_text() {
        let artist = json!({ "type": "library-artists", "id": "r.1", "attributes": { "name": "Tate McRae" },
            "relationships": { "catalog": { "data": [{ "attributes": { "artwork": { "url": "https://x/{w}x{h}.{f}" } } }] } } });
        let c = card(&artist).unwrap();
        assert_eq!((c.kind, c.uri.as_str(), c.image.as_deref()), (Kind::Artist, "apple:artist:r.1", Some("https://x/300x300.jpg")));
        assert_eq!(encode("são paulo & cia"), "s%C3%A3o%20paulo%20%26%20cia");
    }
}
