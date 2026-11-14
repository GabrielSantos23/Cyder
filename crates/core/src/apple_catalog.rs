//! Catálogo do Apple Music (doc 13, fase 3): as prateleiras da tela inicial dele e as playlists e álbuns que elas
//! abrem. Vira `Shelf`, `Card`, `Page` e `Track` como os do Spotify, com URIs `apple:…`; as faixas carregam o ID da
//! Apple, e o player do Apple Music toca direto, sem procurar pelo ISRC.
use serde_json::Value;

use crate::{Card, Kind, Page, Shelf, Track, motion};

/// Prefixo das URIs do catálogo da Apple (`apple:playlist:pl.…`, `apple:album:…`, `apple:song:…`).
pub const URI: &str = "apple:";

/// Imagem da Apple no tamanho pedido: a URL vem com `{w}`, `{h}` e `{f}` para preencher.
fn art(item: &Value, size: u32) -> Option<String> {
    let url = item["attributes"]["artwork"]["url"].as_str()?;
    Some(url.replace("{w}", &size.to_string()).replace("{h}", &size.to_string()).replace("{f}", "jpg"))
}

/// Playlist ou álbum de uma prateleira; o resto (músicas soltas, rádios, vídeos) fica de fora.
fn card(item: &Value) -> Option<Card> {
    let (kind, path, by) = match item["type"].as_str()? {
        "playlists" => (Kind::Playlist, "playlist", "curatorName"),
        "albums" => (Kind::Album, "album", "artistName"),
        _ => return None,
    };
    let id = item["id"].as_str()?;
    let a = &item["attributes"];
    Some(Card {
        kind,
        id: id.into(),
        uri: format!("{URI}{path}:{id}"),
        title: a["name"].as_str()?.into(),
        subtitle: a[by].as_str().unwrap_or_default().into(),
        image: art(item, 300),
    })
}

fn shelves(groupings: &Value) -> Vec<Shelf> {
    let tabs = &groupings["data"][0]["relationships"]["tabs"]["data"];
    let elements = tabs[0]["relationships"]["children"]["data"].as_array().into_iter().flatten();
    elements
        .filter_map(|e| {
            let title = e["attributes"]["name"].as_str()?;
            let cards: Vec<Card> = e["relationships"]["contents"]["data"].as_array()?.iter().filter_map(card).collect();
            // A marca no título separa estas das prateleiras do Spotify ("Somente no Apple Music" já tem).
            let title = if title.contains("Apple Music") { title.to_string() } else { format!("{title} · Apple Music") };
            (!cards.is_empty()).then(|| Shelf { title, cards })
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
    if item["type"].as_str()? != "songs" {
        return None;
    }
    let id = item["id"].as_str()?;
    let a = &item["attributes"];
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

fn page_of(card: &Card, data: &Value) -> Page {
    let item = &data["data"][0];
    let tracks: Vec<Track> = item["relationships"]["tracks"]["data"].as_array().into_iter().flatten().filter_map(track).collect();
    let page_card = Card { image: art(item, 600).or_else(|| card.image.clone()), ..card.clone() };
    Page {
        meta: crate::api::join_meta(&card.subtitle, tracks.len()),
        card: page_card,
        tracks,
        albums: Vec::new(),
        note: None,
        artist: None,
        owner_image: None,
        // shortcut: só as 100 primeiras faixas (o `include` da Apple para aí); paginar `tracks.next` se fizer falta.
        more_from: None,
        total: 0,
    }
}

/// Página de uma playlist ou álbum do catálogo da Apple, com as faixas.
pub async fn page(http: &reqwest::Client, storefront: &str, card: &Card) -> Result<Page, String> {
    let kind = if card.kind == Kind::Album { "albums" } else { "playlists" };
    let data = motion::catalog_in(http, storefront, &format!("/{kind}/{}?include=tracks&l=pt-BR", card.id)).await?;
    Ok(page_of(card, &data))
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
        assert_eq!((s[0].title.as_str(), c.uri.as_str(), c.subtitle.as_str(), c.image.as_deref()), ("Top 100 · Apple Music", "apple:playlist:pl.1", "Apple Music", Some("https://x/300x300bb.jpg")));

        let data = json!({ "data": [{ "attributes": { "artwork": art }, "relationships": { "tracks": { "data": [
            { "type": "songs", "id": "6822497774", "attributes": { "name": "Housewife", "artistName": "Tate McRae", "albumName": "Housewife - Single", "durationInMillis": 208810, "artwork": art } },
            { "type": "music-videos", "id": "9", "attributes": { "name": "clipe" } },
        ] } } }] });
        let p = page_of(c, &data);
        assert_eq!(p.tracks.len(), 1);
        let t = &p.tracks[0];
        assert_eq!((t.uri.as_str(), t.duration_ms, song_id(&t.uri)), ("apple:song:6822497774", 208810, Some("6822497774")));
        assert_eq!(p.meta, "Apple Music · 1 faixa");
    }
}
