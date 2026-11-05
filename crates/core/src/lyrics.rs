//! Letras: primeiro o TTML do Apple Music (palavra por palavra, com quem canta e os ad-libs), depois a do próprio
//! Spotify e, só sem nenhuma delas, o LRCLIB (doc 04). Cache em disco, matching por duração.
use std::{path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Track;

const BASE: &str = "https://lrclib.net/api";
/// Busca de TTML do Apple Music por ISRC ou por nome/artista/álbum/duração (a mesma que o Musique usa).
const APPLE: &str = "https://lyrics-api.binimum.org/";
/// Sem cara de navegador, o Cloudflare na frente da API recusa.
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0 Safari/537.36";

/// Uma palavra (ou sílaba) com o tempo em que é cantada. O texto já traz o espaço que vem depois dela.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub time_ms: u32,
    pub text: String,
    /// Tempos por palavra (TTML). Vazio nas letras só por linha (Spotify, LRCLIB).
    #[serde(default)]
    pub words: Vec<Word>,
    /// Ad-lib / vocal de apoio cantado junto com a linha ("(walk it)"), sem os parênteses.
    #[serde(default)]
    pub adlib: Vec<Word>,
    /// Cantada por outra voz que não a principal: fica do lado direito, como no Apple Music.
    #[serde(default)]
    pub right: bool,
}

impl Line {
    pub fn plain(time_ms: u32, text: impl Into<String>) -> Self {
        Self { time_ms, text: text.into(), words: Vec::new(), adlib: Vec::new(), right: false }
    }

    pub fn adlib_text(&self) -> String {
        self.adlib.iter().map(|w| w.text.as_str()).collect::<String>().trim().to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Lyrics {
    Synced(Vec<Line>),
    Plain(Vec<String>),
    Instrumental,
    NotFound,
    /// O LRCLIB não respondeu (fora do ar, sobrecarregado): não quer dizer que a letra não existe.
    Failed,
}

/// Remove sufixos que divergem entre Spotify e LRCLIB (" - Remastered 2011", "(feat. X)", "[with X]").
pub fn normalize_title(t: &str) -> String {
    let mut s = t.split(" - ").next().unwrap_or(t).to_string();
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let (Some(a), Some(b)) = (s.find(open), s.find(close)) {
            if b < a {
                break;
            }
            s.replace_range(a..=b, "");
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[mm:ss.xx] texto`, com várias tags por linha e `[offset:+/-ms]`.
pub fn parse_lrc(lrc: &str) -> Vec<Line> {
    let mut offset: i64 = 0;
    let mut out = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(tag) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            let (inside, after) = tag;
            if let Some(v) = inside.strip_prefix("offset:") {
                offset = v.trim().parse().unwrap_or(0);
            } else if let Some((m, s)) = inside.split_once(':') {
                if let (Ok(m), Ok(s)) = (m.parse::<f64>(), s.parse::<f64>()) {
                    times.push(((m * 60.0 + s) * 1000.0).round() as i64);
                }
            }
            rest = after;
        }
        for t in times {
            // Offset positivo no LRC adianta a letra.
            let time_ms = (t - offset).max(0) as u32;
            out.push(Line::plain(time_ms, rest.trim()));
        }
    }
    out.sort_by_key(|l| l.time_ms);
    out
}

fn from_record(v: &Value) -> Lyrics {
    if v["instrumental"].as_bool() == Some(true) {
        return Lyrics::Instrumental;
    }
    if let Some(synced) = v["syncedLyrics"].as_str() {
        let lines = parse_lrc(synced);
        if !lines.is_empty() {
            return Lyrics::Synced(lines);
        }
    }
    match v["plainLyrics"].as_str() {
        Some(p) if !p.trim().is_empty() => Lyrics::Plain(p.lines().map(String::from).collect()),
        _ => Lyrics::NotFound,
    }
}

/// Espera antes de cada nova tentativa depois de um 5xx: o LRCLIB responde 503 de vez em quando (sobrecarga
/// atrás do Cloudflare) e o pedido seguinte costuma passar.
const RETRY_WAITS: [u64; 4] = [1, 2, 4, 8];

/// LRCLIB responde 429 com `retry-after: 1` quando chamamos rápido demais (doc 07), e 5xx quando está sobrecarregado.
async fn get(http: &reqwest::Client, path: &str, q: &[(&str, String)]) -> Result<Option<Value>, String> {
    let mut last = String::new();
    for attempt in 0..=RETRY_WAITS.len() {
        let wait = match http.get(format!("{BASE}{path}")).query(q).send().await {
            Ok(r) => match r.status().as_u16() {
                429 => r.headers().get("retry-after").and_then(|h| h.to_str().ok()?.parse().ok()).unwrap_or(1),
                404 => return Ok(None),
                s if s >= 500 => {
                    last = format!("LRCLIB respondeu {s}");
                    RETRY_WAITS.get(attempt).copied().unwrap_or(0)
                }
                s if s >= 400 => return Err(format!("LRCLIB respondeu {s}")),
                _ => return r.json().await.map(Some).map_err(|e| e.to_string()),
            },
            // Conexão caiu ou demorou demais: mesma coisa que um 5xx.
            Err(e) => {
                last = e.to_string();
                RETRY_WAITS.get(attempt).copied().unwrap_or(0)
            }
        };
        if attempt < RETRY_WAITS.len() {
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
    }
    Err(if last.is_empty() { "LRCLIB ocupado".into() } else { last })
}

async fn lookup(http: &reqwest::Client, t: &Track) -> Result<Option<Value>, String> {
    let secs = ((t.duration_ms + 500) / 1000).to_string();
    let artist = t.artist.split(", ").next().unwrap_or(&t.artist).to_string();
    let full = [("track_name", t.name.clone()), ("artist_name", artist.clone()), ("album_name", t.album.clone()), ("duration", secs.clone())];
    if let Some(v) = get(http, "/get", &full).await? {
        return Ok(Some(v));
    }
    let norm = normalize_title(&t.name);
    let list = get(http, "/search", &[("track_name", norm), ("artist_name", artist)]).await?;
    let wanted = t.duration_ms as f64 / 1000.0;
    Ok(list.and_then(|l| {
        l.as_array()?
            .iter()
            .filter(|c| c["duration"].as_f64().is_some_and(|d| (d - wanted).abs() <= 2.0))
            .max_by_key(|c| c["syncedLyrics"].is_string())
            .cloned()
    }))
}

/// `12.3`, `1:02.5`, `00:01:02.500`, `12.3s` → ms.
fn ttml_time(raw: &str) -> Option<u32> {
    let secs = raw.trim().trim_end_matches('s').split(':').try_fold(0., |acc, p| p.parse::<f64>().ok().map(|v| acc * 60. + v))?;
    Some((secs * 1000.).round() as u32)
}

/// Valor do atributo `name` (ignora o prefixo `ttm:`) numa tag já sem os `<>`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(eq) = rest.find("=\"") {
        let key = rest[..eq].rsplit(|c: char| c.is_whitespace()).next().unwrap_or_default();
        let after = &rest[eq + 2..];
        let end = after.find('"')?;
        if key == name || key.rsplit(':').next() == Some(name) {
            return Some(&after[..end]);
        }
        rest = &after[end + 1..];
    }
    None
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&#x27;", "'").replace("&amp;", "&")
}

#[derive(Clone, Copy, PartialEq)]
enum Voice {
    Lead,
    Adlib,
    /// Tradução ou romanização embutida: fora da letra.
    Skip,
}

/// TTML do Apple Music: cada `<p>` é uma linha, cada `<span begin end>` uma palavra, `ttm:role="x-bg"` o ad-lib e
/// `ttm:agent` quem canta. Quem canta mais é a voz principal (esquerda); as outras vão para a direita.
pub fn parse_ttml(raw: &str) -> Vec<Line> {
    let mut out: Vec<(Line, Option<String>)> = Vec::new();
    let mut from = 0;
    while let Some(rel) = raw[from..].find("<p") {
        let open = from + rel;
        from = open + 2;
        if !raw[from..].starts_with([' ', '>']) {
            continue; // <pre>, <path>…
        }
        let Some(gt) = raw[open..].find('>').map(|g| open + g) else { break };
        let Some(close) = raw[gt..].find("</p>").map(|c| gt + c) else { break };
        let tag = &raw[open + 1..gt];
        from = close + 4;

        let (mut lead, mut adlib, mut bare) = (Vec::<Word>::new(), Vec::<Word>::new(), String::new());
        let mut stack: Vec<(Voice, Option<(u32, u32)>)> = Vec::new();
        let inner = &raw[gt + 1..close];
        let mut i = 0;
        while i < inner.len() {
            if inner[i..].starts_with('<') {
                let Some(end) = inner[i..].find('>').map(|e| i + e) else { break };
                let t = &inner[i + 1..end];
                if t.starts_with("/span") {
                    stack.pop();
                } else if t.starts_with("span") && !t.ends_with('/') {
                    let parent = stack.last().map_or(Voice::Lead, |s| s.0);
                    let voice = match attr(t, "role") {
                        Some("x-bg") => Voice::Adlib,
                        Some(r) if r.starts_with("x-") => Voice::Skip,
                        _ => parent,
                    };
                    let time = attr(t, "begin").and_then(ttml_time).zip(attr(t, "end").and_then(ttml_time));
                    stack.push((voice, time));
                }
                i = end + 1;
                continue;
            }
            let next = inner[i..].find('<').map_or(inner.len(), |n| i + n);
            let text = unescape(&inner[i..next]);
            i = next;
            let (voice, time) = stack.last().copied().unwrap_or((Voice::Lead, None));
            let words = match voice {
                Voice::Skip => continue,
                Voice::Adlib => &mut adlib,
                Voice::Lead => &mut lead,
            };
            match time {
                Some((start, end)) if !text.trim().is_empty() => {
                    let mut t = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if text.ends_with(char::is_whitespace) {
                        t.push(' ');
                    }
                    words.push(Word { start, end, text: t });
                }
                // Espaço entre spans: separa a palavra anterior da seguinte (sílabas vêm coladas, sem ele).
                _ if text.trim().is_empty() => {
                    if let Some(w) = words.last_mut().filter(|w| !w.text.ends_with(' ') && !text.is_empty()) {
                        w.text.push(' ');
                    }
                }
                _ if stack.is_empty() => bare.push_str(&text),
                _ => {}
            }
        }
        // O Apple escreve o ad-lib entre parênteses; a interface já mostra que é ad-lib.
        if let Some(w) = adlib.first_mut() {
            w.text = w.text.trim_start_matches('(').to_string();
        }
        if let Some(w) = adlib.last_mut() {
            w.text = w.text.trim_end().trim_end_matches(')').to_string();
        }
        adlib.retain(|w| !w.text.trim().is_empty());
        // Linha só de ad-lib: vira a linha.
        if lead.is_empty() && bare.trim().is_empty() {
            lead = std::mem::take(&mut adlib);
        }
        let begin = attr(tag, "begin").and_then(ttml_time);
        let line = if !lead.is_empty() {
            let text = lead.iter().map(|w| w.text.as_str()).collect::<String>().trim().to_string();
            Line { time_ms: begin.unwrap_or(lead[0].start), text, words: lead, adlib, right: false }
        } else if let Some(b) = begin.filter(|_| !bare.trim().is_empty()) {
            Line { adlib, ..Line::plain(b, bare.split_whitespace().collect::<Vec<_>>().join(" ")) }
        } else {
            continue;
        };
        out.push((line, attr(tag, "agent").map(str::to_string)));
    }
    out.sort_by_key(|(l, _)| l.time_ms);
    // Voz principal: a que canta mais letras. Empate fica com a primeira, para o layout não trocar de lado.
    let mut totals: Vec<(&str, usize)> = Vec::new();
    for (line, agent) in &out {
        let Some(a) = agent.as_deref() else { continue };
        let n = line.text.chars().filter(|c| c.is_alphanumeric()).count();
        match totals.iter_mut().find(|(x, _)| *x == a) {
            Some((_, total)) => *total += n,
            None => totals.push((a, n)),
        }
    }
    let lead = totals.iter().fold(None::<(&str, usize)>, |best, &(a, n)| match best {
        Some((_, m)) if m >= n => best,
        _ => Some((a, n)),
    }).map(|(a, _)| a.to_string());
    out.iter().map(|(line, agent)| Line { right: agent.is_some() && *agent != lead, ..line.clone() }).collect()
}

/// Texto puro de um TTML sem tempos (o Apple também serve letra sem sincronia).
fn ttml_plain(raw: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut from = 0;
    while let Some(open) = raw[from..].find("<p").map(|r| from + r) {
        let (Some(gt), Some(close)) = (raw[open..].find('>').map(|g| open + g), raw[open..].find("</p>").map(|c| open + c)) else { break };
        from = close + 4;
        let mut text = String::new();
        let mut inside = false;
        for c in raw[gt + 1..close].chars() {
            match c {
                '<' => inside = true,
                '>' => inside = false,
                c if !inside => text.push(c),
                _ => {}
            }
        }
        let text = unescape(&text).split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            lines.push(text);
        }
    }
    lines
}

/// ISRC da gravação, para achar exatamente esta versão no Apple Music.
async fn isrc(session: &librespot::core::Session, t: &Track) -> Option<String> {
    use librespot::metadata::{Metadata, Track as MetaTrack};
    if !crate::internal::connected(session) {
        return None;
    }
    let uri = librespot::core::SpotifyUri::from_uri(&t.uri).ok()?;
    let meta = MetaTrack::get(session, &uri).await.ok()?;
    meta.external_ids.iter().find(|e| e.external_type.eq_ignore_ascii_case("isrc")).map(|e| e.id.clone())
}

async fn apple_hits(http: &reqwest::Client, q: &[(&str, String)]) -> Result<Vec<Value>, String> {
    let r = http.get(APPLE).header("user-agent", BROWSER_UA).query(q).send().await.map_err(|e| e.to_string())?;
    match r.status().as_u16() {
        404 => Ok(Vec::new()),
        s if s >= 400 => Err(format!("Apple Music respondeu {s}")),
        _ => Ok(r.json::<Value>().await.map_err(|e| e.to_string())?["results"].as_array().cloned().unwrap_or_default()),
    }
}

/// Letra do Apple Music: pelo ISRC; sem ele (ou sem resultado), pelo nome com a duração batendo em até 2 s.
async fn from_apple(http: &reqwest::Client, isrc: Option<String>, t: &Track) -> Result<Option<Lyrics>, String> {
    let wanted = t.duration_ms as f64 / 1000.;
    let mut hits = match &isrc {
        Some(code) => apple_hits(http, &[("isrc", code.clone())]).await?,
        None => Vec::new(),
    };
    if hits.is_empty() {
        let artist = t.artist.split(", ").next().unwrap_or(&t.artist).to_string();
        let q = [("track", normalize_title(&t.name)), ("artist", artist), ("album", t.album.clone()), ("duration", ((t.duration_ms + 500) / 1000).to_string())];
        hits = apple_hits(http, &q).await?;
    }
    let dist = |h: &Value| h["duration"].as_f64().map_or(f64::MAX, |d| (d - wanted).abs());
    hits.retain(|h| h["lyricsUrl"].is_string() && dist(h) <= 2.);
    // Palavra por palavra primeiro, depois a duração mais próxima.
    hits.sort_by(|a, b| (a["timing_type"] != "word").cmp(&(b["timing_type"] != "word")).then(dist(a).total_cmp(&dist(b))));
    for h in hits {
        let url = h["lyricsUrl"].as_str().unwrap_or_default();
        let Ok(r) = http.get(url).header("user-agent", BROWSER_UA).send().await else { continue };
        let Ok(body) = r.error_for_status().map_err(|_| ()) else { continue };
        let Ok(raw) = body.text().await else { continue };
        let lines = parse_ttml(&raw);
        if !lines.is_empty() {
            return Ok(Some(Lyrics::Synced(lines)));
        }
        let plain = ttml_plain(&raw);
        if !plain.is_empty() {
            return Ok(Some(Lyrics::Plain(plain)));
        }
    }
    Ok(None)
}

/// A letra que o próprio Spotify mostra, pela sessão do áudio.
async fn from_spotify(session: &librespot::core::Session, t: &Track) -> Option<Lyrics> {
    if !t.uri.starts_with("spotify:track:") || !crate::internal::connected(session) {
        return None;
    }
    let id = librespot::core::SpotifyId::from_base62(&t.id).ok()?;
    match session.spclient().get_lyrics(&id).await {
        Ok(body) => parse_spotify(&serde_json::from_slice(&body).ok()?),
        // 404 quando o Spotify não tem a letra.
        Err(e) => {
            log::debug!("letra do Spotify para {}: {e}", t.name);
            None
        }
    }
}

/// `{"lyrics": {"syncType": "LINE_SYNCED", "lines": [{"startTimeMs": "1230", "words": "…"}]}}`; "♪" marca trecho sem voz.
fn parse_spotify(v: &Value) -> Option<Lyrics> {
    let l = &v["lyrics"];
    let lines = l["lines"].as_array()?;
    let words = |x: &Value| x["words"].as_str().unwrap_or_default().replace('♪', "").trim().to_string();
    if l["syncType"] == "LINE_SYNCED" {
        let out: Vec<Line> = lines.iter().filter_map(|x| Some(Line::plain(x["startTimeMs"].as_str()?.parse().ok()?, words(x)))).collect();
        (!out.is_empty()).then_some(Lyrics::Synced(out))
    } else {
        let out: Vec<String> = lines.iter().map(words).collect();
        (!out.is_empty()).then_some(Lyrics::Plain(out))
    }
}

/// Palavra por palavra: guardada para sempre. As outras (e "não encontrada") por 7 dias, para tentar de novo o
/// Apple Music, que pode ganhar a letra depois.
fn keeps_forever(l: &Lyrics) -> bool {
    matches!(l, Lyrics::Synced(lines) if lines.iter().any(|l| !l.words.is_empty()))
}

/// Letra da faixa: cache em disco; senão Apple Music, depois Spotify e, só sem nenhuma das duas, LRCLIB.
/// `Failed` (nada respondeu) não vai para o cache.
pub async fn fetch(http: &reqwest::Client, session: &librespot::core::Session, cache_dir: &Path, t: &Track) -> Result<Lyrics, String> {
    let dir = cache_dir.join("lyrics");
    let path = dir.join(format!("{}.v2.json", t.id));
    if let Ok(text) = tokio::fs::read_to_string(&path).await {
        let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(7 * 24 * 3600));
        if let Ok(l) = serde_json::from_str::<Lyrics>(&text) {
            if fresh || keeps_forever(&l) {
                return Ok(l);
            }
        }
    }
    let found = match from_apple(http, isrc(session, t).await, t).await {
        Ok(Some(l)) => Ok(l),
        apple => {
            if let Err(e) = &apple {
                log::warn!("letra do Apple Music para {}: {e}", t.name);
            }
            match from_spotify(session, t).await {
                Some(l) => Ok(l),
                None => lookup(http, t).await.map(|r| r.as_ref().map(from_record).unwrap_or(Lyrics::NotFound)),
            }
        }
    }?;
    let _ = tokio::fs::create_dir_all(&dir).await;
    let _ = tokio::fs::write(&path, serde_json::to_string(&found).unwrap_or_default()).await;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_common_suffixes() {
        assert_eq!(normalize_title("Something - Remastered 2009"), "Something");
        assert_eq!(normalize_title("Song (feat. Someone) [with X]"), "Song");
        assert_eq!(normalize_title("Odd ) order ("), "Odd ) order (");
    }

    #[test]
    fn parses_multi_tags_offset_and_sorts() {
        let lines = parse_lrc("[ar:Someone]\n[offset:+500]\n[00:10.00][01:00.50] refrão\n[00:05.20]início\n[00:07.00]\n");
        assert_eq!(
            lines,
            vec![
                Line::plain(4700, "início"),
                Line::plain(6500, ""),
                Line::plain(9500, "refrão"),
                Line::plain(60000, "refrão"),
            ]
        );
    }

    #[test]
    fn ttml_words_adlibs_and_sides() {
        let raw = r#"<tt><head><metadata><ttm:agent xml:id="v1"/></metadata></head><body><div>
            <p begin="8.938" end="9.806" ttm:agent="v1"><span begin="8.938" end="9.472">Yeah,</span> <span begin="9.472" end="9.806">yeah</span></p>
            <p begin="9.806" end="11.014" ttm:agent="v2"><span begin="9.806" end="11.014">Deko</span></p>
            <p begin="13.091" end="14.649" ttm:agent="v1"><span begin="13.091" end="13.286">Walk</span> <span begin="13.286" end="13.481">it</span> <span ttm:role="x-bg"><span begin="14.260" end="14.454">(Walk</span> <span begin="14.454" end="14.649">it)</span></span></p>
            <p begin="1:00.000" ttm:agent="v1"><span begin="60.0" end="60.1">H</span><span begin="60.1" end="60.5">ey</span><span ttm:role="x-translation">Oi</span></p>
            <p begin="00:01:10.5">Linha &amp; sem palavras</p>
        </div></body></tt>"#;
        let lines = parse_ttml(raw);
        assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["Yeah, yeah", "Deko", "Walk it", "Hey", "Linha & sem palavras"]);
        assert_eq!(lines[0].words[0], Word { start: 8938, end: 9472, text: "Yeah, ".into() });
        assert_eq!(lines.iter().map(|l| l.right).collect::<Vec<_>>(), [false, true, false, false, false]);
        assert_eq!(lines[2].adlib_text(), "Walk it");
        assert_eq!(lines[2].adlib[0].start, 14260);
        assert_eq!(lines[3].time_ms, 60000);
        assert_eq!(lines[4].time_ms, 70500);
        assert!(lines[4].words.is_empty());
    }

    #[test]
    fn spotify_lyrics() {
        let synced = json!({"lyrics": {"syncType": "LINE_SYNCED", "lines": [
            {"startTimeMs": "1000", "words": "oi"}, {"startTimeMs": "2500", "words": "♪"}, {"startTimeMs": "x", "words": "ruim"}]}});
        assert_eq!(parse_spotify(&synced), Some(Lyrics::Synced(vec![Line::plain(1000, "oi"), Line::plain(2500, "")])));
        let plain = json!({"lyrics": {"syncType": "UNSYNCED", "lines": [{"startTimeMs": "0", "words": "a"}]}});
        assert_eq!(parse_spotify(&plain), Some(Lyrics::Plain(vec!["a".into()])));
        assert_eq!(parse_spotify(&json!({"lyrics": {"lines": []}})), None);
        assert_eq!(parse_spotify(&json!({})), None);
    }

    #[test]
    fn record_states() {
        assert_eq!(from_record(&json!({"instrumental": true})), Lyrics::Instrumental);
        assert_eq!(from_record(&json!({"plainLyrics": "a\nb"})), Lyrics::Plain(vec!["a".into(), "b".into()]));
        assert_eq!(from_record(&json!({"syncedLyrics": "[00:01.00]oi"})), Lyrics::Synced(vec![Line::plain(1000, "oi")]));
        assert_eq!(from_record(&json!({})), Lyrics::NotFound);
    }
}
