//! Leituras pela sessão do librespot, no protocolo dos clients oficiais (doc 01, contornos).
//! Não passam pela cota da Web API e chegam onde ela não chega desde 2026: playlists de outras pessoas
//! e as playlists do Spotify salvas na biblioteca (daylist, Daily Mix, mixes).
//! O mesmo caminho do spotifast e do psst; é interno e pode mudar sem aviso.
//! A tela inicial personalizada do Spotify (Pathfinder) responde 403 para este client (doc 07).
use librespot::core::Session;
use librespot::protocol::extended_metadata::{BatchedEntityRequest, EntityRequest, ExtensionQuery};
use librespot::protocol::extension_kind::ExtensionKind;
use librespot::protocol::playlist4_external::SelectedListContent;
use protobuf::{EnumOrUnknown, Message};

use crate::{Card, Kind, Track};

const IMAGE_HOST: &str = "https://i.scdn.co/image/";
const PAGE: usize = 100;
/// O Spotify limita playlists a 10 mil itens.
const MAX_TRACKS: usize = 10_000;
/// O DJ é um contexto especial que o librespot não toca.
const DJ_URI: &str = "spotify:playlist:37i9dQZF1EYkqdzj48dyYq";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

async fn list(session: &Session, endpoint: &str) -> Result<SelectedListContent, String> {
    let bytes = session.spclient().request(&http::Method::GET, endpoint, None, None).await.map_err(err)?;
    SelectedListContent::parse_from_bytes(&bytes).map_err(err)
}

/// URIs das playlists da conta, na ordem da biblioteca do Spotify (pastas achatadas), com quando entraram
/// nela (segundos Unix; 0 quando o Spotify não informa).
pub async fn rootlist(session: &Session) -> Result<Vec<(String, u64)>, String> {
    let mut uris = Vec::new();
    // Marcadores de pasta (start-group/end-group) também contam na paginação.
    let mut from = 0;
    loop {
        let bytes = session.spclient().get_rootlist(from, Some(500)).await.map_err(err)?;
        let content = SelectedListContent::parse_from_bytes(&bytes).map_err(err)?;
        let items = &content.contents.items;
        uris.extend(items.iter().filter(|i| i.uri().starts_with("spotify:playlist:") && i.uri() != DJ_URI)
            .map(|i| (i.uri().to_string(), (i.attributes.timestamp() / 1000).max(0) as u64)));
        from += items.len();
        if items.is_empty() || from >= content.length() as usize || uris.len() >= 1000 {
            break;
        }
    }
    Ok(uris)
}

/// Artistas seguidos e quando (segundos Unix), pela coleção da conta: a Web API não diz quando se seguiu.
/// O protobuf dela (collection2v2) não vem compilado no librespot; os quatro campos vão à mão.
pub async fn followed_artists(session: &Session) -> Result<Vec<(String, u64)>, String> {
    const PROTO: &str = "application/vnd.collection-v2.spotify.proto";
    let (mut out, mut token) = (Vec::new(), String::new());
    loop {
        // PageRequest: username = 1, set = 2, pagination_token = 3, limit = 4.
        let mut req = Vec::new();
        put_str(&mut req, 1, &session.username());
        put_str(&mut req, 2, "artist");
        if !token.is_empty() {
            put_str(&mut req, 3, &token);
        }
        req.push(4 << 3);
        put_varint(&mut req, 300);
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::CONTENT_TYPE, http::HeaderValue::from_static(PROTO));
        headers.insert(http::header::ACCEPT, http::HeaderValue::from_static(PROTO));
        let body = session.spclient().request(&http::Method::POST, "/collection/v2/paging", Some(headers), Some(&req)).await.map_err(err)?;
        let (items, next) = collection_page(&body).ok_or("resposta da coleção em formato inesperado")?;
        out.extend(items);
        if next.is_empty() || out.len() >= 5000 {
            return Ok(out);
        }
        token = next;
    }
}

fn put_str(buf: &mut Vec<u8>, field: u64, s: &str) {
    put_varint(buf, field << 3 | 2);
    put_varint(buf, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}

/// Campos de um protobuf: (número, valor numérico, bytes). Só varint e bytes; outro tipo devolve None.
fn proto_fields(mut b: &[u8]) -> Option<Vec<(u64, u64, &[u8])>> {
    fn varint(b: &mut &[u8]) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let (&c, rest) = b.split_first()?;
            *b = rest;
            v |= u64::from(c & 0x7f) << shift;
            if c < 0x80 {
                return Some(v);
            }
        }
        None
    }
    let mut out = Vec::new();
    while !b.is_empty() {
        let key = varint(&mut b)?;
        match key & 7 {
            0 => out.push((key >> 3, varint(&mut b)?, &[][..])),
            2 => {
                let len = usize::try_from(varint(&mut b)?).ok()?;
                let (v, rest) = (b.get(..len)?, b.get(len..)?);
                b = rest;
                out.push((key >> 3, 0, v));
            }
            _ => return None,
        }
    }
    Some(out)
}

/// PageResponse: items = 1 (uri = 1, added_at = 2, is_removed = 3), next_page_token = 2.
fn collection_page(body: &[u8]) -> Option<(Vec<(String, u64)>, String)> {
    let (mut items, mut next) = (Vec::new(), String::new());
    for (field, _, bytes) in proto_fields(body)? {
        match field {
            1 => {
                let (mut uri, mut at, mut removed) = (String::new(), 0, false);
                for (f, n, b) in proto_fields(bytes)? {
                    match f {
                        1 => uri = String::from_utf8_lossy(b).into_owned(),
                        2 => at = n,
                        3 => removed = n != 0,
                        _ => {}
                    }
                }
                if !removed && !uri.is_empty() {
                    items.push((uri, at));
                }
            }
            2 => next = String::from_utf8_lossy(bytes).into_owned(),
            _ => {}
        }
    }
    Some((items, next))
}

fn playlist_card(id: &str, content: &SelectedListContent) -> Card {
    let attrs = &content.attributes;
    // A capa vem como URL pronta (playlists do Spotify) ou como id de arquivo (capa enviada ou mosaico).
    let image = attrs
        .picture_size
        .iter()
        .find(|p| p.target_name() == "default")
        .or(attrs.picture_size.first())
        .map(|p| p.url().to_string())
        .filter(|u| !u.is_empty())
        .or_else(|| (!attrs.picture().is_empty()).then(|| format!("{IMAGE_HOST}{}", hex(attrs.picture()))));
    let owner = content.owner_username();
    Card {
        kind: Kind::Playlist,
        id: id.to_string(),
        uri: format!("spotify:playlist:{id}"),
        title: attrs.name().to_string(),
        subtitle: if owner == "spotify" { "Spotify".into() } else { owner.to_string() },
        image,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Só o cabeçalho (nome, dono, capa), sem as faixas.
pub async fn playlist_header(session: &Session, id: &str) -> Result<Card, String> {
    let content = list(session, &format!("/playlist/v2/playlist/{id}?from=0&length=0")).await?;
    Ok(playlist_card(id, &content))
}

/// Cabeçalho com a descrição no lugar do dono, como nos cartões da Home do Spotify. A descrição vem com
/// links em HTML (`<a href=…>Artista</a>`): fica só o texto.
pub async fn playlist_teaser(session: &Session, id: &str) -> Result<Card, String> {
    let content = list(session, &format!("/playlist/v2/playlist/{id}?from=0&length=0")).await?;
    let mut card = playlist_card(id, &content);
    let mut text = String::new();
    let mut in_tag = false;
    for c in content.attributes.description().chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text.replace("&amp;", "&").replace("&#x27;", "'").replace("&quot;", "\"");
    if !text.trim().is_empty() {
        card.subtitle = text.trim().to_string();
    }
    Ok(card)
}

/// Um pedaço de uma playlist de qualquer dono (inclusive do Spotify).
pub struct PlaylistChunk {
    pub card: Card,
    pub tracks: Vec<Track>,
    /// Nome de usuário do dono.
    pub owner: String,
    /// Itens na playlist inteira.
    pub total: usize,
    /// De onde continuar; None quando acabou.
    pub next: Option<usize>,
}

/// Cabeçalho e até `max` itens a partir de `from`. Playlists grandes vêm aos poucos, conforme a lista rola.
pub async fn playlist(session: &Session, id: &str, from: usize, max: usize) -> Result<PlaylistChunk, String> {
    let mut uris = Vec::new();
    let mut header = None;
    let mut owner = String::new();
    let mut total;
    loop {
        let length = PAGE.min(max - uris.len());
        let content = list(session, &format!("/playlist/v2/playlist/{id}?from={}&length={length}", from + uris.len())).await?;
        total = (content.length() as usize).min(MAX_TRACKS);
        let count = content.contents.items.len();
        uris.extend(content.contents.items.iter().map(|i| i.uri().to_string()));
        if header.is_none() {
            owner = content.owner_username().to_string();
            header = Some(playlist_card(id, &content));
        }
        if count == 0 || from + uris.len() >= total || uris.len() >= max {
            break;
        }
    }
    let read = uris.len();
    // Só faixas: episódios de podcast e arquivos locais ficam de fora por enquanto.
    let uris: Vec<String> = uris.into_iter().filter(|u| u.starts_with("spotify:track:")).collect();
    let mut tracks = Vec::with_capacity(uris.len());
    for chunk in uris.chunks(PAGE) {
        tracks.extend(track_metadata(session, chunk).await?);
    }
    let card = header.unwrap_or_else(|| Card { kind: Kind::Playlist, id: id.into(), uri: format!("spotify:playlist:{id}"), title: String::new(), subtitle: String::new(), image: None });
    let next = (read > 0 && from + read < total).then_some(from + read);
    Ok(PlaylistChunk { card, tracks, owner, total, next })
}

/// Nome de exibição e foto de um usuário. A Web API tirou `/users/{id}` em 2026 (doc 01).
pub async fn user_profile(session: &Session, username: &str) -> Result<(String, Option<String>), String> {
    let path = format!("/user-profile-view/v3/profile/{username}?playlist_limit=0&artist_limit=0&episode_limit=0");
    let bytes = session.spclient().request(&http::Method::GET, &path, None, None).await.map_err(err)?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(err)?;
    let name = v["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(username).to_string();
    Ok((name, v["image_url"].as_str().filter(|u| !u.is_empty()).map(String::from)))
}

/// Até 10 faixas mais tocadas do artista, no país da conta, e os artistas parecidos. A Web API tirou os
/// dois endpoints em 2026 (doc 01).
pub async fn artist_extras(session: &Session, id: &str) -> Result<(Vec<Track>, Vec<Card>), String> {
    use librespot::metadata::Metadata;
    let uri = librespot::core::SpotifyUri::from_uri(&format!("spotify:artist:{id}")).map_err(err)?;
    let artist = librespot::metadata::Artist::get(session, &uri).await.map_err(err)?;
    let uris: Vec<String> = artist.top_tracks.for_country(&session.country()).iter().take(10).filter_map(|u| u.to_uri().ok()).collect();
    let top = track_metadata(session, &uris).await?;
    let related_uris: Vec<String> = artist.related.iter().take(RELATED_MAX).filter_map(|a| a.id.to_uri().ok()).collect();
    // Os parecidos vêm sem foto: um pedido em lote traz o artista completo de cada um.
    let related = match related_cards(session, &related_uris).await {
        Ok(cards) => cards,
        Err(e) => {
            log::warn!("artistas parecidos com {id}: {e}");
            Vec::new()
        }
    };
    Ok((top, related))
}

const RELATED_MAX: usize = 12;
/// Smart shuffle: artistas parecidos usados e faixas de cada um.
const SMART_ARTISTS: usize = 8;
const SMART_PER_ARTIST: usize = 2;

/// Recomendações para o smart shuffle: as mais tocadas de artistas parecidos com os da fila, sem as que
/// já estão nela. Tudo pela sessão, sem a cota da Web API.
pub async fn recommendations(session: &Session, seeds: &[String], exclude: &std::collections::HashSet<String>) -> Result<Vec<Track>, String> {
    use librespot::metadata::Metadata;
    let mut related: Vec<String> = Vec::new();
    for id in seeds {
        let Ok(uri) = librespot::core::SpotifyUri::from_uri(&format!("spotify:artist:{id}")) else { continue };
        let artist = librespot::metadata::Artist::get(session, &uri).await.map_err(err)?;
        for a in artist.related.iter().take(4) {
            if let Ok(id) = a.id.to_id() {
                if !seeds.contains(&id) && !related.contains(&id) {
                    related.push(id);
                }
            }
        }
    }
    let mut out = Vec::new();
    for id in related.iter().take(SMART_ARTISTS) {
        let uri = librespot::core::SpotifyUri::from_uri(&format!("spotify:artist:{id}")).map_err(err)?;
        let artist = librespot::metadata::Artist::get(session, &uri).await.map_err(err)?;
        let uris: Vec<String> = artist.top_tracks.for_country(&session.country()).iter().filter_map(|u| u.to_uri().ok()).filter(|u| !exclude.contains(u)).take(SMART_PER_ARTIST).collect();
        out.extend(track_metadata(session, &uris).await?);
    }
    Ok(out)
}

/// Fila com uma recomendação a cada `every` faixas, depois da atual.
pub fn interleave(queue: &[Track], index: usize, recs: Vec<Track>, every: usize) -> Vec<Track> {
    let mut out: Vec<Track> = queue.iter().take(index + 1).cloned().collect();
    let mut recs = recs.into_iter();
    for (n, t) in queue.iter().skip(index + 1).enumerate() {
        out.push(t.clone());
        if (n + 1) % every == 0 {
            out.extend(recs.next());
        }
    }
    // Fila curta: o resto das recomendações vai no fim.
    out.extend(recs);
    out
}

async fn related_cards(session: &Session, uris: &[String]) -> Result<Vec<Card>, String> {
    let mut by_uri = batch(session, uris, ExtensionKind::ARTIST_V4).await?;
    Ok(uris.iter().filter_map(|uri| {
        let msg = librespot::protocol::metadata::Artist::parse_from_bytes(&by_uri.remove(uri)?).ok()?;
        let a = librespot::metadata::Artist::try_from(&msg).ok()?;
        let mut photos: Vec<_> = a.portraits.iter().chain(a.portrait_group.iter()).collect();
        photos.sort_by_key(|p| p.width);
        // Média (~300 px), como os outros cartões.
        let image = photos.iter().find(|p| p.width >= 280).or(photos.last()).and_then(|p| p.id.to_base16().ok()).map(|h| format!("{IMAGE_HOST}{h}"));
        let id = uri.trim_start_matches("spotify:artist:").to_string();
        Some(Card { kind: Kind::Artist, id, uri: uri.clone(), title: a.name, subtitle: "Artista".into(), image })
    }).collect())
}

/// Metadados de várias faixas num pedido só, na ordem pedida; faixas que o Spotify não tem mais somem.
pub(crate) async fn track_metadata(session: &Session, uris: &[String]) -> Result<Vec<Track>, String> {
    let mut request = BatchedEntityRequest::new();
    for uri in uris {
        request.entity_request.push(EntityRequest {
            entity_uri: uri.clone(),
            query: vec![ExtensionQuery { extension_kind: EnumOrUnknown::new(ExtensionKind::TRACK_V4), ..Default::default() }],
            ..Default::default()
        });
    }
    let response = session.spclient().get_extended_metadata(request).await.map_err(err)?;
    let mut by_uri = std::collections::HashMap::new();
    for array in response.extended_metadata {
        for data in array.extension_data {
            if !matches!(data.header.status_code, 0 | 200) {
                continue;
            }
            let Some(any) = data.extension_data.as_ref() else { continue };
            let Ok(msg) = librespot::protocol::metadata::Track::parse_from_bytes(&any.value) else { continue };
            let Ok(t) = librespot::metadata::Track::try_from(&msg) else { continue };
            if let Some(track) = convert(t) {
                by_uri.insert(data.entity_uri, track);
            }
        }
    }
    Ok(uris.iter().filter_map(|u| by_uri.remove(u)).collect())
}

#[cfg(test)]
mod collection_tests {
    use super::*;

    #[test]
    fn reads_a_collection_page() {
        // Item que fica, item removido, item com campo desconhecido, e o token da próxima página.
        let item = |uri: &str, at: u64, removed: bool| {
            let mut b = Vec::new();
            put_str(&mut b, 1, uri);
            b.push(2 << 3);
            put_varint(&mut b, at);
            if removed {
                b.extend([3 << 3, 1]);
            }
            put_str(&mut b, 9, "ctx");
            b
        };
        let mut body = Vec::new();
        for i in [item("spotify:artist:a", 1_700_000_000, false), item("spotify:artist:b", 5, true)] {
            put_varint(&mut body, 1 << 3 | 2);
            put_varint(&mut body, i.len() as u64);
            body.extend(i);
        }
        put_str(&mut body, 2, "next");
        assert_eq!(collection_page(&body), Some((vec![("spotify:artist:a".into(), 1_700_000_000)], "next".into())));
        // Tipo de campo que não sabemos ler (fixo de 64 bits) não vira lixo.
        assert_eq!(collection_page(&[1 << 3 | 1, 0, 0, 0, 0, 0, 0, 0, 0]), None);
        assert_eq!(collection_page(&[]), Some((Vec::new(), String::new())));
    }
}

#[cfg(test)]
mod tests {
    fn t(n: &str) -> crate::Track {
        crate::Track { id: n.into(), uri: n.into(), name: n.into(), artist: String::new(), album: String::new(), album_id: String::new(), duration_ms: 1, cover_small: None, cover_large: None, artists: Vec::new() }
    }

    #[test]
    fn interleaves_after_current() {
        let queue = ["a", "b", "c", "d", "e", "f", "g"].map(t);
        let out = super::interleave(&queue, 1, vec![t("x"), t("y"), t("z")], 3);
        let names: Vec<_> = out.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "d", "e", "x", "f", "g", "y", "z"]);
    }
}

fn convert(t: librespot::metadata::Track) -> Option<Track> {
    let uri = t.id.to_uri().ok()?;
    let mut covers: Vec<_> = t.album.covers.iter().collect();
    covers.sort_by_key(|c| c.width);
    let url = |c: &librespot::metadata::image::Image| c.id.to_base16().ok().map(|h| format!("{IMAGE_HOST}{h}"));
    Some(Track {
        id: t.id.to_id().ok()?,
        uri,
        name: t.name,
        artist: t.artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "),
        album: t.album.name.clone(),
        album_id: t.album.id.to_id().unwrap_or_default(),
        artists: t.artists.iter().filter_map(|a| Some((a.id.to_id().ok()?, a.name.clone()))).collect(),
        duration_ms: t.duration.max(0) as u32,
        cover_small: covers.first().and_then(|c| url(c)),
        cover_large: covers.last().and_then(|c| url(c)),
    })
}

/// Metadados de várias entidades do mesmo tipo num pedido só: URI → bytes do protobuf.
async fn batch(session: &Session, uris: &[String], kind: ExtensionKind) -> Result<std::collections::HashMap<String, Vec<u8>>, String> {
    let mut request = BatchedEntityRequest::new();
    for uri in uris {
        request.entity_request.push(EntityRequest {
            entity_uri: uri.clone(),
            query: vec![ExtensionQuery { extension_kind: EnumOrUnknown::new(kind), ..Default::default() }],
            ..Default::default()
        });
    }
    let response = session.spclient().get_extended_metadata(request).await.map_err(err)?;
    let mut out = std::collections::HashMap::new();
    for array in response.extended_metadata {
        for data in array.extension_data {
            if matches!(data.header.status_code, 0 | 200) {
                if let Some(any) = data.extension_data.into_option() {
                    out.insert(data.entity_uri, any.value);
                }
            }
        }
    }
    Ok(out)
}

/// Lançamentos dos últimos meses que contam como novidade.
const NEW_FOR_DAYS: i64 = 120;
/// Os mais recentes de cada artista que entram na conta (o Spotify os lista do mais novo para o mais velho).
const NEW_PER_GROUP: usize = 2;
const NEW_MAX: usize = 80;

/// Álbuns e singles recentes dos artistas, do mais novo para o mais velho. Uns poucos pedidos em lote,
/// pela sessão (não gastam a cota da Web API).
pub async fn new_releases(session: &Session, artist_ids: &[String]) -> Result<Vec<crate::NewRelease>, String> {
    use librespot::metadata::{Album, Artist};
    let mut candidates = Vec::new();
    for chunk in artist_ids.chunks(PAGE) {
        let uris: Vec<String> = chunk.iter().map(|id| format!("spotify:artist:{id}")).collect();
        for bytes in batch(session, &uris, ExtensionKind::ARTIST_V4).await?.into_values() {
            let Ok(msg) = librespot::protocol::metadata::Artist::parse_from_bytes(&bytes) else { continue };
            let Ok(artist) = Artist::try_from(&msg) else { continue };
            candidates.extend(artist.albums.current_releases().take(NEW_PER_GROUP).chain(artist.singles.current_releases().take(NEW_PER_GROUP)).filter_map(|u| u.to_uri().ok()));
        }
    }
    candidates.sort();
    candidates.dedup();
    let since = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64) - NEW_FOR_DAYS * 86_400;
    let mut out = Vec::new();
    for chunk in candidates.chunks(PAGE) {
        for (uri, bytes) in batch(session, chunk, ExtensionKind::ALBUM_V4).await? {
            let Ok(msg) = librespot::protocol::metadata::Album::parse_from_bytes(&bytes) else { continue };
            let Ok(album) = Album::try_from(&msg) else { continue };
            let date = album.date.as_timestamp_ms() / 1000;
            if date < since {
                continue;
            }
            let mut covers: Vec<_> = album.covers.iter().collect();
            covers.sort_by_key(|c| c.width);
            // Média (~300 px), como os outros cartões.
            let image = covers.get(covers.len().saturating_sub(2)).and_then(|c| c.id.to_base16().ok()).map(|h| format!("{IMAGE_HOST}{h}"));
            let single = matches!(album.album_type, librespot::metadata::album::AlbumType::SINGLE | librespot::metadata::album::AlbumType::EP);
            let id = uri.trim_start_matches("spotify:album:").to_string();
            let card = Card { kind: Kind::Album, id, uri, title: album.name, subtitle: album.artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "), image };
            out.push(crate::NewRelease { card, date, single });
        }
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.date));
    out.truncate(NEW_MAX);
    Ok(out)
}

const MONTHS: [&str; 12] = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];

/// Quem fez a faixa (pelos papéis que o Spotify guarda), a gravadora e os direitos do álbum.
pub async fn credits(session: &Session, track_uri: &str) -> Result<crate::Credits, String> {
    use librespot::metadata::{Album, Metadata, Track as MetaTrack, artist::ArtistRole, copyright::CopyrightType};
    let uri = librespot::core::SpotifyUri::from_uri(track_uri).map_err(err)?;
    let track = MetaTrack::get(session, &uri).await.map_err(err)?;
    let album = Album::get(session, &track.album.id).await.map_err(err)?;
    let mut roles: Vec<(String, Vec<String>)> = Vec::new();
    for a in track.artists_with_role.iter() {
        let role = match a.role {
            ArtistRole::ARTIST_ROLE_FEATURED_ARTIST => "Participação",
            ArtistRole::ARTIST_ROLE_REMIXER => "Remix",
            ArtistRole::ARTIST_ROLE_ACTOR => "Atuação",
            ArtistRole::ARTIST_ROLE_COMPOSER => "Composição",
            ArtistRole::ARTIST_ROLE_CONDUCTOR => "Regência",
            ArtistRole::ARTIST_ROLE_ORCHESTRA => "Orquestra",
            _ => "Artista",
        };
        match roles.iter_mut().find(|(r, _)| r == role) {
            Some((_, names)) => names.push(a.name.clone()),
            None => roles.push((role.into(), vec![a.name.clone()])),
        }
    }
    if roles.is_empty() {
        roles.push(("Artista".into(), track.artists.iter().map(|a| a.name.clone()).collect()));
    }
    let d = album.date.as_utc();
    let released = format!("{} de {} de {}", d.day(), MONTHS[u8::from(d.month()) as usize - 1], d.year());
    let copyrights = album.copyrights.iter().map(|c| match c.copyright_type {
        CopyrightType::P => format!("℗ {}", c.text.trim_start_matches(['℗', ' '])),
        _ => format!("© {}", c.text.trim_start_matches(['©', ' '])),
    }).collect();
    Ok(crate::Credits { title: track.name, roles, label: album.label, copyrights, released })
}

/// Playlist de rádio a partir de uma faixa (a mesma do "Ir para o rádio da música" do Spotify).
pub async fn radio_for(session: &Session, track_uri: &str) -> Result<String, String> {
    let uri = librespot::core::SpotifyUri::from_uri(track_uri).map_err(err)?;
    let bytes = session.spclient().get_radio_for_track(&uri).await.map_err(err)?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(err)?;
    v["mediaItems"][0]["uri"].as_str().filter(|u| u.starts_with("spotify:playlist:")).map(String::from).ok_or_else(|| "o Spotify não tem rádio para esta música".into())
}

/// Endereço do Canvas (o vídeo curto em loop) da faixa, se ela tiver um. Pela sessão, como o app oficial.
pub async fn canvas(session: &Session, track_uri: &str) -> Result<Option<String>, String> {
    use librespot::protocol::canvaz::{Type, entity_canvaz_response::Canvaz};
    // O .proto do librespot não traz o pedido: EntityCanvazRequest { repeated Entity entities = 1 },
    // Entity { string entity_uri = 1 }. Montado à mão.
    let uri = track_uri.as_bytes();
    let mut entity = vec![0x0a];
    put_varint(&mut entity, uri.len() as u64);
    entity.extend(uri);
    let mut body = vec![0x0a];
    put_varint(&mut body, entity.len() as u64);
    body.extend(entity);
    let mut headers = http::HeaderMap::new();
    headers.insert(http::header::CONTENT_TYPE, http::HeaderValue::from_static("application/x-protobuf"));
    let bytes = session.spclient().request(&http::Method::POST, "/canvaz-cache/v0/canvases", Some(headers), Some(&body)).await.map_err(err)?;
    // A resposta tem `repeated Canvaz canvases = 1`, que o .proto também não traz: lê o campo 1 à mão.
    let mut input = protobuf::CodedInputStream::from_bytes(&bytes);
    while let Some(tag) = input.read_raw_tag_or_eof().map_err(err)? {
        if tag >> 3 == 1 && tag & 7 == 2 {
            let item = input.read_bytes().map_err(err)?;
            let c = Canvaz::parse_from_bytes(&item).map_err(err)?;
            let video = matches!(c.type_.enum_value(), Ok(Type::VIDEO | Type::VIDEO_LOOPING | Type::VIDEO_LOOPING_RANDOM));
            if video && c.url.ends_with(".mp4") {
                return Ok(Some(c.url));
            }
        } else {
            input.skip_field(protobuf::rt::WireType::new(tag & 7).ok_or("resposta inesperada do Canvas")?).map_err(err)?;
        }
    }
    Ok(None)
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}
