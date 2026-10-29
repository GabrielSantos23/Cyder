//! Cache em disco de respostas que mudam pouco (biblioteca, páginas, tela inicial): a tela abre na hora
//! com o que estava salvo e se atualiza em segundo plano.
use std::path::PathBuf;

use serde::{Serialize, de::DeserializeOwned};

use crate::config;

/// Páginas guardadas no máximo; passou disso, as mais antigas saem. Cada uma tem de ~5 a ~200 KB.
const MAX_PAGES: usize = 300;

fn path(name: &str) -> PathBuf {
    config::cache_dir().join("data").join(format!("{name}.json"))
}

pub fn load<T: DeserializeOwned>(name: &str) -> Option<T> {
    let text = std::fs::read_to_string(path(name)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Grava num arquivo temporário e renomeia: um cache pela metade nunca é lido.
pub fn save<T: Serialize>(name: &str, value: &T) {
    let to = path(name);
    let Some(dir) = to.parent() else { return };
    let Ok(text) = serde_json::to_string(value) else { return };
    let tmp = to.with_extension("part");
    let ok = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&tmp, text)).and_then(|_| std::fs::rename(&tmp, &to));
    if let Err(e) = ok {
        log::warn!("cache {name}: {e}");
    }
}

/// Guardado há menos de `max`: não vale a pena perguntar de novo ao Spotify.
pub fn fresh(name: &str, max: std::time::Duration) -> bool {
    std::fs::metadata(path(name)).and_then(|m| m.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age < max)
}

pub fn page_name(id: &str) -> String {
    format!("page-{id}")
}

/// Apaga as páginas mais antigas quando passam do limite.
pub fn prune_pages() {
    let Ok(entries) = std::fs::read_dir(config::cache_dir().join("data")) else { return };
    let mut pages: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("page-"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if pages.len() <= MAX_PAGES {
        return;
    }
    pages.sort_by_key(|(at, _)| *at);
    for (_, p) in pages.iter().take(pages.len() - MAX_PAGES) {
        let _ = std::fs::remove_file(p);
    }
}
