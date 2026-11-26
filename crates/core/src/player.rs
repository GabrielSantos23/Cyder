//! Contrato entre o engine e quem toca: `PlayerCmd` de lá para cá, `PlayerOut` de volta.
//! O Spotify toca só pelo Soloist (o player oficial, pela Web API): o player interno sobre o librespot saiu na 0.10.
//! Pelo Spotify, aqui só fica a sessão conectada (letras, metadados, playlists); o Apple Music tem o player dele (apple.rs).
use std::time::Duration;

use librespot::core::{authentication::Credentials, session::Session};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::Track;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Repeat {
    Off,
    All,
    One,
}

pub enum PlayerCmd {
    /// Mostra a fila; só começa a tocar (e só toma a reprodução de outro aparelho) no primeiro play,
    /// a partir de `position_ms` (onde parou quando o app fechou).
    Load { queue: Vec<Track>, index: usize, context: Option<String>, position_ms: u32 },
    /// Toca já. Com `context` (álbum, playlist), o Spirc carrega pela URI; `queue` pode vir vazia.
    /// `start_ms`: começa daí (episódio que ficou pela metade).
    /// `shuffle_start`: começar o aleatório sem faixa escolhida, para o Spirc sortear (ver `start`).
    Play { queue: Vec<Track>, index: usize, context: Option<String>, start_ms: u32, shuffle_start: bool },
    Toggle,
    Next,
    Prev,
    Seek(u32),
    SetShuffle(bool),
    SetRepeat(Repeat),
    /// 0..=65535, como o mixer do librespot.
    SetVolume(u16),
    /// Faixa posta na fila do Spotify: entra na nossa lista na mesma posição, para o índice bater.
    Insert { at: usize, track: Track },
    /// Fila reorganizada à mão: recarrega como lista de faixas, na mesma música e posição.
    Reload { queue: Vec<Track>, index: usize, position_ms: u32 },
    /// Religa o player com as configurações salvas (qualidade, nome, cache…), na mesma fila e posição.
    Restart,
    /// Reorganiza as próximas do Spirc (a ordem do aleatório) sem recarregar: `from_uri` vai para o lugar de `to_uri`.
    MoveNext { from_uri: String, to_uri: String },
    /// Outro aparelho está tocando a faixa `index` da fila: o próximo play por aqui continua dela, nesse contexto.
    Follow { index: usize, context: Option<String>, position_ms: u32 },
    /// Estado do MusicKit, só para a saída Apple Music (apple.rs).
    Apple(crate::apple::AppleState),
}

pub enum PlayerOut {
    /// O Spirc subiu e conectou a sessão (de novo, depois de uma queda: a sessão antiga não serve mais).
    Connected(Session),
    /// Faixa que não estava na fila (outro aparelho mandou tocar, ou autoplay no fim da fila).
    TrackAdded(Track),
    TrackChanged(usize),
    Playback { playing: bool, position_ms: u32 },
    Modes { shuffle: bool, repeat: Repeat, volume: u16 },
    Error(String),
    /// Parou ou pausou: talvez outro aparelho tenha assumido. O engine confere na Web API.
    Idle,
    /// Sem conexão com o Spotify: o que está acontecendo (reconectando, o erro da última tentativa).
    Offline(String),
    /// Várias faixas seguidas não carregaram: quase sempre o Spotify recusando a chave de áudio desta conta.
    Refused,
}


/// Conecta a sessão do Spotify, tentando de novo até conseguir. Sem o Spirc, ninguém mais conecta.
pub(crate) async fn connect_session(mut session: Session, mut credentials: Credentials, out: &UnboundedSender<PlayerOut>) -> Session {
    loop {
        match session.connect(credentials.clone(), true).await {
            Ok(()) => return session,
            Err(e) => {
                let _ = out.send(PlayerOut::Offline(e.to_string()));
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Ok((s, c)) = crate::auth::audio_session(&crate::config::cache_dir()).await {
                    (session, credentials) = (s, c);
                }
            }
        }
    }
}

/// Saída Spotify: só a sessão. Com o Soloist pronto, o engine nem chega aqui com o play; sem ele, o play
/// responde `Refused`, e o engine mostra como ligar o Soloist.
pub async fn run(session: Session, credentials: Credentials, volume: u16, mut cmds: UnboundedReceiver<PlayerCmd>, out: UnboundedSender<PlayerOut>) {
    let session = connect_session(session, credentials, &out).await;
    let _ = out.send(PlayerOut::Connected(session));
    // O engine só sabe do aleatório, do repetir e do volume pelo `Modes` daqui (como o player antigo fazia): sem
    // ele, o play no Soloist ia com o aleatório que a pessoa ligou desligado.
    let (mut shuffle, mut repeat, mut volume) = (false, Repeat::Off, volume);
    let _ = out.send(PlayerOut::Modes { shuffle, repeat, volume });
    while let Some(cmd) = cmds.recv().await {
        match cmd {
            PlayerCmd::Play { .. } | PlayerCmd::Toggle => drop(out.send(PlayerOut::Refused)),
            PlayerCmd::SetShuffle(on) => shuffle = on,
            PlayerCmd::SetRepeat(r) => repeat = r,
            PlayerCmd::SetVolume(v) => volume = v,
            _ => continue,
        }
        let _ = out.send(PlayerOut::Modes { shuffle, repeat, volume });
    }
}
