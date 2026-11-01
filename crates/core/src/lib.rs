//! Core do app: Spotify (áudio via librespot, Web API com Client ID do usuário), letras, imagens.
//! Não depende de UI (doc 03): a UI só manda `Cmd` e recebe `AppEvent`.
pub mod api;
pub mod auth;
pub mod cache;
pub mod config;
pub mod engine;
pub mod fx;
pub mod history;
pub mod images;
pub mod internal;
pub mod lastfm;
pub mod listen;
pub mod lyrics;
pub mod moods;
pub mod player;
pub mod soloist;
pub mod stats;
pub mod update;
mod sink;

use serde::{Deserialize, Serialize};

pub use engine::{AppEvent, Cmd, start};

/// Faixa com só o que a UI e as letras usam.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub album_id: String,
    pub duration_ms: u32,
    pub cover_small: Option<String>,
    pub cover_large: Option<String>,
    /// (id, nome) de cada artista, para abrir a página dele. Vazio em caches antigos.
    #[serde(default)]
    pub artists: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Album,
    Artist,
    Playlist,
    /// Podcast.
    Show,
}

/// Item de grade da biblioteca: álbum, artista, playlist ou podcast.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Card {
    pub kind: Kind,
    pub id: String,
    pub uri: String,
    pub title: String,
    pub subtitle: String,
    /// Imagem de ~300 px.
    pub image: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Library {
    pub albums: Vec<Card>,
    pub artists: Vec<Card>,
    pub playlists: Vec<Card>,
    /// Podcasts salvos. Caches antigos não têm.
    #[serde(default)]
    pub shows: Vec<Card>,
    /// Quando cada item entrou na biblioteca (URI → segundos Unix), para a sidebar mostrar os mais novos primeiro.
    /// Fica sem os que o Spotify não informa; caches antigos não têm nenhum.
    #[serde(default)]
    pub added: std::collections::HashMap<String, u64>,
}

/// Lançamento de um artista seguido, para a tela Novidades. `card.subtitle` é o artista.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewRelease {
    pub card: Card,
    /// Lançamento, em segundos Unix.
    pub date: i64,
    /// Single ou EP; senão, álbum.
    pub single: bool,
}

/// Créditos de uma faixa: quem fez o quê, a gravadora e os direitos.
#[derive(Clone, Debug, Default)]
pub struct Credits {
    pub title: String,
    /// (papel, nomes): "Artista principal", "Participação", "Remix", "Composição"…
    pub roles: Vec<(String, Vec<String>)>,
    pub label: String,
    pub copyrights: Vec<String>,
    /// "24 de outubro de 2026", ou só o ano quando o Spotify não tem o dia.
    pub released: String,
}

/// Tipo de resultado da busca; também é o filtro da tela de busca.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchKind {
    Tracks,
    Artists,
    Albums,
    Playlists,
    Shows,
}

impl SearchKind {
    pub const ALL: [SearchKind; 5] = [SearchKind::Tracks, SearchKind::Artists, SearchKind::Albums, SearchKind::Playlists, SearchKind::Shows];

    /// Nome no parâmetro `type` de /search.
    pub fn api(self) -> &'static str {
        match self {
            SearchKind::Tracks => "track",
            SearchKind::Artists => "artist",
            SearchKind::Albums => "album",
            SearchKind::Playlists => "playlist",
            SearchKind::Shows => "show",
        }
    }
}

/// Resultado de uma busca, separado por tipo, na ordem de relevância do Spotify.
#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub query: String,
    pub tracks: Vec<Track>,
    pub artists: Vec<Card>,
    pub albums: Vec<Card>,
    pub playlists: Vec<Card>,
    pub shows: Vec<Card>,
}

impl SearchResults {
    /// Junta a continuação de uma busca.
    pub fn extend(&mut self, more: SearchResults) {
        self.tracks.extend(more.tracks);
        self.artists.extend(more.artists);
        self.albums.extend(more.albums);
        self.playlists.extend(more.playlists);
        self.shows.extend(more.shows);
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.artists.is_empty() && self.albums.is_empty() && self.playlists.is_empty() && self.shows.is_empty()
    }
}

/// Página de álbum, artista ou playlist.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub card: Card,
    /// Linha de detalhes: "2025 · 12 faixas".
    pub meta: String,
    pub tracks: Vec<Track>,
    /// Discografia, na página de artista.
    pub albums: Vec<Card>,
    /// Aviso quando não dá para listar as faixas (playlist de outra pessoa).
    pub note: Option<String>,
    /// Foto de quem criou a playlist.
    #[serde(default)]
    pub owner_image: Option<String>,
    /// Playlist grande carregada aos poucos: de onde continuar (posição na playlist). None: já veio tudo.
    #[serde(default)]
    pub more_from: Option<usize>,
    /// Itens na playlist inteira (0: igual às faixas carregadas).
    #[serde(default)]
    pub total: usize,
    /// Só na página de artista; `tracks` são as mais tocadas.
    #[serde(default)]
    pub artist: Option<ArtistInfo>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ArtistInfo {
    /// Foto grande (640 px) para o topo.
    pub hero: Option<String>,
    /// `albums` traz primeiro os álbuns e depois singles e EPs; este é o número de álbuns.
    pub album_count: usize,
    /// Lançamento mais recente e a data dele por extenso ("14 de maio de 2026").
    pub latest: Option<(Card, String)>,
    /// Artistas parecidos ("Fãs também curtem"), pela sessão. Caches antigos não têm.
    #[serde(default)]
    pub related: Vec<Card>,
    /// Playlists com o artista: o rádio dele e as que a busca acha pelo nome. Caches antigos não têm.
    #[serde(default)]
    pub featuring: Vec<Card>,
}

/// Aparelho Spotify Connect da conta.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// "Computer", "Smartphone", "Speaker"…
    pub kind: String,
    pub active: bool,
}

/// Prateleira da tela inicial: título e cartões.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shelf {
    pub title: String,
    pub cards: Vec<Card>,
}

/// Fuso local em segundos (hora local menos UTC), para as estatísticas contarem os dias certos
/// e a tela inicial escolher as prateleiras da hora.
#[cfg(windows)]
pub fn local_offset_secs() -> i64 {
    use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    // SAFETY: as duas só escrevem na struct passada.
    let (local, utc) = unsafe {
        let (mut l, mut u) = (std::mem::zeroed(), std::mem::zeroed());
        GetLocalTime(&mut l);
        GetSystemTime(&mut u);
        (l, u)
    };
    let mins = |t: &windows_sys::Win32::Foundation::SYSTEMTIME| (t.wDay as i64 * 24 + t.wHour as i64) * 60 + t.wMinute as i64;
    let mut diff = mins(&local) - mins(&utc);
    // Virada de mês entre os dois relógios: o dia "1" de um contra o "31" do outro.
    if diff > 14 * 60 {
        diff -= (local.wDay as i64) * 24 * 60;
    } else if diff < -14 * 60 {
        diff += (utc.wDay as i64) * 24 * 60;
    }
    // Arredonda a 15 minutos (os fusos existentes).
    ((diff as f64 / 15.).round() as i64) * 15 * 60
}

#[cfg(not(windows))]
pub fn local_offset_secs() -> i64 {
    0
}
