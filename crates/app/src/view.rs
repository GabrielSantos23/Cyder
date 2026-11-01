//! Tela principal: modo imersivo com letra sincronizada (doc 05, tela 1).
use std::{cell::Cell, collections::{HashMap, HashSet}, path::PathBuf, rc::Rc, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

use cyder_core::{AppEvent, Cmd, Device, Library, Page, SearchKind, SearchResults, Shelf, Track, config, images::Art, lyrics::{Line, Lyrics}, player::Repeat};
use gpui::{
    Animation, AnimationExt, AnyElement, Bounds, ClickEvent, ClipboardItem, Context, FocusHandle, Font, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement, Pixels, Render, SharedString,
    SpringAnimation, SpringConfig, Styled, TextRun, Window, WindowControlArea, actions, canvas, div, ease_out_quint,
    Div, font, img, point, prelude::*, px, relative, rgba, svg,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::theme::{self, glass};

mod canvas;
mod discord;
mod library;
mod loading;
mod media;
mod menus;
mod mini;
mod onboarding;
#[cfg(windows)]
mod thumbbar;
#[cfg(windows)]
mod tray;
pub(crate) mod update;
#[cfg(windows)]
pub use tray::hide as hide_window;
#[cfg(windows)]
pub use tray::{running_window, show as show_window};
use library::{Screen, Section, Side, SideFilter};
use menus::{Menu, Sleep};

/// Janela por cima de tudo, com fundo escurecido.
pub(crate) enum Dialog {
    Shortcuts,
    Logout,
    /// Campo de texto: nome de playlist, chaves do Last.fm, app do Discord.
    Input { ask: Ask, text: String },
    Credits(cyder_core::Credits),
    /// Passo a passo para ligar uma integração.
    Help(Help),
    /// Versão nova instalada: reiniciar agora?
    Update(String),
    /// Depois do primeiro login: o que dá para fazer e onde fica.
    Welcome,
    /// Pergunta, uma vez, se pode mandar relatórios de erro.
    Reports,
}

#[derive(Clone, Copy)]
pub(crate) enum Help {
    Lastfm,
    Discord,
    /// Também abre sozinho quando o Spotify recusa o áudio desta conta.
    Soloist,
}

impl Help {
    /// Título, passos, aviso, botão do site e o endereço dele.
    fn content(self) -> (&'static str, &'static [&'static str], &'static str, &'static str, &'static str) {
        match self {
            Help::Lastfm => ("Como ligar o Last.fm", &[
                "Entre na sua conta do Last.fm no navegador. Se não tiver uma, crie; é de graça.",
                "Clique em Abrir o Last.fm, abaixo, para chegar à página de criar uma chave de API.",
                "Em Application name, escreva Cyder. A descrição pode ser qualquer uma.",
                "Deixe Callback URL e Application homepage em branco: o Cyder entra como app de computador, que não usa esses campos.",
                "Envie. A página mostra a API key e o Shared secret; deixe-a aberta.",
                "Aqui no Cyder, clique em Ligar, cole a API key e depois o Shared secret.",
                "O navegador abre o Last.fm pedindo acesso: clique em Yes, allow access. Pronto, o que tocar aqui entra no seu histórico.",
            ], "Se você já ligou o Spotify ao Last.fm, cada música pode entrar duas vezes no histórico. Desligue o Spotify em Configurações do Last.fm › Aplicativos e use só o Cyder.",
            "Abrir o Last.fm", "https://www.last.fm/api/account/create"),
            Help::Discord => ("Como ligar o Discord", &[
                "Clique em Abrir o portal, abaixo, e entre com a sua conta do Discord.",
                "Clique em New Application e dê o nome que deve aparecer no seu perfil. Com Cyder, aparece Ouvindo Cyder.",
                "Aceite os termos e clique em Create.",
                "Na página General Information, copie o Application ID.",
                "Aqui no Cyder, clique em Configurar e cole o ID.",
                "Deixe o Discord aberto neste computador. Se nada aparecer, ligue Compartilhar a sua atividade em Configurações do Discord › Privacidade de atividade.",
            ], "Se o Spotify já está conectado ao seu Discord, a música aparece duas vezes no perfil. Desconecte o Spotify em Configurações do Discord › Conexões e use só o Cyder.",
            "Abrir o portal", "https://discord.com/developers/applications"),
            Help::Soloist => ("O Spotify recusou o áudio desta conta", &[
                "Em Gerar chave, crie a chave do Soloist com a sua conta Premium.",
                "Clique em Colar chave e cole.",
                "No app do Spotify, escolha “Cyder (Soloist)” nos aparelhos. Só na primeira vez.",
            ], "O Soloist é o player oficial do Spotify; o Cyder passa a tocar por ele.",
            "Gerar chave", cyder_core::soloist::KEY_PAGE),
        }
    }
}

/// O que a janela de texto pede.
#[derive(Clone)]
pub(crate) enum Ask {
    /// Nome de playlist: nova (None) ou renomear esta.
    Playlist(Option<cyder_core::Card>),
    LastfmKey,
    /// Segredo, depois da chave.
    LastfmSecret(String),
    DiscordApp,
    SoloistKey,
    /// Código de quem convidou para ouvir junto.
    ListenCode,
}

impl Ask {
    /// Título, texto de ajuda, dica do campo, botão e tamanho máximo.
    fn labels(&self) -> (&'static str, Option<&'static str>, &'static str, &'static str, usize) {
        match self {
            Ask::Playlist(None) => ("Criar playlist", None, "Nome da playlist", "Criar", 100),
            Ask::Playlist(Some(_)) => ("Renomear playlist", None, "Nome da playlist", "Salvar", 100),
            Ask::LastfmKey => ("Ligar o Last.fm", Some("Crie um app de API no Last.fm (é de graça) e cole aqui a chave dele. Depois vem o segredo."), "API key", "Continuar", 64),
            Ask::LastfmSecret(_) => ("Ligar o Last.fm", Some("Agora o segredo compartilhado do mesmo app. Ao ligar, o navegador abre para você autorizar o Cyder."), "Shared secret", "Ligar", 64),
            Ask::ListenCode => ("Ouvir junto", Some("Digite o código que a pessoa recebeu ao convidar. O Cyder toca o mesmo que ela, na mesma hora, cada um pela própria conta."), "Código", "Entrar", 16),
            Ask::SoloistKey => ("Ligar o Soloist", Some("Gere a chave no painel do Spotify (precisa de Premium) e cole aqui. Ela fica só neste computador."), "Chave do Soloist", "Ligar", 200),
            Ask::DiscordApp => ("Discord", Some("Crie um aplicativo no portal de desenvolvedores do Discord com o nome que deve aparecer no seu perfil (por exemplo, Cyder) e cole aqui o Application ID dele."), "Application ID", "Salvar", 32),
        }
    }
}

/// O que os relatórios de erro levam, na pergunta e nas configurações.
pub(crate) const REPORTS_TEXT: &str = "Manda ao desenvolvedor os erros do Cyder, com as versões do app e do Windows. Sem o nome da conta, o que você ouve ou os seus arquivos.";

/// Chave de uma faixa oculta num álbum ou playlist.
pub(crate) fn hidden_key(context_uri: &str, track_uri: &str) -> String {
    format!("{context_uri}|{track_uri}")
}

actions!(cyder, [TogglePlay, SeekForward, SeekBack, NextTrack, PrevTrack, FocusNext, FocusPrev, VolumeUp, VolumeDown, ToggleLyrics, ExitImmersive, FocusSearch, Quit]);

/// "Continuar tocando ao fechar a janela": o fechar da janela (em main.rs) só minimiza.
pub static KEEP_PLAYING: AtomicBool = AtomicBool::new(false);

const SEEK_STEP_MS: u32 = 5000;
const SKIP_GAP: Duration = Duration::from_millis(250);
const VOLUME_SEND_GAP: Duration = Duration::from_millis(80);
const VOLUME_ECHO_IGNORE: Duration = Duration::from_millis(600);
const VOLUME_STEP: f32 = 0.05;
const SLIDER_W: f32 = 112.;
/// Mola criticamente amortecida, a mesma da rolagem da letra.
const SPRING: SpringConfig = SpringConfig::new(380., 39., 1.);
/// Tamanhos da letra sincronizada: grande no imersivo, compacta no painel lateral.
pub(crate) struct LyricStyle {
    size: f32,
    line_h: f32,
    gap: f32,
    pad_x: f32,
    /// Tamanho do texto dos estados vazios ("Sem letra…").
    title: f32,
}

const IMMERSIVE_LYRICS: LyricStyle = LyricStyle { size: 32., line_h: 40., gap: 18., pad_x: 44., title: 22. };
pub(crate) const PANEL_LYRICS: LyricStyle = LyricStyle { size: 21., line_h: 27., gap: 12., pad_x: 16., title: 16. };
const PLAYER_H: f32 = 56.;
const PLAYER_BOTTOM: f32 = 20.;
const KNOB: f32 = 12.;
/// Sem mexer no mouse/teclado por este tempo, a barra do player some (só enquanto toca).
const IDLE_HIDE: Duration = Duration::from_secs(3);
/// Capas decodificadas mantidas na memória. A tela mais cheia (home + sidebar) mostra ~66 de uma vez;
/// abaixo disso as capas seriam recarregadas a cada quadro.
const IMAGES_KEPT: usize = 96;
const NOTICE_FOR: Duration = Duration::from_secs(6);

enum Phase {
    Starting(String),
    Setup,
    Failed(String),
    Ready,
}

/// Alturas das linhas da letra para uma largura, calculadas com o mesmo shaping que o GPUI usa ao desenhar.
struct LyricLayout {
    index: usize,
    width: i32,
    size: i32,
    tops: Vec<f32>,
}

pub struct Cyder {
    cmds: UnboundedSender<Cmd>,
    phase: Phase,
    /// Fila que está tocando; `current` indexa aqui.
    tracks: Vec<Track>,
    current: usize,
    liked: Vec<Track>,
    library: Option<Library>,
    pages: HashMap<String, Page>,
    page_errors: HashMap<String, String>,
    /// URL da imagem → miniatura em disco.
    thumbs: HashMap<String, PathBuf>,
    /// Mesma imagem em 64 px, para listas.
    thumbs_small: HashMap<String, PathBuf>,
    screen: Screen,
    /// Telas anteriores e seguintes, para ‹ ›.
    history: Vec<Screen>,
    forward: Vec<Screen>,
    sidebar_open: bool,
    /// Coluna da direita: fila ou letra.
    side: Option<Side>,
    home: Option<Vec<Shelf>>,
    /// Rolagem horizontal de cada prateleira da tela inicial.
    shelf_scrolls: HashMap<usize, gpui::ScrollHandle>,
    /// URI de contexto → quando tocou por último (segundos Unix).
    recent: HashMap<String, u64>,
    pins: Vec<String>,
    devices_open: bool,
    /// Fotos grandes de artista: URL → arquivo.
    heroes: HashMap<String, PathBuf>,
    /// Filtro da discografia na página de artista: tudo, só álbuns ou só singles e EPs.
    release_filter: library::Releases,
    /// Discografia inteira aberta pelo "Ver mais"; fechada mostra duas linhas.
    discography_open: bool,
    /// Fundos borrados das capas das páginas: URL da capa → arquivo.
    backdrops: HashMap<String, PathBuf>,
    /// Fundo que está saindo e o que está entrando no painel; `bg_gen` reinicia a transição.
    bg_from: Option<PathBuf>,
    bg_to: Option<PathBuf>,
    bg_gen: usize,
    /// Rolagem (de altura variável: cabeçalho + faixas) de cada página de álbum/playlist.
    page_lists: HashMap<String, gpui::ListState>,
    /// Menu suspenso aberto (botão direito, "⋯", filtro, conta).
    menu: Option<Menu>,
    dialog: Option<Dialog>,
    /// Nome e foto da conta.
    profile: Option<(String, Option<String>)>,
    /// Próximas da fila segundo o Spotify (aleatório, autoplay, outro aparelho).
    up_next: Option<Vec<Track>>,
    /// Outro aparelho tocando.
    remote: Option<String>,
    /// Posição da letra rolada à mão; None = acompanha a música.
    lyrics_manual: Option<f32>,
    sleep: Option<Sleep>,
    /// Curtida ou não, por URI, segundo o Spotify (as curtidas guardadas são só as 200 mais recentes).
    saved: HashMap<String, bool>,
    /// Playlists grandes com um pedaço a caminho.
    loading_more: HashSet<String>,
    /// Ordem da lista da sidebar arrastada à mão (URIs).
    library_order: Vec<String>,
    /// Sidebar: o que a lista mostra e a busca dentro dela.
    side_filter: SideFilter,
    side_query: String,
    side_search_open: bool,
    side_search_focus: FocusHandle,
    /// Busca no Spotify: o texto do campo, quando buscar (espera a pessoa parar de digitar) e o resultado.
    search_query: String,
    search_focus: FocusHandle,
    search_due: Option<Instant>,
    search: Option<SearchResults>,
    search_loading: bool,
    search_error: Option<String>,
    /// None: tudo; Some: só um tipo, com mais resultados.
    search_filter: Option<SearchKind>,
    /// Tipos que já pediram mais resultados nesta busca.
    search_more: HashSet<SearchKind>,
    /// Tipos que já vieram inteiros (o Spotify não tem mais, ou chegou ao limite de 1000).
    search_done: HashSet<SearchKind>,
    recent_searches: Vec<String>,
    /// Tela de configurações.
    settings: config::Settings,
    /// "premium", "free"…, pela sessão.
    plan: Option<String>,
    /// O player oficial do Spotify (Soloist), quando ligado.
    soloist: cyder_core::soloist::Status,
    /// None: o player está conectado; Some: o motivo de não estar.
    player_status: Option<String>,
    /// Nome do aparelho sendo digitado (vale no Enter).
    device_draft: String,
    device_focus: FocusHandle,
    /// Religa o player um pouco depois da última mudança, para várias mudanças seguidas religarem uma vez só.
    restart_due: Option<Instant>,
    /// Onde a janela foi parar; salva um pouco depois de parar de mover, não a cada pixel do arraste.
    place_due: Option<(config::WindowPlace, Instant)>,
    /// Título da janela que já foi passado ao Windows (Alt+Tab, barra de tarefas).
    window_title: String,
    /// Versão nova: procurando, disponível, baixando, pronta. `update_manual`: pedida pela pessoa (aí os erros aparecem).
    update: cyder_core::update::State,
    update_manual: bool,
    update_checked: bool,
    /// Último "próxima"/"anterior" aceito (`skip_bounced`).
    last_skip: Option<Instant>,
    /// Volume: quando mudou aqui, quando foi mandado ao player e se há um valor que ainda não foi (`set_volume`).
    volume_changed_at: Option<Instant>,
    volume_sent_at: Option<Instant>,
    volume_dirty: bool,
    /// Aviso embaixo das configurações de reprodução.
    playback_note: Option<&'static str>,
    /// Tamanho das capas e do áudio em cache, medido ao abrir as configurações.
    storage: Option<(u64, u64)>,
    /// Controle de mídia do sistema (teclas de mídia, cartão "tocando agora"); None se o sistema recusou.
    media: Option<souvlaki::MediaControls>,
    media_shown: media::Shown,
    /// Campo de texto das janelas (nome de playlist); `focus_dialog` pede o foco no próximo render.
    dialog_focus: FocusHandle,
    focus_dialog: bool,
    /// O Spotify bloqueou os pedidos do Client ID até este momento.
    limited_until: Option<Instant>,
    /// Ouvir junto.
    together: cyder_core::engine::Listen,
    /// Canvas tocando no imersivo, e o vídeo de cada faixa já pedida (None: ela não tem).
    canvas: Option<canvas::Canvas>,
    canvas_files: HashMap<String, Option<PathBuf>>,
    /// Estatísticas da tela: o período escolhido e as contas dele (None enquanto calcula).
    stats_range: cyder_core::stats::Range,
    stats: Option<cyder_core::stats::Stats>,
    stats_loading: bool,
    /// Smart shuffle ligado; as recomendadas que ele pôs na fila (marcadas com um brilho na fila).
    smart: bool,
    smart_added: HashSet<String>,
    /// Músicas curtidas como a tela mostra (busca e ordem aplicadas: `liked_changed`).
    liked_view: Vec<Track>,
    liked_query: String,
    liked_search_open: bool,
    liked_search_focus: FocusHandle,
    liked_order: library::liked::LikedOrder,
    /// O Spotify tem mais curtidas que as já carregadas.
    liked_has_more: bool,
    liked_loading: bool,
    liked_error: Option<String>,
    /// Onde cada episódio parou (URI → ms).
    episode_positions: HashMap<String, u32>,
    /// Faixas ocultas em álbuns e playlists (`hidden_key`).
    hidden: Vec<String>,
    /// Conta do Last.fm ligada.
    lastfm_user: Option<String>,
    listen: Option<media::Listen>,
    presence: Option<discord::Presence>,
    #[cfg(windows)]
    tray: Option<tray::Tray>,
    /// Janela principal, para a bandeja e o mini player a mostrarem de novo.
    hwnd: isize,
    mini: Option<gpui::AnyWindowHandle>,
    /// Novidades: todos, álbuns, singles (`news::cards`).
    news: Option<[Vec<cyder_core::Card>; 3]>,
    news_loading: bool,
    news_error: Option<String>,
    /// Endereço para abrir no navegador quando o próximo evento for aplicado (apply não tem o App).
    open_url: Option<String>,
    /// A tela do Client ID foi aberta pelas configurações: tem "Cancelar", e salvar refaz o login.
    setup_return: bool,
    /// None enquanto a lista não chega.
    devices: Option<Vec<Device>>,
    /// De onde veio a fila ("Músicas curtidas", nome do álbum) e a URI dela, para tocar de novo a partir da fila.
    queue_label: String,
    queue_context: Option<String>,
    /// Rolagem de cada tela, para voltar no mesmo lugar.
    scrolls: HashMap<String, gpui::UniformListScrollHandle>,
    /// Modo imersivo por cima da biblioteca.
    immersive: bool,
    /// A tela mudou e o botão clicado sumiu: o foco volta para a raiz no próximo render,
    /// senão os atalhos ficam sem destino (o GPUI não despacha ações para um foco que não está na tela).
    refocus: bool,
    playing: bool,
    anchor_ms: u32,
    anchor_at: Instant,
    lyrics: HashMap<usize, Lyrics>,
    art: HashMap<usize, Art>,
    /// Última arte mostrada, para o fundo não piscar enquanto a nova carrega.
    shown_art: Option<Art>,
    notice: Option<(String, Instant)>,
    layout: Option<LyricLayout>,
    shuffle: bool,
    repeat: Repeat,
    /// 0..=1.
    volume: f32,
    volume_open: bool,
    dragging_volume: bool,
    /// Área do slider na janela, medida a cada pintura, para converter a posição do mouse em volume.
    slider_bounds: Rc<Cell<Bounds<Pixels>>>,
    lyrics_on: bool,
    show_total: bool,
    /// Fração da música sob o mouse enquanto arrasta a barra de progresso.
    seek_drag: Option<f32>,
    progress_bounds: Rc<Cell<Bounds<Pixels>>>,
    last_activity: Instant,
    bar_hidden: bool,
    bar_hovered: bool,
    pub focus: FocusHandle,
    /// Campo do Client ID na tela de configuração.
    client_id: String,
    client_id_error: Option<String>,
    /// O texto do campo com foco está todo selecionado (Ctrl+A ou duplo clique): digitar substitui, apagar limpa.
    text_selected: bool,
    client_id_focus: FocusHandle,
    /// Pede foco no campo no próximo render (o foco precisa da janela).
    focus_client_id: bool,
    uri_copied: bool,
    /// Passo da primeira abertura: boas-vindas, criar o app, colar o Client ID.
    onboard_step: usize,
    /// Salvou o Client ID pela primeira vez: no login, mostra o cartão "Tudo pronto".
    welcome_pending: bool,
}

const REDIRECT_URI: &str = "http://127.0.0.1:5589/callback";
/// Mais que isso não é um Client ID; evita colar um texto enorme no campo.
const CLIENT_ID_MAX: usize = 128;

fn fmt(ms: u32) -> String {
    format!("{}:{:02}", ms / 60000, ms / 1000 % 60)
}

fn icon(name: &str, size: f32, color: u32) -> gpui::Svg {
    svg().path(SharedString::from(format!("icons/{name}.svg"))).size(px(size)).text_color(rgba(color))
}

/// Botão só de ícone: o nome serve ao leitor de tela e aparece numa dica ao parar o mouse em cima.
trait Tip {
    fn tip(self, label: impl Into<SharedString>) -> Self;
}

impl Tip for gpui::Stateful<gpui::Div> {
    fn tip(self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.aria_label(label.clone()).tooltip(move |_, cx| cx.new(|_| Tooltip(label.clone())).into())
    }
}

struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .border_1()
            .border_color(rgba(theme::wash(0x1f)))
            .bg(rgba((theme::surface() << 8) | 0xf2))
            .shadow_md()
            .text_xs()
            .text_color(rgba(theme::ink_1()))
            .font_family(theme::FONT)
            .child(self.0.clone())
    }
}

fn line_text(l: &Line) -> SharedString {
    if l.text.is_empty() { "…".into() } else { l.text.clone().into() }
}

impl Cyder {
    pub fn new(cmds: UnboundedSender<Cmd>, mut events: UnboundedReceiver<AppEvent>, cx: &mut Context<Self>) -> Self {
        let settings = config::settings();
        cyder_core::fx::set(settings.fx());
        KEEP_PLAYING.store(settings.keep_playing_on_close, Ordering::Relaxed);
        cx.spawn(async move |this, cx| {
            while let Some(ev) = events.recv().await {
                if this.update(cx, |s, cx| {
                    s.apply(ev);
                    s.sync_media();
                    if let Some(url) = s.open_url.take() {
                        cx.open_url(&url);
                    }
                    cx.notify();
                }).is_err() {
                    break;
                }
            }
        })
        .detach();
        // Relógio da barra de progresso e da letra; só re-renderiza quando algo muda na tela.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(200)).await;
                let alive = this.update(cx, |s, cx| {
                    if s.notice.as_ref().is_some_and(|(_, at)| at.elapsed() > NOTICE_FOR) {
                        s.notice = None;
                        cx.notify();
                    }
                    // Esconde só tocando; pausar traz a barra de volta.
                    if !s.bar_hidden && s.should_hide_bar() {
                        s.bar_hidden = true;
                        cx.notify();
                    } else if s.bar_hidden && !s.playing {
                        s.bar_hidden = false;
                        cx.notify();
                    }
                    if s.playing {
                        cx.notify();
                    }
                    s.check_sleep();
                    s.sync_media();
                    // Arraste parado no meio: o último valor não espera o mouse soltar.
                    if s.volume_dirty && s.volume_sent_at.is_some_and(|at| at.elapsed() >= s.volume_gap()) {
                        s.send_volume();
                    }
                    if let Some((place, _)) = s.place_due.take_if(|(_, at)| at.elapsed() >= Duration::from_millis(800)) {
                        config::save_window_place(&place);
                    }
                    if s.restart_due.is_some_and(|t| t.elapsed() >= library::settings::RESTART_AFTER) {
                        s.restart_due = None;
                        s.playback_note = Some("Aplicando…");
                        s.send(Cmd::RestartPlayer);
                        cx.notify();
                    }
                    if s.search_due.is_some_and(|t| t.elapsed() >= library::search::DEBOUNCE) {
                        s.run_search();
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self::watch_canvas(cx);
        Self {
            cmds,
            phase: Phase::Starting("Iniciando…".into()),
            tracks: Vec::new(),
            current: 0,
            liked: Vec::new(),
            library: None,
            pages: HashMap::new(),
            page_errors: HashMap::new(),
            thumbs: HashMap::new(),
            thumbs_small: HashMap::new(),
            screen: Screen::Section(Section::Home),
            history: Vec::new(),
            forward: Vec::new(),
            sidebar_open: true,
            side: None,
            home: None,
            shelf_scrolls: HashMap::new(),
            recent: HashMap::new(),
            pins: Vec::new(),
            devices_open: false,
            heroes: HashMap::new(),
            release_filter: library::Releases::All,
            discography_open: false,
            backdrops: HashMap::new(),
            bg_from: None,
            bg_to: None,
            bg_gen: 0,
            page_lists: HashMap::new(),
            menu: None,
            dialog: None,
            profile: None,
            up_next: None,
            remote: None,
            lyrics_manual: None,
            sleep: None,
            saved: HashMap::new(),
            loading_more: HashSet::new(),
            library_order: Vec::new(),
            side_filter: SideFilter::All,
            side_query: String::new(),
            side_search_open: false,
            side_search_focus: cx.focus_handle(),
            search_query: String::new(),
            search_focus: cx.focus_handle(),
            search_due: None,
            search: None,
            search_loading: false,
            search_error: None,
            search_filter: None,
            search_more: HashSet::new(),
            search_done: HashSet::new(),
            recent_searches: config::recent_searches(),
            settings: settings.clone(),
            plan: None,
            soloist: cyder_core::soloist::Status::Off,
            player_status: None,
            device_draft: settings.device_name(),
            device_focus: cx.focus_handle(),
            restart_due: None,
            place_due: None,
            window_title: "Cyder".into(),
            update: Default::default(),
            update_manual: false,
            update_checked: false,
            last_skip: None,
            volume_changed_at: None,
            volume_sent_at: None,
            volume_dirty: false,
            playback_note: None,
            storage: None,
            setup_return: false,
            dialog_focus: cx.focus_handle(),
            focus_dialog: false,
            limited_until: None,
            together: cyder_core::engine::Listen::Off,
            canvas: None,
            canvas_files: HashMap::new(),
            stats_range: cyder_core::stats::Range::Month,
            stats: None,
            stats_loading: false,
            smart: false,
            smart_added: HashSet::new(),
            liked_view: Vec::new(),
            liked_query: String::new(),
            liked_search_open: false,
            liked_search_focus: cx.focus_handle(),
            liked_order: Default::default(),
            liked_has_more: false,
            liked_loading: false,
            liked_error: None,
            episode_positions: HashMap::new(),
            hidden: config::hidden(),
            lastfm_user: None,
            listen: None,
            presence: None,
            #[cfg(windows)]
            tray: None,
            hwnd: 0,
            mini: None,
            news: None,
            news_loading: false,
            news_error: None,
            open_url: None,
            media: None,
            media_shown: media::Shown::default(),
            devices: None,
            queue_label: "Músicas curtidas".into(),
            queue_context: None,
            scrolls: HashMap::new(),
            immersive: false,
            refocus: false,
            playing: false,
            anchor_ms: 0,
            anchor_at: Instant::now(),
            lyrics: HashMap::new(),
            art: HashMap::new(),
            shown_art: None,
            notice: crate::take_crash_notice().map(|n| (n, Instant::now())),
            layout: None,
            shuffle: false,
            repeat: Repeat::Off,
            volume: 0.7,
            volume_open: false,
            dragging_volume: false,
            slider_bounds: Rc::new(Cell::new(Bounds::default())),
            lyrics_on: true,
            show_total: false,
            seek_drag: None,
            progress_bounds: Rc::new(Cell::new(Bounds::default())),
            last_activity: Instant::now(),
            bar_hidden: false,
            bar_hovered: false,
            focus: cx.focus_handle(),
            client_id: String::new(),
            client_id_error: None,
            text_selected: false,
            client_id_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            focus_client_id: false,
            uri_copied: false,
            onboard_step: 0,
            welcome_pending: false,
        }
    }

    fn apply(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Status(s) => self.phase = Phase::Starting(s),
            AppEvent::NeedsSetup { problem } => {
                self.phase = Phase::Setup;
                // Client ID salvo mas recusado: volta direto ao campo, com o erro.
                self.onboard_step = if problem.is_some() { onboarding::STEP_CLIENT_ID } else { 0 };
                self.client_id_error = problem;
                self.focus_client_id = true;
            }
            AppEvent::Failed(e) => self.phase = Phase::Failed(e),
            AppEvent::Ready { liked, queue, index, position_ms, label, context } => {
                // Volta onde parou: mesma fila, mesma faixa, mesma posição (pausado).
                self.liked_has_more = liked.len() >= library::liked::LIKED_PAGE;
                self.liked = liked;
                self.liked_changed();
                self.tracks = queue;
                self.current = index;
                self.anchor_ms = position_ms;
                self.anchor_at = Instant::now();
                self.queue_label = label;
                self.queue_context = context;
                self.phase = Phase::Ready;
                if std::mem::take(&mut self.welcome_pending) {
                    self.dialog = Some(Dialog::Welcome);
                } else {
                    self.ask_reports();
                }
                // Uma vez por abertura (o Ready volta ao entrar de novo na conta).
                if !std::mem::replace(&mut self.update_checked, true) {
                    self.check_update(false);
                }
            }
            AppEvent::Update(state) => self.on_update(state),
            AppEvent::Library(lib) => {
                // A daylist e os mixes mudam de nome e de faixas ao longo do dia: a página guardada não vale mais.
                if let Some(old) = &self.library {
                    for c in &lib.playlists {
                        if old.playlists.iter().any(|o| o.id == c.id && (o.title != c.title || o.image != c.image)) && self.pages.remove(&c.id).is_some() {
                            if matches!(&self.screen, Screen::Page(open) if open.id == c.id) {
                                self.send(Cmd::Open(c.clone()));
                            }
                        }
                    }
                }
                self.library = Some(lib);
            }
            AppEvent::Page(page) => {
                self.page_errors.remove(&page.card.id);
                self.pages.insert(page.card.id.clone(), page);
            }
            AppEvent::PageFailed { id, error } => drop(self.page_errors.insert(id, error)),
            AppEvent::Thumb { url, path, small } => {
                self.thumbs_small.insert(url.clone(), small);
                self.thumbs.insert(url, path);
            }
            AppEvent::Backdrop { url, path } => drop(self.backdrops.insert(url, path)),
            AppEvent::Hero { url, path } => drop(self.heroes.insert(url, path)),
            AppEvent::Devices(list) => self.devices = Some(list),
            AppEvent::Liked(tracks) => {
                for t in &tracks {
                    self.saved.insert(t.uri.clone(), true);
                }
                // A atualização traz só as 200 mais recentes: as mais antigas já carregadas continuam.
                let older: Vec<Track> = self.liked.iter().skip(tracks.len()).filter(|o| !tracks.iter().any(|t| t.uri == o.uri)).cloned().collect();
                if older.is_empty() {
                    self.liked_has_more = tracks.len() >= library::liked::LIKED_PAGE;
                }
                self.liked = tracks;
                self.liked.extend(older);
                self.liked_changed();
            }
            AppEvent::Home(shelves) => self.home = Some(shelves),
            AppEvent::Recent(recent) => self.recent = recent,
            AppEvent::Pins(pins) => self.pins = pins,
            AppEvent::Queue(tracks) => {
                // Letras e capas são guardadas por índice da fila; com outra fila, não valem mais.
                self.tracks = tracks;
                self.current = 0;
                // A posição da faixa anterior não vale para a nova.
                self.anchor_ms = 0;
                self.anchor_at = Instant::now();
                self.up_next = None;
                self.lyrics.clear();
                self.art.clear();
                self.layout = None;
            }
            AppEvent::TrackAdded(t) => {
                self.tracks.push(t);
                // Veio de fora da lista (outro aparelho, autoplay): "De <lista>" na fila não vale mais.
                self.queue_label.clear();
                self.queue_context = None;
            }
            AppEvent::TrackChanged(i) => {
                if i != self.current {
                    self.lyrics_manual = None;
                }
                self.current = i;
                if let Some(a) = self.art.get(&i) {
                    self.shown_art = Some(a.clone());
                }
                // Oculta neste álbum ou playlist: pula, como no Spotify.
                let hidden = self.queue_context.as_ref().zip(self.tracks.get(i)).is_some_and(|(c, t)| self.hidden.contains(&hidden_key(c, &t.uri)));
                if hidden && self.elsewhere().is_none() {
                    self.send(Cmd::Next);
                }
            }
            AppEvent::Playback { playing, position_ms } => {
                self.playing = playing;
                self.anchor_ms = position_ms;
                self.anchor_at = Instant::now();
            }
            AppEvent::Modes { shuffle, repeat, volume } => {
                self.shuffle = shuffle;
                self.repeat = repeat;
                // Durante o arraste (e logo depois) o slider já mostra o valor do mouse; o eco do core chega atrasado.
                if !self.dragging_volume && !self.volume_changed_at.is_some_and(|at| at.elapsed() < VOLUME_ECHO_IGNORE) {
                    self.volume = volume as f32 / u16::MAX as f32;
                }
            }
            AppEvent::Lyrics { index, lyrics } => drop(self.lyrics.insert(index, lyrics)),
            AppEvent::Art { index, art } => {
                if index == self.current {
                    self.shown_art = Some(art.clone());
                }
                self.art.insert(index, art);
            }
            AppEvent::Notice(n) => self.notify(n),
            AppEvent::Profile { name, image } => self.profile = Some((name, image)),
            AppEvent::UpNext(list) => self.up_next = Some(list),
            AppEvent::Remote(device) => self.remote = device,
            AppEvent::Inserted { at, track } => {
                let at = at.min(self.tracks.len());
                self.tracks.insert(at, track);
                // A lista não é mais a do álbum/playlist: clicar numa faixa da fila toca esta lista, não o contexto
                // (que não tem a faixa adicionada e fazia o Spotify tocar outra).
                self.queue_context = None;
                if at <= self.current && self.tracks.len() > 1 {
                    self.current += 1;
                }
                // Letras e capas são guardadas por índice; as de depois da inserção mudaram de lugar.
                self.lyrics.retain(|i, _| *i < at);
                self.art.retain(|i, _| *i < at);
            }
            AppEvent::Reordered(tracks) => {
                self.tracks = tracks;
                let current = self.current;
                self.lyrics.retain(|i, _| *i <= current);
                self.art.retain(|i, _| *i <= current);
                self.queue_context = None;
            }
            AppEvent::PageMore { id, tracks, more_from } => {
                self.loading_more.remove(&id);
                if let Some(page) = self.pages.get_mut(&id) {
                    page.tracks.extend(tracks);
                    page.more_from = more_from;
                }
            }
            AppEvent::Saved(list) => self.saved.extend(list),
            AppEvent::LibraryOrder(order) => self.library_order = order,
            // Resposta de uma busca que já mudou: ignora.
            AppEvent::Search { results, more } if results.query == self.search_query.trim() => {
                match (&mut self.search, more) {
                    (Some(found), true) => {
                        // Veio menos que um lote cheio: aquele tipo acabou.
                        for kind in self.search_more.drain() {
                            if library::search::count(&results, kind) < library::search::MORE_BATCH {
                                self.search_done.insert(kind);
                            }
                        }
                        found.extend(results);
                    }
                    _ => {
                        self.search = Some(results);
                        if let Some(kind) = self.search_filter {
                            self.request_more(kind);
                        }
                    }
                }
                self.search_loading = false;
            }
            AppEvent::SearchFailed { query, error } if query == self.search_query.trim() => {
                self.search_loading = false;
                // Não tenta de novo sozinho a cada rolagem; uma busca nova recomeça.
                self.search_done.extend(self.search_more.drain());
                if self.search.as_ref().is_some_and(|s| s.query == query) {
                    self.notify(format!("Não deu para carregar mais resultados: {error}"));
                } else {
                    self.search_error = Some(error);
                }
            }
            AppEvent::SearchPlaylists { query, mut playlists } => {
                // As que o "Ver mais" já trouxe continuam, depois destas.
                if let Some(found) = self.search.as_mut().filter(|s| s.query == query) {
                    playlists.extend(found.playlists.drain(..).filter(|c| !playlists.iter().any(|p| p.id == c.id)).collect::<Vec<_>>());
                    found.playlists = playlists;
                }
            }
            AppEvent::Search { .. } | AppEvent::SearchFailed { .. } => {}
            AppEvent::PlayerStatus(status) => {
                if status.is_none() && self.playback_note == Some("Aplicando…") {
                    self.playback_note = Some("Configurações de reprodução aplicadas.");
                }
                self.player_status = status;
            }
            AppEvent::Plan(plan) => self.plan = Some(plan),
            AppEvent::PlaylistCreated(card) => self.go(Screen::Page(card)),
            AppEvent::RateLimited { secs } => self.limited_until = Some(Instant::now() + Duration::from_secs(secs)),
            AppEvent::LikedMore { from: _, tracks, more, error } => self.liked_more(tracks, more, error),
            AppEvent::Credits(c) => self.dialog = Some(Dialog::Credits(c)),
            AppEvent::Radio(card) => self.play_context(&card),
            AppEvent::NewReleases(list) => {
                self.news = Some(library::news::cards(&list));
                self.news_loading = false;
            }
            AppEvent::NewReleasesFailed(e) => {
                self.news_error = Some(e);
                self.news_loading = false;
            }
            AppEvent::Lastfm(user) => self.lastfm_user = user,
            AppEvent::Soloist(status) => self.soloist = status,
            AppEvent::AudioRefused => {
                if self.dialog.is_none() {
                    self.dialog = Some(Dialog::Help(Help::Soloist));
                }
            }
            AppEvent::Listen(mode) => {
                if let cyder_core::engine::Listen::Hosting(code) = &mode {
                    self.notify(format!("Ouvir junto: passe o código {} para quem vai ouvir com você.", library::pretty_code(code)));
                }
                self.together = mode;
            }
            AppEvent::Canvas { uri, path } => {
                self.canvas_files.insert(uri, path);
                self.sync_canvas();
            }
            AppEvent::Stats { range, stats } => {
                self.stats_loading = false;
                if range == self.stats_range {
                    self.stats = Some(stats);
                }
            }
            AppEvent::SmartShuffle { on, added } => {
                self.smart = on;
                self.smart_added = added.into_iter().collect();
                if on {
                    self.notify(format!("Smart shuffle: {} recomendadas entraram na fila.", self.smart_added.len()));
                }
            }
            AppEvent::EpisodePositions(map) => self.episode_positions = map,
            AppEvent::OpenUrl(url) => self.open_url = Some(url),
        }
    }

    /// Curtida? Pelo que o Spotify respondeu; senão, pelas curtidas guardadas.
    fn is_liked(&self, uri: &str) -> bool {
        self.saved.get(uri).copied().unwrap_or_else(|| self.liked.iter().any(|t| t.uri == uri))
    }

    /// Curte ou descurte já na tela; o core confirma com o Spotify.
    fn set_liked(&mut self, track: &Track, on: bool) {
        self.saved.insert(track.uri.clone(), on);
        self.liked.retain(|t| t.uri != track.uri);
        if on {
            self.liked.insert(0, track.clone());
        }
        self.liked_changed();
        self.send(Cmd::Like { track: track.clone(), on });
    }

    fn cycle_shuffle(&mut self) {
        if self.smart {
            self.smart = false;
            self.shuffle = false;
            self.send(Cmd::SmartShuffle(false));
            self.send(Cmd::SetShuffle(false));
        } else if self.shuffle {
            if self.elsewhere().is_some() {
                return self.notify("O smart shuffle só funciona tocando neste computador.");
            }
            self.smart = true;
            self.notify("Procurando músicas parecidas…");
            self.send(Cmd::SmartShuffle(true));
        } else {
            self.shuffle = true;
            self.send(Cmd::SetShuffle(true));
        }
        self.remember_shuffle();
    }

    /// Tocar/pausar, próxima e anterior respondem na hora; o player confirma em seguida.
    fn toggle_play(&mut self) {
        self.anchor_ms = self.position();
        self.anchor_at = Instant::now();
        self.playing = !self.playing;
        self.send(Cmd::Toggle);
    }

    /// Dois pedidos de pular em menos de `SKIP_GAP` contam como um: um botão de mouse gasto manda dois cliques
    /// colados num toque só. Segurar Ctrl+→ continua pulando, só que no máximo quatro por segundo.
    fn skip_bounced(&mut self) -> bool {
        let now = Instant::now();
        if self.last_skip.is_some_and(|at| now.duration_since(at) < SKIP_GAP) {
            log::debug!("pular ignorado: segundo pedido em menos de {} ms", SKIP_GAP.as_millis());
            return true;
        }
        self.last_skip = Some(now);
        false
    }

    fn next_track(&mut self) {
        if self.skip_bounced() {
            return;
        }
        // Com o aleatório, só o Spotify sabe qual vem.
        if !self.shuffle && self.current + 1 < self.tracks.len() {
            self.show_track(self.current + 1);
        }
        self.playing = true;
        self.send(Cmd::Next);
    }

    fn prev_track(&mut self) {
        if self.skip_bounced() {
            return;
        }
        // Como no Spotify: depois de 3 s, "anterior" volta ao começo da mesma música.
        if self.position() > 3000 || self.current == 0 || self.shuffle {
            self.anchor_ms = 0;
            self.anchor_at = Instant::now();
        } else {
            self.show_track(self.current - 1);
        }
        self.playing = true;
        self.send(Cmd::Prev);
    }

    fn show_track(&mut self, i: usize) {
        self.current = i;
        self.anchor_ms = 0;
        self.anchor_at = Instant::now();
        self.lyrics_manual = None;
        if let Some(a) = self.art.get(&i) {
            self.shown_art = Some(a.clone());
        }
    }

    /// Voltar para a janela do Cyder confere se outro aparelho está tocando: o aviso do Spotify Connect pode ter
    /// se perdido (o PC dormiu, a conexão caiu). O core limita a uma conferência a cada 20 s.
    pub fn watch_window_activation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.observe_window_activation(window, |t, window, _| {
            if window.is_window_active() && matches!(t.phase, Phase::Ready) {
                t.send(Cmd::RefreshRemote);
            }
        })
        .detach();
    }

    /// Guarda onde a janela está a cada mudança; o relógio salva quando para de mexer.
    pub fn watch_window_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.observe_window_bounds(window, |t, window, _| {
            let (b, maximized) = match window.window_bounds() {
                gpui::WindowBounds::Windowed(b) => (b, false),
                gpui::WindowBounds::Maximized(b) => (b, true),
                // Tela cheia não é lembrada: volta para a janela de antes.
                gpui::WindowBounds::Fullscreen(b) => (b, false),
            };
            let place = config::WindowPlace { x: b.origin.x.into(), y: b.origin.y.into(), w: b.size.width.into(), h: b.size.height.into(), maximized };
            // Minimizada, o Windows põe a janela em -32000: não é um lugar para voltar.
            if place.x > -10_000. && place.y > -10_000. {
                t.place_due = Some((place, Instant::now()));
            }
        })
        .detach();
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// Timer: pausa na hora marcada ou quando a música marcada acaba.
    fn check_sleep(&mut self) {
        let due = match self.sleep {
            Some(Sleep::At(at)) => Instant::now() >= at,
            Some(Sleep::EndOfTrack(i)) => self.current != i,
            None => false,
        };
        if due {
            self.sleep = None;
            if self.playing {
                self.send(Cmd::Toggle);
            }
            self.notify("Timer: música pausada.");
        }
    }

    fn set_immersive(&mut self, on: bool) {
        self.immersive = on;
        self.refocus = true;
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.cmds.send(cmd);
    }

    fn position(&self) -> u32 {
        let elapsed = if self.playing { self.anchor_at.elapsed().as_millis() as u32 } else { 0 };
        let pos = self.anchor_ms + elapsed;
        self.tracks.get(self.current).map_or(pos, |t| pos.min(t.duration_ms))
    }

    fn seek(&mut self, ms: u32) {
        // Atualiza já, sem esperar o evento do player, para a letra responder ao clique.
        self.anchor_ms = ms;
        self.anchor_at = Instant::now();
        self.send(Cmd::Seek(ms));
    }

    /// O slider muda na hora; o player recebe no máximo um valor a cada `VOLUME_SEND_GAP` durante o arraste.
    /// Cada valor vira um aviso ao Spotify Connect, que devolve um eco: dezenas por segundo enfileiravam e os
    /// ecos atrasados puxavam o slider de volta (a sensação de "lag" ao arrastar rápido).
    fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0., 1.);
        let now = Instant::now();
        self.volume_changed_at = Some(now);
        if self.dragging_volume && self.volume_sent_at.is_some_and(|at| now.duration_since(at) < self.volume_gap()) {
            self.volume_dirty = true;
            return;
        }
        self.send_volume();
    }

    /// Com outro aparelho tocando, cada valor é um pedido à Web API (com teto por minuto): bem mais espaçado.
    fn volume_gap(&self) -> Duration {
        if self.remote.is_some() { Duration::from_millis(400) } else { VOLUME_SEND_GAP }
    }

    /// Manda o valor atual do slider (o último do arraste, ao soltar ou quando o relógio vê que ficou para trás).
    fn send_volume(&mut self) {
        self.volume_dirty = false;
        self.volume_sent_at = Some(Instant::now());
        self.send(Cmd::SetVolume((self.volume * u16::MAX as f32).round() as u16));
    }

    fn volume_from_mouse(&mut self, x: Pixels) {
        let b = self.slider_bounds.get();
        if f32::from(b.size.width) > 0. {
            self.set_volume(f32::from(x - b.origin.x) / f32::from(b.size.width));
        }
    }

    fn progress_from_mouse(&self, x: Pixels) -> f32 {
        let b = self.progress_bounds.get();
        if f32::from(b.size.width) <= 0. {
            return 0.;
        }
        (f32::from(x - b.origin.x) / f32::from(b.size.width)).clamp(0., 1.)
    }

    /// Mouse ou teclado: traz a barra de volta e reinicia a contagem para escondê-la.
    fn activity(&mut self, cx: &mut Context<Self>) {
        self.last_activity = Instant::now();
        if self.bar_hidden {
            self.bar_hidden = false;
            cx.notify();
        }
    }

    fn should_hide_bar(&self) -> bool {
        matches!(self.phase, Phase::Ready)
            && self.immersive
            && self.playing
            && !self.bar_hovered
            && !self.dragging_volume
            && self.seek_drag.is_none()
            && self.last_activity.elapsed() > IDLE_HIDE
    }

    fn cycle_repeat(&mut self) {
        let next = match self.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
        self.repeat = next;
        self.send(Cmd::SetRepeat(next));
    }

    fn seek_by(&mut self, delta: i64) {
        let max = self.tracks.get(self.current).map_or(0, |t| t.duration_ms) as i64;
        self.seek((self.position() as i64 + delta).clamp(0, max) as u32);
    }

    fn lyric_tops(&mut self, window: &Window, lines: &[Line], width: Pixels, style: &LyricStyle) -> Vec<f32> {
        let w = f32::from(width).round() as i32;
        let size = style.size as i32;
        if let Some(l) = &self.layout {
            if l.index == self.current && l.width == w && l.size == size {
                return l.tops.clone();
            }
        }
        let font = Font { weight: FontWeight::EXTRA_BOLD, ..font(theme::FONT) };
        let mut tops = Vec::with_capacity(lines.len());
        let mut y = 0.;
        for line in lines {
            tops.push(y);
            let text = line_text(line);
            let run = TextRun { len: text.len(), font: font.clone(), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
            let rows = window
                .text_system()
                .shape_text(text, px(style.size), &[run], Some(width), None)
                .map(|ls| ls.iter().map(|l| l.wrap_boundaries.len() + 1).sum::<usize>())
                .unwrap_or(1);
            y += rows as f32 * style.line_h + style.gap;
        }
        self.layout = Some(LyricLayout { index: self.current, width: w, size, tops: tops.clone() });
        tops
    }

    fn titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let control = |id: &'static str, label: &'static str, icon_name: &'static str, area: WindowControlArea, hover: u32| {
            div()
                .id(id)
                .w(px(46.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .hover(move |s| s.bg(rgba(hover)))
                .window_control_area(area)
                .aria_label(label)
                .child(icon(icon_name, 14., theme::ink_2()))
        };
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(theme::TITLEBAR_H))
            .flex()
            .items_center()
            .when(self.immersive && matches!(self.phase, Phase::Ready), |d| d.child(
                Self::focusable("exit-immersive")
                    .ml_2()
                    .h(px(30.))
                    .pl_1()
                    .pr_3()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_sm()
                    .text_color(rgba(theme::ink_2()))
                    .hover(|s| s.bg(rgba(theme::hover())).text_color(rgba(theme::ink_1())))
                    .aria_label("Voltar à biblioteca (Esc)")
                    .on_click(cx.listener(|t, _, _, cx| { t.set_immersive(false); cx.notify(); }))
                    .child(icon("chevron-left", 16., theme::ink_2()))
                    .child("Biblioteca"),
            ))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .pl_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .window_control_area(WindowControlArea::Drag)
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child("Cyder")),
            )
            .child(control("win-min", "Minimizar", "minus", WindowControlArea::Min, theme::hover()))
            .child(control("win-max", "Maximizar", "square", WindowControlArea::Max, theme::hover()))
            .child(control("win-close", "Fechar", "x", WindowControlArea::Close, (theme::DANGER << 8) | 0xff))
    }

    fn background(&mut self, window: &mut Window) -> AnyElement {
        // No imersivo, o Canvas da faixa (quando tem) no lugar da capa borrada.
        if let Some(frame) = self.immersive.then(|| self.canvas_frame(window)).flatten() {
            return div()
                .absolute()
                .inset_0()
                .child(img(frame).absolute().inset_0().size_full().object_fit(ObjectFit::Cover))
                .child(div().absolute().inset_0().bg(rgba(0x0000008c)))
                .into_any_element();
        }
        match &self.shown_art {
            Some(art) => div()
                .absolute()
                .inset_0()
                .child(img(art.background.clone()).absolute().inset_0().size_full().object_fit(ObjectFit::Cover))
                .child(div().absolute().inset_0().bg(rgba(theme::shade(0x73))))
                .into_any_element(),
            None => div().absolute().inset_0().bg(rgba((theme::base() << 8) | 0xff)).into_any_element(),
        }
    }

    /// Botão alcançável por Tab, acionado por clique ou Enter, com anel de foco só no teclado.
    fn focusable(id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .tab_index(0)
            .cursor_pointer()
            // Borda transparente fixa: o anel de foco não muda o tamanho do botão.
            .border_2()
            .border_color(rgba(0x00000000))
            .focus_visible(|s| s.border_color(rgba(theme::ink_1())))
    }

    fn primary_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
        Self::focusable(id)
            .px_5()
            .py_2()
            .rounded_full()
            .bg(rgba(theme::ink_1()))
            .text_color(rgba(theme::on_ink()))
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|s| s.bg(rgba(theme::wash(0xd9))))
            .focus_visible(|s| s.border_color(rgba(theme::ink_3())))
            .child(label)
    }

    fn secondary_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
        Self::focusable(id)
            .px_5()
            .py_2()
            .rounded_full()
            .bg(rgba(theme::wash(0x1f)))
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|s| s.bg(rgba(theme::wash(0x2e))))
            .child(label)
    }

    fn submit_client_id(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match config::save(&self.client_id) {
            Ok(()) => {
                // O campo some; sem devolver o foco, os atalhos do player não teriam onde chegar.
                window.focus(&self.focus, cx);
                self.client_id_error = None;
                self.phase = Phase::Starting("Salvando…".into());
                // Vindo das configurações, o engine está rodando: refaz só o login da Web API com o app novo.
                let from_settings = std::mem::take(&mut self.setup_return);
                self.welcome_pending = !from_settings;
                let cmd = if from_settings { Cmd::Relogin } else { Cmd::Retry };
                self.send(cmd);
            }
            Err(e) => self.client_id_error = Some(e),
        }
    }

    /// Campo de uma linha: digitar, colar (Ctrl+V), apagar e Enter para salvar.
    /// Não bloqueia caracteres; a validação acontece ao salvar.
    fn client_id_key(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        match k.key.as_str() {
            "enter" => {
                self.submit_client_id(window, cx);
                cx.stop_propagation();
                cx.notify();
                return;
            }
            _ if type_into(&mut self.client_id, k, &mut self.text_selected, cx) => {}
            _ => return,
        }
        // O Client ID não tem espaços: os que vêm ao colar saem.
        self.client_id = self.client_id.chars().filter(|c| !c.is_whitespace()).take(CLIENT_ID_MAX).collect();
        self.client_id_error = None;
        cx.stop_propagation();
        cx.notify();
    }

    fn client_id_field(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.client_id_focus.is_focused(window);
        let border = if self.client_id_error.is_some() { (theme::DANGER << 8) | 0xff } else if focused { theme::ink_2() } else { theme::wash(0x29) };
        let caret = || div().w(px(1.5)).h(px(20.)).bg(rgba(theme::ink_1()));
        let field = div()
            .id("client-id")
            .key_context("TextField")
            .track_focus(&self.client_id_focus)
            .aria_label("Client ID")
            .w_full()
            .h(px(44.))
            .px_4()
            .flex()
            .items_center()
            .overflow_hidden()
            .rounded(px(10.))
            .bg(rgba(theme::shade(0x59)))
            .border_1()
            .border_color(rgba(border))
            .font_family("Consolas")
            .cursor_text()
            .on_click(cx.listener(|t, e: &ClickEvent, window, cx| { t.text_selected = e.click_count() >= 2; window.focus(&t.client_id_focus, cx); cx.notify(); }));
        let field = field.on_key_down(cx.listener(|t, e: &gpui::KeyDownEvent, window, cx| t.client_id_key(e, window, cx)));
        if self.client_id.is_empty() {
            // O cursor fica antes do exemplo, como num campo vazio de verdade.
            field.when(focused, |d| d.child(caret())).child(div().text_color(rgba(theme::ink_3())).child("7f3e91c2d84b4a06…")).into_any_element()
        } else {
            field.child(self.field_text(self.client_id.clone(), focused)).when(focused, |d| d.child(caret())).into_any_element()
        }
    }

    /// Telas antes do player: carregando, falta configurar, erro.
    fn gate(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let column = div().absolute().inset_0().flex().flex_col().items_center().justify_center().gap_3().px_8();
        match &self.phase {
            Phase::Starting(status) => column
                .child(div().text_size(px(28.)).font_weight(FontWeight::BOLD).child("Cyder"))
                .child(div().text_color(rgba(theme::ink_2())).child(status.clone()))
                .child(div().text_sm().text_color(rgba(theme::ink_3())).child("Na primeira vez, o navegador abre para você entrar na sua conta do Spotify."))
                .into_any_element(),
            Phase::Setup => self.onboarding(window, cx),
            Phase::Failed(e) => column
                .child(div().text_size(px(24.)).font_weight(FontWeight::BOLD).child("Não deu para conectar"))
                .child(div().max_w(px(640.)).text_color(rgba(theme::ink_2())).child(e.clone()))
                .child(div().mt_4().child(Self::primary_button("retry", "Tentar de novo").on_click(cx.listener(|this, _, _, cx| {
                    this.phase = Phase::Starting("Tentando de novo…".into());
                    this.send(Cmd::Retry);
                    cx.notify();
                }))))
                .into_any_element(),
            Phase::Ready => div().into_any_element(),
        }
    }

    fn lyrics_card(&mut self, window: &Window, card_w: f32, card_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let content = self.lyrics_content(window, card_w, card_h, &IMMERSIVE_LYRICS, cx);
        glass(div()).size_full().rounded(px(theme::RADIUS_CARD)).overflow_hidden().child(content).into_any_element()
    }

    /// Letra da faixa atual num retângulo `w`×`h`: sincronizada rolando sozinha, só texto, ou o estado vazio.
    pub(crate) fn lyrics_content(&mut self, window: &Window, card_w: f32, card_h: f32, style: &LyricStyle, cx: &mut Context<Self>) -> AnyElement {
        let ink = theme::tinted_ink(self.shown_art.as_ref().map_or([255, 255, 255], |a| a.accent));
        let center = |title: SharedString, detail: Option<SharedString>| {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .px_8()
                .child(div().text_size(px(style.title)).font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child(title))
                .children(detail.map(|d| div().text_sm().text_color(rgba(theme::ink_3())).child(d)))
                .into_any_element()
        };
        let track_name = self.tracks.get(self.current).map(|t| t.name.clone()).unwrap_or_default();
        let content = match self.lyrics.get(&self.current).cloned() {
            None => loading::lyrics(style.line_h, style.pad_x),
            Some(Lyrics::NotFound) => center(
                "Sem letra para esta música".into(),
                Some(format!("O LRCLIB ainda não tem a letra de “{track_name}”.").into()),
            ),
            Some(Lyrics::Instrumental) => center("Instrumental".into(), None),
            // Falha de rede ou do LRCLIB: diz isso (não "sem letra") e deixa pedir de novo.
            Some(Lyrics::Failed) => {
                let index = self.current;
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .px_8()
                    .child(div().text_size(px(style.title)).font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child("Não deu para carregar a letra"))
                    .child(div().text_sm().text_color(rgba(theme::ink_3())).child("O LRCLIB, de onde vêm as letras, não respondeu agora."))
                    .child(Self::text_button("lyrics-retry", "Tentar de novo").on_click(cx.listener(move |t, _, _, cx| {
                        // Volta ao esqueleto de carregamento enquanto pede.
                        t.lyrics.remove(&index);
                        t.send(Cmd::RetryLyrics(index));
                        cx.notify();
                    })))
                    .into_any_element()
            }
            Some(Lyrics::Plain(lines)) => div()
                .id("plain-lyrics")
                .size_full()
                .overflow_y_scroll()
                .px(px(style.pad_x))
                .py_10()
                .child(div().mb_4().text_xs().text_color(rgba(theme::ink_3())).child("Letra sem sincronia"))
                .children(lines.into_iter().map(|l| div().min_h(px(style.line_h)).text_size(px(style.title)).font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child(l)))
                .into_any_element(),
            Some(Lyrics::Synced(lines)) => {
                let width = px(card_w - style.pad_x * 2.);
                let tops = self.lyric_tops(window, &lines, width, style);
                let pos = self.position();
                let idx = lines.iter().rposition(|l| l.time_ms <= pos);
                let target = -idx.map_or(0., |i| tops[i]);
                // Rolada à mão, a letra fica onde o usuário deixou até ele pedir para sincronizar.
                let bottom = -tops.last().copied().unwrap_or(0.);
                let manual = self.lyrics_manual;
                let spring = SpringAnimation::new(SPRING).to(px(manual.unwrap_or(target)));
                let rows = lines.iter().enumerate().map(|(i, line)| {
                    let d = idx.map_or(i + 1, |c| i.abs_diff(c)) as f32;
                    // Rolando à mão, todas as linhas ficam legíveis; acompanhando, as de longe apagam.
                    let (opacity, blur) = if d == 0. { (1., 0.) } else if manual.is_some() { (0.7, 0.) } else { ((0.5 - d * 0.07).max(0.14), (d * 0.9).min(4.5)) };
                    let time = line.time_ms;
                    div()
                        .id(("lyric", i))
                        .pb(px(style.gap))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.lyrics_manual = None;
                            this.seek(time);
                            cx.notify();
                        }))
                        .child(
                            div()
                                .text_size(px(style.size))
                                .line_height(px(style.line_h))
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .text_color(ink)
                                .opacity(opacity)
                                .blur(px(blur))
                                .child(line_text(line)),
                        )
                });
                div()
                    .id("synced-lyrics")
                    .size_full()
                    .relative()
                    .on_scroll_wheel(cx.listener(move |this, e: &gpui::ScrollWheelEvent, _, cx| {
                        let dy = f32::from(e.delta.pixel_delta(px(40.)).y);
                        let from = this.lyrics_manual.unwrap_or(target);
                        this.lyrics_manual = Some((from + dy).clamp(bottom, 0.));
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(div().size_full().fade_edges(px(72.), px(160.)).child(
                        div()
                            .absolute()
                            .left(px(style.pad_x))
                            .right(px(style.pad_x))
                            .top(px(card_h * 0.3))
                            .children(rows)
                            // Margem, não layer_translate: a camada é recortada pelo cartão antes de ser
                            // transladada, e as linhas que estavam abaixo da borda chegavam vazias.
                            .with_spring(("lyrics-scroll", self.current), spring, |el, y| el.mt(y)),
                    ))
                    .when(manual.is_some(), |d| d.child(
                        div().absolute().bottom(px(18.)).left_0().right_0().flex().justify_center().child(
                            Self::focusable("lyrics-resync")
                                // A linha da letra embaixo do botão não pode receber o clique (pularia a música).
                                .occlude()
                                .h(px(34.))
                                .px_4()
                                .rounded_full()
                                .flex()
                                .items_center()
                                .gap_2()
                                .bg(rgba(theme::ink_1()))
                                .text_color(rgba(theme::on_ink()))
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .hover(|s| s.bg(rgba(theme::wash(0xd9))))
                                .on_click(cx.listener(|t, _, _, cx| { t.lyrics_manual = None; cx.notify(); }))
                                .child(icon("rotate-cw", 14., theme::on_ink()))
                                .child("Sincronizar letra"),
                        ),
                    ))
                    .into_any_element()
            }
        };
        content
    }

    /// Barra do player do modo imersivo: transporte à esquerda, faixa no meio, volume e letra à direita.
    fn player_bar(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let Some(track) = self.tracks.get(self.current) else { return div().into_any_element() };
        let duration = track.duration_ms;
        // Durante o arraste do progresso, mostra a posição sob o mouse, não a que está tocando.
        let pos = self.seek_drag.map_or(self.position(), |f| (f * duration as f32) as u32);
        let frac = if duration == 0 { 0. } else { (pos as f32 / duration as f32).clamp(0., 1.) };
        let accent = self.accent_ink();
        let button = Self::bar_button;
        let time = if self.show_total {
            format!("{} / {}", fmt(pos), fmt(duration))
        } else {
            format!("{} / -{}", fmt(pos), fmt(duration.saturating_sub(pos)))
        };
        let slider = self.volume_slider(self.volume_open, cx);
        let slider_w = if self.volume_open { SLIDER_W + 8. } else { 0. };
        let progress = self.progress(frac, accent, cx);
        // Nome abre o álbum e cada artista abre a página dele, como no player da biblioteca.
        let (title_link, artist_links) = self.track_links(track, "bar", cx);
        let album = track.album.clone();

        let bar = glass(div())
            .id("player-bar")
            // A letra fica por baixo da barra: o clique não pode pular para a linha de trás.
            .block_mouse_except_scroll()
            .w(px(width))
            .h(px(PLAYER_H))
            .rounded_full()
            .flex()
            .items_center()
            .gap_0p5()
            .px_2()
            .on_hover(cx.listener(|t, hovered: &bool, _, _| t.bar_hovered = *hovered))
            .children(self.transport(cx))
            .child(
                // Clique alterna entre tempo restante e duração total, como no Spotify.
                Self::focusable("time")
                    .ml_1()
                    .w(px(80.))
                    .py_1()
                    .rounded(px(8.))
                    .flex()
                    .justify_center()
                    .text_size(px(11.))
                    .text_color(rgba(theme::ink_2()))
                    .hover(|s| s.text_color(rgba(theme::ink_1())))
                    .aria_label(if self.show_total { "Mostrar tempo restante" } else { "Mostrar duração total" })
                    .on_click(cx.listener(|t, _, _, cx| { t.show_total = !t.show_total; cx.notify(); }))
                    .child(time),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(44.))
                    .px_4()
                    .pt(px(4.))
                    .flex()
                    .flex_col()
                    // Mesmo arredondamento da barra e dos botões: tudo pílula.
                    .rounded(px(22.))
                    .border_1()
                    .border_color(rgba(theme::wash(0x1f)))
                    .bg(rgba(theme::wash(0x0a)))
                    // Altura fixa: a borda do anel de foco dos links não empurra a barra de progresso.
                    .child(div().h(px(16.)).flex().items_center().min_w_0().text_size(px(13.)).line_height(px(16.)).font_weight(FontWeight::SEMIBOLD).child(title_link))
                    .child(div().h(px(14.)).flex().items_center().min_w_0().gap(px(3.)).overflow_hidden().whitespace_nowrap().text_size(px(11.)).line_height(px(14.)).text_color(rgba(theme::ink_2()))
                        .child(div().flex_shrink_0().max_w(px(220.)).overflow_hidden().text_ellipsis().child(format!("{album} –")))
                        .children(artist_links))
                    .child(div().mt(px(-3.)).child(progress)),
            )
            .child(
                // O slider abre empurrando o cartão da faixa, numa mola.
                div()
                    .h(px(36.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .overflow_hidden()
                    .child(div().ml_2().child(slider))
                    .with_spring("volume-slider-width", SpringAnimation::new(SPRING).to(px(slider_w)), |el, w| el.w(w)),
            )
            .child(
                button("volume", "Volume".into(), 36., self.volume_open)
                    .on_click(cx.listener(|t, _, _, cx| { t.volume_open = !t.volume_open; cx.notify(); }))
                    .child(icon(self.volume_icon(), 17., theme::ink_1())),
            )
            .child(
                button("lyrics", if self.lyrics_on { "Esconder letra" } else { "Mostrar letra" }.into(), 36., self.lyrics_on)
                    .on_click(cx.listener(|t, _, _, cx| { t.lyrics_on = !t.lyrics_on; cx.notify(); }))
                    .child(icon("mic-vocal", 17., theme::ink_1())),
            );

        // Some sem atividade: desce um pouco e apaga, numa mola; volta no primeiro movimento.
        let hidden = if self.bar_hidden { 1. } else { 0. };
        div()
            .absolute()
            .bottom(px(PLAYER_BOTTOM))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(bar)
            .with_spring("player-bar-hide", SpringAnimation::new(SPRING).to(hidden), |el, t| {
                el.opacity(1. - t).layer_translate(point(px(0.), px(16. * t)))
            })
            .into_any_element()
    }

    fn accent_ink(&self) -> gpui::Hsla {
        let art = self.shown_art.as_ref().filter(|_| self.settings.cover_colors);
        theme::tinted_ink(art.map_or([255, 255, 255], |a| a.accent))
    }

    /// Botão das barras; `on` = modo ligado (volume aberto, letra), com fundo e ícone claro.
    fn bar_button(id: &'static str, label: SharedString, size: f32, on: bool) -> gpui::Stateful<gpui::Div> {
        Self::focusable(id)
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .when(on, |d| d.bg(rgba(theme::wash(0x26))))
            .hover(|s| s.bg(rgba(theme::hover())))
            .tip(label)
            // Clicar não leva o foco para o botão (o Tab ainda leva). Com o foco nele, o Espaço seguinte virava
            // "clique" no botão além de tocar/pausar: depois de clicar em Próxima, pausar com Espaço pulava outra música.
            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| window.prevent_default())
    }

    fn volume_icon(&self) -> &'static str {
        match self.volume {
            v if v <= 0.001 => "volume-x",
            v if v < 0.34 => "volume",
            v if v < 0.67 => "volume-1",
            _ => "volume-2",
        }
    }

    /// Aleatório, anterior, tocar/pausar, próxima e repetir: iguais nas duas barras.
    fn transport(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let button = Self::bar_button;
        let mode_color = |on: bool| if on { theme::ink_1() } else { theme::ink_3() };
        let play_icon = if self.playing { "pause" } else { "play" };
        let repeat_label = match self.repeat {
            Repeat::Off => "Repetir: desligado",
            Repeat::All => "Repetir: todas",
            Repeat::One => "Repetir: esta música",
        };
        vec![
            // Desligado → aleatório → smart shuffle → desligado, como no Spotify.
            button("shuffle", match (self.shuffle, self.smart) {
                (_, true) => "Smart shuffle: ligado",
                (true, false) => "Aleatório: ligado. Clique para o smart shuffle",
                (false, _) => "Aleatório: desligado",
            }.into(), 36., false)
                .on_click(cx.listener(|t, _, _, cx| { t.cycle_shuffle(); cx.notify(); }))
                .child(self.shuffle_icon(15., mode_color(false)))
                .into_any_element(),
            button("prev", "Anterior".into(), 36., false).on_click(cx.listener(|t, _, _, cx| { t.prev_track(); cx.notify(); })).child(icon("rewind", 18., theme::ink_1())).into_any_element(),
            button("play", if self.playing { "Pausar" } else { "Tocar" }.into(), 42., false)
                .on_click(cx.listener(|t, _, _, cx| { t.toggle_play(); cx.notify(); }))
                // Troca do ícone com escala + blur, como um ícone só mudando de forma.
                .child(icon(play_icon, 22., theme::ink_1()).with_animation(
                    play_icon,
                    Animation::new(Duration::from_millis(200)).with_easing(ease_out_quint()),
                    |el, t| el.opacity(t).layer_scale(0.25 + 0.75 * t).blur(px(4. * (1. - t))),
                ))
                .into_any_element(),
            button("next", "Próxima".into(), 36., false).on_click(cx.listener(|t, _, _, cx| { t.next_track(); cx.notify(); })).child(icon("fast-forward", 18., theme::ink_1())).into_any_element(),
            button("repeat", repeat_label.into(), 36., false)
                .on_click(cx.listener(|t, _, _, cx| { t.cycle_repeat(); cx.notify(); }))
                .child(icon(if self.repeat == Repeat::One { "repeat-1" } else { "repeat" }, 15., mode_color(self.repeat != Repeat::Off)))
                .into_any_element(),
        ]
    }

    /// Volume: trilha com a bolinha dentro; a trilha fica KNOB/2 para dentro de cada lado, e a bolinha nunca passa da borda.
    /// `reachable`: entra no Tab (no imersivo, só com o slider aberto).
    fn volume_slider(&self, reachable: bool, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let volume_bounds = self.slider_bounds.clone();
        if reachable { Self::focusable("volume-slider") } else { div().id("volume-slider") }
            .w(px(SLIDER_W))
            .h(px(28.))
            .px(px(KNOB / 2.))
            .flex()
            .items_center()
            .aria_label(SharedString::from(format!("Volume: {}%", (self.volume * 100.).round())))
            // Roda do mouse: 5% por clique da roda.
            .on_scroll_wheel(cx.listener(|this, e: &gpui::ScrollWheelEvent, _, cx| {
                let dy = f32::from(e.delta.pixel_delta(px(20.)).y);
                if dy != 0. {
                    this.set_volume(this.volume + VOLUME_STEP * dy.signum());
                    // A lista por baixo do player não rola junto.
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| {
                this.dragging_volume = true;
                this.volume_from_mouse(e.position.x);
                cx.notify();
            }))
            .child(
                div().relative().w_full().h(px(4.)).rounded_full().bg(rgba(theme::wash(0x33)))
                    // Mede a trilha na pintura, para o arraste continuar fora dela.
                    .child(canvas(move |b, _, _| volume_bounds.set(b), |_, _, _, _| {}).absolute().inset_0())
                    .child(div().h_full().rounded_full().bg(rgba(theme::ink_1())).w(relative(self.volume)))
                    .child(div().absolute().top(px(-4.)).left(relative(self.volume)).ml(px(-KNOB / 2.)).size(px(KNOB)).rounded_full().bg(rgba(theme::ink_1()))),
            )
    }

    /// Barra de progresso clicável e arrastável; a bolinha só aparece ao passar o mouse ou arrastar, como no Spotify.
    fn progress(&self, frac: f32, accent: gpui::Hsla, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let progress_bounds = self.progress_bounds.clone();
        let dragging_seek = self.seek_drag.is_some();
        div()
            .id("progress")
            .group("progress")
            .w_full()
            .h(px(14.))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| {
                this.seek_drag = Some(this.progress_from_mouse(e.position.x));
                cx.notify();
            }))
            .child(
                div().relative().w_full().h(px(4.)).rounded_full().bg(rgba(theme::wash(0x26)))
                    .child(canvas(move |b, _, _| progress_bounds.set(b), |_, _, _, _| {}).absolute().inset_0())
                    .child(div().h_full().rounded_full().bg(accent).w(relative(frac)))
                    .child(
                        div().absolute().top(px(-4.)).left(relative(frac)).ml(px(-KNOB / 2.)).size(px(KNOB)).rounded_full().bg(rgba(theme::ink_1()))
                            .opacity(if dragging_seek { 1. } else { 0. })
                            .group_hover("progress", |s| s.opacity(1.)),
                    ),
            )
    }
}

/// Edição de um campo de uma linha: digitar, colar, apagar (Ctrl+Backspace apaga tudo). Ctrl+A seleciona tudo;
/// com tudo selecionado (`selected`), digitar ou colar substitui, apagar limpa e Ctrl+C/Ctrl+X copiam.
/// False: a tecla não é do campo.
fn type_into(text: &mut String, k: &gpui::Keystroke, selected: &mut bool, cx: &mut Context<Cyder>) -> bool {
    let all = std::mem::take(selected);
    match k.key.as_str() {
        "a" if k.modifiers.secondary() => *selected = !text.is_empty(),
        "c" | "x" if k.modifiers.secondary() && all => {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            if k.key == "x" {
                text.clear();
            } else {
                *selected = true;
            }
        }
        "backspace" | "delete" if all || k.modifiers.control => text.clear(),
        "backspace" => drop(text.pop()),
        "v" if k.modifiers.secondary() => match cx.read_from_clipboard().and_then(|c| c.text()) {
            Some(t) => {
                if all {
                    text.clear();
                }
                text.push_str(t.lines().next().unwrap_or_default());
            }
            None => return false,
        },
        _ => match &k.key_char {
            Some(c) if !k.modifiers.control && !k.modifiers.platform && !c.chars().any(char::is_control) => {
                if all {
                    text.clear();
                }
                text.push_str(c);
            }
            // Setas, Home, End: só tiram a seleção.
            _ => return all && matches!(k.key.as_str(), "left" | "right" | "home" | "end"),
        },
    }
    *text = text.chars().take(80).collect();
    true
}

/// Fundo do texto selecionado nos campos.
const SELECTION: u32 = 0x3d7eff66;

impl Cyder {
    /// O texto de um campo, com o fundo de seleção quando está todo selecionado.
    pub(crate) fn field_text(&self, text: impl Into<SharedString>, focused: bool) -> Div {
        div().overflow_hidden().whitespace_nowrap().rounded(px(3.)).when(focused && self.text_selected, |d| d.bg(rgba(SELECTION))).child(text.into())
    }

    /// Campo de texto de uma linha (busca da sidebar): digitar, colar, apagar; Esc fecha.
    fn side_search_key(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        match k.key.as_str() {
            "escape" => {
                self.side_search_open = false;
                self.side_query.clear();
                window.focus(&self.focus, cx);
            }
            "enter" => {}
            _ if type_into(&mut self.side_query, k, &mut self.text_selected, cx) => {}
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Abre a janela de nome de playlist (nova ou renomear) com o campo já focado.
    pub(super) fn ask_playlist_name(&mut self, editing: Option<cyder_core::Card>) {
        let text = editing.as_ref().map_or_else(String::new, |c| c.title.clone());
        self.ask(Ask::Playlist(editing), text);
    }

    /// Pergunta dos relatórios de erro, enquanto a pessoa não respondeu (e se o Cyder tem para onde mandar).
    pub(super) fn ask_reports(&mut self) {
        if self.dialog.is_none() && self.settings.send_errors.is_none() && crate::reports::available() {
            self.dialog = Some(Dialog::Reports);
        }
    }

    /// Outro aparelho tocando. O Soloist do próprio Cyder não conta: ele é o player daqui, não outro app.
    pub(crate) fn elsewhere(&self) -> Option<&str> {
        self.remote.as_deref().filter(|d| *d != cyder_core::soloist::DEVICE_NAME)
    }

    /// Abre a janela de texto com o campo já focado.
    pub(super) fn ask(&mut self, ask: Ask, text: String) {
        self.dialog = Some(Dialog::Input { ask, text });
        self.focus_dialog = true;
    }

    fn submit_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Input { ask, text }) = self.dialog.take() else { return };
        let name = text.trim().to_string();
        window.focus(&self.focus, cx);
        if name.is_empty() {
            return;
        }
        let editing = match ask {
            Ask::Playlist(editing) => editing,
            Ask::LastfmKey => return self.ask(Ask::LastfmSecret(name), String::new()),
            Ask::LastfmSecret(key) => {
                self.notify("Falando com o Last.fm…");
                return self.send(Cmd::LastfmConnect { key, secret: name });
            }
            Ask::ListenCode => {
                match cyder_core::listen::normalize(&name) {
                    Some(code) => self.send(Cmd::ListenJoin(Some(code))),
                    None => self.notify("Esse código não parece certo: são 8 letras e números."),
                }
                return;
            }
            Ask::SoloistKey => {
                match cyder_core::soloist::check_key(&name) {
                    Ok(()) => self.send(Cmd::SoloistKey(Some(name))),
                    Err(e) => self.notify(e),
                }
                return;
            }
            Ask::DiscordApp => {
                self.change(|st| {
                    st.discord_app_id = name;
                    st.discord = true;
                }, false);
                return;
            }
        };
        match editing {
            None => self.send(Cmd::CreatePlaylist(name)),
            Some(card) if card.title != name => {
                // Já mostra o nome novo; o core confirma e guarda.
                if let Some(lib) = self.library.as_mut() {
                    lib.playlists.iter_mut().filter(|c| c.id == card.id).for_each(|c| c.title = name.clone());
                }
                if let Some(page) = self.pages.get_mut(&card.id) {
                    page.card.title = name.clone();
                }
                self.send(Cmd::RenamePlaylist { card, name });
            }
            Some(_) => {}
        }
    }

    fn input_key(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        match k.key.as_str() {
            "enter" => self.submit_input(window, cx),
            "escape" => {
                self.dialog = None;
                window.focus(&self.focus, cx);
            }
            _ => {
                let Some(Dialog::Input { ask, text }) = self.dialog.as_mut() else { return };
                if !type_into(text, k, &mut self.text_selected, cx) {
                    return;
                }
                let max = ask.labels().4;
                *text = text.chars().take(max).collect();
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn dialog_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if std::mem::take(&mut self.focus_dialog) {
            window.focus(&self.dialog_focus, cx);
        }
        let dialog = self.dialog.as_ref()?;
        let card = glass(div()).bg(rgba(theme::panel(0xf7))).occlude().w(px(420.)).p_6().rounded(px(18.)).shadow_2xl().flex().flex_col().gap_4();
        let body = match dialog {
            Dialog::Shortcuts => {
                let row = |keys: &'static str, what: &'static str| {
                    div().flex().items_center().justify_between().gap_4().text_sm()
                        .child(div().text_color(rgba(theme::ink_2())).child(what))
                        .child(div().px_2().py_0p5().rounded(px(6.)).bg(rgba(theme::wash(0x14))).text_xs().font_weight(FontWeight::SEMIBOLD).child(keys))
                };
                card.child(div().text_lg().font_weight(FontWeight::BOLD).child("Atalhos"))
                    .child(div().flex().flex_col().gap_2()
                        .child(row("Espaço", "Tocar ou pausar"))
                        .child(row("Ctrl + K", "Buscar"))
                        .child(row("Ctrl + → / ←", "Próxima ou anterior"))
                        .child(row("→ / ←", "Avançar ou voltar 5 segundos"))
                        .child(row("↑ / ↓", "Volume"))
                        .child(row("L", "Letra no modo imersivo"))
                        .child(row("Esc", "Fechar o que está aberto"))
                        .child(row("Tab", "Passar pelos botões"))
                        .child(row("Botões laterais do mouse", "Voltar ou avançar entre telas")))
                    .child(div().flex().justify_end().child(Self::secondary_button("dialog-close", "Fechar")
                        .on_click(cx.listener(|t, _, _, cx| { t.dialog = None; cx.notify(); }))))
            }
            Dialog::Update(version) => card
                .child(div().text_lg().font_weight(FontWeight::BOLD).child("Atualização pronta"))
                .child(div().text_sm().text_color(rgba(theme::ink_2()))
                    .child(format!("A versão {version} foi instalada. Reinicie o Cyder para usar; a música para por alguns segundos e volta de onde estava.")))
                .child(div().flex().justify_end().gap_2()
                    .child(Self::secondary_button("update-later", "Depois").on_click(cx.listener(|t, _, _, cx| { t.dialog = None; cx.notify(); })))
                    .child(Self::primary_button("update-restart", "Reiniciar agora").on_click(cx.listener(|t, _, _, cx| t.restart_for_update(cx))))),
            Dialog::Welcome => self.welcome_dialog(card, cx),
            Dialog::Reports => card
                .child(div().text_lg().font_weight(FontWeight::BOLD).child("Enviar relatórios de erro?"))
                .child(div().text_sm().text_color(rgba(theme::ink_2())).child(REPORTS_TEXT))
                .child(div().text_sm().text_color(rgba(theme::ink_2())).child("Dá para mudar em Configurações › Sobre."))
                .child(div().flex().justify_end().gap_2()
                    .child(Self::secondary_button("reports-no", "Não enviar").on_click(cx.listener(|t, _, _, cx| {
                        t.dialog = None;
                        t.change(|st| st.send_errors = Some(false), false);
                        cx.notify();
                    })))
                    .child(Self::primary_button("reports-yes", "Enviar").on_click(cx.listener(|t, _, _, cx| {
                        t.dialog = None;
                        t.change(|st| st.send_errors = Some(true), false);
                        cx.notify();
                    })))),
            Dialog::Logout => card
                .child(div().text_lg().font_weight(FontWeight::BOLD).child("Sair da conta?"))
                .child(div().text_sm().text_color(rgba(theme::ink_2())).child("Para voltar, você entra de novo pelo navegador. A biblioteca guardada neste computador é apagada."))
                .child(div().flex().justify_end().gap_2()
                    .child(Self::secondary_button("logout-cancel", "Cancelar").on_click(cx.listener(|t, _, _, cx| { t.dialog = None; cx.notify(); })))
                    .child(Self::primary_button("logout-confirm", "Sair").on_click(cx.listener(|t, _, _, cx| {
                        t.dialog = None;
                        t.phase = Phase::Starting("Saindo da conta…".into());
                        t.library = None;
                        t.home = None;
                        t.pages.clear();
                        t.profile = None;
                        t.send(Cmd::Logout);
                        cx.notify();
                    })))),
            Dialog::Credits(c) => {
                let row = |what: String, value: String| {
                    div().flex().gap_4().text_sm()
                        .child(div().w(px(110.)).flex_none().text_color(rgba(theme::ink_3())).child(what))
                        .child(div().flex_1().min_w_0().child(value))
                };
                card.child(div().flex().flex_col().gap_0p5()
                        .child(div().text_lg().font_weight(FontWeight::BOLD).child("Créditos"))
                        .child(div().text_sm().text_color(rgba(theme::ink_2())).child(c.title.clone())))
                    .child(div().flex().flex_col().gap_2()
                        .children(c.roles.iter().map(|(role, names)| row(role.clone(), names.join(", "))))
                        .when(!c.label.is_empty(), |d| d.child(row("Gravadora".into(), c.label.clone())))
                        .child(row("Lançamento".into(), c.released.clone())))
                    .when(!c.copyrights.is_empty(), |d| d.child(div().flex().flex_col().gap_0p5().text_xs().text_color(rgba(theme::ink_3()))
                        .children(c.copyrights.iter().map(|t| div().child(t.clone())))))
                    .child(div().flex().justify_end().child(Self::secondary_button("credits-close", "Fechar")
                        .on_click(cx.listener(|t, _, _, cx| { t.dialog = None; cx.notify(); }))))
            }
            Dialog::Help(help) => {
                let (title, steps, warning, site, url) = help.content();
                let step = |i: usize, text: &'static str| {
                    div().flex().gap_3().text_sm()
                        .child(div().size(px(22.)).flex_none().rounded_full().bg(rgba(theme::wash(0x1a))).flex().items_center().justify_center()
                            .text_xs().font_weight(FontWeight::BOLD).child((i + 1).to_string()))
                        .child(div().flex_1().min_w_0().pt(px(2.)).text_color(rgba(theme::ink_2())).child(text))
                };
                card.w(px(480.))
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child(title))
                    .child(div().flex().flex_col().gap_3().children(steps.iter().enumerate().map(|(i, t)| step(i, t))))
                    .child(div().flex().gap_3().p_3().rounded(px(10.)).bg(rgba(0xf5a5241f)).text_sm()
                        .child(div().pt(px(1.)).child(icon("info", 16., 0xf5a524ff)))
                        .child(div().flex_1().min_w_0().text_color(rgba(theme::ink_1())).child(warning)))
                    .child(div().flex().justify_end().gap_2()
                        .child(Self::secondary_button("help-close", "Fechar").on_click(cx.listener(|t, _, _, cx| { t.dialog = None; cx.notify(); })))
                        .when(!matches!(help, Help::Soloist), |d| d.child(Self::primary_button("help-site", site).on_click(move |_, _, cx| cx.open_url(url))))
                        .when(matches!(help, Help::Soloist), |d| d
                            .child(Self::secondary_button("help-site", site).on_click(move |_, _, cx| cx.open_url(url)))
                            .child(Self::primary_button("help-soloist-key", "Colar chave").on_click(cx.listener(|t, _, _, cx| { t.ask(Ask::SoloistKey, String::new()); cx.notify(); })))))
            }
            Dialog::Input { ask, text } => {
                let (title, help, placeholder, action, _) = ask.labels();
                // O segredo não fica à mostra.
                let shown = if matches!(ask, Ask::LastfmSecret(_)) { "•".repeat(text.chars().count()) } else { text.clone() };
                let focused = self.dialog_focus.is_focused(window);
                let caret = || div().flex_none().w(px(1.5)).h(px(18.)).bg(rgba(theme::ink_1()));
                let field = div()
                    .id("dialog-input")
                    .key_context("TextField")
                    .track_focus(&self.dialog_focus)
                    .aria_label(placeholder)
                    .h(px(42.))
                    .px_3()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .rounded(px(10.))
                    .bg(rgba(theme::shade(0x59)))
                    .border_1()
                    .border_color(rgba(if focused { theme::ink_2() } else { theme::wash(0x29) }))
                    .cursor_text()
                    .on_click(cx.listener(|t, e: &ClickEvent, window, cx| { t.text_selected = e.click_count() >= 2; window.focus(&t.dialog_focus, cx); cx.notify(); }))
                    .on_key_down(cx.listener(|t, e: &gpui::KeyDownEvent, window, cx| t.input_key(e, window, cx)));
                let field = if text.is_empty() {
                    field.when(focused, |d| d.child(caret())).child(div().text_color(rgba(theme::ink_3())).child(placeholder))
                } else {
                    field.child(self.field_text(shown, focused)).when(focused, |d| d.child(caret()))
                };
                card.child(div().text_lg().font_weight(FontWeight::BOLD).child(title))
                    .children(help.map(|h| div().text_sm().text_color(rgba(theme::ink_2())).child(h)))
                    .child(field)
                    .child(div().flex().justify_end().gap_2()
                        .child(Self::secondary_button("name-cancel", "Cancelar").on_click(cx.listener(|t, _, window, cx| {
                            t.dialog = None;
                            window.focus(&t.focus, cx);
                            cx.notify();
                        })))
                        .child(Self::primary_button("name-save", action)
                            .on_click(cx.listener(|t, _, window, cx| { t.submit_input(window, cx); cx.notify(); }))))
            }
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x00000099))
                .occlude()
                .on_mouse_down(MouseButton::Left, cx.listener(|t, _: &MouseDownEvent, _, cx| { t.dialog = None; cx.notify(); }))
                .child(body.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
                .into_any_element(),
        )
    }
}

impl Render for Cyder {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // O imersivo e as telas de entrada ficam sempre escuras (texto claro sobre a capa borrada).
        let light = match self.settings.theme {
            config::Theme::Dark => false,
            config::Theme::Light => true,
            config::Theme::System => matches!(window.appearance(), gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight),
        };
        theme::set_light(light && !self.immersive && matches!(self.phase, Phase::Ready));
        if std::mem::take(&mut self.refocus) {
            window.focus(&self.focus, cx);
        }
        // Como o Spotify: tocando, o Alt+Tab e a barra de tarefas mostram "Artista - Música"; pausado, "Cyder".
        let title = match self.tracks.get(self.current).filter(|_| self.playing && self.elsewhere().is_none()) {
            Some(t) => format!("{} - {}", t.artist, t.name),
            None => "Cyder".to_string(),
        };
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let root = div()
            // Todas as capas passam por aqui: guarda as mais usadas e libera as outras (doc 07, RAM).
            .image_cache(crate::image_lru::lru("images", IMAGES_KEPT))
            .id("root")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgba((theme::base() << 8) | 0xff))
            .text_color(rgba(theme::ink_1()))
            .font_family(theme::FONT)
            .track_focus(&self.focus)
            // Sem contexto nenhum, o "!TextField" dos atalhos nunca casa (o GPUI exige uma pilha não vazia).
            .key_context("Cyder")
            .on_action(cx.listener(|t, _: &TogglePlay, _, cx| { t.toggle_play(); cx.notify(); }))
            .on_action(cx.listener(|t, _: &FocusSearch, window, cx| {
                t.set_immersive(false);
                t.open_search(window, cx);
            }))
            .on_action(cx.listener(|t, _: &NextTrack, _, cx| { t.next_track(); cx.notify(); }))
            .on_action(cx.listener(|t, _: &PrevTrack, _, cx| { t.prev_track(); cx.notify(); }))
            .on_action(cx.listener(|t, _: &SeekForward, _, cx| { t.seek_by(SEEK_STEP_MS as i64); cx.notify(); }))
            .on_action(cx.listener(|t, _: &SeekBack, _, cx| { t.seek_by(-(SEEK_STEP_MS as i64)); cx.notify(); }))
            // A view guarda o último render; sem notify o anel de foco só apareceria no próximo redesenho.
            .on_action(cx.listener(|_, _: &FocusNext, window, cx| { window.focus_next(cx); cx.notify(); }))
            .on_action(cx.listener(|_, _: &FocusPrev, window, cx| { window.focus_prev(cx); cx.notify(); }))
            .on_action(cx.listener(|t, _: &VolumeUp, _, cx| { t.set_volume(t.volume + VOLUME_STEP); cx.notify(); }))
            .on_action(cx.listener(|t, _: &VolumeDown, _, cx| { t.set_volume(t.volume - VOLUME_STEP); cx.notify(); }))
            // Fora do imersivo, L abre o imersivo já com a letra.
            .on_action(cx.listener(|t, _: &ToggleLyrics, _, cx| {
                if t.immersive { t.lyrics_on = !t.lyrics_on } else { t.set_immersive(true); t.lyrics_on = true }
                cx.notify();
            }))
            // Esc fecha o que estiver por cima: a lista de aparelhos, depois o imersivo.
            .on_action(cx.listener(|t, _: &ExitImmersive, _, cx| {
                if t.dialog.is_some() {
                    t.dialog = None;
                } else if t.menu.is_some() {
                    t.menu = None;
                } else if t.devices_open {
                    t.devices_open = false;
                } else {
                    t.set_immersive(false);
                }
                cx.notify();
            }))
            // Arraste do volume na janela inteira: soltar fora do slider também encerra.
            .on_mouse_move(cx.listener(|t, e: &MouseMoveEvent, _, cx| {
                t.activity(cx);
                if e.pressed_button == Some(MouseButton::Left) {
                    if t.dragging_volume {
                        t.volume_from_mouse(e.position.x);
                        cx.notify();
                    } else if t.seek_drag.is_some() {
                        t.seek_drag = Some(t.progress_from_mouse(e.position.x));
                        cx.notify();
                    }
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|t, _: &MouseDownEvent, _, cx| t.activity(cx)))
            // Botões laterais do mouse: voltar e avançar entre as telas.
            .on_mouse_down(MouseButton::Navigate(gpui::NavigationDirection::Back), cx.listener(|t, _: &MouseDownEvent, _, cx| { t.nav_back(); cx.notify(); }))
            .on_mouse_down(MouseButton::Navigate(gpui::NavigationDirection::Forward), cx.listener(|t, _: &MouseDownEvent, _, cx| { t.nav_forward(); cx.notify(); }))
            .on_key_down(cx.listener(|t, _: &gpui::KeyDownEvent, _, cx| t.activity(cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|t, _: &MouseUpEvent, _, cx| {
                t.dragging_volume = false;
                if t.volume_dirty {
                    t.send_volume();
                }
                // Só pede o seek ao soltar: arrastar não fica pulando o áudio.
                if let Some(f) = t.seek_drag.take() {
                    let duration = t.tracks.get(t.current).map_or(0, |tr| tr.duration_ms);
                    t.seek((f * duration as f32) as u32);
                    cx.notify();
                }
            }))
            .child(self.background(window));

        let root = if !matches!(self.phase, Phase::Ready) {
            root.child(self.gate(window, cx))
        } else if !self.immersive {
            root.child(self.shell(window, vw, vh, cx))
        } else {
            // Layout: capa e título à esquerda, cartão de letras à direita, player flutuando embaixo.
            let (pad, gap) = (48., 48.);
            let top = theme::TITLEBAR_H + 16.;
            let bottom = PLAYER_H + PLAYER_BOTTOM + 28.;
            let content_h = (vh - top - bottom).max(200.);
            let content_w = vw - pad * 2.;
            // Sem letra, a coluna da capa ocupa tudo e centraliza.
            let left_w = if self.lyrics_on { ((content_w - gap) * 0.42).max(260.) } else { content_w };
            let card_w = content_w - gap - left_w;
            let cover = left_w.min(content_h - 120.).max(160.);
            let track = self.tracks.get(self.current).cloned();
            let cover_el = match &self.shown_art {
                Some(art) => img(art.cover.clone()).size(px(cover)).rounded(px(20.)).shadow_2xl().into_any_element(),
                None => div().size(px(cover)).rounded(px(20.)).bg(rgba(theme::wash(0x0f))).flex().items_center().justify_center()
                    .child(icon("music", 48., theme::ink_3())).into_any_element(),
            };
            root.child(
                div()
                    .absolute()
                    .top(px(top))
                    .left(px(pad))
                    .w(px(content_w))
                    .h(px(content_h))
                    .flex()
                    .gap(px(gap))
                    .child(
                        div().w(px(left_w)).h_full().flex().flex_col().justify_center().gap_2()
                            .when(!self.lyrics_on, |d| d.items_center())
                            .child(cover_el)
                            .children(track.map(|t| {
                                let (title, artists) = self.track_links(&t, "immersive", cx);
                                div().mt_4().w(px(cover)).flex().flex_col().gap_1()
                                    .when(!self.lyrics_on, |d| d.items_center())
                                    .child(div().flex().min_w_0().text_size(px(26.)).font_weight(FontWeight::BOLD).child(title))
                                    .child(div().flex().min_w_0().gap(px(4.)).overflow_hidden().text_size(px(17.)).text_color(rgba(theme::ink_2())).children(artists))
                            })),
                    )
                    .when(self.lyrics_on, |d| d.child(div().w(px(card_w)).h_full().child(self.lyrics_card(window, card_w, content_h, cx)))),
            )
            .child(self.player_bar((vw - pad * 2.).min(860.), cx))
        };

        let notice_bottom = if self.immersive { PLAYER_H + PLAYER_BOTTOM + 16. } else { library::DOCK_H + 12. };
        root.children(self.notice.as_ref().map(|(text, _)| {
            div().absolute().bottom(px(notice_bottom)).left_0().right_0().flex().justify_center().child(
                glass(div()).px_4().py_2().rounded_full().text_sm().child(text.clone()),
            )
        }))
        // A biblioteca desenha os próprios botões da janela, no recorte do painel.
        .when(self.immersive || !matches!(self.phase, Phase::Ready), |d| d.child(self.titlebar(cx)))
        .children(self.dialog_layer(window, cx).map(|d| library::appear("dialog", d)))
    }
}
