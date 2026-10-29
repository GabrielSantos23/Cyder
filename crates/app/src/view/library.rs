//! Biblioteca (doc 05, tela 2): sidebar, grades, páginas de álbum/artista/playlist e a barra principal do player.
use std::ops::Range;

use std::{path::PathBuf, time::Duration};

use cyder_core::{Card, Cmd, Device, Kind, Page, SearchKind, Shelf, Track};
use gpui::{
    Animation, AnimationExt, AnyElement, ClickEvent, Context, FontWeight, ListAlignment, ListState, MouseButton, MouseDownEvent,
    ObjectFit, ParentElement, SharedString, Styled, UniformListScrollHandle, Window, WindowControlArea, div, ease_out_quint, img,
    linear_color_stop, linear_gradient, list, point, prelude::*, px, rgba, uniform_list,
};

pub(super) mod liked;
pub(super) mod news;
pub(super) mod search;
pub(super) mod settings;
pub(super) mod stats;

use super::menus::{Menu, MenuTarget};
use super::{Cyder, Tip, fmt, icon, loading};
use crate::theme;

pub const SIDEBAR_W: f32 = 212.;
pub const DOCK_H: f32 = 92.;
/// Respiro entre a moldura escura e o painel principal.
const GAP: f32 = 8.;
/// Moldura em cima e embaixo do painel, mais grossa que dos lados, como no Cider.
const V_GAP: f32 = 12.;
/// Faixa da moldura que cobre os cantos: o GPUI recorta em retângulo, e o que rola no painel passava
/// por cima do arredondamento. Precisa de pelo menos 0,42 × PANEL_R.
const CORNER_W: f32 = 6.;
const PANEL_R: f32 = 14.;
const TOOLBAR_H: f32 = 56.;
const QUEUE_W: f32 = 312.;
/// Minimizar, maximizar e fechar.
const CONTROLS_W: f32 = 138.;
/// As listas rolam por baixo do player flutuante; o fim delas fica acima dele.
const LIST_BOTTOM: f32 = DOCK_H + 32.;
const SHELF_CARD: f32 = 176.;
const SHELF_GAP: f32 = 16.;
/// Cartões por prateleira: todos ficam montados (a rolagem é horizontal), então poucos o bastante para a RAM.
const SHELF_MAX: usize = 12;
/// Índice da prateleira de artistas parecidos (as da tela inicial começam em 0, as da busca em 100).
const RELATED_SHELF: usize = 200;
/// Cor da capa de "Músicas curtidas", que não tem imagem no Spotify.
const LIKED_COLOR: u32 = 0x6d4aff;
/// Verde do coração das faixas curtidas, o do Spotify.
const LIKED_GREEN: u32 = 0x1ed760ff;
const PAD: f32 = 32.;
const ROW_H: f32 = 48.;
const CARD_MIN: f32 = 168.;
const GRID_GAP: f32 = 20.;
/// Título e subtítulo embaixo da capa no cartão.
const CARD_TEXT_H: f32 = 48.;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Home,
    Songs,
    Albums,
    Artists,
    Playlists,
    /// Novidades: lançamentos de quem você segue.
    New,
    /// Estatísticas: o que você ouviu pelo Cyder.
    Stats,
}

/// Altura da foto do artista no topo da página.
const HERO_H: f32 = 440.;

/// O que a lista da sidebar mostra.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SideFilter {
    All,
    Playlists,
    Artists,
    Albums,
    Podcasts,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Releases {
    All,
    Albums,
    Singles,
}

/// O que a coluna da direita mostra.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Queue,
    Lyrics,
}

#[derive(Clone)]
pub enum Screen {
    Section(Section),
    Page(Card),
    Search,
    Settings,
}

impl Screen {
    pub(super) fn key(&self) -> String {
        match self {
            Screen::Section(s) => format!("section-{}", *s as u8),
            Screen::Page(c) => format!("page-{}", c.id),
            Screen::Search => "search".into(),
            Screen::Settings => "settings".into(),
        }
    }
}

/// De onde vem uma lista de faixas: as curtidas ou a página aberta.
#[derive(Clone)]
pub(super) enum TrackSource {
    Liked,
    Page(String),
    /// As músicas encontradas na busca.
    Search,
}

#[derive(Clone)]
pub(super) enum CardSource {
    Section(Section),
    ArtistAlbums(String),
    Search(SearchKind),
    News,
}

/// Índice "aleatório" sem dependência nova: o relógio basta para escolher por onde começar.
fn random_index(len: usize) -> usize {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
    if len == 0 { 0 } else { nanos % len }
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Album => "Álbum",
        Kind::Artist => "Artista",
        Kind::Playlist => "Playlist",
        Kind::Show => "Podcast",
    }
}

impl Cyder {
    /// Histórico no tempo, como ‹ › de navegador: abrir algo novo apaga o "avançar".
    pub(super) fn go(&mut self, screen: Screen) {
        if screen.key() == self.screen.key() {
            return;
        }
        if let Screen::Page(card) = &screen {
            if !self.pages.contains_key(&card.id) {
                self.page_errors.remove(&card.id);
                self.send(Cmd::Open(card.clone()));
            }
        }
        // As estatísticas também: o que tocou desde a última vez entra.
        if matches!(screen, Screen::Section(Section::Stats)) {
            self.stats = None;
        }
        // As configurações medem o cache de novo a cada vez que abrem.
        if matches!(screen, Screen::Settings) {
            self.storage = None;
        }
        // Abrir um resultado guarda a busca nas recentes; as letras digitadas no caminho, não.
        if matches!(self.screen, Screen::Search) {
            self.remember_search();
        }
        self.history.push(std::mem::replace(&mut self.screen, screen));
        self.forward.clear();
        self.release_filter = Releases::All;
        self.refocus = true;
    }

    pub(super) fn nav_back(&mut self) {
        if let Some(prev) = self.history.pop() {
            self.forward.push(std::mem::replace(&mut self.screen, prev));
            self.refocus = true;
        }
    }

    /// Fila e letra dividem a coluna da direita: abrir uma troca a outra; clicar de novo fecha.
    fn toggle_side(&mut self, side: Side) {
        self.side = if self.side == Some(side) { None } else { Some(side) };
    }

    pub(super) fn nav_forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            self.history.push(std::mem::replace(&mut self.screen, next));
            self.refocus = true;
        }
    }

    fn scroll(&mut self, key: String) -> UniformListScrollHandle {
        self.scrolls.entry(key).or_default().clone()
    }

    fn source_tracks(&self, source: &TrackSource) -> &[Track] {
        match source {
            TrackSource::Liked => &self.liked_view,
            TrackSource::Page(id) => self.pages.get(id).map_or(&[], |p| &p.tracks),
            TrackSource::Search => self.search.as_ref().map_or(&[], |s| &s.tracks),
        }
    }

    fn source_cards(&self, source: &CardSource) -> &[Card] {
        let lib = self.library.as_ref();
        match source {
            CardSource::Section(Section::Albums) => lib.map_or(&[], |l| &l.albums),
            CardSource::Section(Section::Artists) => lib.map_or(&[], |l| &l.artists),
            CardSource::Section(_) => lib.map_or(&[], |l| &l.playlists),
            CardSource::ArtistAlbums(id) => self.pages.get(id).map_or(&[], |p| {
                let n = p.artist.as_ref().map_or(p.albums.len(), |a| a.album_count.min(p.albums.len()));
                match self.release_filter {
                    Releases::All => &p.albums,
                    Releases::Albums => &p.albums[..n],
                    Releases::Singles => &p.albums[n..],
                }
            }),
            CardSource::News => self.news.as_ref().map_or(&[], |n| match self.release_filter {
                Releases::All => &n[0],
                Releases::Albums => &n[1],
                Releases::Singles => &n[2],
            }),
            CardSource::Search(kind) => self.search.as_ref().map_or(&[], |s| match kind {
                SearchKind::Artists => &s.artists,
                SearchKind::Albums => &s.albums,
                SearchKind::Playlists => &s.playlists,
                SearchKind::Shows | SearchKind::Tracks => &s.shows,
            }),
        }
    }

    /// Toca a lista a partir de `index`. Álbuns e playlists vão com a URI, para o Spotify saber o contexto.
    fn play_from(&mut self, source: &TrackSource, index: usize) {
        let tracks = self.source_tracks(source).to_vec();
        let (context, label) = match source {
            TrackSource::Liked => (None, "Músicas curtidas".to_string()),
            TrackSource::Page(id) => self.pages.get(id).map_or((None, String::new()), |p| (Some(p.card.uri.clone()), p.card.title.clone())),
            TrackSource::Search => {
                self.remember_search();
                (None, format!("Busca: {}", self.search_query.trim()))
            }
        };
        self.queue_label = label.clone();
        self.queue_context = context.clone();
        self.send(Cmd::Play { tracks, index, context, label });
    }

    fn shuffle_from(&mut self, source: &TrackSource) {
        self.shuffle = true;
        self.send(Cmd::SetShuffle(true));
        let index = random_index(self.source_tracks(source).len());
        self.play_from(source, index);
    }

    /// Marca da lista que está tocando (sidebar e cartões): o ícone da faixa tocando, pulsando enquanto toca
    /// e parado e mais apagado quando pausado. Verde, a cor de "ativo" do app (curtida, outro aparelho): a cor
    /// da capa saía cinza em capas sem cor forte e a marca sumia.
    fn playing_mark(&self, id: impl Into<gpui::ElementId>, size: f32, color: u32) -> AnyElement {
        let mark = gpui::svg().path("icons/audio-lines.svg").size(px(size)).flex_none().text_color(rgba(color));
        if !self.playing {
            return mark.opacity(0.6).into_any_element();
        }
        mark.with_animation(id, Animation::new(Duration::from_millis(1400)).repeat(), |s, t| s.opacity(1. - 0.45 * (t * std::f32::consts::PI).sin()))
            .into_any_element()
    }

    /// A fila que está tocando veio desta lista (URI do álbum/playlist/artista; None = curtidas).
    fn is_current(&self, context: Option<&str>) -> bool {
        match context {
            Some(uri) => self.queue_context.as_deref() == Some(uri),
            None => self.queue_context.is_none() && self.queue_label == "Músicas curtidas",
        }
    }

    /// Álbum de uma faixa, se ela tiver um id de álbum de verdade (as que vieram pelo Connect sem completar não têm).
    pub(super) fn album_of(&self, track: &Track) -> Option<Card> {
        (track.album_id.len() == 22 && track.album_id.chars().all(|c| c.is_ascii_alphanumeric())).then(|| Card {
            kind: Kind::Album,
            id: track.album_id.clone(),
            uri: format!("spotify:album:{}", track.album_id),
            title: track.album.clone(),
            subtitle: track.artist.clone(),
            image: track.cover_large.clone(),
        })
    }

    /// Nome da faixa (abre o álbum) e cada artista (abre a página dele), para o player e o imersivo.
    pub(super) fn track_links(&self, track: &Track, prefix: &'static str, cx: &mut Context<Self>) -> (AnyElement, Vec<AnyElement>) {
        let link = |id: (&'static str, usize), label: String| {
            Self::focusable(id)
                .min_w_0()
                .rounded(px(4.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .hover(|s| s.underline())
                .aria_label(SharedString::from(label))
        };
        let title = match self.album_of(track) {
            Some(card) => link((prefix, 0), format!("Abrir {}", card.title))
                .on_click(cx.listener(move |t, _, _, cx| { t.set_immersive(false); t.go(Screen::Page(card.clone())); cx.notify(); }))
                .child(track.name.clone())
                .into_any_element(),
            None => div().overflow_hidden().whitespace_nowrap().text_ellipsis().child(track.name.clone()).into_any_element(),
        };
        // Sem ids (cache antigo), mostra os nomes sem link.
        let artists = if track.artists.is_empty() {
            vec![div().overflow_hidden().whitespace_nowrap().text_ellipsis().child(track.artist.clone()).into_any_element()]
        } else {
            let last = track.artists.len() - 1;
            track.artists.iter().cloned().enumerate().map(|(i, (id, name))| {
                let card = Card { kind: Kind::Artist, uri: format!("spotify:artist:{id}"), id, title: name.clone(), subtitle: "Artista".into(), image: None };
                link((prefix, 100 + i), format!("Abrir {name}"))
                    .flex_shrink_0()
                    .on_click(cx.listener(move |t, _, _, cx| { t.set_immersive(false); t.go(Screen::Page(card.clone())); cx.notify(); }))
                    .child(if i < last { format!("{name},") } else { name })
                    .into_any_element()
            }).collect()
        };
        (title, artists)
    }

    /// Coração da faixa, como no Spotify: cheio e verde se curtida; vazio só com o mouse em cima do grupo `group`
    /// (sem grupo, `""`, fica sempre à vista, como na barra do player).
    fn heart_button(&self, id: impl Into<gpui::ElementId>, group: &'static str, track: &Track, cx: &mut Context<Self>) -> AnyElement {
        let liked = self.is_liked(&track.uri);
        let track = track.clone();
        Self::focusable(id)
            .size(px(28.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .when(!liked && !group.is_empty(), |d| d.opacity(0.).group_hover(group, |s| s.opacity(1.)))
            .hover(|s| s.bg(rgba(theme::wash(0x1f))))
            .tip(if liked { "Remover das Músicas curtidas" } else { "Adicionar às Músicas curtidas" })
            .on_click(cx.listener(move |t, _: &ClickEvent, _, cx| {
                t.set_liked(&track, !liked);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(if liked { icon("heart-filled", 15., LIKED_GREEN) } else { icon("heart", 15., theme::ink_2()) })
            .into_any_element()
    }

    /// Botão "⋯" de uma linha ou cartão: aparece com o mouse em cima (grupo `group`).
    fn more_button(id: impl Into<gpui::ElementId>, group: &'static str, size: f32, target: MenuTarget, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        Self::focusable(id)
            .size(px(size))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .opacity(0.)
            .group_hover(group, |s| s.opacity(1.))
            .hover(|s| s.bg(rgba(theme::wash(0x1f))))
            .tip("Mais opções")
            .on_click(cx.listener(move |t, e: &ClickEvent, _, cx| {
                t.open_menu(target.clone(), e.position());
                cx.stop_propagation();
                cx.notify();
            }))
            .child(icon("ellipsis", 16., theme::ink_1()))
    }

    fn playing_uri(&self) -> Option<&str> {
        self.tracks.get(self.current).map(|t| t.uri.as_str())
    }

    pub(super) fn shell(&mut self, window: &mut Window, vw: f32, vh: f32, cx: &mut Context<Self>) -> AnyElement {
        let left = if self.sidebar_open { SIDEBAR_W } else { GAP };
        let right = if self.side.is_some() { QUEUE_W } else { GAP };
        let content_w = (vw - left - right - PAD * 2.).max(320.);
        let backdrop = self.backdrop_layers();
        // A página de artista começa na borda do painel: a foto passa por baixo do cabeçalho, como no Cider.
        let bleed = matches!(&self.screen, Screen::Page(c) if c.kind == Kind::Artist) && matches!(&self.screen, Screen::Page(c) if self.pages.contains_key(&c.id));
        let panel = div()
            .absolute()
            .top(px(V_GAP))
            .bottom(px(V_GAP))
            .left(px(left))
            .right(px(right))
            .rounded(px(PANEL_R))
            .overflow_hidden()
            .bg(rgba((theme::surface() << 8) | 0xff))
            .children(backdrop)
            .child(
                div()
                    .absolute()
                    .top(px(if bleed { 0. } else { TOOLBAR_H }))
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .px(px(if bleed { 0. } else { PAD }))
                    .flex()
                    .flex_col()
                    .child(self.content(window, content_w, cx)),
            )
            .child(self.toolbar(cx))
            .child(self.dock(cx))
            .children(self.remote_banner(cx))
            .children(self.listen_banner(cx))
            .children(self.limit_banner());
        div()
            .absolute()
            .inset_0()
            .bg(rgba((theme::frame() << 8) | 0xff))
            // Faixa de cima da moldura: dá para arrastar a janela por ela.
            .child(div().absolute().top_0().left_0().right(px(CONTROLS_W)).h(px(GAP)).window_control_area(WindowControlArea::Drag))
            .when(self.sidebar_open, |d| d.child(self.sidebar(window, cx)))
            .child(panel)
            // Uma faixa da cor da moldura em volta do painel, arredondada por dentro: cobre o que a lista,
            // a foto do artista ou o fundo desenharem nos cantos.
            .child(
                div()
                    .absolute()
                    .top(px(V_GAP - CORNER_W))
                    .bottom(px(V_GAP - CORNER_W))
                    .left(px(left - CORNER_W))
                    .right(px(right - CORNER_W))
                    .rounded(px(PANEL_R + CORNER_W))
                    .border(px(CORNER_W))
                    .border_color(rgba((theme::frame() << 8) | 0xff)),
            )
            .child(div().absolute().top(px(V_GAP)).bottom(px(V_GAP)).left(px(left)).right(px(right)).rounded(px(PANEL_R)).border_1().border_color(rgba(theme::wash(0x17))))
            .children(match self.side {
                Some(Side::Queue) => Some(appear("side-queue", self.queue_panel(cx).into_any_element())),
                Some(Side::Lyrics) => Some(appear("side-lyrics", self.lyrics_side(window, vh, cx).into_any_element())),
                None => None,
            })
            .child(self.window_controls())
            .when(self.devices_open, |d| d.child(appear("devices", self.devices_popover(cx).into_any_element())))
            .children(self.menu_layer(vw, vh, cx).map(|m| appear("menu", m)))
            .into_any_element()
    }

    fn sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = |this: &Self, section: Section, label: &'static str, icon_name: &'static str, cx: &mut Context<Self>| {
            let active = matches!(this.screen, Screen::Section(s) if s == section);
            Self::focusable(("nav", section as usize))
                .h(px(32.))
                .px_3()
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap_3()
                .text_sm()
                .aria_selected(active)
                .when(active, |d| d.bg(rgba(theme::wash(0x1f))).font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_1())))
                .when(!active, |d| d.text_color(rgba(theme::ink_2())).hover(|s| s.bg(rgba(theme::wash(0x0f))).text_color(rgba(theme::ink_1()))))
                .on_click(cx.listener(move |t, _, _, cx| { t.go(Screen::Section(section)); cx.notify(); }))
                .child(icon(icon_name, 16., if active { theme::ink_1() } else { theme::ink_2() }))
                .child(label)
        };
        let heading = |text: &'static str| div().px_3().mb_1().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_3())).child(text);
        let small_button = |id: &'static str, label: &'static str, icon_name: &'static str, on: bool| {
            Self::focusable(id)
                .size(px(28.))
                .flex_none()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .when(on, |d| d.bg(rgba(theme::wash(0x1f))))
                .hover(|s| s.bg(rgba(theme::wash(0x14))))
                .tip(label)
                .child(icon(icon_name, 14., if on { theme::ink_1() } else { theme::ink_2() }))
        };
        let liked_active = matches!(self.screen, Screen::Section(Section::Songs));
        let items = self.sidebar_items();
        let count = items.len();
        let query = self.side_query.trim().to_lowercase();
        let show_liked = matches!(self.side_filter, SideFilter::All | SideFilter::Playlists) && (query.is_empty() || "músicas curtidas".contains(&query));
        let side_row = |id: (&'static str, usize), active: bool| {
            Self::focusable(id)
                .w_full()
                .h(px(38.))
                .px_2()
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap_3()
                .text_sm()
                .aria_selected(active)
                .when(active, |d| d.bg(rgba(theme::wash(0x1f))).text_color(rgba(theme::ink_1())))
                .when(!active, |d| d.text_color(rgba(theme::ink_2())).hover(|s| s.bg(rgba(theme::wash(0x0f))).text_color(rgba(theme::ink_1()))))
        };
        let liked_playing = self.is_current(None);
        let accent = rgba(LIKED_GREEN);
        let liked = side_row(("side-liked", 0), liked_active)
            .on_click(cx.listener(|t, _, _, cx| { t.go(Screen::Section(Section::Songs)); cx.notify(); }))
            .child(div().size(px(28.)).flex_none().rounded(px(5.)).bg(rgba((LIKED_COLOR << 8) | 0xff)).flex().items_center().justify_center().child(icon("heart", 14., theme::ink_1())))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().when(liked_playing, |d| d.text_color(accent)).child("Músicas curtidas"))
            .when(liked_playing, |d| d.child(self.playing_mark("side-liked-playing", 15., LIKED_GREEN)));
        let handle = self.scroll("sidebar".into());
        // Virtualizada: só as linhas visíveis existem, e as capas são as de 64 px.
        let rows = uniform_list("sidebar-items", count, cx.processor(move |this, range: Range<usize>, _window, cx| {
            let open_id = match &this.screen {
                Screen::Page(c) => Some(c.id.clone()),
                _ => None,
            };
            this.sidebar_items().into_iter().enumerate().skip(range.start).take(range.len()).map(|(i, card)| {
                let active = open_id.as_deref() == Some(card.id.as_str());
                let pinned = this.pins.contains(&card.uri);
                let playing_here = this.is_current(Some(&card.uri));
                let accent = rgba(LIKED_GREEN);
                let thumb = card.image.as_ref().and_then(|u| this.thumbs_small.get(u)).cloned();
                let round = card.kind == Kind::Artist;
                let title = card.title.clone();
                let menu_card = card.clone();
                let (drag, target_uri) = (SideDrag { uri: card.uri.clone(), title: card.title.clone() }, card.uri.clone());
                side_row(("side-item", i), active)
                    // Arrastar para reorganizar; a ordem fica salva neste computador.
                    .on_drag(drag, |d, _, _, cx| cx.new(|_| DragGhost(d.title.clone().into())))
                    .drag_over::<SideDrag>(|s, _, _, _| s.bg(rgba(theme::wash(0x26))))
                    .on_drop(cx.listener(move |t, d: &SideDrag, _, cx| { t.reorder_library(&d.uri, &target_uri); cx.notify(); }))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |t, e: &MouseDownEvent, _, cx| { t.open_menu(MenuTarget::Card(menu_card.clone()), e.position); cx.notify(); }))
                    .on_click(cx.listener(move |t, _, _, cx| { t.go(Screen::Page(card.clone())); cx.notify(); }))
                    .child(match thumb {
                        Some(p) => {
                            let image = img(p).size(px(28.)).flex_none().object_fit(ObjectFit::Cover);
                            if round { image.rounded_full() } else { image.rounded(px(5.)) }.into_any_element()
                        }
                        None => div().size(px(28.)).flex_none().rounded(px(if round { 14. } else { 5. })).bg(rgba(theme::wash(0x14))).into_any_element(),
                    })
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().when(playing_here, |d| d.text_color(accent)).child(title))
                    .when(pinned, |d| d.child(icon("pin", 12., theme::ink_3())))
                    .when(playing_here, |d| d.child(this.playing_mark(("side-playing", i), 15., LIKED_GREEN)))
                    .into_any_element()
            }).collect::<Vec<_>>()
        }))
        .flex_1()
        .min_h_0()
        .pb_3()
        .track_scroll(&handle);
        let empty = match (self.side_filter, query.is_empty()) {
            (_, false) => Some("Nada encontrado."),
            _ if self.library.is_none() => None,
            _ => Some("Nada aqui ainda."),
        };
        // O modo imersivo não fica aqui: é uma ação, não uma seção. Abre pelo botão da barra do player e pela capa.
        let search = self.search_field(window, cx);
        let filter_label = match self.side_filter {
            SideFilter::All => "Sua biblioteca",
            SideFilter::Playlists => "Suas playlists",
            SideFilter::Artists => "Artistas",
            SideFilter::Albums => "Álbuns",
            SideFilter::Podcasts => "Podcasts",
        };
        let library_bar = div()
            .mt_4()
            .mb_1()
            .flex()
            .items_center()
            .gap_1()
            .child(small_button("side-search", if self.side_search_open { "Fechar a busca na biblioteca" } else { "Buscar na biblioteca" }, "search", self.side_search_open)
                .on_click(cx.listener(|t, _, window, cx| {
                    t.side_search_open = !t.side_search_open;
                    if t.side_search_open {
                        window.focus(&t.side_search_focus, cx);
                    } else {
                        t.side_query.clear();
                        window.focus(&t.focus, cx);
                    }
                    cx.notify();
                })))
            .child(div().flex_1().px_1().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_3())).child(filter_label))
            .child(small_button("side-create", "Criar playlist", "plus", false)
                .on_click(cx.listener(|t, _, _, cx| { t.ask_playlist_name(None); cx.notify(); })))
            .child(small_button("side-filter", "Filtrar a biblioteca", "list-filter", self.side_filter != SideFilter::All)
                .on_click(cx.listener(|t, e: &ClickEvent, _, cx| { t.open_menu(MenuTarget::SidebarFilter, e.position()); cx.notify(); })));
        let search_field = self.side_search_open.then(|| {
            let focused = self.side_search_focus.is_focused(window);
            let caret = || div().w(px(1.5)).h(px(16.)).bg(rgba(theme::ink_1()));
            let field = div()
                .id("side-search-field")
                .key_context("TextField")
                .track_focus(&self.side_search_focus)
                .aria_label("Buscar na biblioteca")
                .mb_1()
                .h(px(32.))
                .px_3()
                .flex()
                .items_center()
                .overflow_hidden()
                .rounded(px(8.))
                .bg(rgba(theme::shade(0x40)))
                .border_1()
                .border_color(rgba(if focused { theme::ink_3() } else { theme::wash(0x1f) }))
                .text_sm()
                .cursor_text()
                .on_click(cx.listener(|t, _, window, cx| { window.focus(&t.side_search_focus, cx); cx.notify(); }))
                .on_key_down(cx.listener(|t, e: &gpui::KeyDownEvent, window, cx| t.side_search_key(e, window, cx)));
            if self.side_query.is_empty() {
                field.when(focused, |d| d.child(caret())).child(div().text_color(rgba(theme::ink_3())).child(format!("Buscar em {}", filter_label.to_lowercase())))
            } else {
                field.child(div().overflow_hidden().whitespace_nowrap().child(self.side_query.clone())).when(focused, |d| d.child(caret()))
            }
        });
        // Rodapé: a conta, com configurações, atalhos e sair.
        let account_top = f32::from(window.viewport_size().height) - V_GAP - 44.;
        let (name, image) = self.profile.clone().unwrap_or_else(|| ("Sua conta".into(), None));
        let avatar = match image.and_then(|u| self.thumbs_small.get(&u).cloned()) {
            Some(p) => img(p).size(px(28.)).flex_none().rounded_full().object_fit(ObjectFit::Cover).into_any_element(),
            None => div().size(px(28.)).flex_none().rounded_full().bg(rgba(theme::wash(0x1f))).flex().items_center().justify_center()
                .text_xs().font_weight(FontWeight::BOLD).child(name.chars().next().unwrap_or('?').to_uppercase().to_string()).into_any_element(),
        };
        let account = Self::focusable("account")
            .mb(px(V_GAP))
            .h(px(44.))
            .px_2()
            .rounded(px(12.))
            .flex()
            .items_center()
            .gap_2()
            .bg(rgba(theme::wash(0x0a)))
            .border_1()
            .border_color(rgba(theme::wash(0x12)))
            .hover(|s| s.bg(rgba(theme::wash(0x14))))
            .aria_label("Conta: configurações, atalhos e sair")
            .on_click(cx.listener(move |t, e: &ClickEvent, _, cx| {
                let mut menu = Menu::new(MenuTarget::Account, e.position());
                // Preso em cima do botão da conta e da mesma largura, como na referência.
                menu.fixed = Some((12., account_top - 6., SIDEBAR_W - 24.));
                t.menu = Some(menu);
                cx.notify();
            }))
            .child(avatar)
            .child(div().flex_1().min_w_0().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
            .child(icon("ellipsis", 14., theme::ink_3()));
        div()
            .absolute()
            .top_0()
            .left_0()
            .bottom_0()
            .w(px(SIDEBAR_W))
            .px_3()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                // Busca e área de arrastar a janela, na altura do cabeçalho do painel.
                div().h(px(TOOLBAR_H + V_GAP)).pt(px(V_GAP)).flex().items_center().window_control_area(WindowControlArea::Drag).child(search),
            )
            .child(nav(self, Section::Home, "Início", "house", cx))
            .child(nav(self, Section::New, "Novidades", "sparkles", cx))
            .child(nav(self, Section::Stats, "Estatísticas", "chart-column", cx))
            .child(div().mt_3().child(heading("Biblioteca")))
            .child(nav(self, Section::Albums, "Álbuns", "disc-3", cx))
            .child(nav(self, Section::Artists, "Artistas", "user-round", cx))
            .child(nav(self, Section::Playlists, "Playlists", "list-music", cx))
            .child(library_bar)
            .children(search_field)
            .when(show_liked, |d| d.child(liked))
            .child(if count == 0 {
                div().flex_1().px_3().pt_2().text_xs().text_color(rgba(theme::ink_3())).children(empty).into_any_element()
            } else {
                rows.into_any_element()
            })
            .child(account)
    }

    /// A lista da sidebar conforme o filtro e a busca.
    fn sidebar_items(&self) -> Vec<Card> {
        let lib = self.library.as_ref();
        let mut items = match self.side_filter {
            SideFilter::All => {
                let mut v = self.sidebar_playlists();
                if let Some(l) = lib {
                    v.extend(l.albums.iter().cloned());
                    v.extend(l.artists.iter().cloned());
                }
                v
            }
            SideFilter::Playlists => self.sidebar_playlists(),
            SideFilter::Artists => lib.map(|l| l.artists.clone()).unwrap_or_default(),
            SideFilter::Albums => lib.map(|l| l.albums.clone()).unwrap_or_default(),
            SideFilter::Podcasts => lib.map(|l| l.shows.clone()).unwrap_or_default(),
        };
        // Ordem arrastada à mão: fixadas continuam no topo; o que é novo (fora da ordem salva) vem antes.
        if !self.library_order.is_empty() {
            let pos: std::collections::HashMap<&str, usize> = self.library_order.iter().enumerate().map(|(i, u)| (u.as_str(), i + 1)).collect();
            items.sort_by_key(|c| (!self.pins.contains(&c.uri), pos.get(c.uri.as_str()).copied().unwrap_or(0)));
        }
        let q = self.side_query.trim().to_lowercase();
        if !q.is_empty() {
            items.retain(|c| c.title.to_lowercase().contains(&q) || c.subtitle.to_lowercase().contains(&q));
        }
        items
    }

    /// Solta `src` no lugar de `dst` na lista da sidebar. A ordem salva é a da lista "Tudo",
    /// então mover num filtro (só artistas, por exemplo) vale para os outros também.
    fn reorder_library(&mut self, src: &str, dst: &str) {
        if src == dst {
            return;
        }
        if self.pins.iter().any(|p| p == src || p == dst) {
            self.notify("As fixadas ficam no topo. Desafixe para mover.");
            return;
        }
        let (filter, query) = (self.side_filter, std::mem::take(&mut self.side_query));
        self.side_filter = SideFilter::All;
        let mut order: Vec<String> = self.sidebar_items().into_iter().filter(|c| !self.pins.contains(&c.uri)).map(|c| c.uri).collect();
        (self.side_filter, self.side_query) = (filter, query);
        let (Some(from), Some(to)) = (order.iter().position(|u| u == src), order.iter().position(|u| u == dst)) else { return };
        let uri = order.remove(from);
        order.insert(to, uri);
        self.library_order = order.clone();
        self.send(Cmd::SaveLibraryOrder(order));
    }

    /// Arrasta na fila (depois da que está tocando); o core recarrega o player na mesma posição.
    /// Já mostra a ordem nova; o core manda para o Spotify (e recarrega a página se ele recusar).
    fn move_in_playlist(&mut self, card: &Card, from: usize, to: usize) {
        let Some(page) = self.pages.get_mut(&card.id) else { return };
        if from == to || from >= page.tracks.len() || to >= page.tracks.len() {
            return;
        }
        let t = page.tracks.remove(from);
        page.tracks.insert(to, t);
        self.send(Cmd::MoveInPlaylist { card: card.clone(), from, to });
    }

    fn move_queue(&mut self, from: usize, to: usize) {
        if from == to || from <= self.current || to <= self.current || from >= self.tracks.len() || to >= self.tracks.len() {
            return;
        }
        let t = self.tracks.remove(from);
        self.tracks.insert(to, t);
        let current = self.current;
        self.lyrics.retain(|i, _| *i <= current);
        self.art.retain(|i, _| *i <= current);
        self.send(Cmd::MoveQueue { from, to });
    }

    /// Fixadas primeiro (na ordem em que foram fixadas); depois as tocadas por último; o resto na ordem do Spotify.
    pub(super) fn sidebar_playlists(&self) -> Vec<Card> {
        let all = self.library.as_ref().map(|l| l.playlists.clone()).unwrap_or_default();
        let mut out: Vec<Card> = self.pins.iter().filter_map(|u| all.iter().find(|c| &c.uri == u).cloned()).collect();
        let mut rest: Vec<(usize, Card)> = all.into_iter().enumerate().filter(|(_, c)| !self.pins.contains(&c.uri)).collect();
        rest.sort_by_key(|(i, c)| (std::cmp::Reverse(self.recent.get(&c.uri).copied().unwrap_or(0)), *i));
        out.extend(rest.into_iter().map(|(_, c)| c));
        out
    }

    fn content(&mut self, window: &mut Window, width: f32, cx: &mut Context<Self>) -> AnyElement {
        match self.screen.clone() {
            Screen::Section(Section::Home) => self.home_view(cx),
            Screen::Section(Section::New) => self.news_view(width, cx),
            Screen::Section(Section::Stats) => self.stats_view(width, cx),
            Screen::Section(Section::Songs) => self.liked_screen(window, cx),
            Screen::Section(section) => {
                let (title, empty, detail, noun) = match section {
                    Section::Albums => ("Álbuns", "Você ainda não salvou nenhum álbum.", "Num álbum, abra “⋯” e escolha Salvar na Sua Biblioteca.", ("álbum", "álbuns")),
                    Section::Artists => ("Artistas", "Você ainda não segue nenhum artista.", "Num artista, abra “⋯” e escolha Seguir.", ("artista", "artistas")),
                    _ => ("Playlists", "Você ainda não tem playlists.", "Crie uma aqui, ou salve as de outras pessoas pela busca.", ("playlist", "playlists")),
                };
                let action = if section == Section::Playlists {
                    Self::primary_button("empty-new-playlist", "Criar playlist").on_click(cx.listener(|t, _, _, cx| { t.ask_playlist_name(None); cx.notify(); }))
                } else {
                    Self::primary_button("empty-search", "Buscar").on_click(cx.listener(|t, _, window, cx| t.open_search(window, cx)))
                };
                let source = CardSource::Section(section);
                let body = match &self.library {
                    None => {
                        let cols = (((width + GRID_GAP) / (CARD_MIN + GRID_GAP)).floor() as usize).max(2);
                        let card_w = (width - GRID_GAP * (cols - 1) as f32) / cols as f32;
                        loading::grid(cols, card_w, section == Section::Artists)
                    }
                    Some(_) if self.source_cards(&source).is_empty() => Self::empty_state(empty, detail, Some(action)),
                    Some(_) => self.card_grid(source.clone(), width, cx),
                };
                let n = self.source_cards(&source).len();
                let subtitle = if self.library.is_none() { String::new() } else if n == 1 { format!("1 {}", noun.0) } else { format!("{n} {}", noun.1) };
                div().size_full().flex().flex_col().child(self.section_header(title, subtitle, None, cx)).child(body).into_any_element()
            }
            Screen::Page(card) => self.page_view(window, card, width, cx),
            Screen::Search => self.search_view(width, cx),
            Screen::Settings => self.settings_view(window, cx),
        }
    }

    fn quiet(text: &'static str) -> impl IntoElement {
        div().pt_8().text_color(rgba(theme::ink_3())).child(text)
    }

    /// Tela vazia: o que falta, por quê ou como encher, e o próximo passo (quando há um).
    pub(super) fn empty_state(title: &'static str, detail: &'static str, action: Option<gpui::Stateful<gpui::Div>>) -> AnyElement {
        div()
            .pt_8()
            .flex()
            .flex_col()
            .items_start()
            .gap_1()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(div().text_sm().text_color(rgba(theme::ink_3())).child(detail))
            .children(action.map(|a| div().pt_3().child(a)))
            .into_any_element()
    }

    /// Título da seção com contagem e, para listas de faixas, Tocar/Aleatório.
    fn section_header(&self, title: &'static str, subtitle: String, play: Option<TrackSource>, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .pb_4()
            .flex()
            .items_end()
            .gap_4()
            .child(
                div().flex_1().flex().flex_col().gap_0p5()
                    .child(div().text_size(px(24.)).font_weight(FontWeight::BOLD).child(title))
                    .child(div().text_sm().text_color(rgba(theme::ink_3())).child(subtitle)),
            )
            .children(play.map(|source| self.play_buttons(source, cx)))
    }

    fn play_buttons(&self, source: TrackSource, cx: &mut Context<Self>) -> impl IntoElement {
        let s2 = source.clone();
        let context = match &source {
            TrackSource::Liked | TrackSource::Search => None,
            TrackSource::Page(id) => self.pages.get(id).map(|p| p.card.uri.clone()),
        };
        let here = self.is_current(context.as_deref());
        div()
            .flex()
            .gap_2()
            .child(Self::primary_button("play-list", if here && self.playing { "Pausar" } else { "Tocar" })
                .on_click(cx.listener(move |t, _, _, cx| {
                    if here { t.send(Cmd::Toggle) } else { t.play_from(&source, 0) }
                    cx.notify();
                })))
            .child(Self::secondary_button("shuffle-list", "Aleatório").on_click(cx.listener(move |t, _, _, cx| { t.shuffle_from(&s2); cx.notify(); })))
    }

    fn track_list(&mut self, source: TrackSource, covers: bool, cx: &mut Context<Self>) -> AnyElement {
        let count = self.source_tracks(&source).len();
        let key = format!("tracks-{}", self.screen.key());
        let handle = self.scroll(key.clone());
        uniform_list(key, count, cx.processor(move |this, range: Range<usize>, _window, cx| this.track_rows(&source, covers, range, cx)))
            .flex_1()
            .min_h_0()
            .pb(px(LIST_BOTTOM))
            // O fundo da linha sai da coluna; o texto fica alinhado com o título acima.
            .mx(px(-12.))
            .track_scroll(&handle)
            .into_any_element()
    }

    fn track_rows(&mut self, source: &TrackSource, covers: bool, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let accent = self.accent_ink();
        let playing_uri = self.playing_uri().map(String::from);
        // Na busca, chegar perto do fim pede mais músicas.
        if matches!(source, TrackSource::Search) && range.end + 10 >= self.source_tracks(source).len() {
            self.request_more(SearchKind::Tracks);
        }
        let tracks = self.source_tracks(source);
        range
            .filter_map(|i| tracks.get(i).map(|t| (i, t.clone())))
            .map(|(i, t)| {
                let now = playing_uri.as_deref() == Some(t.uri.as_str());
                let thumb = t.cover_small.as_ref().and_then(|u| self.thumbs.get(u)).cloned();
                let source = source.clone();
                let menu_track = t.clone();
                let playing = self.playing;
                Self::focusable(("track", i))
                    .group("track-row")
                    .w_full()
                    .h(px(ROW_H))
                    .px_3()
                    .rounded(px(10.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(now, |d| d.bg(rgba(theme::wash(0x1f))))
                    .hover(|s| s.bg(rgba(theme::wash(0x14))))
                    .aria_label(SharedString::from(format!("Tocar {} – {}", t.name, t.artist)))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |t, e: &MouseDownEvent, _, cx| { t.open_menu(MenuTarget::Track(menu_track.clone()), e.position); cx.notify(); }))
                    .on_click(cx.listener(move |this, _, _, cx| { if now { this.toggle_play() } else { this.play_from(&source, i) } cx.notify(); }))
                    .child(index_cell(i, now, playing, "track-row", accent))
                    .when(covers, |d| d.child(match thumb {
                        Some(p) => img(p).size(px(36.)).rounded(px(6.)).object_fit(ObjectFit::Cover).into_any_element(),
                        None => div().size(px(36.)).rounded(px(6.)).bg(rgba(theme::wash(0x0f))).into_any_element(),
                    }))
                    .child(
                        div().flex_1().min_w_0().flex().flex_col()
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis()
                                .when(now, |d| d.text_color(accent)).child(t.name.clone()))
                            .child(div().text_xs().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(t.artist.clone())),
                    )
                    .when(covers, |d| d.child(div().flex_1().min_w_0().text_sm().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(t.album.clone())))
                    .child(self.heart_button(("track-heart", i), "track-row", &t, cx))
                    .child(div().w(px(44.)).flex().justify_end().text_xs().text_color(rgba(theme::ink_3())).child(fmt(t.duration_ms)))
                    .child(Self::more_button(("track-more", i), "track-row", 28., MenuTarget::Track(t.clone()), cx))
                    .into_any_element()
            })
            .collect()
    }

    fn card_grid(&mut self, source: CardSource, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let cols = (((width + GRID_GAP) / (CARD_MIN + GRID_GAP)).floor() as usize).max(2);
        let card_w = (width - GRID_GAP * (cols - 1) as f32) / cols as f32;
        let rows = self.source_cards(&source).len().div_ceil(cols);
        let key = format!("grid-{}", self.screen.key());
        let handle = self.scroll(key.clone());
        uniform_list(key, rows, cx.processor(move |this, range: Range<usize>, _window, cx| {
            range.map(|r| this.card_row(&source, r, cols, card_w, cx)).collect::<Vec<_>>()
        }))
        .flex_1()
        .min_h_0()
        .pb(px(LIST_BOTTOM))
        .track_scroll(&handle)
        .into_any_element()
    }

    fn card_row(&mut self, source: &CardSource, row: usize, cols: usize, card_w: f32, cx: &mut Context<Self>) -> AnyElement {
        if let CardSource::Search(kind) = source {
            if (row + 3) * cols >= self.source_cards(source).len() {
                self.request_more(*kind);
            }
        }
        let cards: Vec<Card> = self.source_cards(source).iter().skip(row * cols).take(cols).cloned().collect();
        div()
            .w_full()
            .h(px(card_w + CARD_TEXT_H + GRID_GAP))
            .flex()
            .gap(px(GRID_GAP))
            .children(cards.into_iter().enumerate().map(|(i, card)| self.card_el(card, card_w, ("card", row * cols + i), cx)))
            .into_any_element()
    }

    /// Cartão de álbum, artista ou playlist: capa (redonda para artista), título e subtítulo; abre a página.
    fn card_el(&mut self, card: Card, card_w: f32, id: (&'static str, usize), cx: &mut Context<Self>) -> AnyElement {
        let thumb = card.image.as_ref().and_then(|u| self.thumbs.get(u)).cloned();
        let round = |d: gpui::Div| if card.kind == Kind::Artist { d.rounded_full() } else { d.rounded(px(12.)) };
        let art = match thumb {
            Some(p) => {
                let image = img(p).size(px(card_w)).object_fit(ObjectFit::Cover);
                if card.kind == Kind::Artist { image.rounded_full() } else { image.rounded(px(12.)) }.into_any_element()
            }
            None => round(div().size(px(card_w)).bg(rgba(theme::wash(0x0f))).flex().items_center().justify_center())
                .child(icon(match card.kind { Kind::Artist => "user-round", Kind::Show => "podcast", _ => "disc-3" }, 32., theme::ink_3()))
                .into_any_element(),
        };
        let (title, subtitle) = (card.title.clone(), card.subtitle.clone());
        let centered = card.kind == Kind::Artist;
        let menu_card = card.clone();
        let more = Self::more_button((id.0, id.1 + 500_000), "card", 30., MenuTarget::Card(card.clone()), cx)
            .absolute()
            .right(px(8.))
            .bottom(px(8.))
            .bg(rgba(theme::shade(0xb3)));
        // Tocando esta lista: contorno verde na capa, um círculo verde com o ícone de tocando no canto e o nome
        // em verde. Precisa aparecer sobre qualquer capa, clara ou escura.
        let playing_here = !card.uri.is_empty() && self.is_current(Some(&card.uri));
        let inset = if card.kind == Kind::Artist { card_w * 0.1 } else { 10. };
        let badge = playing_here.then(|| {
            div().absolute().left(px(inset)).bottom(px(inset)).size(px(36.)).rounded_full().bg(rgba(LIKED_GREEN)).shadow_lg()
                .flex().items_center().justify_center().child(self.playing_mark((id.0, id.1 + 900_000), 18., 0x000000ff))
        });
        let ring = playing_here.then(|| {
            let ring = div().absolute().inset_0().border_2().border_color(rgba(LIKED_GREEN));
            if card.kind == Kind::Artist { ring.rounded_full() } else { ring.rounded(px(12.)) }
        });
        let accent = rgba(LIKED_GREEN);
        let art = div().relative().child(art).children(ring).child(more).children(badge);
        Self::focusable(id)
            .group("card")
            .on_mouse_down(MouseButton::Right, cx.listener(move |t, e: &MouseDownEvent, _, cx| { t.open_menu(MenuTarget::Card(menu_card.clone()), e.position); cx.notify(); }))
            .w(px(card_w))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(14.))
            .when(centered, |d| d.items_center())
            .aria_label(SharedString::from(format!("Abrir {} {}", kind_label(card.kind).to_lowercase(), title)))
            .hover(|s| s.opacity(0.85))
            .on_click(cx.listener(move |t, _, _, cx| { t.go(Screen::Page(card.clone())); cx.notify(); }))
            .child(art)
            .child(
                div().w_full().flex().flex_col().when(centered, |d| d.items_center())
                    .child(div().max_w_full().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().when(playing_here, |d| d.text_color(accent)).child(title))
                    .child(div().max_w_full().text_xs().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(subtitle)),
            )
            .into_any_element()
    }

    /// Tela inicial (como a Home do Apple Music no Cider): prateleiras que rolam na horizontal.
    fn home_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let header = div()
            .pb_4()
            .flex()
            .items_center()
            .child(div().flex_1().text_size(px(24.)).font_weight(FontWeight::BOLD).child("Início"))
            .child(Self::chip("home-refresh", "Atualizar a tela inicial", "rotate-cw", false, true)
                .on_click(cx.listener(|t, _, _, cx| { t.send(Cmd::RefreshHome); cx.notify(); })));
        let shelves = self.home_shelves();
        let body = match shelves {
            None => loading::home(SHELF_CARD),
            Some(shelves) => div()
                .id("home-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pb(px(LIST_BOTTOM))
                .flex()
                .flex_col()
                .gap(px(28.))
                .children(shelves.into_iter().enumerate().map(|(i, mut shelf)| {
                    shelf.cards.truncate(SHELF_MAX);
                    self.shelf(i, shelf, cx)
                }))
                .into_any_element(),
        };
        div().size_full().flex().flex_col().child(header).child(body).into_any_element()
    }

    /// Tocadas recentemente, as playlists que o Spotify fez para você (daylist, Daily Mix, mixes salvos),
    /// as suas playlists e os artistas mais ouvidos. As de playlist saem da biblioteca, na ordem da sidebar.
    fn home_shelves(&self) -> Option<Vec<Shelf>> {
        if self.home.is_none() && self.library.is_none() {
            return None;
        }
        let from_api = self.home.clone().unwrap_or_default();
        let (spotify, mine): (Vec<Card>, Vec<Card>) = self.sidebar_playlists().into_iter().partition(|c| c.subtitle == "Spotify");
        let mut shelves = Vec::new();
        let mut api = from_api.into_iter();
        shelves.extend(api.next());
        for (title, cards) in [("Feitas para você", spotify), ("Suas playlists", mine)] {
            if !cards.is_empty() {
                shelves.push(Shelf { title: title.into(), cards: cards.into_iter().take(SHELF_MAX).collect() });
            }
        }
        shelves.extend(api);
        Some(shelves)
    }

    fn shelf(&mut self, i: usize, shelf: Shelf, cx: &mut Context<Self>) -> AnyElement {
        let handle = self.shelf_scrolls.entry(i).or_default().clone();
        // As setas andam três cartões por vez, sem passar das pontas.
        let step = (SHELF_CARD + SHELF_GAP) * 3.;
        let (h_back, h_next) = (handle.clone(), handle.clone());
        let arrow = |id: (&'static str, usize), label: &'static str, icon_name: &'static str| {
            Self::focusable(id)
                .size(px(30.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(theme::wash(0x12)))
                .hover(|s| s.bg(rgba(theme::wash(0x26))))
                .tip(label)
                .child(icon(icon_name, 15., theme::ink_1()))
        };
        let cards: Vec<AnyElement> = shelf.cards.into_iter().enumerate().map(|(j, card)| self.card_el(card, SHELF_CARD, ("shelf-card", i * 1000 + j), cx)).collect();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div().flex().items_center().gap_2()
                    .child(div().flex_1().text_lg().font_weight(FontWeight::BOLD).child(shelf.title))
                    .child(arrow(("shelf-back", i), "Voltar nesta prateleira", "chevron-left").on_click(cx.listener(move |_, _, _, cx| {
                        let o = h_back.offset();
                        h_back.set_offset(point((o.x + px(step)).min(px(0.)), o.y));
                        cx.notify();
                    })))
                    .child(arrow(("shelf-next", i), "Avançar nesta prateleira", "chevron-right").on_click(cx.listener(move |_, _, _, cx| {
                        let (o, max) = (h_next.offset(), h_next.max_offset());
                        h_next.set_offset(point((o.x - px(step)).max(-max.x), o.y));
                        cx.notify();
                    }))),
            )
            .child(
                div()
                    .id(("shelf", i))
                    .w_full()
                    .overflow_x_scroll()
                    // A roda do mouse (vertical) rola a página, não a prateleira; trackpad e Shift continuam na horizontal.
                    .restrict_scroll_to_axis()
                    .track_scroll(&handle)
                    .flex()
                    .gap(px(SHELF_GAP))
                    .children(cards),
            )
            .into_any_element()
    }

    /// Letra na coluna da direita, no lugar da fila.
    fn lyrics_side(&mut self, window: &mut Window, vh: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let track = self.tracks.get(self.current).cloned();
        let top = theme::TITLEBAR_H;
        let header_h = 52.;
        let body_h = (vh - top - V_GAP - header_h).max(120.);
        let content = self.lyrics_content(window, QUEUE_W, body_h, &super::PANEL_LYRICS, cx);
        div()
            .absolute()
            .top(px(top))
            .right_0()
            .bottom(px(GAP))
            .w(px(QUEUE_W))
            .flex()
            .flex_col()
            .child(
                div().h(px(header_h)).px_4().flex().flex_col().justify_center()
                    .child(div().text_sm().font_weight(FontWeight::BOLD).child("Letra"))
                    .children(track.map(|t| div().text_xs().text_color(rgba(theme::ink_3())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(format!("{} – {}", t.name, t.artist)))),
            )
            .child(div().h(px(body_h)).w_full().overflow_hidden().child(content))
    }

    fn page_view(&mut self, _window: &mut Window, card: Card, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let column = div().size_full().flex().flex_col();

        let Some(page) = self.pages.get(&card.id).cloned() else {
            let body = match self.page_errors.get(&card.id).cloned() {
                Some(e) => div()
                    .pt_8()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_color(rgba(theme::ink_2())).child(format!("Não deu para abrir \"{}\". {e}", card.title)))
                    .child(div().flex().child(Self::primary_button("retry-page", "Tentar de novo").on_click(cx.listener(move |t, _, _, cx| {
                        t.page_errors.remove(&card.id);
                        t.send(Cmd::Open(card.clone()));
                        cx.notify();
                    }))))
                    .into_any_element(),
                None => match card.kind {
                    Kind::Artist => loading::artist(HERO_H - TOOLBAR_H, 0.),
                    kind => loading::media_page(kind == Kind::Playlist),
                },
            };
            return column.child(body).into_any_element();
        };

        if page.card.kind != Kind::Artist {
            return self.media_page(&page, cx);
        }
        self.artist_page(&page, width, cx)
    }

    /// Página de artista como no Cider: foto grande no topo com o nome, o lançamento mais recente ao lado
    /// das mais tocadas e, embaixo, a discografia com filtros. Uma lista só, virtualizada, que rola inteira.
    fn artist_page(&mut self, page: &Page, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let id = page.card.id.clone();
        let cols = (((width + GRID_GAP) / (CARD_MIN + GRID_GAP)).floor() as usize).max(2);
        let card_w = (width - GRID_GAP * (cols - 1) as f32) / cols as f32;
        let rows = self.source_cards(&CardSource::ArtistAlbums(id.clone())).len().div_ceil(cols);
        // Foto, destaques, título da discografia, as linhas de cartões, os parecidos e o respiro embaixo do player.
        let count = 3 + rows.max(1) + 2;
        let state = self.page_lists.entry(format!("artist-{id}")).or_insert_with(|| ListState::new(count, ListAlignment::Top, px(600.))).clone();
        // Trocar o filtro troca só as linhas de cartões; `reset` voltaria a rolagem para o topo.
        let old = state.item_count();
        if old != count {
            state.splice(3..old - 2, count - 5);
        }
        list(state, cx.processor(move |this, i: usize, _window, cx| {
            let source = CardSource::ArtistAlbums(id.clone());
            match i {
                0 => this.artist_hero(&id, cx),
                1 => this.artist_highlights(&id, width, cx),
                2 => this.discography_header(&id, cx),
                _ if i == count - 1 => div().h(px(LIST_BOTTOM)).into_any_element(),
                _ if i == count - 2 => this.related_shelf(&id, cx),
                _ if rows == 0 => div().px(px(PAD)).child(Self::quiet("Nada nesta seção.")).into_any_element(),
                _ => div().px(px(PAD)).child(this.card_row(&source, i - 3, cols, card_w, cx)).into_any_element(),
            }
        }))
        .flex_1()
        .min_h_0()
        .into_any_element()
    }

    /// "Fãs também curtem": os artistas parecidos, numa prateleira como as da tela inicial.
    fn related_shelf(&mut self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let related = self.pages.get(id).and_then(|p| p.artist.as_ref()).map(|a| a.related.clone()).unwrap_or_default();
        if related.is_empty() {
            return div().into_any_element();
        }
        div().px(px(PAD)).pt_8().child(self.shelf(RELATED_SHELF, Shelf { title: "Fãs também curtem".into(), cards: related }, cx)).into_any_element()
    }

    fn artist_hero(&mut self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = self.pages.get(id) else { return div().into_any_element() };
        let c = page.card.clone();
        let has_top = !page.tracks.is_empty();
        // A foto grande chega depois; até lá, a miniatura (240 px) segura o lugar.
        let photo = page.artist.as_ref().and_then(|a| a.hero.as_ref()).and_then(|u| self.heroes.get(u)).cloned()
            .or_else(|| c.image.as_ref().and_then(|u| self.thumbs.get(u)).cloned());
        let source = TrackSource::Page(c.id.clone());
        let here = self.is_current(Some(&c.uri));
        let pause = here && self.playing;
        let (s2, play_card, shuffle_card) = (source.clone(), c.clone(), c.clone());
        let round = |button_id: &'static str, size: f32, label: &'static str| {
            Self::focusable(button_id).size(px(size)).flex_none().rounded_full().flex().items_center().justify_center().tip(label)
        };
        let play = round("artist-play", 56., if pause { "Pausar" } else { "Tocar o artista" })
            .bg(rgba(theme::ink_1()))
            .hover(|s| s.bg(rgba(theme::wash(0xd9))))
            .on_click(cx.listener(move |t, _, _, cx| {
                if here {
                    t.send(Cmd::Toggle);
                } else if has_top {
                    t.play_from(&source, 0);
                } else {
                    t.play_context(&play_card);
                }
                cx.notify();
            }))
            .child(icon(if pause { "pause" } else { "play" }, 22., theme::on_ink()));
        let shuffle = round("artist-shuffle", 44., "Tocar o artista em ordem aleatória")
            .bg(rgba(theme::wash(0x26)))
            .hover(|s| s.bg(rgba(theme::wash(0x40))))
            .on_click(cx.listener(move |t, _, _, cx| {
                if has_top {
                    t.shuffle_from(&s2);
                } else {
                    t.shuffle = true;
                    t.send(Cmd::SetShuffle(true));
                    t.play_context(&shuffle_card);
                }
                cx.notify();
            }))
            .child(icon("shuffle", 18., theme::ink_1()));
        div()
            .relative()
            .w_full()
            .h(px(HERO_H))
            .overflow_hidden()
            .mb_6()
            .child(match photo {
                Some(p) => img(p).absolute().inset_0().size_full().object_fit(ObjectFit::Cover).rounded_t(px(PANEL_R)).into_any_element(),
                None => div().absolute().inset_0().bg(rgba(theme::wash(0x0a))).into_any_element(),
            })
            // Escurece em cima, para os botões do cabeçalho, e funde com o fundo embaixo, onde fica o nome.
            .child(div().absolute().inset_0().bg(linear_gradient(180., linear_color_stop(rgba((theme::surface() << 8) | 0x80), 0.), linear_color_stop(rgba((theme::surface() << 8) | 0x00), 0.3))))
            .child(div().absolute().inset_0().bg(linear_gradient(180., linear_color_stop(rgba((theme::surface() << 8) | 0x00), 0.35), linear_color_stop(rgba((theme::surface() << 8) | 0xff), 1.))))
            .child(
                div().absolute().left(px(PAD)).right(px(PAD)).bottom_0().flex().flex_col().gap_4()
                    .child(div().text_size(px(46.)).line_height(px(52.)).font_weight(FontWeight::BOLD).line_clamp(2).text_ellipsis().child(c.title.clone()))
                    .when(!page.meta.is_empty(), |d| d.child(div().text_sm().text_color(rgba(theme::ink_2())).child(page.meta.clone())))
                    .child(div().flex().items_center().gap_3().child(play).child(shuffle)),
            )
            .into_any_element()
    }

    /// Lançamento mais recente à esquerda e as mais tocadas à direita.
    fn artist_highlights(&mut self, id: &str, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = self.pages.get(id) else { return div().into_any_element() };
        let latest = page.artist.as_ref().and_then(|a| a.latest.clone());
        let top = page.tracks.clone();
        if latest.is_none() && top.is_empty() {
            return div().into_any_element();
        }
        let heading = |text: &'static str| div().text_lg().font_weight(FontWeight::BOLD).child(text);
        let latest_el = latest.map(|(card, date)| {
            let thumb = card.image.as_ref().and_then(|u| self.thumbs.get(u)).cloned();
            let (title, subtitle) = (card.title.clone(), card.subtitle.clone());
            div().w(px(300.)).flex_none().flex().flex_col().gap_3()
                .child(heading("Lançamento mais recente"))
                .child(
                    Self::focusable("artist-latest")
                        .flex()
                        .gap_4()
                        .rounded(px(12.))
                        .hover(|s| s.opacity(0.85))
                        .aria_label(SharedString::from(format!("Abrir {title}")))
                        .on_click(cx.listener(move |t, _, _, cx| { t.go(Screen::Page(card.clone())); cx.notify(); }))
                        .child(match thumb {
                            Some(p) => img(p).size(px(120.)).flex_none().rounded(px(10.)).object_fit(ObjectFit::Cover).into_any_element(),
                            None => div().size(px(120.)).flex_none().rounded(px(10.)).bg(rgba(theme::wash(0x0f))).into_any_element(),
                        })
                        .child(
                            div().flex_1().min_w_0().flex().flex_col().gap_1().pt_1()
                                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child(date))
                                .child(div().text_base().font_weight(FontWeight::SEMIBOLD).line_clamp(2).text_ellipsis().child(title))
                                .child(div().text_sm().text_color(rgba(theme::ink_2())).child(subtitle)),
                        ),
                )
        });
        // Duas colunas de até cinco quando cabe; numa tela estreita, uma.
        let top_cols = if width - 340. >= 560. { 2 } else { 1 };
        let per_col = top.len().div_ceil(top_cols).max(1);
        let source = TrackSource::Page(id.to_string());
        let mut columns: Vec<AnyElement> = Vec::new();
        for (col, chunk) in top.chunks(per_col).enumerate() {
            let rows = chunk.iter().enumerate().map(|(j, t)| self.song_row(&source, col * per_col + j, t, t.album.clone(), cx)).collect::<Vec<_>>();
            columns.push(div().flex_1().min_w_0().flex().flex_col().children(rows).into_any_element());
        }
        div()
            .w_full()
            .px(px(PAD))
            .pb_8()
            .flex()
            .gap_8()
            .children(latest_el)
            .when(!top.is_empty(), |d| d.child(
                div().flex_1().min_w_0().flex().flex_col().gap_3()
                    .child(heading("Mais tocadas"))
                    .child(div().w_full().flex().gap_4().children(columns)),
            ))
            .into_any_element()
    }

    /// Faixa compacta com capa (mais tocadas do artista, músicas da busca); `detail` é a linha de baixo.
    pub(super) fn song_row(&mut self, source: &TrackSource, i: usize, t: &Track, detail: String, cx: &mut Context<Self>) -> AnyElement {
        let accent = self.accent_ink();
        let now = self.playing_uri() == Some(t.uri.as_str());
        let thumb = t.cover_small.as_ref().and_then(|u| self.thumbs.get(u)).cloned();
        let source = source.clone();
        let menu_track = t.clone();
        Self::focusable(("top-song", i))
            .group("top-song")
            .on_mouse_down(MouseButton::Right, cx.listener(move |t, e: &MouseDownEvent, _, cx| { t.open_menu(MenuTarget::Track(menu_track.clone()), e.position); cx.notify(); }))
            .h(px(52.))
            .px_2()
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap_3()
            .when(now, |d| d.bg(rgba(theme::wash(0x1f))))
            .hover(|s| s.bg(rgba(theme::wash(0x14))))
            .aria_label(SharedString::from(format!("Tocar {}", t.name)))
            .on_click(cx.listener(move |this, _, _, cx| { if now { this.toggle_play() } else { this.play_from(&source, i) } cx.notify(); }))
            .child(
                // Com o mouse em cima, a capa mostra tocar (ou pausar, se é a que está tocando).
                div().relative().size(px(40.)).flex_none()
                    .child(match thumb {
                        Some(p) => img(p).size(px(40.)).rounded(px(6.)).object_fit(ObjectFit::Cover).into_any_element(),
                        None => div().size(px(40.)).rounded(px(6.)).bg(rgba(theme::wash(0x0f))).into_any_element(),
                    })
                    .child(div().absolute().inset_0().rounded(px(6.)).bg(rgba(theme::shade(0x8c))).flex().items_center().justify_center()
                        .opacity(0.).group_hover("top-song", |s| s.opacity(1.))
                        .child(icon(if now && self.playing { "pause" } else { "play" }, 16., theme::ink_1()))),
            )
            .child(
                div().flex_1().min_w_0().flex().flex_col()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().when(now, |d| d.text_color(accent)).child(t.name.clone()))
                    .child(div().text_xs().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(detail)),
            )
            .when(now, |d| d.child(now_icon(accent)))
            .child(self.heart_button(("top-heart", i), "top-song", t, cx))
            .child(div().text_xs().text_color(rgba(theme::ink_3())).child(fmt(t.duration_ms)))
            .child(Self::more_button(("top-more", i), "top-song", 26., MenuTarget::Track(t.clone()), cx))
            .into_any_element()
    }

    /// "Discografia" e os filtros (só os que têm algo).
    fn discography_header(&mut self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = self.pages.get(id) else { return div().into_any_element() };
        let total = page.albums.len();
        let albums = page.artist.as_ref().map_or(total, |a| a.album_count.min(total));
        let filters = [(Releases::All, "Tudo", total), (Releases::Albums, "Álbuns", albums), (Releases::Singles, "Singles e EPs", total - albums)];
        let chips: Vec<AnyElement> = filters.into_iter().enumerate().filter(|(_, (_, _, n))| *n > 0).map(|(i, (filter, label, _))| {
            let on = self.release_filter == filter;
            Self::focusable(("release-filter", i))
                .h(px(30.))
                .px_4()
                .rounded_full()
                .flex()
                .items_center()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .aria_selected(on)
                .when(on, |d| d.bg(rgba(theme::ink_1())).text_color(rgba(theme::on_ink())))
                .when(!on, |d| d.bg(rgba(theme::wash(0x14))).hover(|s| s.bg(rgba(theme::wash(0x26)))))
                .on_click(cx.listener(move |t, _, _, cx| { t.release_filter = filter; cx.notify(); }))
                .child(label)
                .into_any_element()
        }).collect();
        div()
            .w_full()
            .px(px(PAD))
            .pb_4()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().text_lg().font_weight(FontWeight::BOLD).child("Discografia"))
            // Com um tipo só, os filtros não ajudam.
            .when(albums > 0 && albums < total, |d| d.children(chips))
            .into_any_element()
    }

    /// Álbum ou playlist, como no Cider: capa grande, título, artista, chips e botões em cima, e a lista
    /// rola junto com o cabeçalho. Virtualizada: só as linhas visíveis existem (playlists vão até 2000 faixas).
    fn media_page(&mut self, page: &Page, cx: &mut Context<Self>) -> AnyElement {
        let id = page.card.id.clone();
        // Cabeçalho, as faixas (ou uma linha de aviso), "carregando mais" e o respiro embaixo do player.
        let more = page.more_from.is_some() && !page.tracks.is_empty();
        let loaded = page.tracks.len();
        let count = loaded.max(1) + 2 + more as usize;
        let state = self.page_lists.entry(id.clone()).or_insert_with(|| ListState::new(count, ListAlignment::Top, px(600.))).clone();
        let old = state.item_count();
        if old != count {
            state.splice(1..old - 1, count - 2);
        }
        list(state, cx.processor(move |this, i: usize, _window, cx| {
            if i == 0 {
                this.media_header(&id, cx)
            } else if i == count - 1 {
                div().h(px(LIST_BOTTOM)).into_any_element()
            } else {
                // Perto do fim do que já veio, pede o próximo pedaço.
                if more && i + 40 >= loaded && !this.loading_more.contains(&id) {
                    if let Some(page) = this.pages.get(&id) {
                        if let Some(from) = page.more_from {
                            this.loading_more.insert(id.clone());
                            this.send(Cmd::LoadMore { card: page.card.clone(), from });
                        }
                    }
                }
                this.media_row(&id, i - 1, cx)
            }
        }))
        .flex_1()
        .min_h_0()
        // O fundo da linha sai da coluna; o texto fica alinhado com o resto da tela.
        .mx(px(-12.))
        .into_any_element()
    }

    fn media_header(&mut self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = self.pages.get(id) else { return div().into_any_element() };
        let c = page.card.clone();
        let n = page.total.max(page.tracks.len());
        // Com a playlist pela metade, a duração seria só do que já veio.
        let total_min = if page.more_from.is_some() { 0 } else { page.tracks.iter().map(|t| t.duration_ms as u64).sum::<u64>() / 60_000 };
        // A meta do álbum começa pelo ano ("2020 · 14 faixas").
        let year = page.meta.split(" · ").next().filter(|y| c.kind == Kind::Album && y.len() == 4 && y.chars().all(|ch| ch.is_ascii_digit())).map(String::from);
        let artist = page.tracks.first().and_then(|t| t.artists.first().cloned());
        // Playlist de outra pessoa sem lista: toca pela URI.
        let by_uri = n == 0 && page.note.is_some();
        let cover = 240.;
        let art = match c.image.as_ref().and_then(|u| self.thumbs.get(u)).cloned() {
            Some(p) => img(p).size(px(cover)).rounded(px(14.)).object_fit(ObjectFit::Cover).shadow_2xl().into_any_element(),
            None => div().size(px(cover)).rounded(px(14.)).bg(rgba(theme::wash(0x0f))).flex().items_center().justify_center()
                .child(icon(if c.kind == Kind::Album { "disc-3" } else { "list-music" }, 48., theme::ink_3())).into_any_element(),
        };
        let owner_photo = page.owner_image.as_ref().and_then(|u| self.thumbs_small.get(u)).cloned();
        let avatar = move || match owner_photo.clone() {
            Some(p) => img(p).size(px(26.)).flex_none().rounded_full().object_fit(ObjectFit::Cover).into_any_element(),
            None => div().size(px(26.)).flex_none().rounded_full().bg(rgba(theme::wash(0x1f))).flex().items_center().justify_center().child(icon("user-round", 14., theme::ink_2())).into_any_element(),
        };
        let by_line = match (c.kind, artist) {
            (Kind::Album, Some((artist_id, name))) => {
                let card = Card { kind: Kind::Artist, uri: format!("spotify:artist:{artist_id}"), id: artist_id, title: name.clone(), subtitle: "Artista".into(), image: None };
                Self::focusable("page-artist")
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(6.))
                    .hover(|s| s.underline())
                    .aria_label(SharedString::from(format!("Abrir {name}")))
                    .on_click(cx.listener(move |t, _, _, cx| { t.go(Screen::Page(card.clone())); cx.notify(); }))
                    .child(avatar())
                    .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(c.subtitle.clone()))
                    .into_any_element()
            }
            _ if c.subtitle.is_empty() => div().into_any_element(),
            _ => div().flex().items_center().gap_2().child(avatar())
                .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(c.subtitle.clone())).into_any_element(),
        };
        let chip = |icon_name: &'static str, text: String| {
            div().flex().items_center().gap_1p5().px_2p5().py_1().rounded_full().border_1().border_color(rgba(theme::wash(0x26)))
                .text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2()))
                .child(icon(icon_name, 12., theme::ink_2())).child(text)
        };
        let chips = div().flex().flex_wrap().gap_2()
            .children(year.map(|y| chip("calendar", y)))
            .when(n > 0, |d| d.child(chip("list-music", match (c.kind == Kind::Show, n == 1) {
                (true, true) => "1 episódio".into(),
                (true, false) => format!("{n} episódios"),
                (false, true) => "1 faixa".into(),
                (false, false) => format!("{n} faixas"),
            })))
            .when(total_min > 0, |d| d.child(chip("clock", minutes_label(total_min))));
        let pill = |pill_id: &'static str, label: &'static str, icon_name: &'static str| {
            Self::focusable(pill_id)
                .h(px(36.))
                .px_5()
                .rounded_full()
                .flex()
                .items_center()
                .gap_2()
                .bg(rgba(theme::ink_1()))
                .text_color(rgba(theme::on_ink()))
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .hover(|s| s.bg(rgba(theme::wash(0xd9))))
                .child(icon(icon_name, 15., theme::on_ink()))
                .child(label)
        };
        let source = TrackSource::Page(c.id.clone());
        let (play_card, shuffle_card, menu_card) = (c.clone(), c.clone(), c.clone());
        let s2 = source.clone();
        let here = self.is_current(Some(&c.uri));
        let pause = here && self.playing;
        let play = pill("page-play", if pause { "Pausar" } else { "Tocar" }, if pause { "pause" } else { "play" }).on_click(cx.listener(move |t, _, _, cx| {
            if here {
                t.send(Cmd::Toggle);
            } else if by_uri {
                t.play_context(&play_card);
            } else {
                t.play_from(&source, 0);
            }
            cx.notify();
        }));
        let shuffle = pill("page-shuffle", "Aleatório", "shuffle").on_click(cx.listener(move |t, _, _, cx| {
            if by_uri {
                t.shuffle = true;
                t.send(Cmd::SetShuffle(true));
                t.play_context(&shuffle_card);
            } else {
                t.shuffle_from(&s2);
            }
            cx.notify();
        }));
        // Podcast: seguir pela página, como no Spotify.
        let following = self.library.as_ref().is_some_and(|l| l.shows.iter().any(|s| s.id == c.id));
        let follow_card = c.clone();
        let follow = (c.kind == Kind::Show).then(|| {
            Self::focusable("page-follow")
                .h(px(36.))
                .px_5()
                .rounded_full()
                .flex()
                .items_center()
                .gap_2()
                .border_1()
                .border_color(rgba(if following { theme::wash(0x26) } else { theme::wash(0x80) }))
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .hover(|s| s.bg(rgba(theme::wash(0x14))))
                .aria_label(if following { "Deixar de seguir o podcast" } else { "Seguir o podcast" })
                .on_click(cx.listener(move |t, _, _, cx| { t.save_card(follow_card.clone(), !following); cx.notify(); }))
                .when(following, |d| d.child(icon("check", 15., theme::ink_1())))
                .child(if following { "Seguindo" } else { "Seguir" })
        });
        let more = Some(()).map(|_| {
            Self::focusable("page-more")
                .size(px(36.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(theme::ink_1()))
                .hover(|s| s.bg(rgba(theme::wash(0xd9))))
                .tip(if c.kind == Kind::Playlist { "Mais opções da playlist" } else { "Mais opções do álbum" })
                .on_click(cx.listener(move |t, e: &ClickEvent, _, cx| { t.open_menu(MenuTarget::Card(menu_card.clone()), e.position()); cx.notify(); }))
                .child(icon("ellipsis", 18., theme::on_ink()))
        });
        let hero = div()
            .w_full()
            .px(px(12.))
            .pt_2()
            .pb_8()
            .flex()
            .items_center()
            .gap_8()
            .child(art)
            .child(
                div().flex_1().min_w_0().flex().flex_col().gap_2()
                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child(kind_label(c.kind).to_uppercase()))
                    .child(div().w_full().text_size(px(32.)).line_height(px(38.)).font_weight(FontWeight::BOLD).line_clamp(2).text_ellipsis().child(c.title.clone()))
                    .child(by_line)
                    .child(chips)
                    .child(div().mt_3().flex().items_center().gap_3().child(play).child(shuffle).children(follow).child(div().flex_1()).children(more)),
            );
        // Playlists têm a coluna do álbum; podcasts, a da data do episódio.
        let middle: &[&'static str] = match c.kind {
            Kind::Album => &[],
            Kind::Show => &["Data"],
            _ => &["Álbum"],
        };
        div().w_full().flex().flex_col().child(hero).child(column_header(middle, c.kind != Kind::Album)).into_any_element()
    }

    /// Linha de faixa da página; zebrada como no Cider. Na playlist, com capa, artista e álbum.
    fn media_row(&mut self, id: &str, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let accent = self.accent_ink();
        let playing_uri = self.playing_uri().map(String::from);
        let Some(page) = self.pages.get(id) else { return div().into_any_element() };
        let Some(t) = page.tracks.get(i).cloned() else {
            if page.more_from.is_some() && !page.tracks.is_empty() {
                return div().px(px(12.)).py_3().text_sm().text_color(rgba(theme::ink_3())).child("Carregando mais faixas…").into_any_element();
            }
            let text = page.note.clone().unwrap_or_else(|| "Nenhuma faixa.".into());
            return div().px(px(12.)).text_color(rgba(theme::ink_2())).child(text).into_any_element();
        };
        // Playlists e podcasts mostram capa e a coluna do álbum (a data, no episódio).
        let playlist = page.card.kind != Kind::Album;
        // No álbum, o artista só aparece na faixa quando é outro (participações); no podcast é sempre o próprio.
        let show_artist = page.card.kind == Kind::Playlist || (page.card.kind == Kind::Album && t.artist != page.card.subtitle);
        let now = playing_uri.as_deref() == Some(t.uri.as_str());
        let thumb = t.cover_small.as_ref().and_then(|u| self.thumbs.get(u)).cloned();
        let source = TrackSource::Page(id.to_string());
        let menu_track = t.clone();
        let playing = self.playing;
        // Nas suas playlists, arrastar a linha muda a ordem no Spotify.
        let reorder = self.owns(&page.card).then(|| page.card.clone());
        // Episódio começado: quanto falta, como no Spotify.
        let album_col = match self.episode_positions.get(&t.uri) {
            Some(&at) => format!("{} · faltam {}", t.album, minutes_label((t.duration_ms.saturating_sub(at) as u64).div_ceil(60_000))),
            None => t.album.clone(),
        };
        let hidden = self.hidden.contains(&super::hidden_key(&page.card.uri, &t.uri));
        Self::focusable(("track", i))
            .when_some(reorder, |d, card| d
                .on_drag(PlaylistDrag { from: i, title: t.name.clone() }, |q, _, _, cx| cx.new(|_| DragGhost(q.title.clone().into())))
                .drag_over::<PlaylistDrag>(|s, _, _, _| s.bg(rgba(theme::wash(0x26))))
                .on_drop(cx.listener(move |this, q: &PlaylistDrag, _, cx| { this.move_in_playlist(&card, q.from, i); cx.notify(); })))
            .group("track-row")
            .w_full()
            .h(px(ROW_H))
            .px_3()
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap_3()
            .when(i % 2 == 0 && !now, |d| d.bg(rgba(theme::wash(0x0a))))
            .when(now, |d| d.bg(rgba(theme::wash(0x1f))))
            .when(hidden, |d| d.opacity(0.4))
            .hover(|s| s.bg(rgba(theme::wash(0x17))))
            .aria_label(SharedString::from(format!("Tocar {} – {}", t.name, t.artist)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |t, e: &MouseDownEvent, _, cx| { t.open_menu(MenuTarget::Track(menu_track.clone()), e.position); cx.notify(); }))
            .on_click(cx.listener(move |this, _, _, cx| { if now { this.toggle_play() } else { this.play_from(&source, i) } cx.notify(); }))
            .child(index_cell(i, now, playing, "track-row", accent))
            .when(playlist, |d| d.child(match thumb {
                Some(p) => img(p).size(px(36.)).rounded(px(6.)).object_fit(ObjectFit::Cover).into_any_element(),
                None => div().size(px(36.)).rounded(px(6.)).bg(rgba(theme::wash(0x0f))).into_any_element(),
            }))
            .child(
                div().flex_1().min_w_0().flex().flex_col()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis()
                        .when(now, |d| d.text_color(accent)).child(t.name.clone()))
                    .when(show_artist, |d| d.child(div().text_xs().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(t.artist.clone()))),
            )
            .when(playlist, |d| d.child(div().flex_1().min_w_0().text_sm().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(album_col)))
            .child(self.heart_button(("track-heart", i), "track-row", &t, cx))
            .child(div().w(px(44.)).flex().justify_end().text_xs().text_color(rgba(theme::ink_3())).child(fmt(t.duration_ms)))
            .child(Self::more_button(("track-more", i), "track-row", 28., MenuTarget::Track(t.clone()), cx))
            .into_any_element()
    }

    /// Toca uma playlist pela URI, sem a lista de faixas (o Spotify resolve).
    pub(super) fn play_context(&mut self, card: &Card) {
        self.queue_label = card.title.clone();
        self.queue_context = Some(card.uri.clone());
        self.send(Cmd::Play { tracks: Vec::new(), index: 0, context: Some(card.uri.clone()), label: card.title.clone() });
    }

    /// Fundo do painel: cor fixa; nas páginas de álbum e playlist, a capa borrada, com troca suave.
    fn backdrop_layers(&mut self) -> Vec<AnyElement> {
        let want = match &self.screen {
            Screen::Page(c) if c.kind != Kind::Artist && self.settings.cover_colors => c.image.as_ref().and_then(|u| self.backdrops.get(u)).cloned(),
            _ => None,
        };
        if want != self.bg_to {
            self.bg_from = std::mem::replace(&mut self.bg_to, want);
            self.bg_gen += 1;
        }
        // Escurece mais embaixo, onde fica a lista, para o texto branco seguir legível em capas claras.
        let layer = |p: PathBuf| {
            div()
                .absolute()
                .inset_0()
                .child(img(p).absolute().inset_0().size_full().object_fit(ObjectFit::Cover))
                .child(div().absolute().inset_0().bg(linear_gradient(
                    180.,
                    linear_color_stop(rgba((theme::surface() << 8) | 0xb8), 0.),
                    linear_color_stop(rgba((theme::surface() << 8) | 0xeb), 0.7),
                )))
        };
        let fade = || Animation::new(Duration::from_millis(600)).with_easing(ease_out_quint());
        let id = ("page-bg", self.bg_gen);
        match (self.bg_from.clone(), self.bg_to.clone()) {
            (Some(from), Some(to)) => vec![
                layer(from).into_any_element(),
                layer(to).with_animation(id, fade(), |d, t| d.opacity(t)).into_any_element(),
            ],
            (None, Some(to)) => vec![layer(to).with_animation(id, fade(), |d, t| d.opacity(t)).into_any_element()],
            (Some(from), None) => vec![layer(from).with_animation(id, fade(), |d, t| d.opacity(1. - t)).into_any_element()],
            (None, None) => Vec::new(),
        }
    }

    /// Player flutuante dentro do painel, como no Cider: progresso em cima; controles, faixa e volume embaixo.
    /// A lista rola por baixo dele e aparece borrada através do vidro.
    fn dock(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = theme::glass(div())
            // A lista rola por baixo do player; sem isto, um clique no player também clicava no cartão de trás.
            .block_mouse_except_scroll()
            .absolute()
            .left(px(16.))
            .right(px(16.))
            .bottom(px(16.))
            .h(px(DOCK_H))
            .px_5()
            .rounded(px(18.))
            .flex()
            .flex_col()
            .justify_center()
            .shadow_2xl();
        let Some(track) = self.tracks.get(self.current).cloned() else {
            return bar.child(div().text_sm().text_color(rgba(theme::ink_3())).child("Escolha uma música para tocar."));
        };
        let duration = track.duration_ms;
        let pos = self.seek_drag.map_or(self.position(), |f| (f * duration as f32) as u32);
        let frac = if duration == 0 { 0. } else { (pos as f32 / duration as f32).clamp(0., 1.) };
        let right_time = if self.show_total { fmt(duration) } else { format!("-{}", fmt(duration.saturating_sub(pos))) };
        let cover = match (&self.shown_art, track.cover_small.as_ref().and_then(|u| self.thumbs.get(u))) {
            // A de 64 px basta para 40 px; a de 640 só no imersivo.
            (_, Some(p)) => img(p.clone()).size(px(40.)).rounded(px(7.)).into_any_element(),
            (Some(art), None) => img(art.cover.clone()).size(px(40.)).rounded(px(7.)).into_any_element(),
            (None, None) => div().size(px(40.)).rounded(px(7.)).bg(rgba(theme::wash(0x0f))).into_any_element(),
        };
        let progress = self.progress(frac, self.accent_ink(), cx);
        bar.child(
            div().flex().items_center().gap_3().text_size(px(11.)).text_color(rgba(theme::ink_3()))
                .child(div().w(px(40.)).flex().justify_end().child(fmt(pos)))
                .child(div().flex_1().child(progress))
                .child(
                    Self::focusable("dock-time")
                        .w(px(48.))
                        .rounded(px(6.))
                        .hover(|s| s.text_color(rgba(theme::ink_1())))
                        .aria_label(if self.show_total { "Mostrar tempo restante" } else { "Mostrar duração total" })
                        .on_click(cx.listener(|t, _, _, cx| { t.show_total = !t.show_total; cx.notify(); }))
                        .child(right_time),
                ),
        )
        .child(
            div()
                .h(px(50.))
                .flex()
                .items_center()
                .gap_4()
                // Os lados não encolhem além dos controles; quem cede espaço é o cartão da faixa.
                .child(div().flex_1().min_w(px(200.)).flex().items_center().gap_0p5().children(self.transport(cx)))
                .child(self.dock_track(&track, cover, cx))
                .child(
                    div().flex_1().min_w(px(200.)).flex().items_center().justify_end().gap_1()
                        .child(icon(self.volume_icon(), 17., theme::ink_2()))
                        .child(self.volume_slider(true, cx))
                        .child(
                            Self::bar_button("dock-immersive", "Modo imersivo".into(), 36., false)
                                .ml_2()
                                .on_click(cx.listener(|t, _, _, cx| { t.set_immersive(true); cx.notify(); }))
                                .child(icon("maximize-2", 16., theme::ink_1())),
                        ),
                ),
        )
    }

    /// Cartão da faixa no centro do player: a capa abre o imersivo, o nome abre o álbum (ou single)
    /// e cada artista abre a página dele.
    fn dock_track(&mut self, track: &Track, cover: AnyElement, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, artists) = self.track_links(track, "dock", cx);
        div()
            .w(px(480.))
            .min_w(px(180.))
            .p_1()
            .pr_3()
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap_3()
            .bg(rgba(theme::wash(0x0a)))
            .border_1()
            .border_color(rgba(theme::wash(0x1f)))
            .child(
                Self::focusable("dock-cover")
                    .rounded(px(8.))
                    .hover(|s| s.opacity(0.8))
                    .tip("Abrir o modo imersivo")
                    .on_click(cx.listener(|t, _, _, cx| { t.set_immersive(true); cx.notify(); }))
                    .child(cover),
            )
            .child(
                div().flex_1().min_w_0().flex().flex_col()
                    .child(div().flex().min_w_0().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().flex().min_w_0().gap(px(3.)).overflow_hidden().text_xs().text_color(rgba(theme::ink_2())).children(artists)),
            )
            // Curtir a que está tocando; episódios de podcast não entram nas Músicas curtidas.
            .when(track.uri.starts_with("spotify:track:"), |d| d.child(self.heart_button("dock-heart", "", track, cx)))
    }

    /// Faixa verde acima do player quando outro aparelho está tocando, como no app do Spotify.
    /// Clicar abre a lista de aparelhos (para trazer a música de volta para cá).
    /// Faixa fixa enquanto o Spotify bloqueia os pedidos: os avisos de erro somem em segundos e não explicam por que
    /// buscar, abrir páginas novas ou editar playlists não funciona. Fica acima da faixa de outro aparelho.
    fn limit_banner(&self) -> Option<AnyElement> {
        let left = self.limited_until?.checked_duration_since(std::time::Instant::now()).filter(|d| !d.is_zero())?;
        let until = finish_clock(left.as_secs()).map_or_else(|| format!("daqui a {} min", left.as_secs().div_ceil(60)), |c| format!("até {c}"));
        let bottom = 16. + DOCK_H + 8. + if self.remote.is_some() { 38. } else { 0. };
        Some(
            div().absolute().left_0().right_0().bottom(px(bottom)).flex().justify_center().child(
                div()
                    .block_mouse_except_scroll()
                    .h(px(30.))
                    .px_4()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgba(0xf5a524ff))
                    .text_color(rgba(0x000000ff))
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .shadow_2xl()
                    .child(icon("info", 15., 0x000000ff))
                    .child(format!("O Spotify limitou os pedidos {until}: busca, páginas novas e edições voltam depois")),
            )
            .into_any_element(),
        )
    }

    /// Ouvir junto, no fim da lista de aparelhos: convidar, entrar com código ou parar.
    fn listen_controls(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use cyder_core::engine::Listen;
        let row = |id: &'static str, label: String, icon_name: &'static str| {
            Self::focusable(id).h(px(38.)).px_3().rounded(px(8.)).flex().items_center().gap_3().text_sm()
                .hover(|s| s.bg(rgba(theme::wash(0x14))))
                .child(icon(icon_name, 16., theme::ink_2()))
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
        };
        let heading = div().px_3().pt_1().pb_1().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_3())).child("Ouvir junto");
        let body = match self.together.clone() {
            Listen::Off => div().flex().flex_col()
                .child(row("listen-host", "Convidar alguém".into(), "users").on_click(cx.listener(|t, _, _, cx| {
                    t.devices_open = false;
                    t.send(Cmd::ListenHost(true));
                    cx.notify();
                })))
                .child(row("listen-join", "Entrar com um código".into(), "log-in").on_click(cx.listener(|t, _, _, cx| {
                    t.devices_open = false;
                    t.ask(super::Ask::ListenCode, String::new());
                    cx.notify();
                }))),
            Listen::Hosting(code) => {
                let pretty = pretty_code(&code);
                div().flex().flex_col()
                    .child(div().px_3().pb_1().text_sm().child(format!("Código: {pretty}")))
                    .child(row("listen-copy", "Copiar o código".into(), "copy").on_click(cx.listener(move |t, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(pretty.clone()));
                        t.notify("Código copiado.");
                        cx.notify();
                    })))
                    .child(row("listen-stop", "Parar de convidar".into(), "x").on_click(cx.listener(|t, _, _, cx| { t.send(Cmd::ListenHost(false)); cx.notify(); })))
            }
            Listen::Joined { host, .. } => div().flex().flex_col()
                .child(row("listen-leave", if host.is_empty() { "Sair (esperando a outra pessoa tocar)".into() } else { format!("Sair de ouvir com {host}") }, "log-out")
                    .on_click(cx.listener(|t, _, _, cx| { t.send(Cmd::ListenJoin(None)); cx.notify(); }))),
        };
        div().flex().flex_col().child(heading).child(body).into_any_element()
    }

    /// Pílula em cima do player enquanto ouve junto; clicar abre os controles.
    fn listen_banner(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use cyder_core::engine::Listen;
        if self.remote.is_some() {
            return None;
        }
        let text = match &self.together {
            Listen::Off => return None,
            Listen::Hosting(code) => format!("Ouvindo junto · código {}", pretty_code(code)),
            Listen::Joined { host, .. } if host.is_empty() => "Ouvir junto: esperando a outra pessoa tocar".into(),
            Listen::Joined { host, .. } => format!("Ouvindo junto com {host}"),
        };
        Some(
            div().absolute().left_0().right_0().bottom(px(16. + DOCK_H + 8.)).flex().justify_center().child(
                Self::focusable("listen-banner")
                    .block_mouse_except_scroll()
                    .h(px(30.))
                    .px_4()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgba((LIKED_COLOR << 8) | 0xff))
                    .text_color(rgba(0xffffffff))
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .shadow_2xl()
                    .hover(|s| s.opacity(0.9))
                    .aria_label(SharedString::from(format!("{text}. Abrir os controles")))
                    .on_click(cx.listener(|t, _, _, cx| {
                        t.devices_open = true;
                        t.send(Cmd::Devices);
                        cx.notify();
                    }))
                    .child(icon("users", 15., 0xffffffff))
                    .child(text),
            )
            .into_any_element(),
        )
    }

    fn remote_banner(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let device = self.remote.clone()?;
        Some(
            div().absolute().left_0().right_0().bottom(px(16. + DOCK_H + 8.)).flex().justify_center().child(
                Self::focusable("remote-banner")
                    .block_mouse_except_scroll()
                    .h(px(30.))
                    .px_4()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgba(LIKED_GREEN))
                    .text_color(rgba(0x000000ff))
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .shadow_2xl()
                    .hover(|s| s.opacity(0.9))
                    .aria_label(SharedString::from(format!("Tocando em {device}. Escolher outro aparelho")))
                    .on_click(cx.listener(|t, _, _, cx| {
                        t.devices_open = true;
                        t.send(Cmd::Devices);
                        cx.notify();
                    }))
                    .child(icon("speaker", 15., 0x000000ff))
                    .child(format!("Tocando em {device}")),
            )
            .into_any_element(),
        )
    }

    /// Botão redondo do cabeçalho; `on` = painel aberto.
    fn chip(id: &'static str, label: &'static str, icon_name: &'static str, on: bool, enabled: bool) -> gpui::Stateful<gpui::Div> {
        Self::focusable(id)
            .size(px(36.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            // Vidro escuro: aparece tanto no fundo cinza quanto sobre a foto do artista.
            .border_1()
            .border_color(rgba(theme::wash(0x1a)))
            .bg(rgba(if on { theme::wash(0x38) } else { theme::shade(0x4d) }))
            .backdrop_blur(px(20.))
            .when(enabled, |d| d.hover(|s| s.bg(rgba(theme::wash(0x26)))))
            .when(!enabled, |d| d.opacity(0.4).cursor_default())
            .tip(label)
            .child(icon(icon_name, 16., theme::ink_1()))
    }

    /// Cabeçalho do painel: navegação e sidebar à esquerda; letra, fila e aparelhos à direita.
    fn toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (can_back, can_forward) = (!self.history.is_empty(), !self.forward.is_empty());
        let (lyrics_on, queue_on) = (self.side == Some(Side::Lyrics), self.side == Some(Side::Queue));
        // Sem a fila aberta, os botões da janela ficam por cima do canto do painel.
        let right_pad = if self.side.is_some() { 16. } else { CONTROLS_W + 12. - GAP };
        // O GPUI recorta em retângulo: cada metade da pílula arredonda o próprio lado, senão o hover sai quadrado.
        let nav_button = |id: &'static str, label: &'static str, icon_name: &'static str, enabled: bool, left_side: bool| {
            Self::focusable(id)
                .when(left_side, |d| d.rounded_l_full())
                .when(!left_side, |d| d.rounded_r_full())
                .w(px(34.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .tip(label)
                .when(enabled, |d| d.hover(|s| s.bg(rgba(theme::wash(0x14)))))
                .when(!enabled, |d| d.opacity(0.35).cursor_default())
                .child(icon(icon_name, 16., theme::ink_1()))
        };
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TOOLBAR_H))
            .pl_4()
            .pr(px(right_pad))
            .flex()
            .items_center()
            .gap_2()
            .child(
                // Voltar e avançar num pílula só, como no Cider.
                div().h(px(36.)).flex().rounded_full().border_1().border_color(rgba(theme::wash(0x1a))).bg(rgba(theme::shade(0x4d))).backdrop_blur(px(20.))
                    .child(nav_button("nav-back", "Voltar", "chevron-left", can_back, true)
                        .on_click(cx.listener(|t, _, _, cx| { t.nav_back(); cx.notify(); })))
                    .child(div().w(px(1.)).my_2().bg(rgba(theme::wash(0x1f))))
                    .child(nav_button("nav-forward", "Avançar", "chevron-right", can_forward, false)
                        .on_click(cx.listener(|t, _, _, cx| { t.nav_forward(); cx.notify(); }))),
            )
            .child(
                Self::chip("toggle-sidebar", if self.sidebar_open { "Esconder a barra lateral" } else { "Mostrar a barra lateral" }, "panel-left", false, true)
                    .on_click(cx.listener(|t, _, _, cx| { t.sidebar_open = !t.sidebar_open; cx.notify(); })),
            )
            .child(
                div().flex_1().h_full().pl_2().flex().items_center().window_control_area(WindowControlArea::Drag)
                    .child(div().text_xs().text_color(rgba(theme::ink_3())).child("Conteúdo fornecido pelo Spotify")),
            )
            // Versão nova: à esquerda do mini player.
            .children(self.update_button(cx))
            .child(
                Self::chip("toolbar-mini", if self.mini.is_some() { "Fechar o mini player" } else { "Abrir o mini player" }, "picture-in-picture-2", self.mini.is_some(), true)
                    .on_click(cx.listener(|t, _, _, cx| { t.toggle_mini(cx); cx.notify(); })),
            )
            .child(
                Self::chip("toolbar-lyrics", if lyrics_on { "Fechar a letra" } else { "Mostrar a letra" }, "mic-vocal", lyrics_on, true)
                    .on_click(cx.listener(|t, _, _, cx| { t.toggle_side(Side::Lyrics); cx.notify(); })),
            )
            .child(
                Self::chip("toolbar-queue", if queue_on { "Fechar a fila" } else { "Abrir a fila" }, "list-end", queue_on, true)
                    .on_click(cx.listener(|t, _, _, cx| { t.toggle_side(Side::Queue); cx.notify(); })),
            )
            .child(
                // Verde enquanto outro aparelho toca, como no Spotify.
                if self.remote.is_some() {
                    Self::focusable("toolbar-devices").size(px(36.)).rounded_full().flex().items_center().justify_center()
                        .bg(rgba(LIKED_GREEN)).hover(|s| s.opacity(0.9)).tip("Aparelhos")
                        .child(icon("headphones", 16., 0x000000ff))
                } else {
                    Self::chip("toolbar-devices", "Aparelhos", "headphones", self.devices_open, true)
                }
                    .on_click(cx.listener(|t, _, _, cx| {
                        t.devices_open = !t.devices_open;
                        if t.devices_open {
                            t.send(Cmd::Devices);
                        }
                        cx.notify();
                    })),
            )
    }

    /// Minimizar, maximizar e fechar num bloco da cor da moldura, com o canto de baixo à esquerda arredondado:
    /// parece um recorte no canto do painel. Encostado no painel, a borda dele contorna o bloco em onda:
    /// curva para fora, para dentro e para fora de novo (a altura do recorte, 28, é 2 × PANEL_R).
    fn window_controls(&self) -> impl IntoElement {
        let r = PANEL_R;
        let edge = rgba(theme::wash(0x17));
        // Canto convexo do painel (centro embaixo à esquerda): moldura por fora do arco e a borda no arco.
        // O GPUI só recorta em retângulo, então a moldura é uma faixa grossa arredondada por dentro.
        let fillet = || {
            div()
                .absolute()
                .size(px(r))
                .overflow_hidden()
                .child(div().absolute().top(px(-r)).right(px(-r)).size(px(4. * r)).rounded(px(2. * r)).border(px(r)).border_color(rgba((theme::frame() << 8) | 0xff)))
                .child(div().absolute().top_0().right_0().size(px(2. * r)).rounded_tr(px(r)).border_t_1().border_r_1().border_color(edge))
        };
        let touches = self.side.is_none();
        div()
            .absolute()
            .top_0()
            .right_0()
            .w(px(CONTROLS_W + r))
            .h(px(theme::TITLEBAR_H + r))
            .when(touches, |d| d.child(fillet().left_0().top(px(V_GAP))).child(fillet().right(px(GAP)).top(px(theme::TITLEBAR_H))))
            .child(self.window_buttons())
            .when(touches, |d| {
                d
                    // Canto côncavo, embaixo à esquerda do bloco, e a linha de baixo até o canto da direita.
                    .child(
                        div().absolute().left(px(r)).top(px(theme::TITLEBAR_H - r)).size(px(r)).overflow_hidden()
                            .child(div().absolute().left_0().bottom_0().size(px(2. * r)).rounded_bl(px(r)).border_l_1().border_b_1().border_color(edge)),
                    )
                    .child(div().absolute().left(px(2. * r)).right(px(GAP + r)).top(px(theme::TITLEBAR_H - 1.)).h(px(1.)).bg(edge))
            })
    }

    fn window_buttons(&self) -> impl IntoElement {
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
            .right_0()
            .w(px(CONTROLS_W))
            .h(px(theme::TITLEBAR_H))
            .flex()
            .rounded_bl(px(PANEL_R))
            .overflow_hidden()
            .bg(rgba((theme::frame() << 8) | 0xff))
            .child(control("win-min", "Minimizar", "minus", WindowControlArea::Min, theme::hover()))
            .child(control("win-max", "Maximizar", "square", WindowControlArea::Max, theme::hover()))
            .child(control("win-close", "Fechar", "x", WindowControlArea::Close, (theme::DANGER << 8) | 0xff))
    }

    /// Fila (2ª imagem do Cider): tocando agora, a seguir e quanto tempo falta.
    /// Com aleatório ou outro aparelho tocando, a ordem que vale é a do Spotify.
    fn spotify_queue(&self) -> Option<&Vec<Track>> {
        // Vazia (antes do primeiro play o Spotify ainda não tem fila deste aparelho): vale a nossa lista.
        self.up_next.as_ref().filter(|l| !l.is_empty() && (self.shuffle || self.remote.is_some()))
    }

    fn queue_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let now = self.tracks.get(self.current).cloned();
        let from_spotify = self.spotify_queue().is_some();
        let upcoming = match self.spotify_queue() {
            Some(list) => list.len(),
            None => self.tracks.len().saturating_sub(self.current + 1),
        };
        let next_ms: u64 = match self.spotify_queue() {
            Some(list) => list.iter().map(|t| t.duration_ms as u64).sum(),
            None => self.tracks.iter().skip(self.current + 1).map(|t| t.duration_ms as u64).sum(),
        };
        let remaining_ms: u64 = now.as_ref().map_or(0, |t| t.duration_ms.saturating_sub(self.position()) as u64) + next_ms;
        let minutes = remaining_ms.div_ceil(60_000);
        // Pausado, não há hora de término.
        let ends = finish_clock(remaining_ms / 1000).filter(|_| self.playing).map_or(String::new(), |c| format!(" → {c}"));
        let items = if upcoming == 1 { "1 item".to_string() } else { format!("{upcoming} itens") };
        let mode_color = |on: bool| if on { theme::ink_1() } else { theme::ink_3() };
        let heading = |text: &'static str| div().text_sm().font_weight(FontWeight::BOLD).child(text);
        let list = uniform_list("queue-list", upcoming, cx.processor(|this, range: Range<usize>, _window, cx| this.queue_rows(range, cx)))
            .flex_1()
            .min_h_0();
        div()
            .absolute()
            .top(px(theme::TITLEBAR_H))
            .right_0()
            .bottom(px(V_GAP))
            .w(px(QUEUE_W))
            .px_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div().flex().items_center().gap_1()
                    .child(Self::bar_button("queue-shuffle", if self.shuffle { "Aleatório: ligado" } else { "Aleatório: desligado" }.into(), 32., self.shuffle)
                        .on_click(cx.listener(|t, _, _, cx| { t.shuffle = !t.shuffle; t.send(Cmd::SetShuffle(t.shuffle)); cx.notify(); }))
                        .child(icon("shuffle", 15., mode_color(self.shuffle))))
                    .child(Self::bar_button("queue-repeat", "Repetir".into(), 32., self.repeat != cyder_core::player::Repeat::Off)
                        .on_click(cx.listener(|t, _, _, cx| { t.cycle_repeat(); cx.notify(); }))
                        .child(icon(if self.repeat == cyder_core::player::Repeat::One { "repeat-1" } else { "repeat" }, 15., mode_color(self.repeat != cyder_core::player::Repeat::Off))))
                    .child(div().flex_1())
                    .child(div().px_3().py_1().rounded_full().bg(rgba(theme::wash(0x12))).text_xs().text_color(rgba(theme::ink_2())).child(items)),
            )
            .child(heading("Tocando agora"))
            .children(now.map(|t| self.queue_row(&t, None, true, cx)))
            .child(
                div().mt_2().flex().items_end()
                    .child(div().flex_1().flex().flex_col()
                        .child(heading("A seguir"))
                        .when(!self.queue_label.is_empty(), |d| d.child(div().text_xs().text_color(rgba(theme::ink_3())).child(format!("De {}", self.queue_label)))))
                    .when(!self.tracks.is_empty() && !from_spotify, |d| d.child(div().text_xs().text_color(rgba(theme::ink_3())).child(format!("{} de {}", self.current + 1, self.tracks.len())))),
            )
            .when(from_spotify, |d| d.child(div().text_xs().text_color(rgba(theme::ink_3()))
                .child(if self.shuffle { "Na ordem do Spotify (até 20 próximas). Desligue o aleatório para reorganizar." } else { "Na ordem do Spotify (até 20 próximas)." })))
            .when(!from_spotify && self.shuffle && upcoming > 0, |d| d.child(div().text_xs().text_color(rgba(theme::ink_3()))
                .child("Na ordem da lista; o aleatório sorteia ao tocar.")))
            .child(if upcoming == 0 { Self::quiet("Nada na fila depois desta.").into_any_element() } else { list.into_any_element() })
            .child(
                div().mb_1().flex().justify_center().child(
                    div().px_3().py_1().rounded_full().bg(rgba(theme::wash(0x12))).text_xs().text_color(rgba(theme::ink_2()))
                        .child(format!("{}{ends}", minutes_label(minutes))),
                ),
            )
    }

    fn queue_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if let Some(list) = self.spotify_queue().cloned() {
            return range
                .filter_map(|i| list.get(i).cloned())
                .map(|t| {
                    let index = self.tracks.iter().position(|x| x.uri == t.uri);
                    self.queue_row(&t, index, false, cx)
                })
                .collect();
        }
        range
            .filter_map(|i| {
                let index = self.current + 1 + i;
                self.tracks.get(index).cloned().map(|t| self.queue_row(&t, Some(index), false, cx))
            })
            .collect()
    }

    /// Linha da fila. `index`: posição na fila, para tocar a partir dela (None = a que está tocando).
    fn queue_row(&mut self, t: &Track, index: Option<usize>, current: bool, cx: &mut Context<Self>) -> AnyElement {
        let thumb = t.cover_small.as_ref().and_then(|u| self.thumbs.get(u)).cloned();
        let cover = match thumb {
            Some(p) => img(p).size(px(40.)).rounded(px(6.)).into_any_element(),
            None => div().size(px(40.)).rounded(px(6.)).bg(rgba(theme::wash(0x0f))).into_any_element(),
        };
        let menu_track = t.clone();
        let accent = self.accent_ink();
        // Só a nossa lista dá para reorganizar: com aleatório ou outro aparelho, quem manda na ordem é o Spotify.
        let draggable = !current && self.spotify_queue().is_none() && self.remote.is_none();
        let row = Self::focusable(("queue-row", index.unwrap_or(usize::MAX)))
            .when_some(index.filter(|_| draggable), |d, i| d
                .on_drag(QueueDrag { from: i, title: t.name.clone() }, |q, _, _, cx| cx.new(|_| DragGhost(q.title.clone().into())))
                .drag_over::<QueueDrag>(|s, _, _, _| s.bg(rgba(theme::wash(0x26))))
                .on_drop(cx.listener(move |this, q: &QueueDrag, _, cx| { this.move_queue(q.from, i); cx.notify(); })))
            .on_mouse_down(MouseButton::Right, cx.listener(move |t, e: &MouseDownEvent, _, cx| { t.open_menu(MenuTarget::Track(menu_track.clone()), e.position); cx.notify(); }))
            .w_full()
            .h(px(52.))
            .px_2()
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap_3()
            .when(current, |d| d.bg(rgba(theme::wash(0x14))))
            .when(!current, |d| d.hover(|s| s.bg(rgba(theme::wash(0x0f)))))
            .when(index.is_some() && self.spotify_queue().is_none(), |d| d.child(div().w(px(22.)).flex().justify_end().text_xs().text_color(rgba(theme::ink_3())).child((index.unwrap_or(0) + 1).to_string())))
            .child(cover)
            .child(
                div().flex_1().min_w_0().flex().flex_col()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(t.name.clone()))
                    .child(div().text_xs().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(format!("{} – {}", t.artist, t.album))),
            )
            // Recomendada pelo smart shuffle, como no Spotify.
            .when(!current && self.smart_added.contains(&t.uri), |d| d.child(icon("sparkles", 14., theme::ink_2())))
            .when(current, |d| d.child(now_icon(accent)))
            .when(draggable && index.is_some(), |d| d.child(icon("grip-vertical", 14., theme::ink_3())));
        match index {
            Some(i) => row
                .aria_label(SharedString::from(format!("Tocar {}", t.name)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let (tracks, context, label) = (this.tracks.clone(), this.queue_context.clone(), this.queue_label.clone());
                    this.send(Cmd::Play { tracks, index: i, context, label });
                    cx.notify();
                }))
                .into_any_element(),
            None => row.into_any_element(),
        }
    }

    /// Aparelhos Connect, abaixo do botão de fones; clicar fora fecha.
    fn devices_popover(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let right = if self.side.is_some() { QUEUE_W + 16. } else { CONTROLS_W + 4. };
        let body: AnyElement = match &self.devices {
            None => div().p_3().text_sm().text_color(rgba(theme::ink_3())).child("Procurando aparelhos…").into_any_element(),
            Some(list) if list.is_empty() => div().p_3().text_sm().text_color(rgba(theme::ink_3())).child("Nenhum aparelho encontrado.").into_any_element(),
            Some(list) => {
                let list: Vec<Device> = list.clone();
                div()
                    .flex()
                    .flex_col()
                    .children(list.into_iter().enumerate().map(|(i, d)| {
                        let icon_name = match d.kind.as_str() {
                            "Computer" => "laptop",
                            "Smartphone" => "smartphone",
                            _ => "speaker",
                        };
                        let (id, active) = (d.id.clone(), d.active);
                        Self::focusable(("device", i))
                            .h(px(40.))
                            .px_3()
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_sm()
                            .hover(|s| s.bg(rgba(theme::wash(0x14))))
                            .aria_selected(active)
                            .aria_label(SharedString::from(if active { format!("{} (tocando aqui)", d.name) } else { format!("Tocar em {}", d.name) }))
                            .on_click(cx.listener(move |t, _, _, cx| {
                                if !active {
                                    t.send(Cmd::Transfer(id.clone()));
                                }
                                t.devices_open = false;
                                cx.notify();
                            }))
                            .child(icon(icon_name, 16., if active { theme::ink_1() } else { theme::ink_2() }))
                            .child(div().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis()
                                .when(active, |d| d.font_weight(FontWeight::SEMIBOLD)).child(d.name))
                            .when(active, |d| d.child(icon("check", 15., theme::ink_1())))
                    }))
                    .into_any_element()
            }
        };
        div()
            .absolute()
            .inset_0()
            // Clique fora do cartão fecha.
            .child(div().absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|t, _: &MouseDownEvent, _, cx| { t.devices_open = false; cx.notify(); })))
            .child(
                theme::glass(div())
                    .occlude()
                    .absolute()
                    .top(px(GAP + TOOLBAR_H - 4.))
                    .right(px(right))
                    .w(px(280.))
                    .p_2()
                    .rounded(px(14.))
                    .shadow_2xl()
                    .child(div().px_3().pt_1().pb_2().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_3())).child("Tocar em"))
                    .child(body)
                    .child(div().my_1().h(px(1.)).bg(rgba(theme::wash(0x14))))
                    .child(self.listen_controls(cx)),
            )
    }
}

/// O que está sendo arrastado na fila: a posição de origem.
struct QueueDrag {
    from: usize,
    title: String,
}

/// Faixa de uma playlist sua sendo arrastada: a posição de origem.
struct PlaylistDrag {
    from: usize,
    title: String,
}

/// Item da sidebar sendo arrastado.
struct SideDrag {
    uri: String,
    title: String,
}

/// O que acompanha o mouse durante o arraste.
struct DragGhost(SharedString);

impl gpui::Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        theme::glass(div()).bg(rgba(theme::panel(0xf2))).px_3().py_1p5().rounded_full().text_sm().text_color(rgba(theme::ink_1())).shadow_2xl().child(self.0.clone())
    }
}

/// Número da faixa; com o mouse em cima, tocar (ou pausar, se é a que está tocando). A que toca mostra as ondas.
fn index_cell(i: usize, now: bool, playing: bool, group: &'static str, accent: gpui::Hsla) -> AnyElement {
    let resting = if now { now_icon(accent) } else { div().child((i + 1).to_string()).into_any_element() };
    div()
        .relative()
        .w(px(28.))
        .h(px(20.))
        .flex()
        .items_center()
        .justify_end()
        .text_sm()
        .text_color(rgba(theme::ink_3()))
        .child(div().group_hover(group, |s| s.opacity(0.)).child(resting))
        .child(
            div().absolute().inset_0().flex().items_center().justify_end().opacity(0.).group_hover(group, |s| s.opacity(1.))
                .child(icon(if now && playing { "pause" } else { "play" }, 15., theme::ink_1())),
        )
        .into_any_element()
}

/// Painéis, menus e diálogos aparecem com um fade curto em vez de surgir de uma vez. A camada cobre a tela
/// sem receber cliques (não tem eventos), e o conteúdo, posicionado por conta própria, fica onde estava.
/// Com "reduzir movimento" do sistema, o GPUI pula a animação.
pub(super) fn appear(id: &'static str, content: AnyElement) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .child(content)
        .with_animation(id, Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
        .into_any_element()
}

/// Nomes das colunas acima das linhas de faixa, nas mesmas larguras de `media_row` (índice 28 px, capa 36,
/// título, coluna do meio, coração 28, duração 44, "⋯" 28). As linhas têm 12 px de respiro mais a borda
/// invisível de 2 px do `focusable`: daí os 14 px dos lados.
/// `middle`: títulos das colunas entre o título e o coração; `cover`: as linhas têm capa.
pub(super) fn column_header(middle: &[&'static str], cover: bool) -> AnyElement {
    let label = |text: &'static str| div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_3())).child(text);
    div()
        .w_full()
        .h(px(32.))
        .px(px(14.))
        .mb_2()
        .flex()
        .items_center()
        .gap_3()
        .border_b_1()
        .border_color(rgba(theme::wash(0x1a)))
        .child(div().w(px(28.)).flex().justify_end().child(label("#")))
        .when(cover, |d| d.child(div().w(px(36.)).flex_none()))
        .child(div().flex_1().min_w_0().child(label("Título")))
        .children(middle.iter().map(|&m| div().flex_1().min_w_0().child(label(m))))
        .child(div().w(px(28.)).flex_none())
        .child(div().w(px(44.)).flex().justify_end().child(icon("clock", 13., theme::ink_3())))
        .child(div().w(px(28.)).flex_none())
        .into_any_element()
}

/// Ícone da faixa que está tocando, na cor da capa.
fn now_icon(accent: gpui::Hsla) -> AnyElement {
    gpui::svg().path("icons/audio-lines.svg").size(px(18.)).flex_none().text_color(accent).into_any_element()
}

fn minutes_label(minutes: u64) -> String {
    if minutes >= 60 { format!("{} h {} min", minutes / 60, minutes % 60) } else { format!("{minutes} min") }
}

/// Horário em que a fila termina, no relógio local ("15:46").
#[cfg(windows)]
fn finish_clock(secs_from_now: u64) -> Option<String> {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: GetLocalTime só escreve na struct passada.
    let now = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    let minutes = (now.wHour as u64 * 60 + now.wMinute as u64 + secs_from_now.div_ceil(60)) % (24 * 60);
    Some(format!("{}:{:02}", minutes / 60, minutes % 60))
}

#[cfg(not(windows))]
fn finish_clock(_secs_from_now: u64) -> Option<String> {
    None
}

/// "ABCDEFGH" → "ABCD-EFGH", mais fácil de ditar.
pub(super) fn pretty_code(code: &str) -> String {
    if code.len() == 8 { format!("{}-{}", &code[..4], &code[4..]) } else { code.to_string() }
}

/// Fuso local em segundos (hora local menos UTC), para as estatísticas contarem os dias certos.
#[cfg(windows)]
fn local_offset_secs() -> i64 {
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
fn local_offset_secs() -> i64 {
    0
}
