//! Capa animada do Apple Music, para quando o Spotify não devolve o Canvas da faixa.
//! O catálogo do Apple Music aceita o token público que o music.apple.com embute no próprio JS; o álbum
//! traz `editorialVideo` (um HLS) quando tem capa animada. O HLS tem variantes H.264 num único MP4
//! fragmentado, que é o que o leitor do Canvas abre.
use std::time::Duration;

use librespot::core::Session;
use serde_json::Value;

use crate::lyrics::BROWSER_UA;

const SITE: &str = "https://music.apple.com";
const CATALOG: &str = "https://amp-api.music.apple.com/v1/catalog/us";
const TOKEN_KEY: &str = "apple-music-token";
/// O token dura meses; trocado antes disso por garantia, e na hora se o catálogo recusar.
const TOKEN_TTL: Duration = Duration::from_secs(24 * 3600);
/// O fundo é borrado e reduzido à metade: mais que isso só pesa no download.
const MAX_SIDE: u32 = 768;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

async fn text(http: &reqwest::Client, url: &str) -> Result<String, String> {
    http.get(url).header("user-agent", BROWSER_UA).send().await.map_err(err)?.error_for_status().map_err(err)?.text().await.map_err(err)
}

/// Primeiro JWT do bundle principal do site (o `developerToken` do MusicKit).
async fn scrape_token(http: &reqwest::Client) -> Result<String, String> {
    let html = text(http, &format!("{SITE}/us/browse")).await?;
    let at = html.find("/assets/index").ok_or("bundle do Apple Music não encontrado")?;
    let end = html[at..].find(".js").ok_or("bundle do Apple Music não encontrado")? + at + 3;
    let js = text(http, &format!("{SITE}{}", &html[at..end])).await?;
    jwt(&js).map(String::from).ok_or_else(|| "token do Apple Music não encontrado".into())
}

fn jwt(js: &str) -> Option<&str> {
    let at = js.find("eyJ0eXAiOiJKV1Qi")?;
    let len = js[at..].find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))).unwrap_or(js.len() - at);
    let tok = &js[at..at + len];
    (tok.matches('.').count() == 2).then_some(tok)
}

async fn token(http: &reqwest::Client, refresh: bool) -> Result<String, String> {
    if !refresh && crate::cache::fresh(TOKEN_KEY, TOKEN_TTL) {
        if let Some(t) = crate::cache::load::<String>(TOKEN_KEY) {
            return Ok(t);
        }
    }
    let t = scrape_token(http).await?;
    crate::cache::save(TOKEN_KEY, &t);
    Ok(t)
}

async fn catalog(http: &reqwest::Client, path: &str) -> Result<Value, String> {
    let mut refreshed = false;
    loop {
        let tok = token(http, refreshed).await?;
        let r = http.get(format!("{CATALOG}{path}")).header("user-agent", BROWSER_UA).header("origin", SITE).bearer_auth(tok).send().await.map_err(err)?;
        if r.status().as_u16() == 401 && !refreshed {
            refreshed = true;
            continue;
        }
        return r.error_for_status().map_err(err)?.json().await.map_err(err);
    }
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// HLS da capa animada do álbum da faixa: entre as edições do ISRC, a de mesmo nome que o álbum do Spotify
/// (uma edição com outro nome teria outra capa).
fn pick_album(songs: &Value, album: &str) -> Option<String> {
    let want = norm(album);
    songs["data"].as_array()?.iter().flat_map(|s| s["relationships"]["albums"]["data"].as_array().into_iter().flatten()).find_map(|a| {
        let attrs = &a["attributes"];
        let name = norm(attrs["name"].as_str()?);
        let v = &attrs["editorialVideo"];
        let hls = v["motionDetailSquare"]["video"].as_str().or_else(|| v["motionSquareVideo1x1"]["video"].as_str())?;
        (!want.is_empty() && (name == want || name.starts_with(&want) || want.starts_with(&name))).then(|| hls.to_string())
    })
}

fn join(base: &str, rel: &str) -> String {
    if rel.starts_with("http") { rel.into() } else { format!("{}/{rel}", &base[..base.rfind('/').unwrap_or(0)]) }
}

/// Maior variante H.264 até `MAX_SIDE` (o openh264 não lê as HEVC).
fn pick_variant(master: &str, base: &str) -> Option<String> {
    let mut lines = master.lines();
    let mut best: Option<(u32, String)> = None;
    while let Some(l) = lines.next() {
        let Some(info) = l.strip_prefix("#EXT-X-STREAM-INF:") else { continue };
        let uri = lines.next()?;
        let side = info.split("RESOLUTION=").nth(1).and_then(|r| r.split('x').next()).and_then(|w| w.parse::<u32>().ok()).unwrap_or(0);
        if info.contains("avc1") && side <= MAX_SIDE && best.as_ref().is_none_or(|(s, _)| side > *s) {
            best = Some((side, join(base, uri)));
        }
    }
    best.map(|(_, u)| u)
}

/// O arquivo de vídeo da playlist da variante: todos os segmentos são faixas de bytes do mesmo MP4.
fn media_file(playlist: &str, base: &str) -> Option<String> {
    let map = playlist.lines().find_map(|l| l.strip_prefix("#EXT-X-MAP:"))?;
    let uri = map.split("URI=\"").nth(1)?.split('"').next()?;
    // shortcut: assume um MP4 só (é como o Apple Music publica hoje); segmentos em arquivos separados ficariam sem vídeo.
    playlist.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).all(|l| l.trim() == uri).then(|| join(base, uri))
}

/// Endereço do MP4 da capa animada da faixa no Apple Music, se o álbum tiver uma.
pub async fn find(http: &reqwest::Client, session: &Session, track_uri: &str) -> Result<Option<String>, String> {
    use librespot::metadata::{Metadata, Track};
    if !crate::internal::connected(session) {
        return Err(crate::internal::NOT_CONNECTED.into());
    }
    let uri = librespot::core::SpotifyUri::from_uri(track_uri).map_err(err)?;
    let meta = Track::get(session, &uri).await.map_err(err)?;
    let Some(isrc) = meta.external_ids.iter().find(|e| e.external_type.eq_ignore_ascii_case("isrc")).map(|e| e.id.clone()) else { return Ok(None) };
    let album = &meta.album.name;
    let songs = catalog(http, &format!("/songs?filter%5Bisrc%5D={isrc}&include=albums&extend=editorialVideo")).await?;
    let Some(hls) = pick_album(&songs, album) else { return Ok(None) };
    let Some(variant) = pick_variant(&text(http, &hls).await?, &hls) else { return Ok(None) };
    Ok(media_file(&text(http, &variant).await?, &variant))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_biggest_h264_that_fits() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:CODECS=\"avc1.64001f\",RESOLUTION=486x486\na_486.m3u8\n#EXT-X-STREAM-INF:CODECS=\"hvc1.2\",RESOLUTION=768x768\nh_768.m3u8\n#EXT-X-STREAM-INF:CODECS=\"avc1.64001f\",RESOLUTION=768x768\na_768.m3u8\n#EXT-X-STREAM-INF:CODECS=\"avc1.640020\",RESOLUTION=1080x1080\na_1080.m3u8\n";
        assert_eq!(pick_variant(master, "https://x/y/master.m3u8").as_deref(), Some("https://x/y/a_768.m3u8"));
        let media = "#EXT-X-MAP:URI=\"v.mp4\",BYTERANGE=\"896@0\"\n#EXTINF:2.8,\n#EXT-X-BYTERANGE:10@896\nv.mp4\n#EXTINF:2.8,\nv.mp4\n";
        assert_eq!(media_file(media, "https://x/y/a.m3u8").as_deref(), Some("https://x/y/v.mp4"));
        assert_eq!(jwt("x=\"eyJ0eXAiOiJKV1Qi.abc.d-_e\",y"), Some("eyJ0eXAiOiJKV1Qi.abc.d-_e"));
    }

    #[test]
    fn only_the_same_album_counts() {
        let songs = serde_json::json!({"data": [
            {"relationships": {"albums": {"data": [{"attributes": {"name": "Hits 2023", "editorialVideo": {"motionDetailSquare": {"video": "other"}}}}]}}},
            {"relationships": {"albums": {"data": [{"attributes": {"name": "Midnights (3am Edition)", "editorialVideo": {"motionDetailSquare": {"video": "mine"}}}}]}}},
        ]});
        assert_eq!(pick_album(&songs, "Midnights").as_deref(), Some("mine"));
        assert_eq!(pick_album(&songs, "Lover"), None);
    }
}
