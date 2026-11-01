//! Player sobre o librespot como dispositivo Spotify Connect (doc 02).
//! O Spirc é dono da fila, do aleatório e do repetir: o app vira um dispositivo da conta,
//! aparece como "Cyder" nos outros clients e pode ser controlado por eles.
use std::{future::Future, path::PathBuf, sync::Arc, time::{Duration, Instant}};

use librespot::connect::{ConnectConfig, LoadContextOptions, LoadRequest, LoadRequestOptions, Options, PlayingTrack, Spirc};
use librespot::core::{authentication::Credentials, config::DeviceType, session::Session};
use librespot::metadata::audio::{AudioItem, UniqueFields};
use librespot::playback::{
    audio_backend,
    config::{AudioFormat, Bitrate, PlayerConfig},
    mixer::{Mixer, MixerConfig, softmixer::SoftMixer},
    player::{Player, PlayerEvent},
};
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
    /// O player não conseguiu subir (login ou Connect); sem ele o app não funciona.
    Fatal(String),
    /// Parou ou pausou: talvez outro aparelho tenha assumido. O engine confere na Web API.
    Idle,
    /// Sem conexão com o Spotify: o que está acontecendo (reconectando, o erro da última tentativa).
    Offline(String),
}


struct State {
    spirc: Spirc,
    mixer: Arc<SoftMixer>,
    queue: Vec<Track>,
    index: usize,
    context: Option<String>,
    /// Onde começar no primeiro play depois de abrir o app.
    start_ms: u32,
    /// O próximo `start` vem do botão Aleatório: sem faixa escolhida.
    shuffle_start: bool,
    /// O Spirc ainda não recebeu a fila: o app só está mostrando a primeira faixa.
    loaded: bool,
    playing: bool,
    shuffle: bool,
    repeat: Repeat,
    /// Última posição conhecida e quando: para voltar ao mesmo ponto se a sessão cair.
    anchor: (u32, Instant),
    /// O fim da música está abaixando (transição suave): a próxima entra subindo.
    fading: bool,
    /// Entre o `load` e a faixa começar: os avisos de aleatório e repetir que chegam aí são velhos (veja `start`).
    settling: bool,
    out: UnboundedSender<PlayerOut>,
}

/// Monta a faixa a partir do que o librespot sabe; usada para faixas fora da fila.
fn track_from_item(item: &AudioItem) -> Track {
    let (artist, album) = match &item.unique_fields {
        UniqueFields::Track { artists, album, .. } => (artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "), album.clone()),
        UniqueFields::Local { artists, album, .. } => (artists.clone().unwrap_or_default(), album.clone().unwrap_or_default()),
        UniqueFields::Episode { show_name, .. } => (show_name.clone(), String::new()),
    };
    let by_width = |largest: bool| {
        let mut covers: Vec<_> = item.covers.iter().collect();
        covers.sort_by_key(|c| c.width);
        if largest { covers.last().map(|c| c.url.clone()) } else { covers.first().map(|c| c.url.clone()) }
    };
    let cover_large = by_width(true);
    Track {
        id: item.uri.rsplit(':').next().unwrap_or_default().to_string(),
        uri: item.uri.clone(),
        name: item.name.clone(),
        artist,
        album,
        // Sem id de álbum aqui: a última parte da URL da capa identifica a imagem para o cache.
        album_id: cover_large.as_deref().and_then(|u| u.rsplit('/').next()).unwrap_or(&item.uri).to_string(),
        // O engine completa com os ids reais (artista e álbum) pela sessão.
        artists: Vec::new(),
        duration_ms: item.duration_ms,
        cover_small: by_width(false),
        cover_large,
    }
}

impl State {
    fn send(&self, ev: PlayerOut) {
        let _ = self.out.send(ev);
    }

    fn send_modes(&self) {
        self.send(PlayerOut::Modes { shuffle: self.shuffle, repeat: self.repeat, volume: self.mixer.volume() });
    }

    fn report(&self, r: Result<(), librespot::core::Error>) {
        if let Err(e) = r {
            self.send(PlayerOut::Error(format!("O Spotify recusou o comando: {e}")));
        }
    }

    /// Transição suave: perto do fim, abaixa até o silêncio; a próxima música sobe (no TrackChanged).
    fn fade_tick(&mut self) {
        let ms = crate::fx::get().fade_ms;
        let Some(duration) = self.queue.get(self.index).map(|t| t.duration_ms) else { return };
        if ms == 0 || !self.playing || !self.loaded {
            return;
        }
        let position = self.anchor.0 + self.anchor.1.elapsed().as_millis() as u32;
        if !self.fading && duration > ms * 3 && position + ms >= duration && position < duration {
            self.fading = true;
            crate::fx::fade(1., 0., duration - position);
        } else if self.fading && position + ms + 1000 < duration {
            // Voltou para trás (seek, repetir esta música): sobe de novo.
            self.end_fade(ms);
        }
    }

    fn end_fade(&mut self, ms: u32) {
        if std::mem::take(&mut self.fading) {
            crate::fx::fade(0., 1., ms.max(300));
        }
    }

    /// Entrega a fila ao Spirc. Isto torna o app o dispositivo ativo da conta.
    fn start(&mut self, play: bool) {
        // Nada escolhido ainda (primeira abertura): tecla de mídia ou Espaço não têm o que tocar.
        if self.queue.is_empty() && self.context.is_none() {
            return;
        }
        let playing = self.queue.get(self.index).map(|t| t.uri.clone());
        let options = LoadRequestOptions {
            start_playing: play,
            seek_to: std::mem::take(&mut self.start_ms),
            context_options: Some(LoadContextOptions::Options(Options {
                shuffle: self.shuffle,
                repeat: self.repeat == Repeat::All,
                repeat_track: self.repeat == Repeat::One,
            })),
            // Pela URI: o contexto do Spotify pode ter faixas que a nossa lista não tem.
            // No Aleatório, nenhuma: o Spirc procura a faixa pedida só na parte da playlist que já carregou e, sem
            // achar, começava pela primeira. Sem faixa pedida, ele sorteia ali mesmo e embaralha o resto.
            playing_track: if std::mem::take(&mut self.shuffle_start) { None } else { playing.map(PlayingTrack::Uri) },
        };
        let request = match &self.context {
            Some(uri) => LoadRequest::from_context_uri(uri.clone(), options),
            None => LoadRequest::from_tracks(self.queue.iter().map(|t| t.uri.clone()).collect(), options),
        };
        // Ativar o aparelho faz o Spirc avisar o aleatório e o repetir de antes do `load` (desligados, na primeira
        // vez); o `load` aplica os nossos sem avisar. Sem ignorar esses avisos, o aleatório ligado antes do play
        // aparecia desligado na tela enquanto a lista tocava embaralhada.
        self.settling = true;
        let r = self.spirc.activate().and_then(|_| self.spirc.load(request));
        self.loaded = r.is_ok();
        self.settling = self.loaded;
        self.report(r);
    }

    /// Antes do primeiro play, anterior/próxima só mudam a faixa mostrada.
    fn browse(&mut self, index: usize) {
        if index < self.queue.len() {
            if index != self.index {
                self.start_ms = 0;
            }
            self.index = index;
            self.send(PlayerOut::TrackChanged(index));
        }
    }

    fn cmd(&mut self, cmd: PlayerCmd) {
        match cmd {
            PlayerCmd::Load { queue, index, context, position_ms } => {
                self.queue = queue;
                self.context = context;
                self.loaded = false;
                self.index = index;
                self.start_ms = position_ms;
                self.browse(index);
            }
            PlayerCmd::Play { queue, index, context, start_ms, shuffle_start } => {
                self.queue = queue;
                self.index = index;
                self.context = context;
                self.start_ms = start_ms;
                self.shuffle_start = shuffle_start;
                self.start(true);
            }
            PlayerCmd::Toggle if !self.loaded => self.start(true),
            PlayerCmd::Toggle => self.report(self.spirc.play_pause()),
            // Passar de faixa também toca, como no Spotify, mesmo se estava pausado.
            PlayerCmd::Next if !self.loaded => {
                self.browse(self.index + 1);
                self.start(true);
            }
            PlayerCmd::Next => {
                log::info!("próxima pelo Spirc (faixa {} de {})", self.index + 1, self.queue.len());
                self.report(self.spirc.next());
                if !self.playing {
                    self.report(self.spirc.play());
                }
            }
            PlayerCmd::Prev if !self.loaded => {
                self.browse(self.index.saturating_sub(1));
                self.start(true);
            }
            PlayerCmd::Prev => {
                self.report(self.spirc.prev());
                if !self.playing {
                    self.report(self.spirc.play());
                }
            }
            // Encerrar o Spirc cai no mesmo caminho da sessão que caiu: reconecta lendo as configurações de novo.
            PlayerCmd::Restart => {
                let _ = self.spirc.shutdown();
            }
            // Sem isto, o play seguinte recarregava a fila e o contexto antigos: voltava ao começo da playlist
            // ou tocava outra faixa. Com o Spirc carregado, quem manda é ele.
            PlayerCmd::Follow { index, context, position_ms } if !self.loaded => {
                (self.index, self.context, self.start_ms) = (index.min(self.queue.len().saturating_sub(1)), context, position_ms);
            }
            PlayerCmd::Follow { .. } => {}
            PlayerCmd::MoveNext { from_uri, to_uri } if self.loaded => self.report(self.spirc.move_next_track(from_uri, to_uri)),
            PlayerCmd::MoveNext { .. } => {}
            PlayerCmd::Insert { at, track } => {
                let at = at.min(self.queue.len());
                self.queue.insert(at, track);
                if at <= self.index && !self.queue.is_empty() {
                    self.index += 1;
                }
            }
            PlayerCmd::Reload { queue, index, position_ms } => {
                self.queue = queue;
                self.index = index;
                self.context = None;
                self.start_ms = position_ms;
                // Antes do primeiro play, só guarda: o play carrega a lista nova.
                if self.loaded {
                    let playing = self.playing;
                    self.start(playing);
                }
            }
            PlayerCmd::Seek(ms) if self.loaded => self.report(self.spirc.set_position_ms(ms)),
            PlayerCmd::Seek(_) => {}
            PlayerCmd::SetShuffle(on) => {
                self.shuffle = on;
                if self.loaded { self.report(self.spirc.shuffle(on)) }
                self.send_modes();
            }
            PlayerCmd::SetRepeat(r) => {
                self.repeat = r;
                if self.loaded {
                    self.report(self.spirc.repeat(r == Repeat::All).and_then(|_| self.spirc.repeat_track(r == Repeat::One)));
                }
                self.send_modes();
            }
            PlayerCmd::SetVolume(v) => {
                // Pelo Spirc, para os outros aparelhos verem o volume novo.
                self.report(self.spirc.set_volume(v));
                self.mixer.set_volume(v);
                self.send_modes();
            }
        }
    }

    fn event(&mut self, ev: PlayerEvent) {
        match ev {
            PlayerEvent::TrackChanged { audio_item } => {
                // Outro aparelho pode ter mandado tocar algo que não está na fila.
                self.loaded = true;
                self.settling = false;
                // Procura da faixa atual para a frente: a mesma música pode estar na fila mais de uma vez.
                let from = self.index.min(self.queue.len());
                let found = self.queue[from..].iter().position(|t| t.uri == audio_item.uri).map(|p| p + from)
                    .or_else(|| self.queue.iter().position(|t| t.uri == audio_item.uri));
                let index = match found {
                    Some(i) => i,
                    None => {
                        let track = track_from_item(&audio_item);
                        self.queue.push(track.clone());
                        self.send(PlayerOut::TrackAdded(track));
                        self.queue.len() - 1
                    }
                };
                log::info!("tocando {} (faixa {} de {})", audio_item.uri, index + 1, self.queue.len());
                self.index = index;
                self.end_fade(crate::fx::get().fade_ms);
                self.send(PlayerOut::TrackChanged(index));
            }
            PlayerEvent::Playing { position_ms, .. } => {
                self.playing = true;
                self.anchor = (position_ms, Instant::now());
                self.send(PlayerOut::Playback { playing: true, position_ms });
            }
            PlayerEvent::Paused { position_ms, .. } => {
                self.playing = false;
                self.anchor = (position_ms, Instant::now());
                self.send(PlayerOut::Playback { playing: false, position_ms });
                self.send(PlayerOut::Idle);
            }
            PlayerEvent::PositionCorrection { position_ms, .. } | PlayerEvent::Seeked { position_ms, .. } => {
                self.anchor = (position_ms, Instant::now());
                self.send(PlayerOut::Playback { playing: self.playing, position_ms });
            }
            PlayerEvent::Stopped { .. } => {
                // Outro aparelho assumiu a reprodução.
                self.playing = false;
                self.loaded = false;
                self.send(PlayerOut::Playback { playing: false, position_ms: 0 });
                self.send(PlayerOut::Idle);
            }
            PlayerEvent::ShuffleChanged { .. } | PlayerEvent::RepeatChanged { .. } if self.settling => {}
            PlayerEvent::ShuffleChanged { shuffle } => {
                self.shuffle = shuffle;
                self.send_modes();
            }
            PlayerEvent::RepeatChanged { context, track } => {
                self.repeat = if track { Repeat::One } else if context { Repeat::All } else { Repeat::Off };
                self.send_modes();
            }
            PlayerEvent::VolumeChanged { .. } => self.send_modes(),
            PlayerEvent::Unavailable { .. } => {
                let name = self.queue.get(self.index).map(|t| t.name.clone()).unwrap_or_default();
                self.send(PlayerOut::Error(format!("\"{name}\" não está disponível na sua região. Pulando.")));
            }
            _ => {}
        }
    }
}

/// Cria o player e o dispositivo Connect. O futuro devolvido é o laço do Spirc; precisa rodar junto.
async fn connect(session: Session, credentials: Credentials, volume: u16) -> Result<(Spirc, impl Future<Output = ()>, Arc<SoftMixer>, Arc<Player>), String> {
    // A saída do librespot fica de reserva, se a nossa não abrir.
    let backend = audio_backend::find(None).ok_or("Nenhuma saída de áudio encontrada.")?;
    let mixer = Arc::new(SoftMixer::open(MixerConfig::default()).map_err(|e| format!("Controle de volume indisponível: {e}"))?);
    mixer.set_volume(volume);
    let settings = crate::config::settings();
    let bitrate = match settings.quality {
        crate::config::Quality::Normal => Bitrate::Bitrate96,
        crate::config::Quality::High => Bitrate::Bitrate160,
        crate::config::Quality::VeryHigh => Bitrate::Bitrate320,
    };
    let player_config = PlayerConfig { bitrate, gapless: settings.gapless, normalisation: settings.normalize, ..PlayerConfig::default() };
    let player = Player::new(player_config, session.clone(), mixer.get_soft_volume(), move || match crate::sink::open() {
        Ok(sink) => Box::new(sink) as Box<dyn audio_backend::Sink>,
        Err(e) => {
            log::warn!("saída de áudio própria falhou ({e}); usando a do librespot");
            backend(None, AudioFormat::default())
        }
    });
    let config = ConnectConfig {
        name: settings.device_name(),
        device_type: DeviceType::Computer,
        initial_volume: volume,
        ..Default::default()
    };
    let (spirc, task) = Spirc::new(config, session, credentials, player.clone(), mixer.clone())
        .await
        .map_err(|e| format!("Não deu para registrar o Cyder no Spotify Connect: {e}"))?;
    Ok((spirc, task, mixer, player))
}

pub async fn run(cache_dir: PathBuf, session: Session, credentials: Credentials, volume: u16, mut cmds: UnboundedReceiver<PlayerCmd>, out: UnboundedSender<PlayerOut>) {
    let (spirc, task, mixer, player) = match connect(session.clone(), credentials, volume).await {
        Ok(parts) => parts,
        Err(e) => {
            let _ = out.send(PlayerOut::Fatal(e));
            return;
        }
    };
    let mut task = tokio::spawn(task);
    let _ = out.send(PlayerOut::Connected(session));
    let mut events = player.get_player_event_channel();
    let mut _player = player;
    let mut state = State { spirc, mixer, queue: Vec::new(), index: 0, context: None, start_ms: 0, shuffle_start: false, loaded: false, playing: false, shuffle: false, repeat: Repeat::Off, anchor: (0, Instant::now()), fading: false, settling: false, out };
    state.send_modes();
    let started = Instant::now();
    let mut fade_tick = tokio::time::interval(std::time::Duration::from_millis(100));
    loop {
        tokio::select! {
            _ = fade_tick.tick() => state.fade_tick(),
            cmd = cmds.recv() => match cmd {
                Some(cmd) => state.cmd(cmd),
                // O engine saiu (saiu da conta): desliga o aparelho Connect junto.
                None => break,
            },
            Some(ev) = events.recv() => state.event(ev),
            // O Spirc encerra sozinho quando a sessão cai (rede, suspensão do PC); sem isto todo comando
            // falha com "channel closed". Reconecta com uma sessão nova e mantém fila e posição.
            _ = &mut task => {
                log::warn!("sessão do Spotify caiu (ou o player foi religado); reconectando");
                state.send(PlayerOut::Offline("Reconectando…".into()));
                let was_playing = state.playing;
                // Antes do primeiro play a posição é a de onde o app parou (start_ms), não a do player.
                let position_ms = if !state.loaded { state.start_ms } else { state.anchor.0 + if was_playing { state.anchor.1.elapsed().as_millis() as u32 } else { 0 } };
                state.send(PlayerOut::Playback { playing: false, position_ms });
                let (session, spirc, t, mixer, player) = loop {
                    let attempt = async {
                        let (session, credentials) = crate::auth::audio_session(&cache_dir).await?;
                        let (spirc, t, mixer, player) = connect(session.clone(), credentials, state.mixer.volume()).await?;
                        Ok::<_, String>((session, spirc, t, mixer, player))
                    };
                    match attempt.await {
                        Ok(parts) => break parts,
                        Err(e) => {
                            log::warn!("reconexão falhou: {e}");
                            state.send(PlayerOut::Offline(e));
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                };
                (state.spirc, state.mixer, state.loaded, state.playing, state.start_ms) = (spirc, mixer, false, false, position_ms);
                events = player.get_player_event_channel();
                _player = player;
                task = tokio::spawn(t);
                state.send(PlayerOut::Connected(session));
                if was_playing {
                    state.start(true);
                }
            }
            else => break,
        }
    }
    log::info!("player encerrado após {:?}", started.elapsed());
    let _ = state.spirc.shutdown();
}
