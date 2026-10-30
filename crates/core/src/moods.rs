//! Prateleiras da tela inicial por hora e dia da semana, com playlists editoriais do Spotify.
//! O feed pronto da Home não chega a este client (doc 11 §7). As listas ficam em `home-moods.json`, na raiz
//! do repositório: o app embute uma cópia e baixa a do GitHub uma vez por dia (engine.rs), então trocar uma
//! playlist é editar o arquivo, sem versão nova. Os nomes e capas vêm da sessão na hora.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A cópia que vai junto com o executável: vale até a do GitHub chegar (e se ela não chegar).
pub const BUNDLED: &str = include_str!("../../../home-moods.json");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mood {
    pub title: String,
    pub playlists: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Moods {
    pub moods: HashMap<String, Mood>,
    /// Revezam um dia por vez, três de cada vez.
    pub extras: Vec<String>,
}

impl Moods {
    pub fn bundled() -> Self {
        serde_json::from_str(BUNDLED).expect("home-moods.json embutido")
    }

    /// As prateleiras de agora, na ordem da tela: a do momento, as mais tocadas do Brasil e três que revezam
    /// por dia. `local_secs`: segundos Unix já no fuso local. Chave que falta no arquivo só some da tela.
    pub fn for_now(&self, local_secs: i64) -> Vec<Mood> {
        let day = local_secs.div_euclid(86_400) as usize;
        let hour = local_secs.rem_euclid(86_400) as u64 / 3600;
        // 1/1/1970 foi uma quinta-feira.
        let weekday = (day as u64 + 4) % 7;
        let extras = (0..3.min(self.extras.len())).map(|i| self.extras[(day + i) % self.extras.len()].as_str());
        [moment(weekday, hour), "em_alta"].into_iter().chain(extras).filter_map(|key| self.moods.get(key).cloned()).collect()
    }
}

/// A chave da prateleira do momento: o que combina com agora. `weekday`: 0 = domingo.
fn moment(weekday: u64, hour: u64) -> &'static str {
    match (weekday, hour) {
        (5, 17..) | (6, ..4) => "sextou",
        (6, 12..) | (0, ..4) => "sabado",
        _ if !(5..23).contains(&hour) => "dormir",
        _ if hour < 11 => "manha",
        (0 | 6, _) if hour < 18 => "fim_de_semana",
        _ if hour < 18 => "foco",
        _ => "noite",
    }
}

/// A ordem das playlists de uma prateleira no dia: começa num ponto diferente a cada dia.
pub fn of_day(playlists: &[String], local_secs: i64) -> Vec<String> {
    let start = local_secs.div_euclid(86_400) as usize % playlists.len().max(1);
    playlists[start..].iter().chain(&playlists[..start]).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 30/10/2026 (sexta) 00:00 UTC.
    const FRIDAY: i64 = 1_793_318_400;

    fn title(secs: i64) -> String {
        Moods::bundled().for_now(secs)[0].title.clone()
    }

    #[test]
    fn moment_follows_hour_and_weekday() {
        assert_eq!(title(FRIDAY + 8 * 3600), "Para começar o dia");
        assert_eq!(title(FRIDAY + 14 * 3600), "Para focar");
        assert_eq!(title(FRIDAY + 20 * 3600), "Sextou!");
        assert_eq!(title(FRIDAY + 86_400 + 2 * 3600), "Sextou!");
        assert_eq!(title(FRIDAY + 86_400 + 21 * 3600), "Sábado à noite");
        assert_eq!(title(FRIDAY + 2 * 86_400 + 15 * 3600), "Fim de semana em casa");
        assert_eq!(title(FRIDAY + 3 * 86_400 + 20 * 3600), "Para relaxar à noite");
        assert_eq!(title(FRIDAY + 3 * 86_400 + 23 * 3600), "Para dormir");
    }

    #[test]
    fn extras_and_order_change_by_day() {
        let moods = Moods::bundled();
        let titles = |secs| moods.for_now(secs).into_iter().map(|m| m.title).collect::<Vec<_>>();
        let (today, tomorrow) = (titles(FRIDAY), titles(FRIDAY + 86_400));
        assert_eq!(today.len(), 5);
        assert_ne!(today[2..], tomorrow[2..]);
        let treino = &moods.moods["treino"].playlists;
        assert_ne!(of_day(treino, FRIDAY), of_day(treino, FRIDAY + 86_400));
        assert_eq!(of_day(treino, FRIDAY).len(), treino.len());
    }

    #[test]
    fn missing_keys_are_skipped() {
        let moods: Moods = serde_json::from_str(r#"{"moods":{"em_alta":{"title":"Em alta","playlists":[]}},"extras":["nada"]}"#).unwrap();
        assert_eq!(moods.for_now(FRIDAY).len(), 1);
    }
}
