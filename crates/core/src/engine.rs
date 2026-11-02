//! Liga tudo: login, biblioteca, player, letras e capas. Roda num runtime tokio próprio.

use std::{collections::{HashMap, HashSet}, sync::{Arc, Mutex}, time::Duration};

use futures_util::StreamExt;
use librespot::core::dealer::protocol::Message;
use librespot::protocol::connect::ClusterUpdate;

use tokio::sync::{Semaphore, mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel}};

use crate::{
    Card, Device, Kind, Library, Page, SearchKind, SearchResults, Shelf, Track, api::{self, WebApi}, auth, cache, config, history, internal, lastfm, listen, soloist, update,
    images::{self, Art},
    lyrics::{self, Lyrics},
    player::{self, PlayerCmd, PlayerOut, Repeat},
};

const LIBRARY_LIMIT: usize = 200;

pub enum Cmd {
    Toggle,
    Next,
    Prev,
    Seek(u32),
    SetShuffle(bool),
    SetRepeat(Repeat),
    SetVolume(u16),
    /// Toca `tracks` a partir de `index`. `context` é a URI do álbum ou da playlist, quando houver:
    /// o Spotify passa a saber de onde a música veio, e playlists sem lista de faixas também tocam.
    /// `label` é de onde a fila veio ("Músicas curtidas", nome do álbum), para mostrar e salvar.
    /// `shuffle_start`: o botão Aleatório; quem sorteia a primeira é o Spotify (`index` é só o que a tela mostra).
    Play { tracks: Vec<Track>, index: usize, context: Option<String>, label: String, shuffle_start: bool },
    /// Abre a página de um álbum, artista ou playlist.
    Open(Card),
    /// Lista os aparelhos Connect.
    Devices,
    /// Fixa (ou desafixa) uma playlist no topo da sidebar.
    Pin { uri: String, on: bool },
    /// Salva na biblioteca ou tira dela: álbum, artista (seguir) ou playlist (seguir).
    Library { card: Card, save: bool },
    /// Curte ou descurte uma faixa.
    Like { track: Track, on: bool },
    AddToPlaylist { playlist: Card, uri: String },
    AddToQueue(Track),
    /// Arrasta uma faixa da fila de `from` para `to` (índices da fila, depois da que está tocando).
    MoveQueue { from: usize, to: usize },
    /// Arrastar na fila do Spotify (aleatório ligado): `from_uri` vai para o lugar de `to_uri` nas próximas.
    MoveUpNext { from_uri: String, to_uri: String },
    /// Próximo pedaço de uma playlist grande.
    LoadMore { card: Card, from: usize },
    /// Ordem da lista da sidebar arrastada à mão.
    SaveLibraryOrder(Vec<String>),
    /// Edição das suas playlists. A UI já mostra a mudança; se o Spotify recusar, a página volta como estava.
    CreatePlaylist(String),
    RenamePlaylist { card: Card, name: String },
    RemoveFromPlaylist { card: Card, uri: String },
    MoveInPlaylist { card: Card, from: usize, to: usize },
    SetPlaylistCover { card: Card, path: std::path::PathBuf },
    /// Busca no Spotify: todos os tipos (`kind` None) ou mais resultados de um tipo a partir de `offset`.
    Search { query: String, kind: Option<SearchKind>, offset: usize },
    /// Ouvir junto: convidar (true) ou parar de convidar (false).
    ListenHost(bool),
    /// Ouvir junto: entrar com o código, ou sair (None).
    ListenJoin(Option<String>),
    /// Canvas da faixa (o vídeo curto em loop); responde com `AppEvent::Canvas`.
    Canvas(String),
    /// Estatísticas do período; `offset_secs` é o fuso local (o core não sabe ler o do sistema).
    Stats { range: crate::stats::Range, offset_secs: i64 },
    /// Smart shuffle: põe na fila músicas recomendadas (de artistas parecidos) entre as da lista; desligar tira.
    SmartShuffle(bool),
    /// Mais curtidas a partir de `from` (a lista guardada tem só as 200 mais recentes).
    LoadMoreLiked { from: usize },
    /// Créditos da faixa (responde com `AppEvent::Credits`).
    Credits(Track),
    /// Toca o rádio da faixa (o core acha a playlist e responde com `AppEvent::Radio`).
    Radio(Track),
    /// Lançamentos dos artistas seguidos; `force` busca de novo mesmo com o cache recente.
    NewReleases { force: bool },
    /// Liga o Last.fm com a chave e o segredo do app do usuário: abre o navegador para autorizar.
    LastfmConnect { key: String, secret: String },
    LastfmDisconnect,
    /// Começou a tocar por aqui (o "ouvindo agora" do Last.fm).
    NowPlaying(Track),
    /// Ouvida o bastante para contar: desde `at` (segundos Unix).
    Scrobble { track: Track, at: u64 },
    /// Sai da conta e volta para o login.
    Logout,
    RefreshHome,
    /// Passa a reprodução para o aparelho com este id.
    Transfer(String),
    /// Música tocada da busca: depois dela, recomendadas parecidas com ela e com o que a pessoa ouve, como no Spotify.
    /// Vem logo depois do `Play` dessa música sozinha.
    SearchRadio(Track),
    /// Tira da fila tudo o que vem depois da música atual.
    ClearQueue,
    /// Tentar o login de novo depois de um erro ou de preencher o config.
    Retry,
    /// Pedir de novo a letra da faixa `index` da fila (o LRCLIB não tinha respondido).
    RetryLyrics(usize),
    /// Procurar versão nova nas Releases do GitHub (ao abrir e pelo botão das configurações).
    CheckUpdate,
    /// A janela voltou ao primeiro plano: conferir se outro aparelho está tocando (no máximo a cada 20 s).
    RefreshRemote,
    /// Baixar e instalar a versão encontrada; depois falta só reiniciar.
    InstallUpdate(crate::update::Release),
    /// Religa o player para valerem as configurações de reprodução (ou depois de uma falha).
    RestartPlayer,
    /// O Client ID mudou: refaz o login da Web API com o app novo.
    Relogin,
    /// Chave do Soloist (o player oficial do Spotify) nova, ou None para desligar.
    SoloistKey(Option<String>),
}

pub enum AppEvent {
    /// Etapa atual da inicialização, para a tela de carregamento.
    Status(String),
    /// Falta o Client ID. `problem` explica o erro quando o arquivo foi preenchido errado.
    NeedsSetup { problem: Option<String> },
    Failed(String),
    /// `queue` e `index`: onde o app parou da última vez (ou as curtidas, na primeira vez).
    Ready { liked: Vec<Track>, queue: Vec<Track>, index: usize, position_ms: u32, label: String, context: Option<String> },
    Library(Library),
    Page(Page),
    PageFailed { id: String, error: String },
    /// Miniatura baixada: URL da imagem → arquivo em disco.
    Thumb { url: String, path: std::path::PathBuf, small: std::path::PathBuf },
    /// Foto grande no topo da página de artista: URL → arquivo.
    Hero { url: String, path: std::path::PathBuf },
    /// Fundo borrado da capa de uma página (álbum, playlist): URL da capa → arquivo.
    Backdrop { url: String, path: std::path::PathBuf },
    /// A fila mudou inteira (tocar um álbum, por exemplo); os índices antigos não valem mais.
    Queue(Vec<Track>),
    Devices(Vec<Device>),
    /// Curtidas atualizadas (o app abriu com as do cache).
    Liked(Vec<Track>),
    Home(Vec<Shelf>),
    /// Quando cada contexto (URI de playlist/álbum) tocou por último, em segundos Unix.
    Recent(HashMap<String, u64>),
    /// URIs fixadas, na ordem.
    Pins(Vec<String>),
    /// Faixa fora da lista inicial (tocada por outro aparelho ou pelo autoplay); entra no fim da lista.
    TrackAdded(Track),
    TrackChanged(usize),
    Playback { playing: bool, position_ms: u32 },
    Modes { shuffle: bool, repeat: Repeat, volume: u16 },
    Lyrics { index: usize, lyrics: Lyrics },
    Update(crate::update::State),
    Art { index: usize, art: Art },
    /// Erro que não impede de continuar (faixa indisponível, letra falhou).
    Notice(String),
    /// Nome e foto da conta.
    Profile { name: String, image: Option<String> },
    /// Próximas da fila do Spotify, na ordem real (aleatório, autoplay, outro aparelho).
    UpNext(Vec<Track>),
    /// Outro aparelho está tocando (nome dele); None quando volta a ser o Cyder.
    Remote(Option<String>),
    /// Faixa posta na fila nesta posição.
    Inserted { at: usize, track: Track },
    /// A fila foi reorganizada (a faixa atual continua no mesmo índice).
    Reordered(Vec<Track>),
    /// Mais faixas de uma playlist carregada aos poucos.
    PageMore { id: String, tracks: Vec<Track>, more_from: Option<usize> },
    /// Se cada faixa está curtida (URI, curtida).
    Saved(Vec<(String, bool)>),
    LibraryOrder(Vec<String>),
    /// Resultado de `Cmd::Search`; `more` quando é a continuação de um tipo.
    Search { results: SearchResults, more: bool },
    /// Playlists da busca `query` com o rádio do artista buscado na frente; chega depois do resultado.
    SearchPlaylists { query: String, playlists: Vec<Card> },
    /// O Spotify bloqueou os pedidos do Client ID por `secs` segundos (a partir de agora).
    RateLimited { secs: u64 },
    /// Playlist criada: a UI abre a página dela.
    PlaylistCreated(Card),
    /// None: tocando por aqui está ok; Some: sem conexão, e o motivo.
    PlayerStatus(Option<String>),
    /// Plano da conta ("premium", "free"), pela sessão.
    Plan(String),
    SearchFailed { query: String, error: String },
    /// Curtidas seguintes, a partir de `from`; `more` quando ainda há outras depois. `tracks` vazio e
    /// `error` quando falhou.
    LikedMore { from: usize, tracks: Vec<Track>, more: bool, error: Option<String> },
    Credits(crate::Credits),
    /// Playlist de rádio pronta para tocar.
    Radio(Card),
    NewReleases(Vec<crate::NewRelease>),
    NewReleasesFailed(String),
    Stats { range: crate::stats::Range, stats: crate::stats::Stats },
    /// Ouvir junto mudou.
    Listen(Listen),
    /// Vídeo do Canvas da faixa em disco; None quando ela não tem.
    Canvas { uri: String, path: Option<std::path::PathBuf> },
    /// Smart shuffle ligado ou desligado; `added` são as recomendadas que entraram na fila.
    SmartShuffle { on: bool, added: Vec<String> },
    /// Onde cada episódio de podcast parou (URI → ms), para continuar e para a lista mostrar quanto falta.
    EpisodePositions(HashMap<String, u32>),
    /// Conta do Last.fm ligada (nome de usuário) ou None.
    Lastfm(Option<String>),
    /// Abrir no navegador (autorizar o Last.fm).
    OpenUrl(String),
    Soloist(crate::soloist::Status),
    /// O Spotify recusa o áudio desta conta ao player do Cyder e o Soloist não está ligado.
    AudioRefused,
}

/// Modo de ouvir junto.
#[derive(Clone, Debug, PartialEq)]
pub enum Listen {
    Off,
    /// Convidando: o código para passar a quem vai entrar.
    Hosting(String),
    /// Seguindo quem convidou (nome dele, quando já chegou alguma música).
    Joined { code: String, host: String },
}

pub fn start() -> (UnboundedSender<Cmd>, UnboundedReceiver<AppEvent>) {
    images::clear_if_requested();
    // Sobra da última atualização (o executável antigo, renomeado para dar lugar ao novo).
    update::cleanup();
    let (cmd_tx, cmd_rx) = unbounded_channel();
    let (out_tx, out_rx) = unbounded_channel();
    std::thread::Builder::new()
        .name("cyder-core".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio");
            rt.block_on(run(cmd_rx, out_tx));
        })
        .expect("thread do core");
    (cmd_tx, out_rx)
}

struct Ready {
    session: librespot::core::session::Session,
    credentials: librespot::core::authentication::Credentials,
    api: Arc<WebApi>,
    tracks: Vec<Track>,
    /// As curtidas vieram do cache: atualizar depois que a sessão subir.
    liked_from_cache: bool,
}

async fn bootstrap(http: &reqwest::Client, out: &UnboundedSender<AppEvent>) -> Result<Ready, AppEvent> {
    let cfg = config::load().map_err(|e| AppEvent::NeedsSetup {
        problem: match e {
            config::ConfigError::Missing => None,
            config::ConfigError::Invalid(m) => Some(m),
        },
    })?;
    let cache_dir = config::cache_dir();
    let _ = out.send(AppEvent::Status("Entrando no Spotify…".into()));
    let (session, credentials) = auth::audio_session(&cache_dir).await.map_err(|e| AppEvent::Failed(format!("Não deu para entrar no Spotify: {e}")))?;
    let _ = out.send(AppEvent::Status("Autorizando o acesso à sua biblioteca…".into()));
    let web = auth::WebAuth::login(&cfg.client_id).await.map_err(|e| AppEvent::Failed(format!("Não deu para autorizar a biblioteca: {e}")))?;
    let api = Arc::new(WebApi::new(http.clone(), web, out.clone()));
    // Com cache, o app abre na hora e as curtidas se atualizam em segundo plano.
    if let Some(tracks) = cache::load::<Vec<Track>>("liked") {
        return Ok(Ready { session, credentials, api, tracks, liked_from_cache: true });
    }
    let _ = out.send(AppEvent::Status("Carregando suas músicas curtidas…".into()));
    let tracks = api.liked_tracks(LIBRARY_LIMIT).await.map_err(AppEvent::Failed)?;
    cache::save("liked", &tracks);
    Ok(Ready { session, credentials, api, tracks, liked_from_cache: false })
}

/// Baixa miniaturas em segundo plano, poucas por vez para não disputar banda com o áudio.
fn fetch_thumbs(http: &reqwest::Client, out: &UnboundedSender<AppEvent>, limit: &Arc<Semaphore>, urls: impl IntoIterator<Item = String>) {
    let cache = config::cache_dir();
    for url in urls {
        let (http, out, cache, limit) = (http.clone(), out.clone(), cache.clone(), limit.clone());
        tokio::spawn(async move {
            let Ok(_permit) = limit.acquire().await else { return };
            match images::thumb(&http, &cache, &url).await {
                Ok((path, small)) => drop(out.send(AppEvent::Thumb { url, path, small })),
                Err(e) => log::warn!("miniatura {url}: {e}"),
            }
        });
    }
}

fn card_images(cards: &[Card]) -> impl Iterator<Item = String> + '_ {
    cards.iter().filter_map(|c| c.image.clone())
}

fn library_images(lib: &Library) -> Vec<String> {
    card_images(&lib.albums).chain(card_images(&lib.artists)).chain(card_images(&lib.playlists)).collect()
}

/// De quais artistas fazer rádios: os que mais aparecem no que você ouviu por último (o histórico da conta, que
/// agora inclui o Cyder), depois os mais ouvidos neste computador no mês e, por fim, o top do Spotify, que muda
/// devagar e puxa os de sempre. (URI, nome), sem repetir, até oito.
fn radio_seeds(played: &[api::Played], top: &[Card]) -> Vec<(String, String)> {
    let mut count: HashMap<&str, (usize, &str)> = HashMap::new();
    for (id, name) in played.iter().filter_map(|p| p.track.artists.first()) {
        count.entry(id.as_str()).or_insert((0, name.as_str())).0 += 1;
    }
    let mut recent: Vec<_> = count.into_iter().collect();
    recent.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    let month = crate::stats::compute(&crate::stats::load(), crate::stats::Range::Month, now_secs(), crate::local_offset_secs()).top_artists;
    let mut seeds: Vec<(String, String)> = Vec::new();
    let candidates = recent.into_iter().map(|(id, (_, name))| (id.to_string(), name.to_string()))
        .chain(month.into_iter().map(|r| (r.id, r.name)))
        .chain(top.iter().map(|c| (c.id.clone(), c.title.clone())));
    for (id, name) in candidates {
        let uri = format!("spotify:artist:{id}");
        if id.len() == 22 && !seeds.iter().any(|(u, _)| *u == uri) {
            seeds.push((uri, name));
        }
        if seeds.len() == 8 {
            break;
        }
    }
    seeds
}

fn shelf_images(shelves: &[Shelf]) -> Vec<String> {
    shelves.iter().flat_map(|s| card_images(&s.cards)).collect()
}

fn page_images(page: &Page) -> Vec<String> {
    let owner = page.owner_image.clone();
    let latest = page.artist.as_ref().and_then(|a| a.latest.as_ref()).and_then(|(c, _)| c.image.clone());
    let related = page.artist.as_ref().map(|a| card_images(&a.related).chain(card_images(&a.featuring)).collect::<Vec<_>>()).unwrap_or_default();
    page.card.image.clone().into_iter().chain(owner).chain(latest).chain(related).chain(card_images(&page.albums)).chain(page.tracks.iter().filter_map(|t| t.cover_small.clone())).collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Estado do player salvo em disco, para o app abrir onde parou.
#[derive(serde::Serialize, serde::Deserialize)]
struct Resume {
    queue: Vec<Track>,
    index: usize,
    position_ms: u32,
    context: Option<String>,
    label: String,
    shuffle: bool,
    repeat: Repeat,
    volume: u16,
}

/// Ouvir junto: quanto adiantar quem entrou (a mensagem demora a chegar) e quanto de diferença tolerar antes de pular.
const LISTEN_DELAY_MS: u32 = 700;
const LISTEN_DRIFT_MS: u32 = 3000;

/// Publica o que toca aqui para quem entrou no ouvir junto (se estiver convidando).
fn publish_listen(ctx: &Ctx, code: Option<&str>, tracks: &[Track], index: usize, playing: bool, anchor: (u32, std::time::Instant)) {
    let (Some(code), Some(track)) = (code, tracks.get(index)) else { return };
    let position_ms = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
    let host = cache::load::<(String, Option<String>)>("profile").map(|p| p.0).unwrap_or_default();
    let state = listen::State { uri: track.uri.clone(), position_ms, playing, host };
    let (http, code) = (ctx.http.clone(), code.to_string());
    tokio::spawn(async move {
        if let Err(e) = listen::publish(&http, &code, &state).await {
            log::warn!("ouvir junto: {e}");
        }
    });
}

/// Menos que isto ouvido não conta como começado; faltando menos que `EPISODE_DONE_MS`, conta como ouvido.
const EPISODE_STARTED_MS: u32 = 10_000;
const EPISODE_DONE_MS: u32 = 30_000;

/// Guarda onde o episódio parou; devolve se mudou.
fn remember_episode(episodes: &mut HashMap<String, u32>, track: &Track, position_ms: u32) -> bool {
    if !track.uri.starts_with("spotify:episode:") {
        return false;
    }
    if position_ms + EPISODE_DONE_MS >= track.duration_ms {
        return episodes.remove(&track.uri).is_some();
    }
    if position_ms < EPISODE_STARTED_MS {
        return false;
    }
    episodes.insert(track.uri.clone(), position_ms) != Some(position_ms)
}

const RECENT_SHELF: &str = "Tocadas recentemente";
/// Álbuns tocados por aqui que ficam guardados (o Spotify devolve 50 faixas dos outros aparelhos).
const RECENT_LOCAL_MAX: usize = 50;

/// Álbuns do Spotify e deste computador, do mais recente para o mais antigo, sem repetir.
/// Edições diferentes do mesmo álbum têm ids diferentes: compara também nome e artista.
fn recent_albums(spotify: &[(Card, u64)], local: &[(Card, u64)]) -> Vec<Card> {
    let mut all: Vec<&(Card, u64)> = spotify.iter().chain(local).collect();
    all.sort_by_key(|(_, at)| std::cmp::Reverse(*at));
    let mut out: Vec<Card> = Vec::new();
    for (a, _) in all {
        if !out.iter().any(|x| x.id == a.id || (x.title == a.title && x.subtitle == a.subtitle)) {
            out.push(a.clone());
        }
    }
    out
}

/// As Novidades mudam devagar: duas buscas por dia bastam.
const NEW_RELEASES_FRESH: Duration = Duration::from_secs(12 * 60 * 60);
const REFRESH_EVERY: Duration = Duration::from_secs(30 * 60);
/// Nome e foto da conta quase nunca mudam.
const PROFILE_FRESH: Duration = Duration::from_secs(24 * 60 * 60);
/// Espera depois de um aviso do Spotify Connect antes de consultar, para juntar os avisos seguidos.
const CLUSTER_SETTLE: Duration = Duration::from_millis(600);
const REMOTE_FALLBACK: Duration = Duration::from_secs(30);
/// De quanto em quanto o relógio de parede é olhado, e o pulo que conta como "o PC dormiu".
const WAKE_CHECK: Duration = Duration::from_secs(15);
const WAKE_GAP: Duration = Duration::from_secs(60);
const REFRESH_REMOTE_GAP: Duration = Duration::from_secs(20);
/// Faixas por pedaço nas playlists grandes.
const PLAYLIST_CHUNK: usize = 200;
/// Faixas por página conferidas em "está curtida?" (5 pedidos de 40).
const SAVED_CHECK_MAX: usize = 200;

/// Volume na primeira execução (~70%).
const DEFAULT_VOLUME: u16 = 45_000;

/// Tudo que roda em segundo plano depois do login.
#[derive(Clone)]
struct Ctx {
    /// Faixas cuja curtida já foi perguntada ao Spotify nesta sessão: não pergunta de novo.
    saved_known: Arc<Mutex<HashSet<String>>>,
    http: reqwest::Client,
    out: UnboundedSender<AppEvent>,
    api: Arc<WebApi>,
    session: librespot::core::session::Session,
    thumbs: Arc<Semaphore>,
}

impl Ctx {
    fn send(&self, ev: AppEvent) {
        let _ = self.out.send(ev);
    }

    fn thumbs(&self, urls: Vec<String>) {
        fetch_thumbs(&self.http, &self.out, &self.thumbs, urls);
    }

    async fn refresh_liked(self) {
        match self.api.liked_tracks(LIBRARY_LIMIT).await {
            Ok(tracks) => {
                cache::save("liked", &tracks);
                self.thumbs(tracks.iter().filter_map(|t| t.cover_small.clone()).collect());
                self.send(AppEvent::Liked(tracks));
            }
            Err(e) => log::warn!("curtidas: {e}"),
        }
    }

    /// Álbuns e artistas pela Web API; playlists na ordem da biblioteca do Spotify (rootlist),
    /// incluindo as do próprio Spotify (daylist, mixes salvos), que a Web API não devolve mais.
    async fn refresh_library(self) {
        let mut lib = match self.api.library().await {
            Ok(l) => l,
            Err(e) => return self.send(AppEvent::Notice(format!("Não deu para atualizar a biblioteca: {e}"))),
        };
        match internal::rootlist(&self.session).await {
            Ok(uris) => {
                let mut by_id: HashMap<String, Card> = lib.playlists.drain(..).map(|c| (c.id.clone(), c)).collect();
                for (uri, at) in uris {
                    if at > 0 {
                        lib.added.insert(uri.clone(), at);
                    }
                    let id = uri.trim_start_matches("spotify:playlist:");
                    if let Some(card) = by_id.remove(id) {
                        lib.playlists.push(card);
                    } else if let Ok(card) = internal::playlist_header(&self.session, id).await {
                        if !card.title.is_empty() {
                            lib.playlists.push(card);
                        }
                    }
                }
                lib.playlists.extend(by_id.into_values());
            }
            Err(e) => log::warn!("rootlist: {e}"),
        }
        // Sem as datas dos artistas, eles só vão para o fim da sidebar: o resto da biblioteca vem igual.
        match internal::followed_artists(&self.session).await {
            Ok(list) => lib.added.extend(list.into_iter().filter(|(_, at)| *at > 0)),
            Err(e) => log::warn!("datas dos artistas seguidos: {e}"),
        }
        cache::save("library", &lib);
        self.thumbs(library_images(&lib));
        self.send(AppEvent::Library(lib));
    }

    /// Prateleiras da tela inicial que vêm da Web API: tocadas recentemente e artistas mais ouvidos.
    /// As de playlists (do Spotify e suas) a UI monta com a biblioteca.
    async fn refresh_home(self) {
        let local = now_secs() as i64 + crate::local_offset_secs();
        let moods = self.moods().await.for_now(local);
        let top_artists = self.api.top_artists().await.unwrap_or_default();
        let played = self.api.recently_played().await;
        let radios = self.radios(&radio_seeds(played.as_deref().unwrap_or_default(), &top_artists)).await;
        let editorial = self.editorial(moods.iter().flat_map(|m| m.playlists.iter().cloned()).chain(radios.iter().cloned()).collect()).await;
        let shelf = |title: &str, ids: Vec<String>| Shelf { title: title.into(), cards: ids.iter().filter_map(|id| editorial.get(id).cloned()).collect() };
        let mood_shelf = |m: &crate::moods::Mood| shelf(&m.title, crate::moods::of_day(&m.playlists, local));
        // Tocadas recentemente abre a tela e a do momento vem logo depois (a UI põe as playlists da biblioteca em seguida).
        let mut shelves: Vec<Shelf> = Vec::new();
        let mut rest: Vec<Shelf> = moods.iter().take(2).map(mood_shelf).collect();
        rest.push(shelf("Rádios dos seus artistas", radios));
        rest.extend(moods.iter().skip(2).map(mood_shelf));
        if let Ok(played) = played {
            // Guardada à parte para juntar de novo com o que toca por aqui sem pedir ao Spotify.
            let spotify: Vec<(Card, u64)> = played.iter().filter_map(|p| Some((p.album.clone()?, p.at))).collect();
            cache::save("recent-spotify", &spotify);
            let albums = recent_albums(&spotify, &cache::load::<Vec<(Card, u64)>>("recent-local").unwrap_or_default());
            if !albums.is_empty() {
                shelves.push(Shelf { title: RECENT_SHELF.into(), cards: albums });
            }
            // De onde as faixas vieram, para a sidebar pôr as playlists tocadas no topo.
            let mut history: HashMap<String, u64> = cache::load("history").unwrap_or_default();
            for p in played {
                if let Some(ctx) = p.context {
                    let at = history.entry(ctx).or_insert(0);
                    *at = (*at).max(p.at);
                }
            }
            cache::save("history", &history);
            self.send(AppEvent::Recent(history));
        }
        shelves.extend(rest);
        shelves.push(Shelf { title: "Seus artistas mais ouvidos".into(), cards: top_artists });
        shelves.retain(|s| !s.cards.is_empty());
        if shelves.is_empty() {
            return self.send(AppEvent::Notice("Não deu para montar a tela inicial agora.".into()));
        }
        cache::save("home", &shelves);
        self.thumbs(shelf_images(&shelves));
        self.send(AppEvent::Home(shelves));
    }

    /// As prateleiras de `home-moods.json`: a do GitHub, baixada no máximo uma vez por dia, para dar para trocar
    /// playlists sem versão nova; sem internet, a última baixada; nunca baixada, a que vem no executável.
    async fn moods(&self) -> crate::moods::Moods {
        const DAY: Duration = Duration::from_secs(86_400);
        if cache::fresh("moods", DAY) {
            if let Some(m) = cache::load("moods") {
                return m;
            }
        }
        let url = format!("https://raw.githubusercontent.com/{}/main/home-moods.json", crate::update::repo_path());
        let fetched = async { self.http.get(&url).send().await?.error_for_status()?.json::<crate::moods::Moods>().await };
        match fetched.await {
            Ok(m) => {
                cache::save("moods", &m);
                m
            }
            Err(e) => {
                log::warn!("home-moods.json do GitHub: {e}");
                cache::load("moods").unwrap_or_else(crate::moods::Moods::bundled)
            }
        }
    }

    /// A rádio que o Spotify gera para cada artista (`radio_seeds`): muda com o que você ouve, sem lista fixa.
    /// Guardadas por um dia, um pedido por artista.
    async fn radios(&self, artists: &[(String, String)]) -> Vec<String> {
        const DAY: u64 = 86_400;
        let mut known: HashMap<String, (Option<String>, u64)> = cache::load("artist-radios").unwrap_or_default();
        let now = now_secs();
        let mut out = Vec::new();
        for (uri, name) in artists {
            let radio = match known.get(uri) {
                Some((radio, at)) if now - at < DAY => radio.clone(),
                _ => {
                    let radio = internal::radio_for(&self.session, uri).await.inspect_err(|e| log::warn!("rádio de {name}: {e}")).ok();
                    known.insert(uri.clone(), (radio.clone(), now));
                    radio
                }
            };
            out.extend(radio.map(|uri| uri.trim_start_matches("spotify:playlist:").to_string()));
        }
        cache::save("artist-radios", &known);
        out
    }

    /// Capa, nome e descrição das playlists da tela inicial, pela sessão (a Web API não devolve as do Spotify).
    /// Guardados por uma semana: mudam pouco, e assim a troca de hora não pede nada ao Spotify. A que o Spotify
    /// apagou (404) fica marcada e some da tela até a semana seguinte, sem pedir de novo a cada vez.
    async fn editorial(&self, mut wanted: Vec<String>) -> HashMap<String, Card> {
        use futures_util::StreamExt;
        const WEEK: u64 = 7 * 86_400;
        let mut known: HashMap<String, (Option<Card>, u64)> = cache::load("editorial").unwrap_or_default();
        let now = now_secs();
        wanted.sort_unstable();
        wanted.dedup();
        let missing: Vec<String> = wanted.iter().filter(|id| known.get(*id).is_none_or(|(_, at)| now - at > WEEK)).cloned().collect();
        let fetched: Vec<(String, Result<Card, String>)> = futures_util::stream::iter(missing)
            .map(|id| {
                let session = self.session.clone();
                async move {
                    let card = internal::playlist_teaser(&session, &id).await;
                    (id, card)
                }
            })
            .buffer_unordered(6)
            .collect()
            .await;
        for (id, card) in fetched {
            match card {
                Ok(card) if !card.title.is_empty() => {
                    known.insert(id, (Some(card), now));
                }
                Err(e) if e.contains("404") || e.to_lowercase().contains("not found") => {
                    log::info!("playlist {id} não existe mais no Spotify; fora da tela inicial");
                    known.insert(id, (None, now));
                }
                // Falha de rede: tenta de novo na próxima atualização.
                Err(e) => log::warn!("playlist da tela inicial {id}: {e}"),
                Ok(_) => {}
            }
        }
        cache::save("editorial", &known);
        wanted.into_iter().filter_map(|id| Some((id.clone(), known.get(&id)?.0.clone()?))).collect()
    }

    /// O Spotify não registra o que toca pelo Cyder (doc 07): o álbum da faixa entra no histórico deste
    /// computador e em "Tocadas recentemente" na hora, junto com o que o Spotify conta dos outros aparelhos.
    fn note_played(&self, track: &Track) {
        if track.album_id.is_empty() {
            return;
        }
        let artist = track.artists.first().map_or_else(|| track.artist.clone(), |a| a.1.clone());
        let image = track.cover_large.clone().or_else(|| track.cover_small.clone());
        let card = Card { kind: Kind::Album, id: track.album_id.clone(), uri: format!("spotify:album:{}", track.album_id), title: track.album.clone(), subtitle: artist, image };
        let mut local: Vec<(Card, u64)> = cache::load("recent-local").unwrap_or_default();
        local.retain(|(c, _)| c.id != card.id);
        local.insert(0, (card.clone(), now_secs()));
        local.truncate(RECENT_LOCAL_MAX);
        cache::save("recent-local", &local);
        let cards = recent_albums(&cache::load::<Vec<(Card, u64)>>("recent-spotify").unwrap_or_default(), &local);
        let mut shelves: Vec<Shelf> = cache::load("home").unwrap_or_default();
        match shelves.iter_mut().find(|s| s.title == RECENT_SHELF) {
            Some(shelf) => shelf.cards = cards,
            None => shelves.insert(0, Shelf { title: RECENT_SHELF.into(), cards }),
        }
        cache::save("home", &shelves);
        self.thumbs(card.image.into_iter().collect());
        self.send(AppEvent::Home(shelves));
    }

    /// Marca o coração das faixas: as curtidas guardadas são só as 200 mais recentes, então pergunta ao Spotify.
    fn check_saved(&self, tracks: &[Track]) {
        let uris: Vec<String> = {
            let known = self.saved_known.lock().unwrap_or_else(|e| e.into_inner());
            tracks.iter().take(SAVED_CHECK_MAX).filter(|t| !known.contains(&t.uri)).map(|t| t.uri.clone()).collect()
        };
        if uris.is_empty() {
            return;
        }
        let ctx = self.clone();
        tokio::spawn(async move {
            match ctx.api.saved_contains(&uris).await {
                Ok(flags) => {
                    ctx.saved_known.lock().unwrap_or_else(|e| e.into_inner()).extend(uris.iter().cloned());
                    ctx.send(AppEvent::Saved(uris.into_iter().zip(flags).collect()));
                }
                Err(e) => log::warn!("curtidas da página: {e}"),
            }
        });
    }

    async fn create_playlist(self, name: String) {
        match self.api.create_playlist(&name).await {
            Ok(card) => {
                let mut lib = cache::load::<Library>("library").unwrap_or_default();
                lib.playlists.insert(0, card.clone());
                cache::save("library", &lib);
                self.send(AppEvent::Library(lib));
                self.send(AppEvent::PlaylistCreated(card));
            }
            Err(e) => self.send(AppEvent::Notice(format!("Não deu para criar a playlist: {e}"))),
        }
    }

    async fn rename_playlist(self, card: Card, name: String) {
        match self.api.rename_playlist(&card.id, &name).await {
            Ok(()) => self.update_card(&card.id, |c| c.title = name.clone()),
            Err(e) => {
                self.send(AppEvent::Notice(format!("Não deu para renomear: {e}")));
                // A tela já mostrava o nome novo: volta o que está guardado.
                self.update_card(&card.id, |_| {});
            }
        }
    }

    /// Troca o cartão da playlist na biblioteca e na página guardadas, e manda as duas para a UI.
    fn update_card(&self, id: &str, f: impl Fn(&mut Card)) {
        let mut lib = cache::load::<Library>("library").unwrap_or_default();
        lib.playlists.iter_mut().filter(|c| c.id == id).for_each(&f);
        cache::save("library", &lib);
        self.send(AppEvent::Library(lib));
        let name = cache::page_name(id);
        if let Some(mut page) = cache::load::<Page>(&name) {
            f(&mut page.card);
            cache::save(&name, &page);
            self.send(AppEvent::Page(page));
        }
    }

    /// Faixa tirada ou movida: com sucesso, a página guardada deixa de valer e vem de novo do Spotify;
    /// com erro, também, e assim a tela volta como estava.
    async fn after_edit(&self, card: Card, result: Result<(), String>, what: &str) {
        if let Err(e) = result {
            self.send(AppEvent::Notice(format!("{what}: {e}")));
        }
        let _ = std::fs::remove_file(config::cache_dir().join("data").join(format!("{}.json", cache::page_name(&card.id))));
        self.clone().open(card).await;
    }

    async fn set_cover(self, card: Card, path: std::path::PathBuf) {
        let encoded = match tokio::task::spawn_blocking(move || images::playlist_cover(&path)).await {
            Ok(Ok(e)) => e,
            Ok(Err(e)) => return self.send(AppEvent::Notice(e)),
            Err(e) => return self.send(AppEvent::Notice(e.to_string())),
        };
        if let Err(e) = self.api.set_playlist_cover(&card.id, encoded).await {
            return self.send(AppEvent::Notice(format!("Não deu para trocar a capa. {e}")));
        }
        self.send(AppEvent::Notice("Capa enviada. O Spotify leva alguns segundos para mostrar a nova.".into()));
        // A capa nova aparece no Spotify com um pequeno atraso.
        tokio::time::sleep(Duration::from_secs(6)).await;
        match internal::playlist_header(&self.session, &card.id).await {
            Ok(head) if head.image.is_some() => {
                self.thumbs(head.image.clone().into_iter().collect());
                self.backdrop(head.image.clone());
                self.update_card(&card.id, |c| c.image = head.image.clone());
            }
            Ok(_) => {}
            Err(e) => log::warn!("capa nova de {}: {e}", card.id),
        }
    }

    /// Novidades: do cache, se for recente; senão, dos artistas seguidos na biblioteca guardada.
    async fn new_releases(self, force: bool) {
        if !force && cache::fresh("new-releases", NEW_RELEASES_FRESH) {
            if let Some(list) = cache::load::<Vec<crate::NewRelease>>("new-releases") {
                self.thumbs(list.iter().filter_map(|r| r.card.image.clone()).collect());
                return self.send(AppEvent::NewReleases(list));
            }
        }
        let artists: Vec<String> = cache::load::<Library>("library").unwrap_or_default().artists.into_iter().map(|c| c.id).collect();
        match internal::new_releases(&self.session, &artists).await {
            Ok(list) => {
                cache::save("new-releases", &list);
                self.thumbs(list.iter().filter_map(|r| r.card.image.clone()).collect());
                self.send(AppEvent::NewReleases(list));
            }
            Err(e) => self.send(AppEvent::NewReleasesFailed(e)),
        }
    }

    async fn lastfm_connect(self, key: String, secret: String) {
        let token = match lastfm::token(&self.http, &key, &secret).await {
            Ok(t) => t,
            Err(e) => return self.send(AppEvent::Notice(format!("O Last.fm recusou a chave: {}", e.message))),
        };
        self.send(AppEvent::OpenUrl(lastfm::auth_url(&key, &token)));
        self.send(AppEvent::Notice("Autorize o Cyder na página do Last.fm que abriu no navegador.".into()));
        // O token vale por 60 minutos; espera a autorização por até 5.
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_secs(3)).await;
            match lastfm::session(&self.http, &key, &secret, &token).await {
                Ok(None) => continue,
                Ok(Some((session, user))) => {
                    lastfm::save(&lastfm::Account { key, secret, session, user: user.clone() });
                    self.send(AppEvent::Notice(format!("Last.fm ligado como {user}.")));
                    return self.send(AppEvent::Lastfm(Some(user)));
                }
                Err(e) => return self.send(AppEvent::Notice(format!("Não deu para ligar o Last.fm: {}", e.message))),
            }
        }
        self.send(AppEvent::Notice("A autorização do Last.fm demorou demais. Tente ligar de novo.".into()));
    }

    /// Manda o scrobble junto com os que falharam antes (sem internet, Last.fm fora do ar), que ficam guardados.
    async fn scrobble(self, track: Track, at: u64) {
        let Some(account) = lastfm::saved() else { return };
        let mut pending: Vec<(Track, u64)> = cache::load("scrobbles").unwrap_or_default();
        pending.push((track, at));
        while !pending.is_empty() {
            let n = pending.len().min(lastfm::BATCH_MAX);
            match lastfm::scrobble(&self.http, &account, &pending[..n]).await {
                Ok(()) => drop(pending.drain(..n)),
                Err(e) => {
                    log::warn!("scrobble: {}", e.message);
                    if lastfm::is_auth_error(e.code) {
                        pending.clear();
                    }
                    self.lastfm_failed(e);
                    break;
                }
            }
        }
        // Os mais recentes; o Last.fm recusa scrobbles de mais de duas semanas.
        let extra = pending.len().saturating_sub(1000);
        pending.drain(..extra);
        cache::save("scrobbles", &pending);
    }

    /// Chave ou sessão inválidas (o usuário revogou o acesso): desliga e avisa. Outros erros só vão para o log.
    fn lastfm_failed(&self, e: lastfm::Error) {
        if lastfm::is_auth_error(e.code) {
            lastfm::forget();
            self.send(AppEvent::Lastfm(None));
            self.send(AppEvent::Notice(format!("O Last.fm foi desligado: {}. Ligue de novo nas Configurações.", e.message)));
        }
    }

    async fn search(self, query: String, kind: Option<SearchKind>, offset: usize) {
        let kinds = kind.map_or(SearchKind::ALL.to_vec(), |k| vec![k]);
        let mut found = self.api.search(&query, &kinds, offset).await;
        // Ver mais de um tipo: mais três páginas de 10 de uma vez, até acabar.
        if let (Some(_), Ok(results)) = (kind, &mut found) {
            for page in 1..4 {
                match self.api.search(&query, &kinds, offset + page * 10).await {
                    Ok(next) if !next.is_empty() => results.extend(next),
                    _ => break,
                }
            }
        }
        match found {
            Ok(results) => {
                let cards = results.artists.iter().chain(&results.albums).chain(&results.playlists).chain(&results.shows);
                self.thumbs(cards.filter_map(|c| c.image.clone()).chain(results.tracks.iter().filter_map(|t| t.cover_small.clone())).collect());
                self.check_saved(&results.tracks);
                // Buscou um artista: o rádio dele entra na frente das playlists, como no Spotify.
                let q = query.trim().to_lowercase();
                let artist = results.artists.first().filter(|a| a.title.to_lowercase() == q).cloned();
                let found = results.playlists.clone();
                self.send(AppEvent::Search { results, more: kind.is_some() });
                if let (None, Some(artist)) = (kind, artist) {
                    let playlists = self.featuring(&artist, found).await;
                    self.thumbs(card_images(&playlists).collect());
                    self.send(AppEvent::SearchPlaylists { query, playlists });
                }
            }
            Err(e) => {
                log::warn!("busca \"{query}\": {e}");
                self.send(AppEvent::SearchFailed { query, error: e });
            }
        }
    }

    /// Playlists com o artista: o rádio dele (pela sessão) e `found`, as que a busca achou pelo nome, com a
    /// descrição no lugar do dono, como no Spotify. As "This Is" do Spotify a Web API não devolve desde 2024.
    async fn featuring(&self, artist: &Card, found: Vec<Card>) -> Vec<Card> {
        let mut cards = Vec::new();
        match internal::radio_for(&self.session, &artist.uri).await {
            Ok(uri) => cards.push(Card { kind: Kind::Playlist, id: uri.trim_start_matches("spotify:playlist:").to_string(), uri, title: String::new(), subtitle: String::new(), image: None }),
            Err(e) => log::warn!("rádio de {}: {e}", artist.title),
        }
        for c in found {
            if !cards.iter().any(|k| k.id == c.id) {
                cards.push(c);
            }
        }
        futures_util::stream::iter(cards)
            .map(|card| {
                let session = self.session.clone();
                async move { internal::playlist_teaser(&session, &card.id).await.unwrap_or(card) }
            })
            .buffered(6)
            .filter(|c| std::future::ready(!c.title.is_empty()))
            .collect()
            .await
    }

    async fn load_more(self, card: Card, from: usize) {
        match internal::playlist(&self.session, &card.id, from, PLAYLIST_CHUNK).await {
            Ok(chunk) => {
                // O cache guarda tudo o que já veio: da próxima vez a página abre inteira.
                let name = cache::page_name(&card.id);
                if let Some(mut page) = cache::load::<Page>(&name) {
                    page.tracks.extend(chunk.tracks.iter().cloned());
                    page.more_from = chunk.next;
                    cache::save(&name, &page);
                }
                self.thumbs(chunk.tracks.iter().filter_map(|t| t.cover_small.clone()).collect());
                self.check_saved(&chunk.tracks);
                self.send(AppEvent::PageMore { id: card.id, tracks: chunk.tracks, more_from: chunk.next });
            }
            Err(e) => {
                log::warn!("mais faixas de {}: {e}", card.id);
                self.send(AppEvent::PageMore { id: card.id, tracks: Vec::new(), more_from: Some(from) });
            }
        }
    }

    fn hero(&self, page: &Page) {
        let Some(url) = page.artist.as_ref().and_then(|a| a.hero.clone()) else { return };
        let (http, out) = (self.http.clone(), self.out.clone());
        tokio::spawn(async move {
            match images::original(&http, &config::cache_dir(), &url).await {
                Ok(path) => drop(out.send(AppEvent::Hero { url, path })),
                Err(e) => log::warn!("foto de artista {url}: {e}"),
            }
        });
    }

    fn backdrop(&self, url: Option<String>) {
        let Some(url) = url else { return };
        let (http, out) = (self.http.clone(), self.out.clone());
        tokio::spawn(async move {
            match images::backdrop(&http, &config::cache_dir(), &url).await {
                Ok(path) => drop(out.send(AppEvent::Backdrop { url, path })),
                Err(e) => log::warn!("fundo de {url}: {e}"),
            }
        });
    }

    /// Salva ou tira da biblioteca e atualiza o cache; se o Spotify recusar, a lista volta como estava.
    async fn set_saved(self, card: Card, save: bool) {
        let result = match (card.kind, save) {
            (Kind::Playlist, false) => self.api.unfollow_playlist(&card.id).await,
            _ => self.api.set_saved(std::slice::from_ref(&card.uri), save).await,
        };
        let mut lib = cache::load::<Library>("library").unwrap_or_default();
        match result {
            Ok(()) => {
                let list = match card.kind {
                    Kind::Album => &mut lib.albums,
                    Kind::Artist => &mut lib.artists,
                    Kind::Playlist => &mut lib.playlists,
                    Kind::Show => &mut lib.shows,
                };
                list.retain(|c| c.id != card.id);
                if save {
                    list.insert(0, card.clone());
                }
                cache::save("library", &lib);
                let what = if save { "salvo na biblioteca" } else { "removido da biblioteca" };
                self.send(AppEvent::Notice(format!("\"{}\" {what}.", card.title)));
            }
            Err(e) => self.send(AppEvent::Notice(format!("Não deu para mudar \"{}\" na biblioteca: {e}", card.title))),
        }
        self.send(AppEvent::Library(lib));
    }

    async fn like(self, track: Track, on: bool) {
        let mut liked = cache::load::<Vec<Track>>("liked").unwrap_or_default();
        match self.api.set_saved(std::slice::from_ref(&track.uri), on).await {
            Ok(()) => {
                liked.retain(|t| t.uri != track.uri);
                if on {
                    liked.insert(0, track.clone());
                }
                cache::save("liked", &liked);
                self.send(AppEvent::Notice(if on { format!("\"{}\" curtida.", track.name) } else { format!("\"{}\" saiu das curtidas.", track.name) }));
            }
            Err(e) => self.send(AppEvent::Notice(format!("Não deu para mudar a curtida: {e}"))),
        }
        self.send(AppEvent::Liked(liked));
    }

    /// Mostra a página do cache na hora e atualiza em seguida. Álbuns não mudam: o cache basta.
    async fn open(self, card: Card) {
        if card.kind != Kind::Artist {
            self.backdrop(card.image.clone());
        }
        let name = cache::page_name(&card.id);
        let cached: Option<Page> = cache::load(&name);
        if let Some(page) = &cached {
            self.hero(page);
            self.thumbs(page_images(page));
            self.check_saved(&page.tracks);
            self.send(AppEvent::Page(page.clone()));
            if card.kind == Kind::Album {
                return;
            }
        }
        let fresh = match card.kind {
            // Pela sessão: funciona para playlists de qualquer dono, inclusive as do Spotify.
            // Aos poucos: o primeiro pedaço (ou o tanto que já estava no cache) e o resto conforme a lista rola.
            Kind::Playlist => match internal::playlist(&self.session, &card.id, 0, cached.as_ref().map_or(PLAYLIST_CHUNK, |p| p.more_from.unwrap_or(p.total.max(p.tracks.len())).max(PLAYLIST_CHUNK))).await {
                Ok(internal::PlaylistChunk { card: head, tracks, owner: username, total, next }) => {
                    // Nome e foto do dono; o Spotify não tem foto.
                    let profile = if username.is_empty() || username == "spotify" {
                        None
                    } else {
                        internal::user_profile(&self.session, &username).await.map_err(|e| log::warn!("perfil de {username}: {e}")).ok()
                    };
                    let fallback = if card.subtitle.is_empty() { head.subtitle.clone() } else { card.subtitle.clone() };
                    let owner = profile.as_ref().map_or(fallback, |p| p.0.clone());
                    let page_card = Card {
                        title: if head.title.is_empty() { card.title.clone() } else { head.title },
                        subtitle: owner.clone(),
                        image: card.image.clone().or(head.image),
                        ..card.clone()
                    };
                    let owner_image = profile.and_then(|p| p.1);
                    Ok(Page { card: page_card, meta: api::join_meta(&owner, total), tracks, albums: Vec::new(), note: None, artist: None, owner_image, more_from: next, total })
                }
                Err(e) => Err(e),
            },
            Kind::Artist => match self.api.page(&card).await {
                Ok(mut page) => {
                    // As mais tocadas vêm pela sessão; sem elas a página abre do mesmo jeito (o coração delas é conferido junto com a página).
                    match internal::artist_extras(&self.session, &card.id).await {
                        Ok((top, related)) => {
                            page.tracks = top;
                            if let Some(info) = page.artist.as_mut() {
                                info.related = related;
                            }
                        }
                        Err(e) => log::warn!("mais tocadas de {}: {e}", card.id),
                    }
                    let found = match self.api.search(&page.card.title, &[SearchKind::Playlists], 0).await {
                        Ok(r) => r.playlists,
                        Err(e) => {
                            log::warn!("playlists com {}: {e}", card.id);
                            Vec::new()
                        }
                    };
                    let featuring = self.featuring(&page.card, found).await;
                    if let Some(info) = page.artist.as_mut() {
                        info.featuring = featuring;
                    }
                    Ok(page)
                }
                Err(e) => Err(e),
            },
            _ => self.api.page(&card).await,
        };
        match fresh {
            Ok(page) => {
                cache::save(&name, &page);
                cache::prune_pages();
                self.hero(&page);
                self.thumbs(page_images(&page));
                self.check_saved(&page.tracks);
                self.send(AppEvent::Page(page));
            }
            Err(error) if cached.is_none() => self.send(AppEvent::PageFailed { id: card.id, error }),
            Err(error) => log::warn!("página {}: {error}", card.id),
        }
    }
}

/// Rádio de uma música: a playlist que o Spotify monta a partir dela (a mesma do "Ir para o rádio da música").
/// Sem ela, o nosso: algumas do mesmo artista no meio das mais tocadas de artistas parecidos com ele e com os
/// que a pessoa mais ouviu no Cyder nos últimos 30 dias. Tudo pela sessão, sem a cota da Web API.
async fn radio(session: &librespot::core::Session, seed: &Track) -> Result<Vec<Track>, String> {
    let spotify = match internal::radio_for(session, &seed.uri).await {
        Ok(uri) => internal::playlist(session, uri.trim_start_matches("spotify:playlist:"), 0, RADIO_MAX).await.map(|c| c.tracks),
        Err(e) => Err(e),
    };
    match spotify {
        Ok(tracks) if tracks.iter().any(|t| t.uri != seed.uri) => Ok(tracks.into_iter().filter(|t| t.uri != seed.uri).collect()),
        Ok(_) => own_radio(session, seed).await,
        Err(e) => {
            log::warn!("rádio do Spotify para {}: {e}", seed.name);
            own_radio(session, seed).await
        }
    }
}

const RADIO_MAX: usize = 50;

async fn own_radio(session: &librespot::core::Session, seed: &Track) -> Result<Vec<Track>, String> {
    use librespot::metadata::Metadata;
    let Some((artist, _)) = seed.artists.first().cloned() else { return Ok(Vec::new()) };
    let since = now_secs().saturating_sub(30 * 24 * 3600);
    let plays = tokio::task::spawn_blocking(crate::stats::load).await.unwrap_or_default();
    let mut count: HashMap<String, usize> = HashMap::new();
    for p in plays.iter().filter(|p| p.at >= since && !p.artist_id.is_empty() && p.artist_id != artist) {
        *count.entry(p.artist_id.clone()).or_default() += 1;
    }
    let mut top: Vec<(String, usize)> = count.into_iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let seeds: Vec<String> = std::iter::once(artist.clone()).chain(top.into_iter().take(2).map(|(id, _)| id)).collect();
    let uri = librespot::core::SpotifyUri::from_uri(&format!("spotify:artist:{artist}")).map_err(|e| e.to_string())?;
    let own: Vec<String> = librespot::metadata::Artist::get(session, &uri).await.map_err(|e| e.to_string())?
        .top_tracks.for_country(&session.country()).iter().filter_map(|u| u.to_uri().ok()).filter(|u| *u != seed.uri).take(3).collect();
    let exclude: HashSet<String> = own.iter().cloned().chain([seed.uri.clone()]).collect();
    let mut own = internal::track_metadata(session, &own).await?.into_iter();
    // As recomendadas vêm duas por artista: primeiro uma de cada, depois as segundas, com uma do artista a cada quatro.
    let mut recs: Vec<(usize, Track)> = internal::recommendations(session, &seeds, &exclude).await?.into_iter().enumerate().collect();
    recs.sort_by_key(|(i, _)| (i % 2, *i));
    let mut out = Vec::new();
    for (n, (_, t)) in recs.into_iter().enumerate() {
        if n % 4 == 0 {
            out.extend(own.next());
        }
        out.push(t);
    }
    out.extend(own);
    Ok(out)
}

/// Capa e letra da faixa `index`, em segundo plano.
/// Corpo do play da Web API para o Soloist: álbum e playlist vão como contexto (o Spotify segue a lista dele);
/// o resto vai como lista de faixas, até 100 em volta da escolhida. Devolve também onde a lista mandada acaba
/// na nossa (`usize::MAX` com contexto: o Spotify segue sozinho).
fn soloist_body(tracks: &[Track], index: usize, context: Option<&str>, position_ms: u32) -> (serde_json::Value, usize) {
    const MAX: usize = 100;
    let uri = tracks.get(index).map(|t| t.uri.as_str());
    match context.filter(|c| c.starts_with("spotify:album:") || c.starts_with("spotify:playlist:")) {
        Some(c) => match uri {
            Some(u) => (serde_json::json!({ "context_uri": c, "offset": { "uri": u }, "position_ms": position_ms }), usize::MAX),
            None => (serde_json::json!({ "context_uri": c }), usize::MAX),
        },
        None => {
            let start = index.saturating_sub(MAX / 4).min(tracks.len().saturating_sub(MAX));
            let uris: Vec<&str> = tracks.iter().skip(start).take(MAX).map(|t| t.uri.as_str()).collect();
            let end = start + uris.len();
            (serde_json::json!({ "uris": uris, "offset": { "position": index.saturating_sub(start) }, "position_ms": position_ms }), end)
        }
    }
}

/// O Cyder toca: pelo próprio player (nenhum aparelho remoto) ou pelo Soloist dele.
fn plays_here(remote: Option<&str>) -> bool {
    remote.is_none_or(|d| d == soloist::DEVICE_NAME)
}

/// A fila mudou (mover, limpar, smart shuffle, rádio): manda a lista nova para quem toca. Outro aparelho segue a
/// fila do Spotify e não recebe nada. Devolve até onde vai a lista mandada ao Soloist.
#[allow(clippy::too_many_arguments)]
fn reload_queue(ctx: &Ctx, pcmd: &UnboundedSender<PlayerCmd>, remote: Option<&str>, tracks: &[Track], index: usize, position_ms: u32, playing: bool, shuffle: bool, repeat: Repeat) -> usize {
    match remote {
        None => {
            let _ = pcmd.send(PlayerCmd::Reload { queue: tracks.to_vec(), index, position_ms });
            usize::MAX
        }
        Some(soloist::DEVICE_NAME) => {
            // Sem o contexto: a lista agora é a nossa, não a do álbum.
            let (body, end) = soloist_body(tracks, index, None, position_ms);
            tokio::spawn(play_on_soloist(ctx.clone(), body, shuffle, repeat, !playing));
            end
        }
        Some(_) => usize::MAX,
    }
}

/// Toca pelo Soloist: acha o aparelho na conta e manda o play, com o aleatório e o repetir do Cyder.
/// `paused`: era só trocar a lista com a música pausada; pausa de novo.
async fn play_on_soloist(ctx: Ctx, body: serde_json::Value, shuffle: bool, repeat: Repeat, paused: bool) {
    let mut device = None;
    // Logo depois de parear, o aparelho demora um pouco para aparecer na lista.
    for _ in 0..3 {
        match ctx.api.devices().await {
            Ok(list) => device = list.into_iter().find(|d| d.name == soloist::DEVICE_NAME).map(|d| d.id),
            Err(e) => return ctx.send(AppEvent::Notice(format!("Não deu para achar o Soloist: {e}"))),
        }
        if device.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
    let Some(id) = device else {
        return ctx.send(AppEvent::Notice("O Soloist não aparece nos aparelhos desta conta. Ele precisa ser escolhido no app do Spotify com a mesma conta do Cyder.".into()));
    };
    log::info!("tocando pelo Soloist ({id})");
    if let Err(e) = ctx.api.play_on(&id, body).await {
        return ctx.send(AppEvent::Notice(format!("O Soloist não aceitou o play: {e}")));
    }
    let repeat = match repeat { Repeat::Off => "off", Repeat::All => "context", Repeat::One => "track" };
    let pause = paused.then(|| format!("/pause?device_id={id}"));
    for path in [format!("/shuffle?state={shuffle}&device_id={id}"), format!("/repeat?state={repeat}&device_id={id}")].into_iter().chain(pause) {
        if let Err(e) = ctx.api.control(reqwest::Method::PUT, &path).await {
            log::warn!("soloist: {path}: {e}");
        }
    }
}

fn fetch_extras(http: &reqwest::Client, out: &UnboundedSender<AppEvent>, session: &librespot::core::Session, tracks: &[Track], index: usize) {
    let Some(track) = tracks.get(index).cloned() else { return };
    let cache = config::cache_dir();
    let (http2, out2, cache2, track2) = (http.clone(), out.clone(), cache.clone(), track.clone());
    tokio::spawn(async move {
        match images::prepare(&http2, &cache2, &track2).await {
            Ok(art) => drop(out2.send(AppEvent::Art { index, art })),
            Err(e) => log::warn!("capa de {}: {e}", track2.name),
        }
    });
    let (http, out, session) = (http.clone(), out.clone(), session.clone());
    tokio::spawn(async move {
        // Episódio de podcast não tem letra no LRCLIB.
        if track.uri.starts_with("spotify:episode:") {
            let _ = out.send(AppEvent::Lyrics { index, lyrics: Lyrics::NotFound });
            return;
        }
        // Falha não é cacheada: a próxima vez que a faixa tocar (ou "Tentar de novo") pede outra vez.
        let lyrics = lyrics::fetch(&http, &cache, &track).await.unwrap_or_else(|e| {
            log::warn!("letra de {}: {e}", track.name);
            Lyrics::Failed
        });
        // LRCLIB fora do ar ou sem a letra: tenta a do Spotify. Sem ela também, fica o resultado do LRCLIB.
        let lyrics = match lyrics {
            Lyrics::Failed | Lyrics::NotFound => lyrics::from_spotify(&session, &track).await.unwrap_or(lyrics),
            found => found,
        };
        let _ = out.send(AppEvent::Lyrics { index, lyrics });
    });
}

async fn run(mut cmds: UnboundedReceiver<Cmd>, out: UnboundedSender<AppEvent>) {
    let http = reqwest::Client::builder()
        // Sem isto uma conexão que trava deixa a tela carregando para sempre.
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("Cyder/", env!("CARGO_PKG_VERSION"), " (https://github.com/cyder-spotify)"))
        .build()
        .expect("http client");

    // Laço externo: se o player cair na inicialização, mostra o erro e recomeça do login no "Tentar de novo".
    loop {
        let ready = loop {
            match bootstrap(&http, &out).await {
                Ok(r) => break r,
                Err(e) => {
                    let _ = out.send(e);
                    if !wait_retry(&mut cmds).await {
                        return;
                    }
                }
            }
        };
        match run_ready(&http, &out, &mut cmds, ready).await {
            Flow::Exit => return,
            // Saiu da conta: o próximo login abre o navegador.
            Flow::Relogin => continue,
            Flow::Restart => {
                if !wait_retry(&mut cmds).await {
                    return;
                }
            }
        }
    }
}

/// Repassa os avisos do Spotify Connect da sessão: o id do aparelho ativo ("" quando nenhum toca).
/// O fluxo acaba junto com a sessão; a próxima (depois de reconectar) assina de novo.
fn watch_cluster(session: &librespot::core::Session, tx: UnboundedSender<String>) {
    let session = session.clone();
    tokio::spawn(async move {
        // Enquanto o Spirc ainda liga o dealer (logo depois de religar o player), não dá para se inscrever:
        // espera ele subir.
        let mut tries = 0;
        let mut updates = loop {
            match session.dealer().listen_for("hm://connect-state/v1/cluster", Message::from_raw::<ClusterUpdate>) {
                Ok(updates) => break updates,
                Err(e) if tries < 20 && !session.is_invalid() => {
                    tries += 1;
                    log::debug!("avisos do Spotify Connect, tentativa {tries}: {e}");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(e) => return log::warn!("avisos do Spotify Connect: {e}"),
            }
        };
        while let Some(update) = updates.next().await {
            let Ok(update) = update else { continue };
            let active = update.cluster.as_ref().map_or(String::new(), |c| c.active_device_id.clone());
            if tx.send(active).is_err() {
                break;
            }
        }
    });
}

enum Flow {
    Exit,
    Restart,
    Relogin,
}

/// Espera o usuário pedir outra tentativa; outros comandos não fazem sentido sem login.
async fn wait_retry(cmds: &mut UnboundedReceiver<Cmd>) -> bool {
    loop {
        match cmds.recv().await {
            Some(Cmd::Retry) => return true,
            Some(_) => continue,
            None => return false,
        }
    }
}

async fn run_ready(http: &reqwest::Client, out: &UnboundedSender<AppEvent>, cmds: &mut UnboundedReceiver<Cmd>, ready: Ready) -> Flow {
    let liked = ready.tracks;
    // Miniaturas do tamanho antigo (320 px): a pasta nova as substitui.
    let _ = std::fs::remove_dir_all(config::cache_dir().join("thumbs"));
    let resume = cache::load::<Resume>("resume").filter(|r| r.index < r.queue.len());
    // Sem nada tocado ainda, a fila começa vazia e a janela esconde o player até o primeiro play.
    let mut tracks = resume.as_ref().map_or_else(Vec::new, |r| r.queue.clone());
    // O que vai sendo salvo: onde está, de onde veio e os modos.
    let mut index = resume.as_ref().map_or(0, |r| r.index);
    let mut context = resume.as_ref().and_then(|r| r.context.clone());
    let mut label = resume.as_ref().map_or_else(String::new, |r| r.label.clone());
    let (mut shuffle, mut repeat) = resume.as_ref().map_or((false, Repeat::Off), |r| (r.shuffle, r.repeat));
    let mut volume = resume.as_ref().map_or(DEFAULT_VOLUME, |r| r.volume);
    let mut anchor = (resume.as_ref().map_or(0, |r| r.position_ms), std::time::Instant::now());
    let mut playing = false;
    let _ = out.send(AppEvent::Ready { liked: liked.clone(), queue: tracks.clone(), index, position_ms: anchor.0, label: label.clone(), context: context.clone() });
    // As curtidas guardadas já se sabe que estão curtidas; curtir e descurtir passam pelo Cyder.
    let saved_known = Arc::new(Mutex::new(liked.iter().map(|t| t.uri.clone()).collect::<HashSet<_>>()));
    let mut ctx = Ctx { saved_known, http: http.clone(), out: out.clone(), api: ready.api.clone(), session: ready.session.clone(), thumbs: Arc::new(Semaphore::new(6)) };
    // O que estava salvo aparece já; a versão nova chega quando a sessão subir.
    if let Some(lib) = cache::load::<Library>("library") {
        ctx.thumbs(library_images(&lib));
        ctx.send(AppEvent::Library(lib));
    }
    if let Some(shelves) = cache::load::<Vec<Shelf>>("home") {
        ctx.thumbs(shelf_images(&shelves));
        ctx.send(AppEvent::Home(shelves));
    }
    ctx.send(AppEvent::Recent(cache::load("history").unwrap_or_default()));
    if let Some((name, image)) = cache::load::<(String, Option<String>)>("profile") {
        ctx.thumbs(image.clone().into_iter().collect());
        ctx.send(AppEvent::Profile { name, image });
    }
    ctx.send(AppEvent::Lastfm(lastfm::saved().map(|a| a.user)));
    let mut pins = config::pins();
    ctx.send(AppEvent::Pins(pins.clone()));
    ctx.send(AppEvent::LibraryOrder(config::library_order()));
    // Faixas postas na fila do Spotify que ainda não tocaram, na ordem: a próxima entra depois delas.
    let mut queued: Vec<String> = Vec::new();
    // Última faixa posta no histórico deste computador.
    let mut noted = String::new();
    ctx.thumbs(liked.iter().chain(tracks.iter()).filter_map(|t| t.cover_small.clone()).collect());

    // Com outro aparelho tocando, o estado vem da Web API (o Spirc só conta o que ele mesmo toca).
    let mut device_id = ready.session.device_id().to_string();
    let mut remote: Option<String> = None;
    let mut remote_uri: Option<String> = None;
    // O Spotify Connect avisa pela sessão quando qualquer aparelho da conta muda o que toca; só então
    // pergunta à Web API o que é (a consulta a cada 8 s estourava o limite de pedidos do Client ID).
    let (cluster_tx, mut cluster_rx) = unbounded_channel::<String>();
    // Recomendações do smart shuffle prontas (vêm de uma tarefa à parte).
    let (smart_tx, mut smart_rx) = unbounded_channel::<Vec<Track>>();
    let mut smart: Vec<String> = Vec::new();
    // Rádio de uma música da busca: (URI da música, recomendadas), quando ficam prontas.
    let (radio_tx, mut radio_rx) = unbounded_channel::<(String, Vec<Track>)>();
    // Música tocando por aqui para as estatísticas: quando começou, quanto já tocou e desde quando está tocando.
    // O contexto (álbum, playlist) vai junto para o histórico do Spotify.
    let mut listen: Option<(Track, u64, u32, Option<std::time::Instant>, Option<String>)> = None;
    let mut cluster_due: Option<tokio::time::Instant> = None;
    // Volta da suspensão: a conexão da sessão pode voltar morta ou reconectando, e o aviso de que outro aparelho
    // começou a tocar se perde. O relógio de parede pula (o PC dormiu) e aí o estado é conferido pela Web API.
    let mut wake_tick = tokio::time::interval(WAKE_CHECK);
    wake_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut wall = std::time::SystemTime::now();
    // Última conferência pedida pela janela (Cmd::RefreshRemote), para não pedir a cada troca de janela.
    let mut remote_refreshed: Option<std::time::Instant> = None;
    // Reserva, só com outro aparelho tocando, se algum aviso se perder.
    let mut remote_tick = tokio::time::interval(REMOTE_FALLBACK);
    // Sem isto, os tiques perdidos com o ramo desligado (ninguém tocando fora) disparam todos juntos quando
    // outro aparelho começa: 3 h parado viravam ~380 GET /me/player num segundo e o Spotify bloqueava o Client ID.
    remote_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let (state_tx, mut state_rx) = unbounded_channel::<Option<api::PlayerState>>();
    let poll = |ctx: &Ctx, delay_ms: u64| {
        let (ctx, tx) = (ctx.clone(), state_tx.clone());
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            match ctx.api.player_state().await {
                Ok(s) => drop(tx.send(s)),
                Err(e) => log::warn!("estado do player: {e}"),
            }
        });
    };
    // A ordem do aleatório (e o que o autoplay escolheu) só o Spotify sabe; a fila dele vem pela Web API.
    let up_next = |ctx: &Ctx, delay_ms: u64| {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            match ctx.api.queue().await {
                Ok(list) => {
                    ctx.thumbs(list.iter().filter_map(|t| t.cover_small.clone()).collect());
                    ctx.send(AppEvent::UpNext(list));
                }
                Err(e) => log::warn!("fila do Spotify: {e}"),
            }
        });
    };
    // A daylist e os mixes mudam ao longo do dia: a biblioteca e a tela inicial se atualizam sozinhas.
    let mut refresh = tokio::time::interval_at(tokio::time::Instant::now() + REFRESH_EVERY, REFRESH_EVERY);
    // Depois de o PC dormir, uma atualização só, não uma por meia hora dormida.
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let (pcmd_tx, pcmd_rx) = unbounded_channel();
    let (pout_tx, mut pout_rx) = unbounded_channel();
    tokio::spawn(player::run(config::cache_dir(), ready.session, ready.credentials, volume, pcmd_rx, pout_tx));
    let _ = pcmd_tx.send(PlayerCmd::SetShuffle(shuffle));
    let _ = pcmd_tx.send(PlayerCmd::SetRepeat(repeat));
    let _ = pcmd_tx.send(PlayerCmd::Load { queue: tracks.clone(), index, context: context.clone(), position_ms: anchor.0 });
    let mut liked_from_cache = ready.liked_from_cache;
    // Com o Soloist pareado, o play vai para ele; o resto segue o modo "outro aparelho".
    let (soloist_tx, mut soloist_rx) = unbounded_channel();
    let mut _soloist = soloist::key().map(|key| soloist::start(ctx.http.clone(), key, soloist_tx.clone()));
    let mut soloist_ready = false;
    // O Spotify recusou o áudio: quando o Soloist ficar pronto, a fila continua por ele sem pedir nada.
    let mut resume_on_soloist = false;
    // Fora de álbum e playlist, o Soloist recebe até 100 faixas: o índice (na nossa lista) onde a mandada acaba.
    let mut soloist_end = usize::MAX;
    // Teste da chave de áudio ao abrir (uma vez): sem o Soloist, a recusa já aparece antes do primeiro play.
    let (refused_tx, mut refused_rx) = unbounded_channel::<bool>();
    let mut probed = false;
    ctx.send(AppEvent::Soloist(if _soloist.is_some() { soloist::Status::Starting("Abrindo o Soloist…".into()) } else { soloist::Status::Off }));
    // O app pode fechar a qualquer momento (o processo morre junto com a janela): salva com frequência.
    let mut autosave = tokio::time::interval(std::time::Duration::from_secs(5));
    // Ouvir junto: o código de quem convida e, de quem entrou, o que chega do canal.
    let mut hosting: Option<String> = None;
    let mut follow: Option<(String, UnboundedReceiver<Result<listen::State, String>>)> = None;
    let mut host_name = String::new();
    // Quem entra agora pega a música mesmo sem mudança nenhuma de quem convidou.
    let mut listen_tick = tokio::time::interval(std::time::Duration::from_secs(15));
    listen_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut dirty = false;
    // O Spotify não guarda onde o episódio parou quando toca pelo Cyder: fica aqui.
    let mut episodes: HashMap<String, u32> = cache::load("episode-positions").unwrap_or_default();
    ctx.send(AppEvent::EpisodePositions(episodes.clone()));

    loop {
        tokio::select! {
            _ = autosave.tick() => {
                if dirty || playing {
                    let position_ms = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
                    let position_ms = tracks.get(index).map_or(0, |t| position_ms.min(t.duration_ms));
                    cache::save("resume", &Resume { queue: tracks.clone(), index, position_ms, context: context.clone(), label: label.clone(), shuffle, repeat, volume });
                    dirty = false;
                    if tracks.get(index).is_some_and(|t| remember_episode(&mut episodes, t, position_ms)) {
                        cache::save("episode-positions", &episodes);
                        ctx.send(AppEvent::EpisodePositions(episodes.clone()));
                    }
                }
            }
            _ = listen_tick.tick(), if hosting.is_some() => publish_listen(&ctx, hosting.as_deref(), &tracks, index, playing, anchor),
            Some(msg) = async { follow.as_mut()?.1.recv().await }, if follow.is_some() => {
                let st = match msg {
                    Ok(st) => st,
                    Err(e) => {
                        log::warn!("ouvir junto: {e}");
                        continue;
                    }
                };
                if host_name != st.host {
                    host_name = st.host.clone();
                    if let Some((code, _)) = &follow {
                        ctx.send(AppEvent::Listen(Listen::Joined { code: code.clone(), host: host_name.clone() }));
                    }
                }
                // Um pouco à frente: a mensagem leva um tempo para chegar.
                let target = st.position_ms + if st.playing { LISTEN_DELAY_MS } else { 0 };
                if tracks.get(index).map(|t| &t.uri) != Some(&st.uri) {
                    let Ok(mut found) = internal::track_metadata(&ctx.session, std::slice::from_ref(&st.uri)).await else { continue };
                    if found.is_empty() {
                        continue;
                    }
                    tracks = vec![found.remove(0)];
                    (index, context, label, anchor, dirty) = (0, None, format!("Ouvindo junto com {}", st.host), (target, std::time::Instant::now()), true);
                    queued.clear();
                    ctx.thumbs(tracks.iter().filter_map(|t| t.cover_small.clone()).collect());
                    ctx.send(AppEvent::Queue(tracks.clone()));
                    ctx.send(AppEvent::TrackChanged(0));
                    if soloist_ready {
                        if st.playing {
                            let (body, end) = soloist_body(&tracks, 0, None, target);
                            soloist_end = end;
                            tokio::spawn(play_on_soloist(ctx.clone(), body, shuffle, repeat, false));
                            poll(&ctx, 1500);
                        }
                    } else {
                        let _ = pcmd_tx.send(if st.playing {
                            PlayerCmd::Play { queue: tracks.clone(), index: 0, context: None, start_ms: target, shuffle_start: false }
                        } else {
                            PlayerCmd::Load { queue: tracks.clone(), index: 0, context: None, position_ms: target }
                        });
                    }
                } else {
                    let here = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
                    // Pelo Soloist (ou outro aparelho), os ajustes vão pela Web API.
                    let mut calls = Vec::new();
                    if here.abs_diff(target) > LISTEN_DRIFT_MS {
                        match remote {
                            Some(_) => calls.push(format!("/seek?position_ms={target}")),
                            None => drop(pcmd_tx.send(PlayerCmd::Seek(target))),
                        }
                    }
                    if st.playing != playing {
                        match remote {
                            Some(_) => calls.push(if st.playing { "/play".into() } else { "/pause".into() }),
                            None => drop(pcmd_tx.send(PlayerCmd::Toggle)),
                        }
                    }
                    if !calls.is_empty() {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            for path in calls {
                                if let Err(e) = ctx.api.control(reqwest::Method::PUT, &path).await {
                                    log::warn!("ouvir junto: {path}: {e}");
                                }
                            }
                        });
                    }
                }
            }
            _ = refresh.tick() => {
                tokio::spawn(ctx.clone().refresh_library());
                tokio::spawn(ctx.clone().refresh_home());
            }
            _ = remote_tick.tick(), if remote.is_some() => poll(&ctx, 0),
            Some(st) = soloist_rx.recv() => {
                soloist_ready = st == soloist::Status::Ready;
                ctx.send(AppEvent::Soloist(st));
                if soloist_ready && std::mem::take(&mut resume_on_soloist) && !tracks.is_empty() {
                    let (body, end) = soloist_body(&tracks, index, context.as_deref(), anchor.0);
                    soloist_end = end;
                    tokio::spawn(play_on_soloist(ctx.clone(), body, shuffle, repeat, false));
                    poll(&ctx, 1500);
                }
            }
            Some(true) = refused_rx.recv() => {
                log::error!("o Spotify recusa a chave de áudio desta conta ao librespot");
                if _soloist.is_none() {
                    ctx.send(AppEvent::AudioRefused);
                }
            }
            _ = wake_tick.tick() => {
                let now = std::time::SystemTime::now();
                let gap = now.duration_since(wall).unwrap_or_default();
                wall = now;
                if gap > WAKE_GAP {
                    log::info!("o PC voltou da suspensão ({} s parado): conferindo se outro aparelho está tocando", gap.as_secs());
                    // A rede costuma levar uns segundos para voltar; a segunda pega o que a primeira perder.
                    poll(&ctx, 3_000);
                    poll(&ctx, 15_000);
                }
            }
            Some(recs) = smart_rx.recv() => {
                // Uma recomendada a cada três da lista, como o smart shuffle do Spotify.
                smart = recs.iter().map(|t| t.uri.clone()).collect();
                ctx.thumbs(recs.iter().filter_map(|t| t.cover_small.clone()).collect());
                tracks = internal::interleave(&tracks, index, recs, 3);
                queued.clear();
                context = None;
                dirty = true;
                let position_ms = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
                ctx.send(AppEvent::Reordered(tracks.clone()));
                ctx.send(AppEvent::SmartShuffle { on: true, added: smart.clone() });
                soloist_end = reload_queue(&ctx, &pcmd_tx, remote.as_deref(), &tracks, index, position_ms, playing, shuffle, repeat);
            }
            Some((seed, recs)) = radio_rx.recv() => {
                // Só se a fila ainda é a da música da busca: a pessoa pode ter mudado de música nesse meio-tempo.
                if plays_here(remote.as_deref()) && context.is_none() && tracks.len() == 1 && tracks[0].uri == seed {
                    tracks.extend(recs);
                    dirty = true;
                    let position_ms = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
                    ctx.send(AppEvent::Reordered(tracks.clone()));
                    soloist_end = reload_queue(&ctx, &pcmd_tx, remote.as_deref(), &tracks, index, position_ms, playing, shuffle, repeat);
                }
            }
            Some(active) = cluster_rx.recv() => {
                // Com o próprio Cyder tocando, o Spirc já conta tudo.
                if remote.is_some() || (!active.is_empty() && active != device_id) {
                    // Um play no celular gera vários avisos seguidos: junta numa consulta só.
                    cluster_due.get_or_insert(tokio::time::Instant::now() + CLUSTER_SETTLE);
                }
            }
            _ = tokio::time::sleep_until(cluster_due.unwrap_or_else(tokio::time::Instant::now)), if cluster_due.is_some() => {
                cluster_due = None;
                poll(&ctx, 0);
            }
            Some(state) = state_rx.recv() => match state {
                // Só assume o estado de outro aparelho se ele estiver tocando (ou já estava sendo seguido). Com o
                // Cyder tocando aqui, quem toca é ele: a conta só toca num aparelho. Sem isto, a consulta logo depois
                // de religar o player (trocar a saída de áudio, sessão que caiu) podia tomar o próprio Cyder por outro
                // aparelho, e o "próxima" ia pela Web API e pelo Spirc ao mesmo tempo: pulava duas.
                Some(st) if !st.device_id.is_empty() && st.device_id != device_id && (st.playing || remote.is_some()) && (remote.is_some() || !playing) => {
                    if remote.as_deref() != Some(st.device.as_str()) {
                        log::info!("seguindo outro aparelho: {} ({}); este é {device_id}", st.device, st.device_id);
                        remote = Some(st.device.clone());
                        ctx.send(AppEvent::Remote(Some(st.device.clone())));
                    }
                    if let Some(t) = st.track {
                        if remote_uri.as_deref() != Some(t.uri.as_str()) {
                            remote_uri = Some(t.uri.clone());
                            let i = match tracks.iter().position(|x| x.uri == t.uri) {
                                Some(i) => i,
                                None => {
                                    // Na fila do player também: o Spirc avisa a faixa pelo índice da fila dele, que
                                    // tem que bater com a nossa (sem isto, depois de trocar de aparelho a tela pulava
                                    // para outra música e o "próxima" parecia pular duas).
                                    let _ = pcmd_tx.send(PlayerCmd::Insert { at: tracks.len(), track: t.clone() });
                                    tracks.push(t.clone());
                                    label.clear();
                                    ctx.thumbs(t.cover_small.clone().into_iter().collect());
                                    ctx.send(AppEvent::TrackAdded(t));
                                    tracks.len() - 1
                                }
                            };
                            (index, context, dirty) = (i, st.context.clone(), true);
                            fetch_extras(http, out, &ctx.session, &tracks, i);
                            ctx.send(AppEvent::TrackChanged(i));
                            up_next(&ctx, 0);
                            if st.device == soloist::DEVICE_NAME {
                                // Começou a última da lista mandada: manda as próximas, quase do começo da faixa.
                                if i + 1 >= soloist_end && soloist_end < tracks.len() {
                                    soloist_end = reload_queue(&ctx, &pcmd_tx, remote.as_deref(), &tracks, i, st.position_ms, st.playing, st.shuffle, st.repeat);
                                }
                                // O Soloist manda o histórico do Spotify sozinho; as estatísticas do Cyder contam aqui.
                                if let Some((t, at, ms, since, _)) = listen.take() {
                                    let ms = (ms + since.map_or(0, |s| s.elapsed().as_millis() as u32)).min(t.duration_ms);
                                    if ms >= crate::stats::MIN_PLAYED_MS {
                                        tokio::task::spawn_blocking(move || crate::stats::record(&t, ms, at));
                                    }
                                }
                                listen = tracks.get(i).map(|t| (t.clone(), now_secs(), 0, None, context.clone()));
                            }
                        }
                    }
                    if st.device == soloist::DEVICE_NAME {
                        if let Some(l) = listen.as_mut() {
                            match (st.playing, l.3) {
                                (true, None) => l.3 = Some(std::time::Instant::now()),
                                (false, Some(since)) => {
                                    l.2 += since.elapsed().as_millis() as u32;
                                    l.3 = None;
                                }
                                _ => {}
                            }
                        }
                        if let Some(t) = tracks.get(index).filter(|t| st.playing && t.uri != noted) {
                            noted = t.uri.clone();
                            ctx.note_played(t);
                        }
                    }
                    let _ = pcmd_tx.send(PlayerCmd::Follow { index, context: context.clone(), position_ms: st.position_ms });
                    (playing, anchor) = (st.playing, (st.position_ms, std::time::Instant::now()));
                    ctx.send(AppEvent::Playback { playing, position_ms: st.position_ms });
                    volume = st.volume_percent.map_or(volume, |p| (p.min(100) * u16::MAX as u64 / 100) as u16);
                    (shuffle, repeat) = (st.shuffle, st.repeat);
                    ctx.send(AppEvent::Modes { shuffle, repeat, volume });
                }
                _ => {
                    if remote.take().is_some() {
                        remote_uri = None;
                        ctx.send(AppEvent::Remote(None));
                    }
                }
            },
            Some(cmd) = cmds.recv() => {
                // Outro aparelho tocando: os controles vão para ele pela Web API.
                if remote.is_some() {
                    use reqwest::Method;
                    let call = match &cmd {
                        Cmd::Toggle => Some((Method::PUT, if playing { "/pause".to_string() } else { "/play".to_string() })),
                        Cmd::Next => Some((Method::POST, "/next".into())),
                        Cmd::Prev => Some((Method::POST, "/previous".into())),
                        Cmd::Seek(ms) => Some((Method::PUT, format!("/seek?position_ms={ms}"))),
                        Cmd::SetShuffle(on) => Some((Method::PUT, format!("/shuffle?state={on}"))),
                        Cmd::SetRepeat(r) => Some((Method::PUT, format!("/repeat?state={}", match r { Repeat::Off => "off", Repeat::All => "context", Repeat::One => "track" }))),
                        Cmd::SetVolume(v) => Some((Method::PUT, format!("/volume?volume_percent={}", *v as u32 * 100 / u16::MAX as u32))),
                        _ => None,
                    };
                    if let Some((method, path)) = call {
                        log::info!("comando para {}: {path}", remote.as_deref().unwrap_or_default());
                        if matches!(cmd, Cmd::Toggle) {
                            // Responde já; a consulta seguinte confirma.
                            playing = !playing;
                            ctx.send(AppEvent::Playback { playing, position_ms: anchor.0 + anchor.1.elapsed().as_millis() as u32 });
                        }
                        let ctx2 = ctx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = ctx2.api.control(method, &path).await {
                                ctx2.send(AppEvent::Notice(format!("O outro aparelho não aceitou o comando: {e}")));
                            }
                        });
                        poll(&ctx, 700);
                        continue;
                    }
                }
                let pcmd = match cmd {
                    // Nada tocando: retoma a fila pelo Soloist.
                    Cmd::Toggle if soloist_ready && !playing && !tracks.is_empty() => {
                        let (body, end) = soloist_body(&tracks, index, context.as_deref(), anchor.0);
                        soloist_end = end;
                        tokio::spawn(play_on_soloist(ctx.clone(), body, shuffle, repeat, false));
                        poll(&ctx, 1500);
                        continue;
                    }
                    Cmd::Toggle => PlayerCmd::Toggle,
                    // Pausado há muito tempo, o Spotify esquece o Soloist: a fila segue por ele. O player daqui ainda
                    // tem a fila de quando o app abriu, e o "próxima" nele voltava a lista e esvaziava o player da tela.
                    Cmd::Next | Cmd::Prev if soloist_ready && !playing && !tracks.is_empty() => {
                        let i = match cmd {
                            Cmd::Prev => index.saturating_sub(1),
                            _ if index + 1 < tracks.len() => index + 1,
                            _ if repeat == Repeat::All => 0,
                            _ => continue,
                        };
                        let (body, end) = soloist_body(&tracks, i, context.as_deref(), 0);
                        soloist_end = end;
                        tokio::spawn(play_on_soloist(ctx.clone(), body, shuffle, repeat, false));
                        poll(&ctx, 1500);
                        continue;
                    }
                    Cmd::Next => PlayerCmd::Next,
                    Cmd::Prev => PlayerCmd::Prev,
                    Cmd::Seek(ms) => PlayerCmd::Seek(ms),
                    Cmd::SetShuffle(on) => {
                        if on {
                            up_next(&ctx, 1500);
                        }
                        PlayerCmd::SetShuffle(on)
                    }
                    Cmd::SetRepeat(r) => PlayerCmd::SetRepeat(r),
                    Cmd::SetVolume(v) => PlayerCmd::SetVolume(v),
                    Cmd::Play { tracks: queue, index: i, context: c, label: l, shuffle_start } => {
                        // O Spotify não registra o que toca pelo Cyder (doc 07): guardamos aqui.
                        if let Some(uri) = &c {
                            let mut history: HashMap<String, u64> = cache::load("history").unwrap_or_default();
                            history.insert(uri.clone(), now_secs());
                            cache::save("history", &history);
                            ctx.send(AppEvent::Recent(history));
                        }
                        tracks = queue.clone();
                        (index, context, label, anchor, dirty) = (i, c.clone(), l, (0, std::time::Instant::now()), true);
                        ctx.send(AppEvent::Queue(queue.clone()));
                        // Já mostra a faixa escolhida; sem isto a tela mostrava a primeira até o Spotify responder.
                        ctx.send(AppEvent::TrackChanged(i));
                        if shuffle {
                            up_next(&ctx, 2000);
                        }
                        // No Aleatório a faixa `i` é só um palpite da tela: não continua episódio nenhum.
                        let start_ms = queue.get(i).filter(|_| !shuffle_start).and_then(|t| episodes.get(&t.uri)).copied().unwrap_or(0);
                        if soloist_ready {
                            let (body, end) = soloist_body(&queue, i, c.as_deref(), start_ms);
                            soloist_end = end;
                            tokio::spawn(play_on_soloist(ctx.clone(), body, shuffle, repeat, false));
                            poll(&ctx, 1500);
                            continue;
                        }
                        PlayerCmd::Play { queue, index: i, context: c, start_ms, shuffle_start }
                    }
                    Cmd::Open(card) => {
                        tokio::spawn(ctx.clone().open(card));
                        continue;
                    }
                    Cmd::RefreshHome => {
                        tokio::spawn(ctx.clone().refresh_home());
                        continue;
                    }
                    Cmd::Pin { uri, on } => {
                        pins.retain(|p| *p != uri);
                        if on {
                            pins.insert(0, uri);
                        }
                        config::save_pins(&pins);
                        ctx.send(AppEvent::Pins(pins.clone()));
                        continue;
                    }
                    Cmd::Library { card, save } => {
                        tokio::spawn(ctx.clone().set_saved(card, save));
                        continue;
                    }
                    Cmd::Like { track, on } => {
                        tokio::spawn(ctx.clone().like(track, on));
                        continue;
                    }
                    Cmd::AddToPlaylist { playlist, uri } => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let text = match ctx.api.add_to_playlist(&playlist.id, &uri).await {
                                Ok(()) => format!("Adicionada a \"{}\".", playlist.title),
                                Err(e) => format!("Não deu para adicionar a \"{}\": {e}", playlist.title),
                            };
                            ctx.send(AppEvent::Notice(text));
                        });
                        continue;
                    }
                    Cmd::AddToQueue(track) => {
                        // Na nossa lista também, logo depois da atual (e das já enfileiradas), onde o Spotify vai tocar.
                        let at = (index + 1 + queued.len()).min(tracks.len());
                        tracks.insert(at, track.clone());
                        queued.push(track.uri.clone());
                        dirty = true;
                        ctx.thumbs(track.cover_small.clone().into_iter().collect());
                        // Tocando por aqui na ordem da lista: recarrega o Spirc com a lista nova, como o "mover na fila".
                        // Pela Web API a faixa só chegava ao Spirc depois de um tempo (ou nunca, com o Client ID
                        // limitado), e o "próxima" tocava a que vinha antes; o contexto do álbum também não tem a faixa.
                        // Com aleatório (a ordem é do Spotify) ou outro aparelho tocando, continua pela fila do Spotify.
                        if !shuffle && remote.is_none() {
                            context = None;
                            let position_ms = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
                            ctx.send(AppEvent::Reordered(tracks.clone()));
                            soloist_end = reload_queue(&ctx, &pcmd_tx, remote.as_deref(), &tracks, index, position_ms, playing, shuffle, repeat);
                            ctx.send(AppEvent::Notice("Adicionada à fila.".into()));
                            continue;
                        }
                        ctx.send(AppEvent::Inserted { at, track: track.clone() });
                        let _ = pcmd_tx.send(PlayerCmd::Insert { at, track: track.clone() });
                        up_next(&ctx, 1200);
                        let ctx = ctx.clone();
                        let uri = track.uri;
                        tokio::spawn(async move {
                            let text = match ctx.api.add_to_queue(&uri).await {
                                Ok(()) => "Adicionada à fila.".to_string(),
                                Err(e) => format!("Não deu para adicionar à fila: {e}"),
                            };
                            ctx.send(AppEvent::Notice(text));
                        });
                        continue;
                    }
                    Cmd::ListenHost(on) => {
                        follow = None;
                        hosting = on.then(listen::new_code);
                        ctx.send(AppEvent::Listen(hosting.clone().map_or(Listen::Off, Listen::Hosting)));
                        publish_listen(&ctx, hosting.as_deref(), &tracks, index, playing, anchor);
                        continue;
                    }
                    Cmd::ListenJoin(code) => {
                        hosting = None;
                        host_name.clear();
                        follow = code.map(|code| {
                            let (tx, rx) = unbounded_channel();
                            tokio::spawn(listen::follow(code.clone(), tx));
                            (code, rx)
                        });
                        ctx.send(AppEvent::Listen(follow.as_ref().map_or(Listen::Off, |(code, _)| Listen::Joined { code: code.clone(), host: String::new() })));
                        continue;
                    }
                    Cmd::Canvas(uri) => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let path = match internal::canvas(&ctx.session, &uri).await {
                                Ok(Some(url)) => images::canvas(&ctx.http, &config::cache_dir(), &url).await.map_err(|e| log::warn!("canvas {uri}: {e}")).ok(),
                                Ok(None) => None,
                                Err(e) => {
                                    log::warn!("canvas {uri}: {e}");
                                    None
                                }
                            };
                            ctx.send(AppEvent::Canvas { uri, path });
                        });
                        continue;
                    }
                    Cmd::Stats { range, offset_secs } => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let stats = tokio::task::spawn_blocking(move || crate::stats::compute(&crate::stats::load(), range, now_secs(), offset_secs)).await;
                            if let Ok(stats) = stats {
                                ctx.thumbs(stats.top_artists.iter().chain(&stats.top_tracks).filter_map(|r| r.image.clone()).collect());
                                ctx.send(AppEvent::Stats { range, stats });
                            }
                        });
                        continue;
                    }
                    Cmd::SmartShuffle(true) => {
                        let ctx = ctx.clone();
                        let smart_tx = smart_tx.clone();
                        // Os artistas que mais aparecem na fila puxam os parecidos.
                        let mut count: HashMap<String, usize> = HashMap::new();
                        for t in &tracks {
                            if let Some((id, _)) = t.artists.first() {
                                *count.entry(id.clone()).or_default() += 1;
                            }
                        }
                        let mut seeds: Vec<(String, usize)> = count.into_iter().collect();
                        seeds.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
                        let seeds: Vec<String> = seeds.into_iter().take(3).map(|(id, _)| id).collect();
                        let exclude: HashSet<String> = tracks.iter().map(|t| t.uri.clone()).collect();
                        tokio::spawn(async move {
                            match internal::recommendations(&ctx.session, &seeds, &exclude).await {
                                Ok(recs) if !recs.is_empty() => drop(smart_tx.send(recs)),
                                Ok(_) => ctx.send(AppEvent::Notice("Não achei recomendações para esta fila.".into())),
                                Err(e) => ctx.send(AppEvent::Notice(format!("Não deu para ligar o smart shuffle: {e}"))),
                            }
                        });
                        continue;
                    }
                    Cmd::SmartShuffle(false) => {
                        if !smart.is_empty() {
                            // Tira as recomendadas que ainda não tocaram.
                            let keep: Vec<Track> = tracks.iter().enumerate().filter(|(i, t)| *i <= index || !smart.contains(&t.uri)).map(|(_, t)| t.clone()).collect();
                            tracks = keep;
                            smart.clear();
                            queued.clear();
                            context = None;
                            dirty = true;
                            let position_ms = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
                            ctx.send(AppEvent::Reordered(tracks.clone()));
                            soloist_end = reload_queue(&ctx, &pcmd_tx, remote.as_deref(), &tracks, index, position_ms, playing, shuffle, repeat);
                        }
                        ctx.send(AppEvent::SmartShuffle { on: false, added: Vec::new() });
                        continue;
                    }
                    Cmd::SearchRadio(seed) => {
                        let (ctx, radio_tx) = (ctx.clone(), radio_tx.clone());
                        tokio::spawn(async move {
                            match radio(&ctx.session, &seed).await {
                                Ok(recs) if !recs.is_empty() => {
                                    ctx.thumbs(recs.iter().filter_map(|t| t.cover_small.clone()).collect());
                                    drop(radio_tx.send((seed.uri, recs)));
                                }
                                Ok(_) => log::warn!("rádio de {}: nada para tocar depois", seed.name),
                                Err(e) => log::warn!("rádio de {}: {e}", seed.name),
                            }
                        });
                        continue;
                    }
                    Cmd::ClearQueue => {
                        if !plays_here(remote.as_deref()) {
                            ctx.send(AppEvent::Notice("Só dá para limpar a fila tocando neste computador.".into()));
                            continue;
                        }
                        // As que já tocaram ficam, para o "anterior" e o índice continuarem valendo.
                        tracks.truncate(index + 1);
                        (queued, smart, context, dirty) = (Vec::new(), Vec::new(), None, true);
                        let position_ms = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
                        ctx.send(AppEvent::Reordered(tracks.clone()));
                        ctx.send(AppEvent::SmartShuffle { on: false, added: Vec::new() });
                        soloist_end = reload_queue(&ctx, &pcmd_tx, remote.as_deref(), &tracks, index, position_ms, playing, shuffle, repeat);
                        // Com o autoplay, o Spotify já pode ter escolhido as próximas.
                        up_next(&ctx, 1500);
                        continue;
                    }
                    Cmd::MoveUpNext { from_uri, to_uri } => {
                        if remote.is_some() {
                            continue;
                        }
                        // A tela já mostra a ordem nova; a fila do Spotify confirma (ou desfaz, se não deu).
                        up_next(&ctx, 800);
                        PlayerCmd::MoveNext { from_uri, to_uri }
                    }
                    Cmd::MoveQueue { from, to } => {
                        // Só o que vem depois da atual; a lista recarrega como faixas soltas na mesma posição.
                        if from > index && to > index && from < tracks.len() && to < tracks.len() && from != to {
                            let t = tracks.remove(from);
                            tracks.insert(to, t);
                            queued.clear();
                            context = None;
                            dirty = true;
                            let position_ms = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
                            ctx.send(AppEvent::Reordered(tracks.clone()));
                            soloist_end = reload_queue(&ctx, &pcmd_tx, remote.as_deref(), &tracks, index, position_ms, playing, shuffle, repeat);
                        }
                        continue;
                    }
                    Cmd::LoadMore { card, from } => {
                        tokio::spawn(ctx.clone().load_more(card, from));
                        continue;
                    }
                    Cmd::CreatePlaylist(name) => {
                        tokio::spawn(ctx.clone().create_playlist(name));
                        continue;
                    }
                    Cmd::RenamePlaylist { card, name } => {
                        tokio::spawn(ctx.clone().rename_playlist(card, name));
                        continue;
                    }
                    Cmd::RemoveFromPlaylist { card, uri } => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let r = ctx.api.remove_from_playlist(&card.id, &uri).await;
                            ctx.after_edit(card, r, "Não deu para tirar a faixa").await;
                        });
                        continue;
                    }
                    Cmd::MoveInPlaylist { card, from, to } => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let r = ctx.api.move_in_playlist(&card.id, from, to).await;
                            ctx.after_edit(card, r, "Não deu para mudar a ordem").await;
                        });
                        continue;
                    }
                    Cmd::SetPlaylistCover { card, path } => {
                        tokio::spawn(ctx.clone().set_cover(card, path));
                        continue;
                    }
                    Cmd::LoadMoreLiked { from } => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let ev = match ctx.api.liked_page(from, LIBRARY_LIMIT).await {
                                Ok((tracks, more)) => {
                                    ctx.thumbs(tracks.iter().filter_map(|t| t.cover_small.clone()).collect());
                                    ctx.check_saved(&tracks);
                                    AppEvent::LikedMore { from, tracks, more, error: None }
                                }
                                Err(e) => AppEvent::LikedMore { from, tracks: Vec::new(), more: true, error: Some(e) },
                            };
                            ctx.send(ev);
                        });
                        continue;
                    }
                    Cmd::Credits(track) => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            match internal::credits(&ctx.session, &track.uri).await {
                                Ok(c) => ctx.send(AppEvent::Credits(c)),
                                Err(e) => ctx.send(AppEvent::Notice(format!("Não deu para ver os créditos: {e}"))),
                            }
                        });
                        continue;
                    }
                    Cmd::Radio(track) => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            match internal::radio_for(&ctx.session, &track.uri).await {
                                Ok(uri) => {
                                    let id = uri.trim_start_matches("spotify:playlist:").to_string();
                                    ctx.send(AppEvent::Radio(Card { kind: Kind::Playlist, id, uri, title: format!("Rádio de {}", track.name), subtitle: "Spotify".into(), image: None }));
                                }
                                Err(e) => ctx.send(AppEvent::Notice(format!("Não deu para abrir o rádio: {e}"))),
                            }
                        });
                        continue;
                    }
                    Cmd::NewReleases { force } => {
                        tokio::spawn(ctx.clone().new_releases(force));
                        continue;
                    }
                    Cmd::LastfmConnect { key, secret } => {
                        tokio::spawn(ctx.clone().lastfm_connect(key, secret));
                        continue;
                    }
                    Cmd::LastfmDisconnect => {
                        lastfm::forget();
                        ctx.send(AppEvent::Lastfm(None));
                        continue;
                    }
                    Cmd::NowPlaying(track) => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let Some(account) = lastfm::saved() else { return };
                            if let Err(e) = lastfm::now_playing(&ctx.http, &account, &track).await {
                                ctx.lastfm_failed(e);
                            }
                        });
                        continue;
                    }
                    Cmd::Scrobble { track, at } => {
                        tokio::spawn(ctx.clone().scrobble(track, at));
                        continue;
                    }
                    Cmd::Search { query, kind, offset } => {
                        tokio::spawn(ctx.clone().search(query, kind, offset));
                        continue;
                    }
                    Cmd::SaveLibraryOrder(order) => {
                        config::save_library_order(&order);
                        ctx.send(AppEvent::LibraryOrder(order));
                        continue;
                    }
                    Cmd::Logout => {
                        auth::logout(&config::cache_dir());
                        // Fechar o canal do player desliga o aparelho Connect desta conta.
                        return Flow::Relogin;
                    }
                    Cmd::Devices | Cmd::Transfer(_) => {
                        if matches!(cmd, Cmd::Transfer(_)) {
                            poll(&ctx, 2500);
                        }
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            if let Cmd::Transfer(id) = &cmd {
                                if let Err(e) = ctx.api.transfer(id).await {
                                    ctx.send(AppEvent::Notice(format!("Não deu para trocar de aparelho: {e}")));
                                }
                            }
                            match ctx.api.devices().await {
                                Ok(d) => ctx.send(AppEvent::Devices(d)),
                                Err(e) => ctx.send(AppEvent::Notice(format!("Não deu para listar os aparelhos: {e}"))),
                            }
                        });
                        continue;
                    }
                    Cmd::Retry => continue,
                    Cmd::RefreshRemote => {
                        if remote_refreshed.is_none_or(|at| at.elapsed() >= REFRESH_REMOTE_GAP) {
                            remote_refreshed = Some(std::time::Instant::now());
                            poll(&ctx, 0);
                        }
                        continue;
                    }
                    Cmd::CheckUpdate => {
                        let (http, out) = (http.clone(), out.clone());
                        tokio::spawn(async move {
                            let _ = out.send(AppEvent::Update(update::State::Checking));
                            let state = match update::check(&http).await {
                                Ok(Some(release)) => update::State::Available(release),
                                Ok(None) => update::State::UpToDate,
                                Err(e) => {
                                    log::warn!("atualização: {e}");
                                    update::State::Failed(format!("Não deu para procurar atualizações: {e}"))
                                }
                            };
                            let _ = out.send(AppEvent::Update(state));
                        });
                        continue;
                    }
                    Cmd::InstallUpdate(release) => {
                        let out = out.clone();
                        tokio::spawn(async move {
                            // O cliente de sempre corta em 30 s, pouco para baixar o executável numa conexão lenta.
                            let http = reqwest::Client::builder()
                                .connect_timeout(std::time::Duration::from_secs(15))
                                .user_agent(concat!("Cyder/", env!("CARGO_PKG_VERSION")))
                                .build()
                                .unwrap_or_default();
                            let dir = config::cache_dir().join("update");
                            // Avisa a cada ponto percentual, não a cada pedaço do download.
                            let last = std::sync::atomic::AtomicU32::new(u32::MAX);
                            let progress = |p: f32| {
                                let pct = (p * 100.) as u32;
                                if last.swap(pct, std::sync::atomic::Ordering::Relaxed) != pct {
                                    let _ = out.send(AppEvent::Update(update::State::Downloading(release.clone(), p)));
                                }
                            };
                            progress(0.);
                            let result = update::download(&http, &release, &dir, progress).await.and_then(|path| update::install(&path));
                            let _ = out.send(AppEvent::Update(match result {
                                Ok(()) => update::State::Ready(release),
                                Err(e) => {
                                    log::warn!("atualização: {e}");
                                    update::State::Failed(format!("Não deu para atualizar: {e}"))
                                }
                            }));
                        });
                        continue;
                    }
                    Cmd::RetryLyrics(i) => {
                        fetch_extras(http, out, &ctx.session, &tracks, i);
                        continue;
                    }
                    Cmd::RestartPlayer => PlayerCmd::Restart,
                    Cmd::Relogin => {
                        auth::forget_web_login();
                        return Flow::Relogin;
                    }
                    Cmd::SoloistKey(key) => {
                        if let Err(e) = soloist::save_key(key.as_deref()) {
                            ctx.send(AppEvent::Notice(format!("Não deu para guardar a chave do Soloist: {e}")));
                            continue;
                        }
                        // Trocar a tarefa fecha o Soloist que estava aberto.
                        soloist_ready = false;
                        _soloist = key.map(|key| soloist::start(ctx.http.clone(), key, soloist_tx.clone()));
                        ctx.send(AppEvent::Soloist(if _soloist.is_some() { soloist::Status::Starting("Abrindo o Soloist…".into()) } else { soloist::Status::Off }));
                        continue;
                    }
                };
                let _ = pcmd_tx.send(pcmd);
            }
            Some(ev) = pout_rx.recv() => {
                let ev = match ev {
                    // A sessão conectou: as leituras internas (rootlist, playlists) já funcionam.
                    PlayerOut::Connected(session) => {
                        device_id = session.device_id().to_string();
                        watch_cluster(&session, cluster_tx.clone());
                        ctx.send(AppEvent::PlayerStatus(None));
                        if let Some(plan) = session.get_user_attribute("type") {
                            ctx.send(AppEvent::Plan(plan));
                        }
                        // Com o Soloist ligado, ele já toca: não precisa testar.
                        if !std::mem::replace(&mut probed, true) && _soloist.is_none() {
                            if let Some(uri) = tracks.iter().chain(liked.iter()).map(|t| t.uri.clone()).find(|u| u.starts_with("spotify:track:")) {
                                let (session, tx) = (session.clone(), refused_tx.clone());
                                tokio::spawn(async move {
                                    if let Some(refused) = player::audio_key_refused(&session, &uri).await {
                                        let _ = tx.send(refused);
                                    }
                                });
                            }
                        }
                        ctx.session = session;
                        // Outro aparelho já pode estar tocando quando o Cyder abre.
                        poll(&ctx, 1500);
                        // O que foi guardado há pouco não é pedido de novo: abrir o app várias vezes, mudar uma
                        // configuração de reprodução ou a sessão cair não gastam pedidos do Client ID.
                        // O timer de 30 min e os botões de atualizar continuam buscando a versão nova.
                        if !cache::fresh("profile", PROFILE_FRESH) {
                            let profile = ctx.clone();
                            tokio::spawn(async move {
                                match profile.api.me().await {
                                    Ok((name, image)) => {
                                        cache::save("profile", &(name.clone(), image.clone()));
                                        profile.thumbs(image.clone().into_iter().collect());
                                        profile.send(AppEvent::Profile { name, image });
                                    }
                                    Err(e) => log::warn!("perfil: {e}"),
                                }
                            });
                        }
                        if std::mem::take(&mut liked_from_cache) && !cache::fresh("liked", REFRESH_EVERY) {
                            tokio::spawn(ctx.clone().refresh_liked());
                        }
                        if !cache::fresh("library", REFRESH_EVERY) {
                            tokio::spawn(ctx.clone().refresh_library());
                        }
                        if !cache::fresh("home", REFRESH_EVERY) {
                            tokio::spawn(ctx.clone().refresh_home());
                        }
                        continue;
                    }
                    PlayerOut::Refused => {
                        log::error!("várias faixas seguidas não carregaram; provável recusa da chave de áudio");
                        resume_on_soloist = true;
                        if soloist_ready {
                            resume_on_soloist = false;
                            let (body, end) = soloist_body(&tracks, index, context.as_deref(), anchor.0);
                            soloist_end = end;
                            tokio::spawn(play_on_soloist(ctx.clone(), body, shuffle, repeat, false));
                            poll(&ctx, 1500);
                            continue;
                        }
                        if _soloist.is_some() {
                            AppEvent::Notice("O Spotify recusou o áudio ao player do Cyder. A música continua pelo Soloist assim que ele ficar pronto.".into())
                        } else {
                            AppEvent::AudioRefused
                        }
                    }
                    PlayerOut::TrackAdded(t) => {
                        // O que o Connect conta da faixa não traz os ids de artista e álbum; a sessão traz.
                        // Espera aqui (rápido, com limite) para o TrackChanged seguinte já achar a faixa completa.
                        let full = tokio::time::timeout(std::time::Duration::from_secs(3), internal::track_metadata(&ctx.session, std::slice::from_ref(&t.uri))).await;
                        let t = match full {
                            Ok(Ok(mut v)) if !v.is_empty() => v.remove(0),
                            _ => t,
                        };
                        tracks.push(t.clone());
                        // Fica salvo na retomada sem o "De <lista>" antigo.
                        label.clear();
                        dirty = true;
                        AppEvent::TrackAdded(t)
                    }
                    PlayerOut::TrackChanged(i) => {
                        if remote.take().is_some() {
                            remote_uri = None;
                            ctx.send(AppEvent::Remote(None));
                        }
                        if shuffle || i + 1 >= tracks.len() {
                            up_next(&ctx, 800);
                        }
                        if let Some(t) = tracks.get(i) {
                            queued.retain(|u| *u != t.uri);
                        }
                        // Trocou de música: a anterior entra nas estatísticas se tocou o bastante.
                        if i != index || listen.is_none() {
                            if let Some((t, at, ms, since, play_context)) = listen.take() {
                                let ms = (ms + since.map_or(0, |s| s.elapsed().as_millis() as u32)).min(t.duration_ms);
                                // O Spotify recebe toda reprodução, até a pulada (o client oficial faz igual): ele decide o que conta.
                                if ms > 0 {
                                    let play = history::EndSong { track_uri: t.uri.clone(), context: play_context, started_ms: at * 1000, ms_played: ms, duration_ms: t.duration_ms, shuffle };
                                    let ctx = ctx.clone();
                                    tokio::spawn(async move {
                                        // Avisa uma vez quando para de funcionar (endereço mudou, evento recusado, sem rede) e de novo só depois de voltar.
                                        static FAILING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                                        match history::report_end_song(&ctx.session, &play).await {
                                            Ok(()) => {
                                                FAILING.store(false, std::sync::atomic::Ordering::Relaxed);
                                                log::debug!("histórico do Spotify: {} ({} ms)", play.track_uri, play.ms_played);
                                            }
                                            Err(e) => {
                                                log::warn!("histórico do Spotify: {e}");
                                                if !FAILING.swap(true, std::sync::atomic::Ordering::Relaxed) {
                                                    ctx.send(AppEvent::Notice("As músicas tocadas aqui pararam de ir para o histórico do Spotify.".into()));
                                                }
                                            }
                                        }
                                    });
                                }
                                if ms >= crate::stats::MIN_PLAYED_MS {
                                    tokio::task::spawn_blocking(move || crate::stats::record(&t, ms, at));
                                }
                            }
                            listen = tracks.get(i).map(|t| (t.clone(), now_secs(), 0, playing.then(std::time::Instant::now), context.clone()));
                        }
                        // O episódio que estava tocando guarda onde parou (perto do fim, conta como ouvido).
                        if i != index {
                            let position_ms = anchor.0 + if playing { anchor.1.elapsed().as_millis() as u32 } else { 0 };
                            if tracks.get(index).is_some_and(|t| remember_episode(&mut episodes, t, position_ms)) {
                                cache::save("episode-positions", &episodes);
                                ctx.send(AppEvent::EpisodePositions(episodes.clone()));
                            }
                            // Passou para um episódio começado: continua de onde parou.
                            if let Some(&at) = tracks.get(i).and_then(|t| episodes.get(&t.uri)) {
                                let _ = pcmd_tx.send(PlayerCmd::Seek(at));
                            }
                        }
                        (index, dirty) = (i, true);
                        publish_listen(&ctx, hosting.as_deref(), &tracks, index, playing, anchor);
                        fetch_extras(http, out, &ctx.session, &tracks, i);
                        // Pré-busca a próxima para a letra já estar pronta na troca (doc 04).
                        fetch_extras(http, out, &ctx.session, &tracks, i + 1);
                        AppEvent::TrackChanged(i)
                    }
                    PlayerOut::Playback { playing: p, position_ms } => {
                        (playing, anchor, dirty) = (p, (position_ms, std::time::Instant::now()), true);
                        publish_listen(&ctx, hosting.as_deref(), &tracks, index, playing, anchor);
                        if let Some(l) = listen.as_mut() {
                            match (p, l.3) {
                                (true, None) => l.3 = Some(std::time::Instant::now()),
                                (false, Some(since)) => {
                                    l.2 += since.elapsed().as_millis() as u32;
                                    l.3 = None;
                                }
                                _ => {}
                            }
                        }
                        // Conta quando a faixa começa a tocar de fato por aqui, não quando a fila é só carregada.
                        if let Some(t) = tracks.get(index).filter(|t| p && t.uri != noted) {
                            noted = t.uri.clone();
                            ctx.note_played(t);
                        }
                        AppEvent::Playback { playing: p, position_ms }
                    }
                    PlayerOut::Modes { shuffle: s, repeat: r, volume: v } => {
                        if s && !shuffle {
                            up_next(&ctx, 1200);
                        }
                        (shuffle, repeat, volume, dirty) = (s, r, v, true);
                        AppEvent::Modes { shuffle: s, repeat: r, volume: v }
                    }
                    PlayerOut::Idle => {
                        poll(&ctx, 1500);
                        continue;
                    }
                    PlayerOut::Error(e) => AppEvent::Notice(e),
                    PlayerOut::Offline(why) => AppEvent::PlayerStatus(Some(why)),
                    PlayerOut::Fatal(e) => {
                        let _ = out.send(AppEvent::Failed(e));
                        return Flow::Restart;
                    }
                };
                if out.send(ev).is_err() {
                    return Flow::Exit;
                }
            }
            else => return Flow::Exit,
        }
    }
}

#[cfg(test)]
mod episode_tests {
    use std::collections::HashMap;

    #[test]
    fn remembers_where_it_stopped() {
        let ep = crate::Track { id: "e".into(), uri: "spotify:episode:e".into(), name: "E".into(), artist: "P".into(), album: String::new(), album_id: String::new(), duration_ms: 600_000, cover_small: None, cover_large: None, artists: Vec::new() };
        let mut map = HashMap::new();
        assert!(!super::remember_episode(&mut map, &ep, 5_000));
        assert!(super::remember_episode(&mut map, &ep, 120_000));
        assert_eq!(map.get(&ep.uri), Some(&120_000));
        // Perto do fim: ouvido, sai da lista.
        assert!(super::remember_episode(&mut map, &ep, 590_000));
        assert!(map.is_empty());
        let song = crate::Track { uri: "spotify:track:t".into(), ..ep };
        assert!(!super::remember_episode(&mut map, &song, 120_000));
    }
}

#[cfg(test)]
mod recent_tests {
    use crate::{Card, Kind};

    fn album(id: &str, title: &str) -> Card {
        Card { kind: Kind::Album, id: id.into(), uri: format!("spotify:album:{id}"), title: title.into(), subtitle: "X".into(), image: None }
    }

    #[test]
    fn merges_by_time_without_repeats() {
        let spotify = vec![(album("a", "A"), 30), (album("b", "B"), 10)];
        // "B" tocado aqui depois, com outro id (outra edição): sobe e não repete.
        let local = vec![(album("b2", "B"), 40), (album("c", "C"), 20)];
        let ids: Vec<String> = super::recent_albums(&spotify, &local).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["b2", "a", "c"]);
    }
}

#[cfg(test)]
mod soloist_tests {
    fn track(n: usize) -> crate::Track {
        crate::Track { id: n.to_string(), uri: format!("spotify:track:{n}"), name: String::new(), artist: String::new(), album: String::new(), album_id: String::new(), duration_ms: 1, cover_small: None, cover_large: None, artists: Vec::new() }
    }

    #[test]
    fn play_body() {
        let tracks: Vec<_> = (0..300).map(track).collect();
        let (b, end) = super::soloist_body(&tracks, 5, Some("spotify:album:a"), 1000);
        assert_eq!(end, usize::MAX);
        assert_eq!(b["context_uri"], "spotify:album:a");
        assert_eq!(b["offset"]["uri"], "spotify:track:5");
        // Sem álbum nem playlist: 100 faixas em volta, com a escolhida no lugar certo.
        let (b, end) = super::soloist_body(&tracks, 150, Some("spotify:collection"), 0);
        assert_eq!(b["uris"].as_array().unwrap().len(), 100);
        assert_eq!(end, 225);
        assert_eq!(b["uris"][b["offset"]["position"].as_u64().unwrap() as usize], "spotify:track:150");
        let (b, end) = super::soloist_body(&tracks, 290, None, 0);
        assert_eq!(end, 300);
        assert_eq!(b["uris"][b["offset"]["position"].as_u64().unwrap() as usize], "spotify:track:290");
        let (b, _) = super::soloist_body(&tracks[..3], 2, None, 0);
        assert_eq!(b["offset"]["position"], 2);
    }
}
