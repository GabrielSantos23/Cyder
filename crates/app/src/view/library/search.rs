//! Busca no Spotify: o campo no topo da sidebar e a tela de resultados. Em "Tudo", o melhor resultado ao lado
//! das músicas e uma prateleira por tipo; com um filtro, a lista daquele tipo, que carrega mais ao rolar.
use std::time::{Duration, Instant};

use cyder_core::{Card, Cmd, Kind, SearchKind, SearchResults, Shelf, config};
use gpui::{AnyElement, Context, FontWeight, ObjectFit, ParentElement, SharedString, Styled, Window, div, img, prelude::*, px, rgba};

use super::{CardSource, LIST_BOTTOM, SHELF_CARD, Screen, TrackSource, kind_label};
use crate::theme;
use crate::view::{Cyder, Tip, icon, loading};

/// Espera a pessoa parar de digitar antes de buscar.
pub(in crate::view) const DEBOUNCE: Duration = Duration::from_millis(300);
const RECENT_MAX: usize = 8;
/// Músicas ao lado do melhor resultado, em "Tudo".
const SONGS: usize = 5;
/// Índices das prateleiras da busca no mapa de rolagens (as da tela inicial vêm antes).
const SHELF_BASE: usize = 100;
/// As da tela inicial vazia da busca vêm depois das de resultado (uma por tipo).
const START_SHELVES: usize = 10;

fn label(kind: SearchKind) -> &'static str {
    match kind {
        SearchKind::Tracks => "Músicas",
        SearchKind::Artists => "Artistas",
        SearchKind::Albums => "Álbuns",
        SearchKind::Playlists => "Playlists",
        SearchKind::Shows => "Podcasts",
    }
}

/// Resultados pedidos de cada vez ao rolar um filtro (quatro páginas de 10 do Spotify).
pub(in crate::view) const MORE_BATCH: usize = 40;
/// O Spotify não passa do resultado 1000.
const SEARCH_MAX: usize = 1000;

pub(in crate::view) fn count(found: &SearchResults, kind: SearchKind) -> usize {
    match kind {
        SearchKind::Tracks => found.tracks.len(),
        SearchKind::Artists => found.artists.len(),
        SearchKind::Albums => found.albums.len(),
        SearchKind::Playlists => found.playlists.len(),
        SearchKind::Shows => found.shows.len(),
    }
}

/// O Spotify ordena cada tipo, mas não diz qual tipo ganhou. Nome igual à busca vence; depois, um artista
/// cujo nome contém a busca (ou vice-versa); senão o primeiro artista, álbum, playlist ou podcast.
fn top_result(found: &SearchResults) -> Option<Card> {
    let q = found.query.trim().to_lowercase();
    let lists = [&found.artists, &found.albums, &found.playlists, &found.shows];
    lists.iter().find_map(|l| l.first().filter(|c| c.title.to_lowercase() == q))
        .or_else(|| found.artists.first().filter(|c| {
            let name = c.title.to_lowercase();
            name.contains(&q) || q.contains(&name)
        }))
        .or_else(|| lists.iter().find_map(|l| l.first()))
        .cloned()
}

impl Cyder {
    /// O campo de busca do topo da sidebar. Clicar ou digitar abre a tela de busca.
    pub(in crate::view) fn search_field(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let focused = self.search_focus.is_focused(window);
        let empty = self.search_query.is_empty();
        let caret = || div().flex_none().w(px(1.5)).h(px(14.)).bg(rgba(theme::ink_1()));
        let text = div().flex_1().min_w_0().flex().items_center().overflow_hidden().whitespace_nowrap();
        let text = if empty {
            text.when(focused, |d| d.child(caret())).child(div().text_color(rgba(theme::ink_3())).child("Buscar"))
        } else {
            text.child(self.search_query.clone()).when(focused, |d| d.child(caret()))
        };
        div()
            .id("global-search")
            .key_context("TextField")
            .track_focus(&self.search_focus)
            .aria_label("Buscar no Spotify")
            .w_full()
            .h(px(32.))
            .pl_3()
            .pr_1()
            .rounded_full()
            .flex()
            .items_center()
            .gap_2()
            .border_1()
            .border_color(rgba(if focused { theme::ink_3() } else { theme::wash(0x17) }))
            .bg(rgba(theme::wash(0x0d)))
            .text_sm()
            .cursor_text()
            .hover(|s| s.bg(rgba(theme::wash(0x17))))
            // No mouse down: o campo fica na faixa de arrastar a janela, que engole o clique.
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|t, _: &gpui::MouseDownEvent, window, cx| t.open_search(window, cx)))
            .on_key_down(cx.listener(|t, e: &gpui::KeyDownEvent, window, cx| t.search_key(e, window, cx)))
            .child(icon("search", 14., theme::ink_3()))
            .child(text)
            .when(!empty, |d| d.child(
                Self::focusable("search-clear")
                    .size(px(24.))
                    .flex_none()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(rgba(theme::wash(0x1f))))
                    .tip("Limpar a busca")
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|t, _: &gpui::MouseDownEvent, window, cx| {
                        t.search_query.clear();
                        t.run_search();
                        window.focus(&t.search_focus, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(icon("x", 13., theme::ink_2())),
            ))
    }

    pub(in crate::view) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.screen, Screen::Search) {
            self.go(Screen::Search);
            // O go devolve o foco à raiz no próximo render; aqui ele fica no campo.
            self.refocus = false;
        }
        window.focus(&self.search_focus, cx);
        cx.notify();
    }

    /// Esc limpa e sai do campo; Enter busca na hora e guarda nas recentes.
    fn search_key(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        match k.key.as_str() {
            "escape" => {
                self.search_query.clear();
                self.run_search();
                window.focus(&self.focus, cx);
            }
            "enter" => {
                self.run_search();
                self.remember_search();
            }
            _ if super::super::type_into(&mut self.search_query, k, cx) => {
                self.search_due = Some(Instant::now());
                if !matches!(self.screen, Screen::Search) {
                    self.go(Screen::Search);
                    self.refocus = false;
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Busca o que está no campo (todos os tipos, 10 de cada).
    pub(in crate::view) fn run_search(&mut self) {
        self.search_due = None;
        self.search_error = None;
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            self.search = None;
            self.search_loading = false;
            return;
        }
        if self.search.as_ref().is_some_and(|s| s.query == query) {
            return;
        }
        self.search_loading = true;
        self.search_more.clear();
        self.search_done.clear();
        self.send(Cmd::Search { query, kind: None, offset: 0 });
    }

    /// Com um filtro, mais daquele tipo: ao abrir o filtro e quando a rolagem chega perto do fim.
    /// Um pedido por vez, e só enquanto o Spotify ainda tiver mais.
    pub(in crate::view) fn request_more(&mut self, kind: SearchKind) {
        // Resultado antigo na tela enquanto a busca nova não chega: pedir mais dele é desperdício
        // (a resposta seria ignorada) e deixaria o tipo preso em `search_more`.
        let Some(found) = self.search.as_ref().filter(|s| s.query == self.search_query.trim()) else { return };
        let have = count(found, kind);
        if have < 10 || have >= SEARCH_MAX || self.search_done.contains(&kind) || self.search_more.contains(&kind) {
            return;
        }
        self.search_more.insert(kind);
        let query = found.query.clone();
        self.send(Cmd::Search { query, kind: Some(kind), offset: have });
    }

    pub(in crate::view) fn remember_search(&mut self) {
        let q = self.search_query.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.recent_searches.retain(|r| !r.eq_ignore_ascii_case(&q));
        self.recent_searches.insert(0, q);
        self.recent_searches.truncate(RECENT_MAX);
        config::save_recent_searches(&self.recent_searches);
    }

    fn set_search_filter(&mut self, filter: Option<SearchKind>) {
        self.search_filter = filter;
        if let Some(kind) = filter {
            self.request_more(kind);
        }
    }

    pub(super) fn search_view(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let query = self.search_query.trim().to_string();
        let title = if query.is_empty() { "Buscar".to_string() } else { format!("Resultados para “{query}”") };
        let header = div()
            .pb_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_size(px(24.)).font_weight(FontWeight::BOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(title))
            .children(self.search.as_ref().filter(|s| !query.is_empty() && !s.is_empty()).map(|s| self.search_filters(s, cx)));
        let body = if query.is_empty() {
            self.search_start(cx)
        } else if let Some(error) = self.search_error.clone() {
            Self::search_message("Não deu para buscar.", error)
                .child(Self::text_button("search-retry", "Tentar de novo").on_click(cx.listener(|t, _, _, cx| {
                    t.search = None;
                    t.run_search();
                    cx.notify();
                })))
                .into_any_element()
        } else {
            match self.search.clone() {
                None => loading::home(SHELF_CARD),
                Some(s) if s.is_empty() && s.query == query => {
                    Self::search_message(format!("Nada encontrado para “{query}”"), "Confira a grafia ou tente outras palavras.".into()).into_any_element()
                }
                // Enquanto a busca nova não chega, o resultado anterior fica, apagado.
                Some(s) => {
                    let stale = s.query != query;
                    let results = match self.search_filter {
                        None => self.search_all(&s, width, cx),
                        Some(SearchKind::Tracks) => self.track_list(TrackSource::Search, true, cx),
                        Some(kind) => self.card_grid(CardSource::Search(kind), width, cx),
                    };
                    div().flex_1().min_h_0().flex().flex_col().when(stale, |d| d.opacity(0.5)).child(results).into_any_element()
                }
            }
        };
        div().size_full().flex().flex_col().child(header).child(body).into_any_element()
    }

    /// Tudo, e um chip por tipo que trouxe algo.
    fn search_filters(&self, found: &SearchResults, cx: &mut Context<Self>) -> AnyElement {
        let options = std::iter::once(None).chain(SearchKind::ALL.into_iter().filter(|k| count(found, *k) > 0).map(Some));
        let chips: Vec<AnyElement> = options.enumerate().map(|(i, filter)| {
            let on = self.search_filter == filter;
            Self::focusable(("search-filter", i))
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
                .on_click(cx.listener(move |t, _, _, cx| { t.set_search_filter(filter); cx.notify(); }))
                .child(filter.map_or("Tudo", label))
                .into_any_element()
        }).collect();
        div().flex().flex_wrap().gap_2().children(chips).into_any_element()
    }

    /// Campo vazio: as buscas recentes (ou o que dá para buscar) e, embaixo, o que você anda ouvindo, para não
    /// deixar a tela em branco. As prateleiras são as da tela inicial, já carregadas: nenhum pedido a mais.
    fn search_start(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let recent = self.recent_searches_block(cx);
        let shelves: Vec<AnyElement> = self.home.clone().unwrap_or_default().into_iter().enumerate()
            .map(|(i, shelf)| self.shelf(SHELF_BASE + START_SHELVES + i, shelf, cx))
            .collect();
        div()
            .id("search-start")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(px(LIST_BOTTOM))
            .flex()
            .flex_col()
            .gap_8()
            .child(recent)
            .children(shelves)
            .into_any_element()
    }

    fn recent_searches_block(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.recent_searches.is_empty() {
            return Self::search_message("Encontre o que ouvir", "Busque músicas, artistas, álbuns, playlists e podcasts.".into()).into_any_element();
        }
        let chips: Vec<AnyElement> = self.recent_searches.iter().enumerate().map(|(i, q)| {
            let q = q.clone();
            Self::focusable(("recent-search", i))
                .h(px(34.))
                .px_4()
                .rounded_full()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .bg(rgba(theme::wash(0x0f)))
                .border_1()
                .border_color(rgba(theme::wash(0x12)))
                .hover(|s| s.bg(rgba(theme::wash(0x1f))))
                .aria_label(SharedString::from(format!("Buscar {q} de novo")))
                .child(icon("clock", 13., theme::ink_3()))
                .child(q.clone())
                .on_click(cx.listener(move |t, _, _, cx| {
                    t.search_query = q.clone();
                    t.run_search();
                    cx.notify();
                }))
                .into_any_element()
        }).collect();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div().flex().items_center().justify_between()
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child("Buscas recentes"))
                    .child(Self::text_button("recent-clear", "Limpar").on_click(cx.listener(|t, _, _, cx| {
                        t.recent_searches.clear();
                        config::save_recent_searches(&[]);
                        cx.notify();
                    }))),
            )
            .child(div().flex().flex_wrap().gap_2().children(chips))
            .into_any_element()
    }

    /// Melhor resultado e músicas lado a lado (um embaixo do outro numa tela estreita); depois as prateleiras.
    fn search_all(&mut self, found: &SearchResults, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let top = top_result(found).map(|card| self.top_card(card, cx));
        let songs: Vec<AnyElement> = found.tracks.iter().take(SONGS).enumerate()
            .map(|(i, t)| self.song_row(&TrackSource::Search, i, t, t.artist.clone(), cx))
            .collect();
        let has_songs = !songs.is_empty();
        let side_by_side = width >= 720.;
        let songs_el = div().flex_1().min_w_0().flex().flex_col().gap_3()
            .child(
                div().flex().items_center().justify_between()
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child("Músicas"))
                    .when(found.tracks.len() > SONGS, |d| d.child(Self::text_button("search-all-songs", "Ver todas")
                        .on_click(cx.listener(|t, _, _, cx| { t.set_search_filter(Some(SearchKind::Tracks)); cx.notify(); })))),
            )
            .child(div().flex().flex_col().children(songs));
        let first = div()
            .w_full()
            .flex()
            .gap_6()
            .when(!side_by_side, |d| d.flex_col())
            .children(top.map(|el| div().when(side_by_side, |d| d.w(px(340.)).flex_none()).flex().flex_col().gap_3()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child("Melhor resultado"))
                .child(el)))
            .when(has_songs, |d| d.child(songs_el));
        let shelves = [
            (SearchKind::Artists, &found.artists),
            (SearchKind::Albums, &found.albums),
            (SearchKind::Playlists, &found.playlists),
            (SearchKind::Shows, &found.shows),
        ];
        let shelves: Vec<AnyElement> = shelves.into_iter().enumerate().filter(|(_, (_, cards))| !cards.is_empty())
            .map(|(i, (kind, cards))| self.shelf(SHELF_BASE + i, Shelf { title: label(kind).into(), cards: cards.clone() }, cx))
            .collect();
        div()
            .id("search-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(px(LIST_BOTTOM))
            .flex()
            .flex_col()
            .gap(px(28.))
            .child(first)
            .children(shelves)
            .into_any_element()
    }

    /// O melhor resultado: foto grande, nome e tipo. Com o mouse em cima aparece o tocar.
    fn top_card(&mut self, card: Card, cx: &mut Context<Self>) -> AnyElement {
        let round = card.kind == Kind::Artist;
        let art = match card.image.as_ref().and_then(|u| self.thumbs.get(u)).cloned() {
            Some(p) => {
                let image = img(p).size(px(112.)).flex_none().object_fit(ObjectFit::Cover);
                if round { image.rounded_full() } else { image.rounded(px(10.)) }.into_any_element()
            }
            None => div().size(px(112.)).flex_none().rounded(px(if round { 56. } else { 10. })).bg(rgba(theme::wash(0x0f))).flex().items_center().justify_center()
                .child(icon(match card.kind { Kind::Artist => "user-round", Kind::Show => "podcast", _ => "disc-3" }, 36., theme::ink_3()))
                .into_any_element(),
        };
        let kind = kind_label(card.kind);
        // O subtítulo do artista é "Artista", igual ao tipo.
        let subtitle = Some(card.subtitle.clone()).filter(|s| !s.is_empty() && s != kind);
        let (open, play) = (card.clone(), card.clone());
        Self::focusable("search-top")
            .group("search-top")
            .relative()
            .w_full()
            .p_5()
            .rounded(px(16.))
            .flex()
            .flex_col()
            .gap_4()
            .bg(rgba(theme::wash(0x0a)))
            .border_1()
            .border_color(rgba(theme::wash(0x12)))
            .hover(|s| s.bg(rgba(theme::wash(0x14))))
            .aria_label(SharedString::from(format!("Abrir {} {}", kind.to_lowercase(), card.title)))
            .on_click(cx.listener(move |t, _, _, cx| { t.go(Screen::Page(open.clone())); cx.notify(); }))
            .child(art)
            .child(
                div().flex().flex_col().gap_2()
                    .child(div().text_size(px(28.)).font_weight(FontWeight::BOLD).line_clamp(1).text_ellipsis().child(card.title.clone()))
                    .child(
                        div().flex().items_center().gap_2().min_w_0()
                            .child(div().flex_none().px_2().py_0p5().rounded_full().bg(rgba(theme::shade(0x4d))).text_xs().font_weight(FontWeight::SEMIBOLD).child(kind))
                            .children(subtitle.map(|s| div().min_w_0().text_sm().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(s))),
                    ),
            )
            .child(
                Self::focusable("search-top-play")
                    .absolute()
                    .right(px(20.))
                    .bottom(px(20.))
                    .size(px(48.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgba(theme::ink_1()))
                    .shadow_lg()
                    .opacity(0.)
                    .group_hover("search-top", |s| s.opacity(1.))
                    .aria_label(SharedString::from(format!("Tocar {}", card.title)))
                    .on_click(cx.listener(move |t, _, _, cx| {
                        t.remember_search();
                        t.play_context(&play);
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(icon("play", 20., theme::on_ink())),
            )
            .into_any_element()
    }

    fn search_message(title: impl Into<SharedString>, detail: String) -> gpui::Div {
        div()
            .pt_12()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(icon("search", 36., theme::ink_3()))
            .child(div().pt_2().text_lg().font_weight(FontWeight::BOLD).child(title.into()))
            .child(div().text_sm().text_color(rgba(theme::ink_2())).child(detail))
    }

    /// Ação discreta em texto ("Limpar", "Ver todas").
    pub(in crate::view) fn text_button(id: &'static str, text: &'static str) -> gpui::Stateful<gpui::Div> {
        Self::focusable(id)
            .px_2()
            .py_1()
            .rounded(px(6.))
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(theme::ink_2()))
            .hover(|s| s.text_color(rgba(theme::ink_1())).bg(rgba(theme::wash(0x0f))))
            .child(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(kind: Kind, title: &str) -> Card {
        Card { kind, id: title.into(), uri: String::new(), title: title.into(), subtitle: String::new(), image: None }
    }

    #[test]
    fn top_result_prefers_exact_name_then_artist() {
        let mut found = SearchResults { query: "Abbey Road".into(), ..Default::default() };
        found.artists = vec![card(Kind::Artist, "The Beatles")];
        found.albums = vec![card(Kind::Album, "Abbey Road")];
        assert_eq!(top_result(&found).unwrap().title, "Abbey Road");

        found.query = "beatles".into();
        assert_eq!(top_result(&found).unwrap().title, "The Beatles");

        found.query = "come together".into();
        assert_eq!(top_result(&found).unwrap().title, "The Beatles");

        found.artists.clear();
        assert_eq!(top_result(&found).unwrap().title, "Abbey Road");
    }
}
