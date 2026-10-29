//! Letras via LRCLIB (doc 04): cache em disco, matching por duração, parser LRC.
use std::{path::Path, time::Duration};

use serde_json::Value;

use crate::Track;

const BASE: &str = "https://lrclib.net/api";

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub time_ms: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
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
            out.push(Line { time_ms, text: rest.trim().to_string() });
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

/// Letra da faixa, do cache em disco ou do LRCLIB. "Não encontrada" também é cacheada, por 7 dias.
pub async fn fetch(http: &reqwest::Client, cache_dir: &Path, t: &Track) -> Result<Lyrics, String> {
    let dir = cache_dir.join("lyrics");
    let path = dir.join(format!("{}.json", t.id));
    if let Ok(text) = tokio::fs::read_to_string(&path).await {
        let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(7 * 24 * 3600));
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Null) if fresh => return Ok(Lyrics::NotFound),
            Ok(Value::Null) => {}
            Ok(v) => return Ok(from_record(&v)),
            Err(_) => {}
        }
    }
    let record = lookup(http, t).await?;
    let _ = tokio::fs::create_dir_all(&dir).await;
    let _ = tokio::fs::write(&path, serde_json::to_string(&record).unwrap_or_default()).await;
    Ok(record.as_ref().map(from_record).unwrap_or(Lyrics::NotFound))
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
                Line { time_ms: 4700, text: "início".into() },
                Line { time_ms: 6500, text: "".into() },
                Line { time_ms: 9500, text: "refrão".into() },
                Line { time_ms: 60000, text: "refrão".into() },
            ]
        );
    }

    #[test]
    fn record_states() {
        assert_eq!(from_record(&json!({"instrumental": true})), Lyrics::Instrumental);
        assert_eq!(from_record(&json!({"plainLyrics": "a\nb"})), Lyrics::Plain(vec!["a".into(), "b".into()]));
        assert_eq!(from_record(&json!({"syncedLyrics": "[00:01.00]oi"})), Lyrics::Synced(vec![Line { time_ms: 1000, text: "oi".into() }]));
        assert_eq!(from_record(&json!({})), Lyrics::NotFound);
    }
}
