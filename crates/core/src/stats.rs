//! Estatísticas pessoais, um "Wrapped" próprio: cada música ouvida neste computador vai para
//! `plays.jsonl` (uma linha por audição), e as contas saem dali na hora de abrir a tela.
use std::collections::HashMap;
use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::{Track, config};

/// Menos que isto não conta como ouvida (o mesmo corte do Spotify para contar uma reprodução).
pub const MIN_PLAYED_MS: u32 = 30_000;
const TOP: usize = 10;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Play {
    pub uri: String,
    pub name: String,
    pub artist: String,
    /// Primeiro artista: id e nome.
    #[serde(default)]
    pub artist_id: String,
    pub album: String,
    #[serde(default)]
    pub image: Option<String>,
    pub ms: u32,
    /// Quando começou, em segundos Unix.
    pub at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Month,
    Year,
    All,
}

/// Uma linha de top: nome, detalhe (artista da música), capa, audições e minutos.
#[derive(Clone, Debug, Default)]
pub struct Ranked {
    pub name: String,
    pub detail: String,
    pub image: Option<String>,
    /// URI da música, ou id do artista.
    pub id: String,
    pub plays: usize,
    pub minutes: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub minutes: u64,
    pub plays: usize,
    pub tracks: usize,
    pub artists: usize,
    pub top_artists: Vec<Ranked>,
    pub top_tracks: Vec<Ranked>,
    /// Minutos por mês do ano corrente (jan..dez), ou por dia do mês corrente.
    pub timeline: Vec<u64>,
    /// Hora do dia (0..24) em que mais ouve.
    pub top_hour: Option<u32>,
}

fn path() -> std::path::PathBuf {
    config::config_dir().join("plays.jsonl")
}

pub fn record(track: &Track, ms: u32, at: u64) {
    let play = Play {
        uri: track.uri.clone(),
        name: track.name.clone(),
        artist: track.artist.clone(),
        artist_id: track.artists.first().map(|a| a.0.clone()).unwrap_or_default(),
        album: track.album.clone(),
        image: track.cover_small.clone(),
        ms,
        at,
    };
    let line = serde_json::to_string(&play).unwrap_or_default();
    let file = std::fs::create_dir_all(config::config_dir()).and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(path()));
    if let Err(e) = file.and_then(|mut f| writeln!(f, "{line}")) {
        log::warn!("plays.jsonl: {e}");
    }
}

pub fn load() -> Vec<Play> {
    std::fs::read_to_string(path()).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Data civil (ano, mês 1..12, dia 1..31) de um instante Unix, no fuso `offset_secs`.
fn civil(at: u64, offset_secs: i64) -> (i64, u32, u32) {
    // Algoritmo de Howard Hinnant (days_from_civil ao contrário).
    let z = (at as i64 + offset_secs).div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// As contas do período. `now` e `offset_secs` (fuso local) vêm de fora para dar para testar.
pub fn compute(plays: &[Play], range: Range, now: u64, offset_secs: i64) -> Stats {
    let (year, month, _) = civil(now, offset_secs);
    let inside = |p: &&Play| {
        let (y, m, _) = civil(p.at, offset_secs);
        match range {
            Range::Month => y == year && m == month,
            Range::Year => y == year,
            Range::All => true,
        }
    };
    let chosen: Vec<&Play> = plays.iter().filter(inside).collect();
    let mut by_track: HashMap<&str, Ranked> = HashMap::new();
    let mut by_artist: HashMap<String, Ranked> = HashMap::new();
    let mut hours = [0u64; 24];
    let mut timeline = vec![0u64; if range == Range::Month { 31 } else { 12 }];
    let mut total_ms = 0u64;
    for p in &chosen {
        total_ms += p.ms as u64;
        let t = by_track.entry(&p.uri).or_insert_with(|| Ranked { name: p.name.clone(), detail: p.artist.clone(), image: p.image.clone(), id: p.uri.clone(), ..Default::default() });
        t.plays += 1;
        t.minutes += p.ms as u64;
        let main = p.artist.split(", ").next().unwrap_or_default().to_string();
        let key = if p.artist_id.is_empty() { main.clone() } else { p.artist_id.clone() };
        let a = by_artist.entry(key).or_insert_with(|| Ranked { name: main, image: p.image.clone(), id: p.artist_id.clone(), ..Default::default() });
        a.plays += 1;
        a.minutes += p.ms as u64;
        let local = p.at as i64 + offset_secs;
        hours[(local.rem_euclid(86_400) / 3600) as usize] += p.ms as u64;
        let (_, m, d) = civil(p.at, offset_secs);
        if range != Range::All {
            let slot = if range == Range::Month { d - 1 } else { m - 1 } as usize;
            timeline[slot] += p.ms as u64 / 60_000;
        }
    }
    let rank = |map: Vec<Ranked>| {
        let mut v: Vec<Ranked> = map.into_iter().map(|mut r| { r.minutes /= 60_000; r }).collect();
        v.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.minutes.cmp(&a.minutes)).then(a.name.cmp(&b.name)));
        v
    };
    let tracks = by_track.len();
    let artists = by_artist.len();
    let mut top_tracks = rank(by_track.into_values().collect());
    let mut top_artists = rank(by_artist.into_values().collect());
    top_tracks.truncate(TOP);
    top_artists.truncate(TOP);
    let top_hour = hours.iter().enumerate().filter(|(_, ms)| **ms > 0).max_by_key(|(_, ms)| **ms).map(|(h, _)| h as u32);
    if range == Range::All {
        timeline.clear();
    }
    Stats { minutes: total_ms / 60_000, plays: chosen.len(), tracks, artists, top_artists, top_tracks, timeline, top_hour }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(name: &str, artist: &str, ms: u32, at: u64) -> Play {
        Play { uri: format!("spotify:track:{name}"), name: name.into(), artist: artist.into(), artist_id: artist.into(), album: String::new(), image: None, ms, at }
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0, 0), (1970, 1, 1));
        // 29/10/2026 12:00 UTC.
        assert_eq!(civil(1_793_275_200, 0), (2026, 10, 29));
        // No fuso de Brasília, 1h UTC do dia 1º ainda é o mês anterior.
        assert_eq!(civil(1_790_816_400, -3 * 3600), (2026, 9, 30));
    }

    #[test]
    fn ranks_by_plays() {
        let now = 1_793_275_200;
        let plays = [play("a", "X", 180_000, now - 100), play("a", "X", 180_000, now - 50), play("b", "Y", 240_000, now - 10), play("old", "Z", 60_000, 1_600_000_000)];
        let s = compute(&plays, Range::Month, now, 0);
        assert_eq!((s.plays, s.tracks, s.artists, s.minutes), (3, 2, 2, 10));
        assert_eq!(s.top_tracks[0].name, "a");
        assert_eq!(s.top_artists[0].name, "X");
        assert_eq!(s.timeline[28], 10);
        assert_eq!(compute(&plays, Range::All, now, 0).plays, 4);
    }
}
