//! Saída Apple Music (doc 13): o lugar do player do librespot, com os mesmos `PlayerCmd` e `PlayerOut`.
//! O engine continua igual (fila, letra, estatísticas); quem toca é o MusicKit do WebView2 do app, que recebe
//! `AppEvent::Apple` e devolve o estado por `Cmd::Apple`. Cada faixa do Spotify é achada na Apple pelo ISRC.
use std::{collections::HashMap, time::{Duration, Instant}};

use librespot::core::{authentication::Credentials, session::Session};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{
    AppEvent, Track,
    player::{PlayerCmd, PlayerOut, Repeat},
};

/// O que o MusicKit contou por último (vem do app).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppleState {
    pub authorized: bool,
    pub playing: bool,
    pub position_ms: u32,
    pub duration_ms: u32,
    /// ID da música no catálogo da Apple.
    pub song: Option<String>,
    /// A música acabou.
    #[serde(default)]
    pub ended: bool,
    /// Loja da conta (`br`, `us`…): o ID de uma música só vale na loja dela.
    #[serde(default)]
    pub storefront: Option<String>,
    /// O MusicKit não conseguiu tocar (licença negada, música fora da loja…).
    #[serde(default)]
    pub error: Option<String>,
}

/// Pedido para o player do app.
#[derive(Clone, Debug, PartialEq)]
pub enum AppleCmd {
    Play { song: String, start_ms: u32 },
    Pause,
    Resume,
    Seek(u32),
    /// 0.0 a 1.0.
    Volume(f32),
}

/// Na troca de música, quantas seguidas sem par na Apple antes de desistir.
const MAX_MISSES: u32 = 5;
/// Diferença entre a posição do MusicKit e a esperada que conta como pulo (seek), não como atraso.
const DRIFT_MS: u32 = 2000;
/// Como no Spotify: depois de 3 s, o "anterior" volta ao começo da faixa.
const PREV_RESTART_MS: u32 = 3000;

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Entre as músicas de uma busca, a desta faixa: mesmo nome, mesmo artista e duração a até 3 s.
fn pick_by_name(songs: &serde_json::Value, t: &Track) -> Option<String> {
    let (name, artist) = (norm(&t.name), norm(&t.artist));
    songs["results"]["songs"]["data"].as_array()?.iter().find_map(|s| {
        let a = &s["attributes"];
        let same = norm(a["name"].as_str()?) == name
            && norm(a["artistName"].as_str()?).contains(&artist)
            && (a["durationInMillis"].as_u64()? as i64 - t.duration_ms as i64).abs() <= 3000;
        same.then(|| s["id"].as_str().map(String::from)).flatten()
    })
}

/// ID da Apple da faixa do Spotify: pelo ISRC (a mesma gravação); sem ele, pelo nome, artista e duração.
async fn find(http: &reqwest::Client, session: &Session, storefront: &str, t: &Track) -> Result<Option<String>, String> {
    if let Some(isrc) = crate::lyrics::isrc(session, t).await {
        let songs = crate::motion::catalog_in(http, storefront, &format!("/songs?filter%5Bisrc%5D={isrc}")).await?;
        if let Some(id) = songs["data"][0]["id"].as_str() {
            return Ok(Some(id.into()));
        }
    }
    let term: String = format!("{} {}", t.name, t.artist).chars().filter(|c| !matches!(c, '&' | '?' | '#' | '%')).collect();
    let songs = crate::motion::catalog_in(http, storefront, &format!("/search?types=songs&limit=10&term={}", term.replace(' ', "+"))).await?;
    Ok(pick_by_name(&songs, t))
}

struct State {
    queue: Vec<Track>,
    index: usize,
    /// O primeiro play depois de carregar começa daqui.
    start_ms: u32,
    /// Já mandou esta faixa ao MusicKit.
    loaded: bool,
    playing: bool,
    shuffle: bool,
    repeat: Repeat,
    volume: u16,
    anchor: (u32, Instant),
    /// A faixa que o MusicKit deve estar tocando (o ID da Apple), para ignorar o estado de uma anterior.
    song: Option<String>,
    /// O MusicKit já tocou esta faixa: só então um "acabou" é dela (repetindo, o ID é o mesmo da vez anterior).
    heard: bool,
    misses: u32,
    /// Loja da conta, do MusicKit; até ela chegar, a faixa espera para ser procurada.
    storefront: Option<String>,
    authorized: bool,
    warned_login: bool,
    warned_preview: bool,
    out: UnboundedSender<PlayerOut>,
    ui: UnboundedSender<AppEvent>,
    resolve: UnboundedSender<(usize, String, Result<Option<String>, String>)>,
    ids: HashMap<String, Option<String>>,
    http: reqwest::Client,
    session: Session,
}

impl State {
    fn send(&self, ev: PlayerOut) {
        let _ = self.out.send(ev);
    }

    fn apple(&self, cmd: AppleCmd) {
        let _ = self.ui.send(AppEvent::Apple(cmd));
    }

    fn position(&self) -> u32 {
        self.anchor.0 + if self.playing { self.anchor.1.elapsed().as_millis() as u32 } else { 0 }
    }

    fn set_playback(&mut self, playing: bool, position_ms: u32) {
        (self.playing, self.anchor) = (playing, (position_ms, Instant::now()));
        self.send(PlayerOut::Playback { playing, position_ms });
    }

    fn send_modes(&self) {
        self.send(PlayerOut::Modes { shuffle: self.shuffle, repeat: self.repeat, volume: self.volume });
    }

    /// Toca a faixa `i` a partir de `ms`: acha o ID dela na Apple (do cache ou numa tarefa) e manda ao MusicKit.
    fn start(&mut self, i: usize, ms: u32) {
        let Some(t) = self.queue.get(i).cloned() else { return };
        (self.index, self.loaded, self.start_ms, self.song, self.heard) = (i, true, ms, None, false);
        self.send(PlayerOut::TrackChanged(i));
        self.set_playback(false, ms);
        let Some(storefront) = self.storefront.clone() else { return };
        match self.ids.get(&t.uri).cloned() {
            Some(id) => self.found(i, &t.uri, Ok(id)),
            None => {
                let (http, session, tx) = (self.http.clone(), self.session.clone(), self.resolve.clone());
                tokio::spawn(async move {
                    let r = find(&http, &session, &storefront, &t).await;
                    let _ = tx.send((i, t.uri, r));
                });
            }
        }
    }

    fn found(&mut self, i: usize, uri: &str, r: Result<Option<String>, String>) {
        if let Ok(id) = &r {
            self.ids.insert(uri.into(), id.clone());
        }
        // A pessoa já trocou de música enquanto procurava.
        if i != self.index || self.queue.get(i).is_none_or(|t| t.uri != uri) {
            return;
        }
        match r {
            Ok(Some(song)) => {
                self.misses = 0;
                self.song = Some(song.clone());
                self.apple(AppleCmd::Play { song, start_ms: self.start_ms });
            }
            Ok(None) | Err(_) => {
                let name = self.queue[i].name.clone();
                if let Err(e) = &r {
                    log::warn!("apple music: procurando {name}: {e}");
                }
                self.send(PlayerOut::Error(format!("“{name}” não está no Apple Music.")));
                self.misses += 1;
                if self.misses < MAX_MISSES {
                    self.advance(false);
                } else {
                    self.misses = 0;
                    self.set_playback(false, 0);
                }
            }
        }
    }

    /// Próxima faixa: `auto` quando a anterior acabou sozinha (o repetir uma volta nela).
    fn advance(&mut self, auto: bool) {
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let next = if auto && self.repeat == Repeat::One {
            Some(self.index)
        } else if self.shuffle && n > 1 {
            // shortcut: aleatório sorteia a cada faixa (pode repetir antes de tocar todas); guardar uma ordem se incomodar.
            let r = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
            Some((self.index + 1 + r % (n - 1)) % n)
        } else if self.index + 1 < n {
            Some(self.index + 1)
        } else {
            (self.repeat == Repeat::All).then_some(0)
        };
        match next {
            Some(i) => self.start(i, 0),
            // Acabou a lista.
            None => {
                self.apple(AppleCmd::Pause);
                self.set_playback(false, 0);
                self.loaded = false;
            }
        }
    }

    fn cmd(&mut self, cmd: PlayerCmd) {
        match cmd {
            PlayerCmd::Load { queue, index, position_ms, .. } | PlayerCmd::Reload { queue, index, position_ms } if !self.playing => {
                (self.queue, self.index, self.start_ms, self.loaded) = (queue, index, position_ms, false);
                self.set_playback(false, position_ms);
            }
            PlayerCmd::Load { queue, index, position_ms, .. } | PlayerCmd::Reload { queue, index, position_ms } => {
                let same = self.queue.get(self.index).map(|t| &t.uri) == queue.get(index).map(|t| &t.uri);
                self.queue = queue;
                if same {
                    self.index = index;
                } else {
                    self.start(index, position_ms);
                }
            }
            PlayerCmd::Play { queue, index, start_ms, shuffle_start, .. } => {
                if queue.is_empty() {
                    return self.send(PlayerOut::Error("Esta lista não toca pelo Apple Music: o Spotify não mostra as faixas dela.".into()));
                }
                self.queue = queue;
                let i = if shuffle_start { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize) % self.queue.len() } else { index };
                self.start(i, start_ms);
            }
            PlayerCmd::Toggle if !self.loaded => self.start(self.index, self.start_ms),
            PlayerCmd::Toggle => self.apple(if self.playing { AppleCmd::Pause } else { AppleCmd::Resume }),
            PlayerCmd::Next => self.advance(false),
            PlayerCmd::Prev if self.position() > PREV_RESTART_MS || self.index == 0 => {
                self.apple(AppleCmd::Seek(0));
                self.set_playback(self.playing, 0);
            }
            PlayerCmd::Prev => self.start(self.index - 1, 0),
            PlayerCmd::Seek(ms) if self.loaded => {
                self.apple(AppleCmd::Seek(ms));
                self.set_playback(self.playing, ms);
            }
            PlayerCmd::Seek(ms) => self.start_ms = ms,
            PlayerCmd::SetShuffle(on) => {
                self.shuffle = on;
                self.send_modes();
            }
            PlayerCmd::SetRepeat(r) => {
                self.repeat = r;
                self.send_modes();
            }
            PlayerCmd::SetVolume(v) => {
                self.volume = v;
                self.apple(AppleCmd::Volume(v as f32 / u16::MAX as f32));
                self.send_modes();
            }
            PlayerCmd::Insert { at, track } => {
                let at = at.min(self.queue.len());
                self.queue.insert(at, track);
                if at <= self.index && !self.queue.is_empty() && self.loaded {
                    self.index += 1;
                }
            }
            PlayerCmd::Follow { index, position_ms, .. } if !self.playing => (self.index, self.start_ms, self.loaded) = (index, position_ms, false),
            // A ordem do aleatório é sorteada aqui, e religar não muda nada no MusicKit.
            PlayerCmd::Follow { .. } | PlayerCmd::MoveNext { .. } | PlayerCmd::Restart => {}
            PlayerCmd::Apple(st) => self.state(st),
        }
    }

    /// Estado novo do MusicKit.
    fn state(&mut self, st: AppleState) {
        self.authorized = st.authorized;
        // A loja chegou: a faixa que esperava por ela é procurada agora.
        if self.storefront.is_none() && st.storefront.is_some() {
            self.storefront = st.storefront.clone();
            if self.loaded && self.song.is_none() {
                self.start(self.index, self.start_ms);
            }
        }
        // Sem pular sozinho: uma licença negada faria a fila inteira passar sem som.
        if let Some(e) = st.error {
            if self.song.is_some() {
                let name = self.queue.get(self.index).map_or(String::new(), |t| t.name.clone());
                let why = if e.contains("MEDIA_LICENSE") { "a Apple negou a licença".into() } else { e };
                self.send(PlayerOut::Error(format!("O Apple Music não tocou “{name}”: {why}.")));
                let at = self.position();
                self.set_playback(false, at);
            }
            return;
        }
        // Estado de outra música (a anterior, ainda saindo): não vale para esta.
        if self.song.is_none() || st.song != self.song {
            return;
        }
        if st.ended {
            return if std::mem::take(&mut self.heard) { self.advance(true) };
        }
        self.heard |= st.playing;
        if !self.warned_preview && st.duration_ms > 0 && st.duration_ms <= 90_000 && self.queue.get(self.index).is_some_and(|t| t.duration_ms > 120_000) {
            self.warned_preview = true;
            self.send(PlayerOut::Error("O Apple Music está tocando só a prévia. Confira a assinatura na janela do Apple Music.".into()));
        }
        if st.playing != self.playing || st.position_ms.abs_diff(self.position()) > DRIFT_MS {
            self.set_playback(st.playing, st.position_ms);
        }
    }
}

/// Conecta a sessão do Spotify (as letras, os metadados e a Web API usam) e fica tocando pelo Apple Music até o engine fechar o canal.
pub async fn run(session: Session, credentials: Credentials, volume: u16, mut cmds: UnboundedReceiver<PlayerCmd>, out: UnboundedSender<PlayerOut>, ui: UnboundedSender<AppEvent>) {
    let mut session = session;
    let mut credentials = credentials;
    // Sem o Spirc, ninguém mais conecta a sessão.
    loop {
        match session.connect(credentials.clone(), true).await {
            Ok(()) => break,
            Err(e) => {
                let _ = out.send(PlayerOut::Offline(e.to_string()));
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Ok((s, c)) = crate::auth::audio_session(&crate::config::cache_dir()).await {
                    (session, credentials) = (s, c);
                }
            }
        }
    }
    let _ = out.send(PlayerOut::Connected(session.clone()));
    let (resolve, mut resolved) = unbounded_channel();
    let mut state = State {
        queue: Vec::new(), index: 0, start_ms: 0, loaded: false, playing: false, shuffle: false, repeat: Repeat::Off, volume,
        anchor: (0, Instant::now()), song: None, heard: false, misses: 0, storefront: None, authorized: true, warned_login: false, warned_preview: false,
        out, ui, resolve, ids: HashMap::new(), http: reqwest::Client::new(), session,
    };
    state.apple(AppleCmd::Volume(volume as f32 / u16::MAX as f32));
    state.send_modes();
    loop {
        tokio::select! {
            cmd = cmds.recv() => match cmd {
                Some(cmd) => {
                    let wants_sound = matches!(cmd, PlayerCmd::Play { .. } | PlayerCmd::Toggle);
                    state.cmd(cmd);
                    if wants_sound && !state.authorized && !std::mem::replace(&mut state.warned_login, true) {
                        state.send(PlayerOut::Error("Entre com o seu Apple ID na janela do Apple Music.".into()));
                    }
                }
                // O engine trocou de saída ou saiu: a música do Apple Music para junto.
                None => break,
            },
            Some((i, uri, r)) = resolved.recv() => state.found(i, &uri, r),
        }
    }
    state.apple(AppleCmd::Pause);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str, artist: &str, ms: u32) -> Track {
        Track { id: String::new(), uri: String::new(), name: name.into(), artist: artist.into(), album: String::new(), album_id: String::new(), duration_ms: ms, cover_small: None, cover_large: None, artists: Vec::new() }
    }

    #[test]
    fn finds_the_same_song_by_name() {
        let songs = serde_json::json!({"results": {"songs": {"data": [
            {"id": "1", "attributes": {"name": "Housewife (Live)", "artistName": "Tate McRae", "durationInMillis": 230000}},
            {"id": "2", "attributes": {"name": "Housewife", "artistName": "Tate McRae", "durationInMillis": 208810}},
        ]}}});
        assert_eq!(pick_by_name(&songs, &track("Housewife", "Tate McRae", 209000)).as_deref(), Some("2"));
        assert_eq!(pick_by_name(&songs, &track("Housewife", "Tate McRae", 190000)), None);
    }
}
