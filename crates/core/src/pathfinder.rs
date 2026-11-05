//! A tela inicial personalizada do Spotify ("Feito para você", Daily Mix, Discover Weekly, rádios…), pelo
//! Pathfinder: o GraphQL que o web player usa (`api-partner.spotify.com`). Nada disso existe na Web API.
//!
//! As consultas são "persisted queries": vai o nome e o sha256 da consulta, que muda a cada versão do web player.
//! Por isso os hashes são lidos do bundle do próprio web player (guardados 12 h) e relidos quando o Pathfinder diz
//! que não conhece o hash. Mesmo caminho do Musique. É interno e pode mudar sem aviso: falhando, a home volta a
//! ser a montada com o que a Web API permite.
use std::collections::HashMap;
use std::time::Duration;

use librespot::core::Session;
use serde_json::{Value, json};

use crate::{Card, Kind, Shelf};

const ENDPOINT: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
const WEB_PLAYER: &str = "https://open.spotify.com/";
const BUNDLE_PREFIX: &str = "https://open.spotifycdn.com/cdn/build/web-player/";
/// O que o web player manda; o Pathfinder escolhe o formato da resposta por ele.
const APP_VERSION: &str = "896000000";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0 Safari/537.36";
const HASH_TTL: Duration = Duration::from_secs(12 * 3600);
/// Último hash conhecido da `home` (web player de set/2026), para quando o bundle não puder ser lido.
const HOME_FALLBACK: &str = "76243c78b0e20ecdbe41b794dec8cbe73f75e585b0a7201b8d2e84578412847a";

/// Todo `"<op>","query|mutation","<sha256>"` do bundle: é assim que o web player declara as consultas.
pub(crate) fn extract_hashes(js: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for marker in ["\",\"query\",\"", "\",\"mutation\",\""] {
        let mut from = 0;
        while let Some(pos) = js[from..].find(marker) {
            let at = from + pos;
            from = at + marker.len();
            let Some(hash) = js.get(from..from + 64) else { continue };
            if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let Some(start) = js[..at].rfind('"') else { continue };
            let name = &js[start + 1..at];
            if !name.is_empty() && name.len() < 80 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                out.entry(name.to_string()).or_insert_with(|| hash.to_string());
            }
        }
    }
    out
}

async fn scrape_hashes(http: &reqwest::Client) -> Result<HashMap<String, String>, String> {
    let html = http.get(WEB_PLAYER).header("user-agent", UA).send().await.map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
    let mut hashes = HashMap::new();
    let mut from = 0;
    while let Some(pos) = html[from..].find(BUNDLE_PREFIX) {
        let at = from + pos;
        let end = html[at..].find(|c: char| c == '"' || c == '\'' || c.is_whitespace()).map_or(html.len(), |e| at + e);
        from = end.max(at + 1);
        let url = &html[at..end];
        // O bundle principal tem quase todas as consultas; os de vendor/encore não têm nenhuma.
        if !(url.ends_with(".js") && url.contains("/web-player.")) {
            continue;
        }
        if let Ok(r) = http.get(url).header("user-agent", UA).send().await {
            if let Ok(js) = r.text().await {
                hashes.extend(extract_hashes(&js));
            }
        }
    }
    if hashes.is_empty() { Err("nenhuma consulta no bundle do web player".into()) } else { Ok(hashes) }
}

async fn hash_for(http: &reqwest::Client, op: &str, refresh: bool) -> String {
    if !refresh && crate::cache::fresh("pathfinder-hashes", HASH_TTL) {
        if let Some(h) = crate::cache::load::<HashMap<String, String>>("pathfinder-hashes").and_then(|m| m.get(op).cloned()) {
            return h;
        }
    }
    match scrape_hashes(http).await {
        Ok(map) => {
            crate::cache::save("pathfinder-hashes", &map);
            map.get(op).cloned().unwrap_or_else(|| HOME_FALLBACK.into())
        }
        Err(e) => {
            log::warn!("pathfinder: hashes do web player: {e}");
            crate::cache::load::<HashMap<String, String>>("pathfinder-hashes").and_then(|m| m.get(op).cloned()).unwrap_or_else(|| HOME_FALLBACK.into())
        }
    }
}

/// Token "de primeira mão" (o do login5 da sessão) e o client-token: o par que o client oficial manda.
pub async fn first_party_tokens(session: &Session) -> Result<(String, String), String> {
    if !crate::internal::connected(session) {
        return Err(crate::internal::NOT_CONNECTED.into());
    }
    let token = session.login5().auth_token().await.map_err(|e| e.to_string())?;
    let client = session.spclient().client_token().await.map_err(|e| e.to_string())?;
    Ok((token.access_token, client))
}

async fn post(http: &reqwest::Client, token: &str, client_token: Option<&str>, op: &str, hash: &str, vars: &Value) -> Result<(u16, Value), String> {
    let body = json!({ "variables": vars, "operationName": op, "extensions": { "persistedQuery": { "version": 1, "sha256Hash": hash } } });
    let mut req = http
        .post(ENDPOINT)
        .bearer_auth(token)
        .header("app-platform", "WebPlayer")
        .header("spotify-app-version", APP_VERSION)
        .header("accept", "application/json")
        // Títulos das seções em português ("Feito para…"), como o resto do app.
        .header("accept-language", "pt-BR")
        .header("user-agent", UA)
        .json(&body);
    if let Some(ct) = client_token {
        req = req.header("client-token", ct);
    }
    let r = req.send().await.map_err(|e| e.to_string())?;
    let status = r.status().as_u16();
    Ok((status, r.json().await.unwrap_or(Value::Null)))
}

fn unknown_hash(v: &Value) -> bool {
    v["errors"].as_array().is_some_and(|e| e.iter().any(|e| e["message"].as_str().is_some_and(|m| m.contains("PersistedQueryNotFound") || m.contains("persisted query"))))
}

/// Roda uma consulta com cada token em `tokens` (token, client-token) até um responder; o `data` da resposta.
/// Hash desconhecido: relê o web player uma vez.
pub async fn query(http: &reqwest::Client, tokens: &[(String, Option<String>)], op: &str, vars: Value) -> Result<Value, String> {
    let mut hash = hash_for(http, op, false).await;
    let mut refreshed = false;
    loop {
        let mut last = (0, Value::Null);
        for (token, ct) in tokens {
            let (status, v) = post(http, token, ct.as_deref(), op, &hash, &vars).await?;
            if (200..300).contains(&status) && !v["data"].is_null() {
                return Ok(v["data"].clone());
            }
            let stop = unknown_hash(&v) || !matches!(status, 400 | 401 | 403);
            last = (status, v);
            if stop {
                break;
            }
        }
        if unknown_hash(&last.1) && !refreshed {
            refreshed = true;
            hash = hash_for(http, op, true).await;
            continue;
        }
        let msg = last.1["errors"][0]["message"].as_str().unwrap_or("sem dados").to_string();
        return Err(format!("pathfinder {op} ({}): {msg}", last.0));
    }
}

fn get<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(v, |cur, k| match k.parse::<usize>() {
        Ok(i) if cur.is_array() => &cur[i],
        _ => &cur[*k],
    })
}

fn get_str<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    get(v, path).as_str().filter(|s| !s.is_empty())
}

/// A imagem de ~300 px (a de 640 é grande demais para o cartão).
fn image(d: &Value) -> Option<String> {
    let spots: [&[&str]; 5] = [
        &["images", "items", "0", "sources"],
        &["coverArt", "sources"],
        &["visuals", "avatarImage", "sources"],
        &["podcastV2", "data", "coverArt", "sources"],
        &["albumOfTrack", "coverArt", "sources"],
    ];
    spots.iter().find_map(|p| {
        get(d, p).as_array()?.iter()
            .filter_map(|s| Some((s["url"].as_str()?, s["width"].as_i64().unwrap_or(0))))
            .min_by_key(|(_, w)| (w - 300).abs())
            .map(|(u, _)| u.to_string())
    })
}

/// Tira as tags HTML das descrições das playlists do Spotify ("<a href=…>Drake</a>, …").
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in text.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&#x27;", "'").replace("&#39;", "'").replace("&quot;", "\"").replace('\n', " ")
}

fn artists_line(d: &Value) -> Option<String> {
    let names: Vec<&str> = d["artists"]["items"].as_array()?.iter().filter_map(|a| get_str(a, &["profile", "name"])).collect();
    (!names.is_empty()).then(|| names.join(", "))
}

/// Um item da home vira cartão; faixas e episódios soltos (sem página aqui) ficam de fora.
fn card(v: &Value) -> Option<Card> {
    let d = [get(v, &["content", "data"]), get(v, &["itemV2", "data"])].into_iter().find(|d| !d.is_null()).unwrap_or(v);
    let uri = get_str(d, &["uri"]).or_else(|| get_str(v, &["uri"]))?;
    let mut parts = uri.split(':');
    let (_, kind, id) = (parts.next()?, parts.next()?, parts.next()?);
    let kind = match kind {
        "playlist" => Kind::Playlist,
        "album" => Kind::Album,
        "artist" => Kind::Artist,
        "show" => Kind::Show,
        _ => return None,
    };
    let title = get_str(d, &["name"]).or_else(|| get_str(d, &["profile", "name"]))?.to_string();
    let subtitle = match kind {
        Kind::Playlist => get_str(d, &["description"]).map(plain).filter(|s| !s.trim().is_empty())
            .or_else(|| get_str(d, &["ownerV2", "data", "name"]).map(|o| format!("De {o}"))),
        Kind::Album => artists_line(d),
        Kind::Artist => Some("Artista".into()),
        Kind::Show => get_str(d, &["publisher", "name"]).map(str::to_string),
    };
    Some(Card { kind, id: id.to_string(), uri: uri.to_string(), title, subtitle: subtitle.unwrap_or_default(), image: image(d) })
}

/// As seções da home, na ordem do Spotify, sem as vazias.
pub(crate) fn shelves(data: &Value) -> Vec<Shelf> {
    get(data, &["home", "sectionContainer", "sections", "items"]).as_array().map(|secs| {
        // Só as prateleiras de verdade: as "baseline" são um feed de cartões soltos (um por seção, títulos repetidos)
        // e "Recents" tem um item só.
        secs.iter().filter(|s| s["data"]["__typename"] == "HomeGenericSectionData").filter_map(|s| {
            let title = ["transformedLabel", "text", "originalLabel"].iter().find_map(|k| get_str(s, &["data", "title", k]))?.to_string();
            let cards: Vec<Card> = s["sectionItems"]["items"].as_array()?.iter().filter_map(card).collect();
            (!cards.is_empty()).then_some(Shelf { title, cards })
        }).collect()
    }).unwrap_or_default()
}

/// Fuso no formato IANA que o Pathfinder entende. Sem o nome do fuso do Windows, usa o deslocamento
/// ("Etc/GMT+3" é UTC-3: o sinal do Etc é invertido).
fn time_zone() -> String {
    let hours = crate::local_offset_secs() / 3600;
    match hours {
        0 => "UTC".into(),
        h if h > 0 => format!("Etc/GMT-{h}"),
        h => format!("Etc/GMT+{}", -h),
    }
}

/// A home personalizada do Spotify. `extra_token`: outro token para tentar se o do login5 for recusado.
pub async fn home(http: &reqwest::Client, session: &Session, extra_token: Option<String>) -> Result<Vec<Shelf>, String> {
    let mut tokens = Vec::new();
    match first_party_tokens(session).await {
        Ok((t, ct)) => tokens.push((t, Some(ct))),
        Err(e) => log::warn!("pathfinder: token do login5: {e}"),
    }
    tokens.extend(extra_token.map(|t| (t, None)));
    if tokens.is_empty() {
        return Err("sem token para o pathfinder".into());
    }
    let vars = json!({
        "homeEndUserIntegration": "INTEGRATION_WEB_PLAYER",
        "timeZone": time_zone(),
        "sp_t": "",
        "facet": "",
        "sectionItemsLimit": 12,
        "includeEpisodeContentRatingsV2": false,
    });
    let shelves = shelves(&query(http, &tokens, "home", vars).await?);
    if shelves.is_empty() { Err("a home do pathfinder veio vazia".into()) } else { Ok(shelves) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hashes_from_a_bundle() {
        let js = r#"new i.l("home","query","76243c78b0e20ecdbe41b794dec8cbe73f75e585b0a7201b8d2e84578412847a",null);
            x("bad name!","query","76243c78b0e20ecdbe41b794dec8cbe73f75e585b0a7201b8d2e84578412847a"); y("short","query","abc");"#;
        let h = extract_hashes(js);
        assert_eq!(h.get("home").map(String::as_str), Some(HOME_FALLBACK));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn parses_home_sections() {
        let data = json!({"home": {"sectionContainer": {"sections": {"items": [
            {"data": {"__typename": "HomeFeedBaselineSectionData", "title": {"text": "Solta"}}, "sectionItems": {"items": [{"content": {"data": {"uri": "spotify:playlist:x", "name": "x"}}}]}},
            {"data": {"__typename": "HomeGenericSectionData", "title": {"transformedLabel": "Made For gabs"}}, "sectionItems": {"items": [
                {"content": {"data": {"uri": "spotify:playlist:37i9", "name": "Daily Mix 1", "description": "<a href=\"x\">Drake</a>, A$AP Rocky",
                    "images": {"items": [{"sources": [{"url": "big", "width": 640}, {"url": "mid", "width": 300}]}]}}}},
                {"content": {"data": {"uri": "spotify:album:a", "name": "Disco", "artists": {"items": [{"profile": {"name": "Banda"}}]}, "coverArt": {"sources": [{"url": "c"}]}}}},
                {"content": {"data": {"uri": "spotify:episode:e", "name": "Episódio"}}}
            ]}},
            {"data": {"__typename": "HomeGenericSectionData", "title": {"text": "Vazia"}}, "sectionItems": {"items": []}}
        ]}}}});
        let s = shelves(&data);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "Made For gabs");
        assert_eq!(s[0].cards.len(), 2);
        assert_eq!(s[0].cards[0].subtitle, "Drake, A$AP Rocky");
        assert_eq!(s[0].cards[0].image.as_deref(), Some("mid"));
        assert_eq!(s[0].cards[1].subtitle, "Banda");
    }
}
