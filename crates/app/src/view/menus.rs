//! Menus suspensos: botão direito e "⋯" em playlists, álbuns, artistas e faixas, o filtro da sidebar
//! e o menu da conta. Os itens são montados a cada render a partir do alvo; um item com setinha abre um submenu.
use std::time::{Duration, Instant};

use cyder_core::{Card, Cmd, Kind, Track};
use gpui::{AnyElement, ClickEvent, Context, FontWeight, MouseButton, MouseDownEvent, ParentElement, Pixels, Point, SharedString, Styled, Window, div, prelude::*, px, rgba};

use super::library::{SideFilter, Screen, liked::LikedSort};
use super::{Cyder, Dialog, icon};
use crate::theme;

const W: f32 = 264.;
const ITEM_H: f32 = 34.;
const SEP_H: f32 = 9.;
/// Respiro do cartão (p_2 dos dois lados).
const PAD_H: f32 = 16.;
/// Playlists no submenu "Adicionar à playlist"; mais que isso não cabe na tela.
const ADD_TO_MAX: usize = 14;
const MARGIN: f32 = 8.;

#[derive(Clone)]
pub enum MenuTarget {
    Card(Card),
    Track(Track),
    SidebarFilter,
    Account,
    /// Ordem e modo de ver das músicas curtidas.
    LikedSort,
}

pub struct Menu {
    pub target: MenuTarget,
    pub at: Point<Pixels>,
    /// Abre para cima do ponto.
    pub above: bool,
    /// Posição fixa em vez da do mouse: esquerda, borda de baixo e largura (o menu da conta, em cima do rodapé).
    pub fixed: Option<(f32, f32, f32)>,
    /// Item com submenu aberto.
    pub sub: Option<usize>,
}

impl Menu {
    pub fn new(target: MenuTarget, at: Point<Pixels>) -> Self {
        Self { target, at, above: false, fixed: None, sub: None }
    }
}

/// Timer para pausar.
#[derive(Clone, Copy, PartialEq)]
pub enum Sleep {
    At(Instant),
    /// Pausa quando a faixa com este índice da fila acabar.
    EndOfTrack(usize),
}

#[derive(Clone)]
enum Action {
    PlayCard { card: Card, shuffle: bool },
    Save { card: Card, save: bool },
    Pin { uri: String, on: bool },
    Copy(String),
    Open(String),
    Go(Card),
    Like { track: Track, on: bool },
    Queue(Track),
    AddTo { playlist: Card, uri: String },
    Rename(Card),
    Cover(Card),
    RemoveFrom { playlist: Card, uri: String },
    NewPlaylist,
    /// Oculta (ou mostra de novo) a faixa no álbum ou na playlist: ela pula quando toca ali.
    Hide { key: String, on: bool },
    Radio(Track),
    Credits(Track),
    Sleep(Option<SleepChoice>),
    Filter(SideFilter),
    /// Volta a sidebar para "mais novos primeiro", descartando a ordem arrastada à mão.
    LibraryByAdded,
    /// Escolher a ordem já escolhida inverte a direção.
    LikedSort(LikedSort),
    LikedCompact(bool),
    Settings,
    Shortcuts,
    Logout,
}

#[derive(Clone, Copy)]
enum SleepChoice {
    Minutes(u64),
    EndOfTrack,
}

struct Item {
    label: SharedString,
    icon: &'static str,
    action: Option<Action>,
    sub: Vec<Entry>,
    checked: bool,
    /// Ícone de marcado no lugar do ✓ (a seta da direção da ordem).
    mark: &'static str,
}

enum Entry {
    Item(Item),
    Sep,
    /// Título de grupo, sem ação.
    Heading(&'static str),
}

const HEADING_H: f32 = 28.;

fn entry_h(e: &Entry) -> f32 {
    match e {
        Entry::Sep => SEP_H,
        Entry::Heading(_) => HEADING_H,
        Entry::Item(_) => ITEM_H,
    }
}

fn item(label: impl Into<SharedString>, icon: &'static str, action: Action) -> Entry {
    Entry::Item(Item { label: label.into(), icon, action: Some(action), sub: Vec::new(), checked: false, mark: "check" })
}

fn parent(label: impl Into<SharedString>, icon: &'static str, sub: Vec<Entry>) -> Entry {
    Entry::Item(Item { label: label.into(), icon, action: None, sub, checked: false, mark: "check" })
}

fn height(entries: &[Entry]) -> f32 {
    entries.iter().map(entry_h).sum::<f32>() + PAD_H
}

fn web_link(kind: &str, id: &str) -> String {
    format!("https://open.spotify.com/{kind}/{id}")
}

impl Cyder {
    pub(super) fn open_menu(&mut self, target: MenuTarget, at: Point<Pixels>) {
        self.menu = Some(Menu::new(target, at));
    }

    fn entries(&self, target: &MenuTarget) -> Vec<Entry> {
        match target {
            MenuTarget::Card(card) => self.card_entries(card),
            MenuTarget::Track(track) => self.track_entries(track),
            MenuTarget::SidebarFilter => {
                let mut out: Vec<Entry> = [
                    (SideFilter::All, "Tudo", "layout-grid"),
                    (SideFilter::Playlists, "Playlists", "list-music"),
                    (SideFilter::Artists, "Artistas", "user-round"),
                    (SideFilter::Albums, "Álbuns", "disc-3"),
                    (SideFilter::Podcasts, "Podcasts", "podcast"),
                ]
                .into_iter()
                .map(|(f, label, icon)| Entry::Item(Item { label: label.into(), icon, action: Some(Action::Filter(f)), sub: Vec::new(), checked: self.side_filter == f, mark: "check" }))
                .collect();
                // Arrastar um item muda para a ordem personalizada; ela só aparece aqui para mostrar que está valendo.
                let custom = !self.library_order.is_empty();
                out.push(Entry::Sep);
                out.push(Entry::Heading("Ordenar por"));
                out.push(Entry::Item(Item { label: "Adicionados recentemente".into(), icon: "clock", action: Some(Action::LibraryByAdded), sub: Vec::new(), checked: !custom, mark: "check" }));
                if custom {
                    out.push(Entry::Item(Item { label: "Ordem personalizada".into(), icon: "grip-vertical", action: None, sub: Vec::new(), checked: true, mark: "check" }));
                }
                out
            }
            MenuTarget::LikedSort => {
                let o = self.liked_order;
                let mut out = vec![Entry::Heading("Ordenar por")];
                out.extend(LikedSort::ALL.into_iter().map(|sort| Entry::Item(Item {
                    label: sort.label().into(),
                    icon: match sort { LikedSort::Added => "clock", LikedSort::Title => "music", LikedSort::Artist => "user-round", LikedSort::Album => "disc-3" },
                    action: Some(Action::LikedSort(sort)),
                    sub: Vec::new(),
                    checked: o.sort == sort,
                    mark: if o.reversed { "arrow-up" } else { "arrow-down" },
                })));
                out.push(Entry::Sep);
                out.push(Entry::Heading("Ver como"));
                for (compact, label, icon) in [(true, "Compacta", "menu"), (false, "Lista", "list")] {
                    out.push(Entry::Item(Item { label: label.into(), icon, action: Some(Action::LikedCompact(compact)), sub: Vec::new(), checked: o.compact == compact, mark: "check" }));
                }
                out
            }
            MenuTarget::Account => vec![
                item("Configurações", "settings", Action::Settings),
                item("Atalhos", "keyboard", Action::Shortcuts),
                Entry::Sep,
                item("Sair da conta", "log-out", Action::Logout),
            ],
        }
    }

    fn card_entries(&self, card: &Card) -> Vec<Entry> {
        let lib = self.library.as_ref();
        let kind = match card.kind {
            Kind::Album => "album",
            Kind::Artist => "artist",
            Kind::Playlist => "playlist",
            Kind::Show => "show",
        };
        let link = web_link(kind, &card.id);
        let mut out = vec![
            item(if card.kind == Kind::Artist { "Tocar o artista" } else { "Reproduzir" }, "play", Action::PlayCard { card: card.clone(), shuffle: false }),
            item("Reproduzir em ordem aleatória", "shuffle", Action::PlayCard { card: card.clone(), shuffle: true }),
        ];
        let saved = lib.is_some_and(|l| match card.kind {
            Kind::Album => l.albums.iter().any(|c| c.id == card.id),
            Kind::Artist => l.artists.iter().any(|c| c.id == card.id),
            Kind::Playlist => l.playlists.iter().any(|c| c.id == card.id),
            Kind::Show => l.shows.iter().any(|c| c.id == card.id),
        });
        out.push(match (card.kind, saved) {
            (Kind::Artist, true) => item("Deixar de seguir", "circle-x", Action::Save { card: card.clone(), save: false }),
            (Kind::Artist, false) => item("Seguir", "plus", Action::Save { card: card.clone(), save: true }),
            (_, true) => item("Remover da Sua Biblioteca", "circle-x", Action::Save { card: card.clone(), save: false }),
            (_, false) => item("Salvar na Sua Biblioteca", "plus", Action::Save { card: card.clone(), save: true }),
        });
        out.push(Entry::Sep);
        out.push(item("Copiar link", "copy", Action::Copy(link.clone())));
        out.push(item("Abrir no Spotify", "external-link", Action::Open(link)));
        if self.owns(card) {
            out.push(Entry::Sep);
            out.push(item("Renomear", "pencil", Action::Rename(card.clone())));
            out.push(item("Trocar capa", "image", Action::Cover(card.clone())));
        }
        if card.kind == Kind::Playlist {
            let pinned = self.pins.contains(&card.uri);
            out.push(item(if pinned { "Desafixar do topo" } else { "Fixar no topo" }, if pinned { "pin-off" } else { "pin" }, Action::Pin { uri: card.uri.clone(), on: !pinned }));
        }
        out
    }

    /// Playlist sua: o dono tem o seu nome. Só essas dá para editar (e para adicionar faixas).
    pub(super) fn owns(&self, card: &Card) -> bool {
        card.kind == Kind::Playlist && match self.profile.as_ref().map(|p| p.0.as_str()) {
            Some(name) => card.subtitle == name,
            None => card.subtitle != "Spotify",
        }
    }

    fn track_entries(&self, track: &Track) -> Vec<Entry> {
        let liked = self.is_liked(&track.uri);
        let mine: Vec<Card> = self.sidebar_playlists().into_iter().filter(|c| self.owns(c)).take(ADD_TO_MAX).collect();
        // Aberta numa playlist sua: dá para tirar a faixa dela.
        let here = match &self.screen {
            Screen::Page(c) if self.owns(c) && self.pages.get(&c.id).is_some_and(|p| p.tracks.iter().any(|t| t.uri == track.uri)) => Some(c.clone()),
            _ => None,
        };
        let mut add_to: Vec<Entry> = mine.into_iter().map(|p| item(p.title.clone(), "list-music", Action::AddTo { playlist: p, uri: track.uri.clone() })).collect();
        if add_to.is_empty() {
            add_to.push(item("Criar playlist", "plus", Action::NewPlaylist));
        }
        // Aberta num álbum ou numa playlist que tem a faixa: dá para ocultá-la ali.
        let hide = match &self.screen {
            Screen::Page(c) if c.kind != Kind::Artist && self.pages.get(&c.id).is_some_and(|p| p.tracks.iter().any(|t| t.uri == track.uri)) => {
                let key = super::hidden_key(&c.uri, &track.uri);
                let on = !self.hidden.contains(&key);
                let label = match (on, c.kind) {
                    (true, Kind::Album) => "Ocultar neste álbum",
                    (true, _) => "Ocultar nesta playlist",
                    (false, Kind::Album) => "Mostrar neste álbum",
                    (false, _) => "Mostrar nesta playlist",
                };
                Some(item(label, "eye-off", Action::Hide { key, on }))
            }
            _ => None,
        };
        let mut sleep = vec![
            item("15 minutos", "timer", Action::Sleep(Some(SleepChoice::Minutes(15)))),
            item("30 minutos", "timer", Action::Sleep(Some(SleepChoice::Minutes(30)))),
            item("45 minutos", "timer", Action::Sleep(Some(SleepChoice::Minutes(45)))),
            item("1 hora", "timer", Action::Sleep(Some(SleepChoice::Minutes(60)))),
            item("Fim da música", "timer", Action::Sleep(Some(SleepChoice::EndOfTrack))),
        ];
        if self.sleep.is_some() {
            sleep.push(Entry::Sep);
            sleep.push(item("Desligar o timer", "x", Action::Sleep(None)));
        }
        let artist_card = |(id, name): &(String, String)| Card { kind: Kind::Artist, uri: format!("spotify:artist:{id}"), id: id.clone(), title: name.clone(), subtitle: "Artista".into(), image: None };
        let go_artist = match track.artists.as_slice() {
            [] => None,
            [one] => Some(item("Ir para o artista", "user-round", Action::Go(artist_card(one)))),
            many => Some(parent("Ir para o artista", "user-round", many.iter().map(|a| item(a.1.clone(), "user-round", Action::Go(artist_card(a)))).collect())),
        };
        let episode = track.uri.starts_with("spotify:episode:");
        let link = web_link(if track.uri.starts_with("spotify:episode:") { "episode" } else { "track" }, &track.id);
        let mut out = vec![
            parent("Adicionar à playlist", "list-plus", add_to),
            if liked {
                item("Remover das Músicas curtidas", "heart-off", Action::Like { track: track.clone(), on: false })
            } else {
                item("Adicionar às Músicas curtidas", "heart", Action::Like { track: track.clone(), on: true })
            },
            item("Adicionar à fila", "list-end", Action::Queue(track.clone())),
        ];
        if let Some(playlist) = here {
            out.push(item("Remover desta playlist", "list-minus", Action::RemoveFrom { playlist, uri: track.uri.clone() }));
        }
        out.extend(hide);
        out.push(Entry::Sep);
        out.push(parent(if self.sleep.is_some() { "Timer (ligado)" } else { "Timer" }, "timer", sleep));
        out.push(Entry::Sep);
        if !episode {
            out.push(item("Ir para o rádio da música", "radio", Action::Radio(track.clone())));
        }
        out.extend(go_artist);
        if let Some(album) = self.album_of(track) {
            out.push(item("Ir para o álbum", "disc-3", Action::Go(album)));
        }
        if !episode {
            out.push(item("Ver créditos", "info", Action::Credits(track.clone())));
        }
        out.push(parent("Compartilhar", "share-2", vec![
            item("Copiar link da música", "copy", Action::Copy(link.clone())),
            item("Abrir no Spotify", "external-link", Action::Open(link)),
        ]));
        out
    }

    /// Salva na biblioteca ou tira (seguir artista, podcast). Já mostra; o core confirma com o Spotify
    /// (e devolve a lista certa se der errado).
    pub(super) fn save_card(&mut self, card: Card, save: bool) {
        if let Some(lib) = self.library.as_mut() {
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
        }
        if !save && self.pins.contains(&card.uri) {
            self.pins.retain(|p| *p != card.uri);
            self.send(Cmd::Pin { uri: card.uri.clone(), on: false });
        }
        self.send(Cmd::Library { card, save });
    }

    fn run_action(&mut self, action: Action, cx: &mut Context<Self>) {
        self.menu = None;
        match action {
            Action::PlayCard { card, shuffle } => {
                if shuffle != self.shuffle {
                    self.shuffle = shuffle;
                    self.send(Cmd::SetShuffle(shuffle));
                }
                // Escolha explícita no menu: vira o aleatório lembrado para esse tipo de lista.
                if let Some(kind) = super::library::ListKind::of(&card.uri) {
                    self.save_shuffle(kind, shuffle);
                }
                self.play_context(&card);
            }
            Action::Save { card, save } => self.save_card(card, save),
            Action::Pin { uri, on } => {
                self.pins.retain(|p| *p != uri);
                if on {
                    self.pins.insert(0, uri.clone());
                }
                self.send(Cmd::Pin { uri, on });
            }
            Action::Copy(link) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(link));
                self.notify("Link copiado.");
            }
            Action::Open(link) => cx.open_url(&link),
            Action::Go(card) => {
                self.set_immersive(false);
                self.go(Screen::Page(card));
            }
            Action::Like { track, on } => self.set_liked(&track, on),
            Action::Queue(track) => self.send(Cmd::AddToQueue(track)),
            Action::AddTo { playlist, uri } => self.send(Cmd::AddToPlaylist { playlist, uri }),
            Action::Sleep(choice) => {
                self.sleep = match choice {
                    Some(SleepChoice::Minutes(m)) => Some(Sleep::At(Instant::now() + Duration::from_secs(m * 60))),
                    Some(SleepChoice::EndOfTrack) => Some(Sleep::EndOfTrack(self.current)),
                    None => None,
                };
                self.notify(match choice {
                    Some(SleepChoice::Minutes(m)) if m >= 60 => "A música para em 1 hora.".to_string(),
                    Some(SleepChoice::Minutes(m)) => format!("A música para em {m} minutos."),
                    Some(SleepChoice::EndOfTrack) => "A música para quando esta acabar.".into(),
                    None => "Timer desligado.".into(),
                });
            }
            Action::LikedSort(sort) => {
                let o = &mut self.liked_order;
                o.reversed = o.sort == sort && !o.reversed;
                o.sort = sort;
                self.liked_changed();
            }
            Action::LikedCompact(compact) => self.liked_order.compact = compact,
            Action::Filter(f) => {
                self.side_filter = f;
                self.refocus = true;
            }
            Action::LibraryByAdded => {
                self.library_order.clear();
                self.send(Cmd::SaveLibraryOrder(Vec::new()));
            }
            Action::Settings => self.go(Screen::Settings),
            Action::Rename(card) => self.ask_playlist_name(Some(card)),
            Action::RemoveFrom { playlist, uri } => {
                if let Some(page) = self.pages.get_mut(&playlist.id) {
                    page.tracks.retain(|t| t.uri != uri);
                }
                self.send(Cmd::RemoveFromPlaylist { card: playlist, uri });
            }
            Action::Cover(card) => {
                let picked = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Escolher capa".into()) });
                cx.spawn(async move |this, cx| {
                    let Ok(Ok(Some(paths))) = picked.await else { return };
                    let Some(path) = paths.into_iter().next() else { return };
                    let _ = this.update(cx, |t, _| t.send(Cmd::SetPlaylistCover { card, path }));
                })
                .detach();
            }
            Action::Shortcuts => self.dialog = Some(Dialog::Shortcuts),
            Action::Logout => self.dialog = Some(Dialog::Logout),
            Action::NewPlaylist => self.ask_playlist_name(None),
            Action::Hide { key, on } => {
                self.hidden.retain(|k| *k != key);
                if on {
                    self.hidden.push(key);
                }
                cyder_core::config::save_hidden(&self.hidden);
                self.notify(if on { "Oculta aqui: ela pula quando tocar neste lugar." } else { "Ela volta a tocar aqui." });
            }
            Action::Radio(track) => {
                self.notify("Abrindo o rádio…");
                self.send(Cmd::Radio(track));
            }
            Action::Credits(track) => self.send(Cmd::Credits(track)),
        }
        cx.notify();
    }

    /// O menu aberto (e o submenu), sem sair da janela; clicar fora ou Esc fecha.
    pub(super) fn menu_layer(&mut self, vw: f32, vh: f32, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let (target, at, above, fixed, sub) = (menu.target.clone(), menu.at, menu.above, menu.fixed, menu.sub);
        let entries = self.entries(&target);
        let h = height(&entries);
        let w = fixed.map_or(W, |f| f.2);
        let (x, y) = match fixed {
            Some((left, bottom, _)) => (left, bottom - h),
            None => {
                let x = f32::from(at.x).min(vw - W - MARGIN).max(MARGIN);
                let y = if above { f32::from(at.y) - h - 8. } else { f32::from(at.y) };
                (x, y)
            }
        };
        let y = y.min(vh - h - MARGIN).max(MARGIN);
        // Submenu ao lado do item; vira para a esquerda se não couber à direita.
        let mut sub_panel = None;
        if let Some(Entry::Item(it)) = sub.and_then(|i| entries.get(i)) {
            if !it.sub.is_empty() {
                let top = y + 8. + entries[..sub.unwrap_or(0)].iter().map(entry_h).sum::<f32>() - 8.;
                let sh = height(&it.sub);
                let sx = if x + w + W - 4. <= vw - MARGIN { x + w - 4. } else { (x - W + 4.).max(MARGIN) };
                let sy = top.min(vh - sh - MARGIN).max(MARGIN);
                sub_panel = Some((sx, sy));
            }
        }
        let main_items = self.render_entries(entries, true, cx);
        let close = |t: &mut Self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>| { t.menu = None; cx.notify(); };
        let mut layer = div()
            .absolute()
            .inset_0()
            .child(div().absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(close)).on_mouse_down(MouseButton::Right, cx.listener(close)))
            .child(Self::menu_card(x, y, w).children(main_items));
        if let (Some((sx, sy)), Some(i)) = (sub_panel, sub) {
            // Monta de novo para pegar os filhos (os itens não são Clone).
            if let Some(Entry::Item(it)) = self.entries(&target).into_iter().nth(i) {
                let sub_items = self.render_entries(it.sub, false, cx);
                layer = layer.child(Self::menu_card(sx, sy, W).children(sub_items));
            }
        }
        Some(layer.into_any_element())
    }

    fn menu_card(x: f32, y: f32, w: f32) -> gpui::Div {
        theme::glass(div())
            .bg(rgba(theme::panel(0xf2)))
            .occlude()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w))
            .p_2()
            .rounded(px(12.))
            .shadow_2xl()
            .flex()
            .flex_col()
    }

    fn render_entries(&mut self, entries: Vec<Entry>, top_level: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        entries.into_iter().enumerate().map(|(i, e)| match e {
            Entry::Sep => div().my_1().h(px(1.)).bg(rgba(theme::wash(0x14))).into_any_element(),
            Entry::Heading(text) => div().h(px(HEADING_H)).px_2().flex().items_end().pb_1().text_xs().font_weight(FontWeight::SEMIBOLD)
                .text_color(rgba(theme::ink_3())).child(text).into_any_element(),
            Entry::Item(it) => {
                let has_sub = !it.sub.is_empty();
                let open = top_level && self.menu.as_ref().is_some_and(|m| m.sub == Some(i));
                let action = it.action.clone();
                Self::focusable((if top_level { "menu-item" } else { "submenu-item" }, i))
                    .h(px(ITEM_H))
                    .px_2()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_sm()
                    .when(open, |d| d.bg(rgba(theme::wash(0x14))))
                    .hover(|s| s.bg(rgba(theme::wash(0x14))))
                    // Passar o mouse num item abre o submenu dele (e fecha o de outro).
                    .when(top_level, |d| d.on_hover(cx.listener(move |t, hovered: &bool, _, cx| {
                        if *hovered {
                            if let Some(m) = t.menu.as_mut() {
                                let want = has_sub.then_some(i);
                                if m.sub != want {
                                    m.sub = want;
                                    cx.notify();
                                }
                            }
                        }
                    })))
                    .on_click(cx.listener(move |t, _: &ClickEvent, _, cx| {
                        match action.clone() {
                            Some(a) => t.run_action(a, cx),
                            None => {
                                if let Some(m) = t.menu.as_mut() {
                                    m.sub = Some(i);
                                }
                                cx.notify();
                            }
                        }
                    }))
                    .child(icon(it.icon, 15., theme::ink_2()))
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis()
                        .when(it.checked, |d| d.font_weight(FontWeight::SEMIBOLD))
                        .child(it.label))
                    .when(it.checked, |d| d.child(icon(it.mark, 14., theme::ink_1())))
                    .when(has_sub, |d| d.child(icon("chevron-right", 14., theme::ink_3())))
                    .into_any_element()
            }
        }).collect()
    }
}
