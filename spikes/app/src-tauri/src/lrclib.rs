// Matching do doc 04: get-cached -> get -> título normalizado sem álbum -> search por duração.
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const BASE: &str = "https://lrclib.net/api";

/// Remove sufixos que divergem entre Spotify e LRCLIB (" - Remastered 2011", "(feat. X)", "[Live]").
pub fn normalize_title(t: &str) -> String {
    let mut s = t.split(" - ").next().unwrap_or(t).to_string();
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let (Some(a), Some(b)) = (s.find(open), s.find(close)) {
            if b < a { break; }
            s.replace_range(a..=b, "");
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

static RATE_LIMITED: AtomicU32 = AtomicU32::new(0);

/// LRCLIB responde 429 com `retry-after: 1` (Cloudflare) quando chamamos rápido demais.
async fn get(http: &reqwest::Client, path: &str, q: &[(&str, String)]) -> Option<Value> {
    for _ in 0..5 {
        let r = http.get(format!("{BASE}{path}")).query(q).send().await.ok()?;
        if r.status() == 429 {
            RATE_LIMITED.fetch_add(1, Ordering::Relaxed);
            let secs = r.headers().get("retry-after").and_then(|h| h.to_str().ok()?.parse().ok()).unwrap_or(1);
            tokio::time::sleep(Duration::from_secs(secs)).await;
            continue;
        }
        if !r.status().is_success() { return None; }
        return r.json().await.ok();
    }
    None
}

/// Devolve (registro, etapa que achou).
async fn find(http: &reqwest::Client, title: &str, artist: &str, album: &str, secs: u64) -> Option<(Value, &'static str)> {
    let full = [("track_name", title.to_string()), ("artist_name", artist.to_string()),
                ("album_name", album.to_string()), ("duration", secs.to_string())];
    if let Some(v) = get(http, "/get-cached", &full).await { return Some((v, "get-cached")); }
    if let Some(v) = get(http, "/get", &full).await { return Some((v, "get")); }
    let norm = normalize_title(title);
    let short = [("track_name", norm.clone()), ("artist_name", artist.to_string()), ("duration", secs.to_string())];
    if let Some(v) = get(http, "/get-cached", &short).await { return Some((v, "normalizado")); }
    let list = get(http, "/search", &[("track_name", norm), ("artist_name", artist.to_string())]).await?;
    list.as_array()?
        .iter()
        .filter(|c| c["duration"].as_f64().is_some_and(|d| (d - secs as f64).abs() <= 2.0))
        .max_by_key(|c| c["syncedLyrics"].is_string())
        .map(|c| (c.clone(), "search"))
}

pub async fn lookup(http: &reqwest::Client, title: &str, artist: &str, album: &str, secs: u64) -> Value {
    find(http, title, artist, album, secs).await.map(|(v, _)| v).unwrap_or(Value::Null)
}

pub async fn measure(http: &reqwest::Client, tracks: &[Value], progress: impl Fn(usize, usize)) -> Value {
    let (mut synced, mut plain, mut instrumental, mut missing) = (0, 0, 0, 0);
    let mut by_stage = serde_json::Map::new();
    let mut misses = Vec::new();
    let mut times = Vec::new();
    for (i, t) in tracks.iter().enumerate() {
        progress(i + 1, tracks.len());
        let title = t["name"].as_str().unwrap_or_default();
        let artist = t["artists"][0]["name"].as_str().unwrap_or_default();
        let album = t["album"]["name"].as_str().unwrap_or_default();
        let secs = (t["duration_ms"].as_u64().unwrap_or(0) + 500) / 1000;
        let start = Instant::now();
        let found = find(http, title, artist, album, secs).await;
        times.push(start.elapsed().as_millis() as u64);
        match found {
            Some((v, stage)) => {
                let n = by_stage.get(stage).and_then(|x| x.as_u64()).unwrap_or(0);
                by_stage.insert(stage.into(), json!(n + 1));
                if v["instrumental"].as_bool() == Some(true) { instrumental += 1 }
                else if v["syncedLyrics"].is_string() { synced += 1 }
                else { plain += 1 }
            }
            None => { missing += 1; misses.push(format!("{artist} — {title}")); }
        }
    }
    times.sort_unstable();
    let p = |q: f64| times.get(((times.len() as f64 - 1.0) * q) as usize).copied().unwrap_or(0);
    json!({
        "total": tracks.len(), "synced": synced, "plain_only": plain,
        "instrumental": instrumental, "missing": missing,
        "found_by_stage": by_stage, "latency_ms_p50": p(0.5), "latency_ms_p95": p(0.95),
        "http_429_retries": RATE_LIMITED.swap(0, Ordering::Relaxed),
        "misses_sample": misses.into_iter().take(25).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_title;

    #[test]
    fn normalizes_common_suffixes() {
        assert_eq!(normalize_title("Something - Remastered 2009"), "Something");
        assert_eq!(normalize_title("Song (feat. Someone) [Live]"), "Song");
        assert_eq!(normalize_title("Plain"), "Plain");
        assert_eq!(normalize_title("Odd ) order ("), "Odd ) order (");
    }
}
