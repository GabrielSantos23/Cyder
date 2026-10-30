//! Músicas curtidas: cabeçalho como o das playlists, busca dentro da lista, ordem e modo de ver
//! (como no Spotify) e o resto das curtidas vindo aos poucos enquanto rola.
use cyder_core::{Cmd, Track};
use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, ListAlignment, ListState, MouseButton, MouseDownEvent, ObjectFit, ParentElement,
    SharedString, Styled, Window, div, img, linear_color_stop, linear_gradient, list, prelude::*, px, rgba,
};

use super::{LIKED_COLOR, LIST_BOTTOM, ROW_H, TrackSource, fmt, icon, index_cell};
use crate::theme;
use crate::view::{Cyder, Tip};
use crate::view::menus::MenuTarget;

/// O Spotify devolve as curtidas de 200 em 200 (o mesmo tamanho da lista guardada).
pub(in crate::view) const LIKED_PAGE: usize = 200;
const COMPACT_H: f32 = 36.;
const LIST_KEY: &str = "liked";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::view) enum LikedSort {
    Added,
    Title,
    Artist,
    Album,
}

impl LikedSort {
    pub(in crate::view) const ALL: [LikedSort; 4] = [LikedSort::Title, LikedSort::Added, LikedSort::Artist, LikedSort::Album];

    pub(in crate::view) fn label(self) -> &'static str {
        match self {
            LikedSort::Added => "Adicionadas recentemente",
            LikedSort::Title => "Título",
            LikedSort::Artist => "Artista",
            LikedSort::Album => "Álbum",
        }
    }
}

/// Ordem e busca da tela; `reversed` inverte (as mais antigas primeiro, Z a A).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::view) struct LikedOrder {
    pub sort: LikedSort,
    pub reversed: bool,
    /// Linhas baixas, sem capa, com artista e álbum em colunas.
    pub compact: bool,
}

impl Default for LikedOrder {
    fn default() -> Self {
        LikedOrder { sort: LikedSort::Added, reversed: false, compact: false }
    }
}

/// As curtidas como a tela mostra: filtradas pela busca e na ordem escolhida.
fn arrange(liked: &[Track], query: &str, order: LikedOrder) -> Vec<Track> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<Track> = liked.iter()
        .filter(|t| q.is_empty() || [&t.name, &t.artist, &t.album].iter().any(|f| f.to_lowercase().contains(&q)))
        .cloned()
        .collect();
    let key = |t: &Track| match order.sort {
        LikedSort::Added => String::new(),
        LikedSort::Title => t.name.to_lowercase(),
        LikedSort::Artist => t.artist.to_lowercase(),
        LikedSort::Album => t.album.to_lowercase(),
    };
    // Estável: no empate (mesmo artista), fica a ordem de quando foi curtida.
    if order.sort != LikedSort::Added {
        out.sort_by_cached_key(key);
    }
    if order.reversed {
        out.reverse();
    }
    out
}

impl Cyder {
    /// Refaz a lista mostrada; chamar sempre que as curtidas, a busca ou a ordem mudam.
    pub(in crate::view) fn liked_changed(&mut self) {
        self.liked_view = arrange(&self.liked, &self.liked_query, self.liked_order);
    }

    /// Mais curtidas antigas chegaram (ou falharam).
    pub(in crate::view) fn liked_more(&mut self, tracks: Vec<Track>, more: bool, error: Option<String>) {
        self.liked_loading = false;
        self.liked_error = error;
        if self.liked_error.is_none() {
            self.liked_has_more = more;
        }
        for t in tracks {
            if !self.liked.iter().any(|l| l.uri == t.uri) {
                self.liked.push(t);
            }
        }
        self.liked_changed();
    }

    fn load_more_liked(&mut self) {
        if self.liked_has_more && !self.liked_loading && self.liked_error.is_none() {
            self.liked_loading = true;
            self.send(Cmd::LoadMoreLiked { from: self.liked.len() });
        }
    }

    pub(super) fn liked_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Curtidas vazias antes da biblioteca chegar: ainda carregando.
        if self.liked.is_empty() && self.library.is_none() {
            return crate::view::loading::tracks(true);
        }
        let rows = self.liked_view.len();
        let searching = !self.liked_query.trim().is_empty();
        // Cabeçalho, as linhas (ou o aviso de vazio), o rodapé de "carregando mais" e o respiro embaixo do player.
        let count = rows.max(1) + 3;
        let state = self.page_lists.entry(LIST_KEY.into()).or_insert_with(|| ListState::new(count, ListAlignment::Top, px(600.))).clone();
        super::resize_list(&state, 1, 1, count);
        let focused = self.liked_search_focus.is_focused(window);
        list(state, cx.processor(move |this, i: usize, _window, cx| {
            if i == 0 {
                let compact = this.liked_order.compact;
                let columns = super::column_header(if compact { &["Artista", "Álbum"] } else { &["Álbum"] }, !compact);
                // w_full: sem isto o item da lista encolhe para o tamanho do conteúdo e as colunas ficam espremidas.
                return div().w_full().flex().flex_col().child(this.liked_header(focused, cx)).child(columns).into_any_element();
            }
            if i == count - 1 {
                return div().h(px(LIST_BOTTOM)).into_any_element();
            }
            if i == count - 2 {
                return this.liked_footer(cx);
            }
            if rows == 0 {
                let (title, detail) = if searching {
                    ("Nenhuma música curtida com isso.", "Confira a grafia ou busque por outro nome, artista ou álbum.")
                } else {
                    ("Você ainda não curtiu nenhuma música.", "Toque no coração de uma música para ela aparecer aqui.")
                };
                return div().px(px(12.)).child(Self::empty_state(title, detail, None)).into_any_element();
            }
            // Perto do fim do que já veio, pede mais. Buscando, não: a busca é no que já está aqui.
            if !searching && i + 40 >= rows {
                this.load_more_liked();
            }
            this.liked_row(i - 1, cx)
        }))
        .flex_1()
        .min_h_0()
        // O fundo da linha sai da coluna; o texto fica alinhado com o resto da tela.
        .mx(px(-12.))
        .into_any_element()
    }

    fn liked_header(&mut self, search_focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let n = self.liked.len();
        let count = match (n, self.liked_has_more) {
            (1, false) => "1 música".to_string(),
            (n, false) => format!("{n} músicas"),
            (n, true) => format!("{n}+ músicas"),
        };
        let art = div()
            .size(px(240.))
            .flex_none()
            .rounded(px(14.))
            .shadow_2xl()
            .bg(linear_gradient(135., linear_color_stop(rgba((LIKED_COLOR << 8) | 0xff), 0.), linear_color_stop(rgba(0x9fd6ffff), 1.)))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("heart-filled", 84., theme::ink_1()));
        let (name, image) = self.profile.clone().unwrap_or_else(|| ("Você".into(), None));
        let avatar = match image.and_then(|u| self.thumbs_small.get(&u).cloned()) {
            Some(p) => img(p).size(px(26.)).flex_none().rounded_full().object_fit(ObjectFit::Cover).into_any_element(),
            None => div().size(px(26.)).flex_none().rounded_full().bg(rgba(theme::wash(0x1f))).flex().items_center().justify_center()
                .child(icon("user-round", 14., theme::ink_2())).into_any_element(),
        };
        let chip = div().flex().items_center().gap_1p5().px_2p5().py_1().rounded_full().border_1().border_color(rgba(theme::wash(0x26)))
            .text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2()))
            .child(icon("list-music", 12., theme::ink_2())).child(count);
        let pill = |id: &'static str, label: &'static str, icon_name: &'static str| {
            Self::focusable(id)
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
        let here = self.is_current(None);
        let pause = here && self.playing;
        let empty = self.liked_view.is_empty();
        let play = pill("liked-play", if pause { "Pausar" } else { "Tocar" }, if pause { "pause" } else { "play" })
            .when(empty && !here, |d| d.opacity(0.4))
            .on_click(cx.listener(move |t, _, _, cx| {
                if here {
                    t.send(Cmd::Toggle);
                } else if !t.liked_view.is_empty() {
                    t.play_from(&TrackSource::Liked, 0);
                }
                cx.notify();
            }));
        let shuffle = pill("liked-shuffle", "Aleatório", "shuffle")
            .when(empty, |d| d.opacity(0.4))
            .on_click(cx.listener(|t, _, _, cx| {
                if !t.liked_view.is_empty() {
                    t.shuffle_from(&TrackSource::Liked);
                }
                cx.notify();
            }));
        div()
            .w_full()
            .px(px(12.))
            .pt_2()
            .pb_6()
            .flex()
            .items_center()
            .gap_8()
            .child(art)
            .child(
                div().flex_1().min_w_0().flex().flex_col().gap_2()
                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child("PLAYLIST"))
                    .child(div().text_size(px(32.)).line_height(px(38.)).font_weight(FontWeight::BOLD).child("Músicas curtidas"))
                    .child(div().flex().items_center().gap_2().child(avatar).child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(name)))
                    .child(div().flex().child(chip))
                    .child(div().mt_3().flex().items_center().gap_3()
                        .child(play)
                        .child(shuffle)
                        .child(div().flex_1())
                        .child(self.liked_search(search_focused, cx))
                        .child(self.liked_sort_button(cx))),
            )
            .into_any_element()
    }

    /// Lupa que vira campo, como no Spotify: busca no nome, no artista e no álbum.
    fn liked_search(&mut self, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        if !self.liked_search_open {
            return Self::focusable("liked-search")
                .size(px(36.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(rgba(theme::wash(0x1a))))
                .tip("Buscar nas músicas curtidas")
                .on_click(cx.listener(|t, _, window, cx| {
                    t.liked_search_open = true;
                    window.focus(&t.liked_search_focus, cx);
                    cx.notify();
                }))
                .child(icon("search", 17., theme::ink_2()))
                .into_any_element();
        }
        let caret = || div().flex_none().w(px(1.5)).h(px(16.)).bg(rgba(theme::ink_1()));
        let text = if self.liked_query.is_empty() {
            div().flex_1().min_w_0().flex().items_center().when(focused, |d| d.child(caret())).child(div().text_color(rgba(theme::ink_3())).child("Buscar nas curtidas"))
        } else {
            div().flex_1().min_w_0().flex().items_center().overflow_hidden().whitespace_nowrap().child(self.field_text(self.liked_query.clone(), focused)).when(focused, |d| d.child(caret()))
        };
        div()
            .id("liked-search-field")
            .key_context("TextField")
            .track_focus(&self.liked_search_focus)
            .aria_label("Buscar nas músicas curtidas")
            .w(px(240.))
            .h(px(36.))
            .pl_3()
            .pr_1()
            .flex()
            .items_center()
            .gap_2()
            .rounded_full()
            .bg(rgba(theme::shade(0x40)))
            .border_1()
            .border_color(rgba(if focused { theme::ink_3() } else { theme::wash(0x1f) }))
            .text_sm()
            .cursor_text()
            .on_click(cx.listener(|t, e: &gpui::ClickEvent, window, cx| { t.text_selected = e.click_count() >= 2; window.focus(&t.liked_search_focus, cx); cx.notify(); }))
            .on_key_down(cx.listener(|t, e: &gpui::KeyDownEvent, window, cx| t.liked_search_key(e, window, cx)))
            .child(icon("search", 15., theme::ink_3()))
            .child(text)
            .child(
                Self::focusable("liked-search-close")
                    .size(px(28.))
                    .flex_none()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(rgba(theme::wash(0x1a))))
                    .tip("Fechar a busca")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|t, _, window, cx| { t.close_liked_search(window, cx); cx.notify(); }))
                    .child(icon("x", 14., theme::ink_2())),
            )
            .into_any_element()
    }

    fn close_liked_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.liked_search_open = false;
        self.liked_query.clear();
        self.liked_changed();
        window.focus(&self.focus, cx);
    }

    fn liked_search_key(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        match k.key.as_str() {
            "escape" => self.close_liked_search(window, cx),
            "enter" => {}
            _ if crate::view::type_into(&mut self.liked_query, k, &mut self.text_selected, cx) => self.liked_changed(),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// "Adicionadas recentemente ☰": abre o menu de ordem e de modo de ver.
    fn liked_sort_button(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let o = self.liked_order;
        Self::focusable("liked-sort")
            .h(px(36.))
            .pl_3()
            .pr_2()
            .rounded_full()
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(theme::ink_2()))
            .hover(|s| s.bg(rgba(theme::wash(0x1a))).text_color(rgba(theme::ink_1())))
            .aria_label(SharedString::from(format!("Ordem: {}. Mudar ordem e modo de ver", o.sort.label())))
            .on_click(cx.listener(|t, e: &ClickEvent, _, cx| { t.open_menu(MenuTarget::LikedSort, e.position()); cx.notify(); }))
            .child(o.sort.label())
            .child(icon(if o.compact { "menu" } else { "list" }, 16., theme::ink_2()))
            .into_any_element()
    }

    fn liked_footer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let base = div().px(px(12.)).py_3().text_sm().text_color(rgba(theme::ink_3()));
        if let Some(e) = self.liked_error.clone() {
            return base.flex().items_center().gap_3()
                .child(div().flex_1().min_w_0().child(format!("Não deu para carregar mais músicas. {e}")))
                .child(Self::secondary_button("liked-retry", "Tentar de novo").on_click(cx.listener(|t, _, _, cx| {
                    t.liked_error = None;
                    t.load_more_liked();
                    cx.notify();
                })))
                .into_any_element();
        }
        if self.liked_loading {
            return base.child("Carregando mais músicas…").into_any_element();
        }
        div().into_any_element()
    }

    /// Linha zebrada como nas playlists. Compacta: sem capa, mais baixa, artista em coluna.
    fn liked_row(&mut self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(t) = self.liked_view.get(i).cloned() else { return div().into_any_element() };
        let accent = self.accent_ink();
        let compact = self.liked_order.compact;
        let now = self.playing_uri() == Some(t.uri.as_str());
        let thumb = t.cover_small.as_ref().and_then(|u| self.thumbs.get(u)).cloned();
        let menu_track = t.clone();
        let playing = self.playing;
        let column = |text: String| div().flex_1().min_w_0().text_sm().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(text);
        let name = t.name.clone();
        let title = move || div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().when(now, |d| d.text_color(accent)).child(name.clone());
        Self::focusable(("liked-track", i))
            .group("track-row")
            .w_full()
            .h(px(if compact { COMPACT_H } else { ROW_H }))
            .px_3()
            .rounded(px(if compact { 8. } else { 10. }))
            .flex()
            .items_center()
            .gap_3()
            .when(i % 2 == 0 && !now, |d| d.bg(rgba(theme::wash(0x0a))))
            .when(now, |d| d.bg(rgba(theme::wash(0x1f))))
            .hover(|s| s.bg(rgba(theme::wash(0x17))))
            .aria_label(SharedString::from(format!("Tocar {} – {}", t.name, t.artist)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |t, e: &MouseDownEvent, _, cx| { t.open_menu(MenuTarget::Track(menu_track.clone()), e.position); cx.notify(); }))
            .on_click(cx.listener(move |this, _, _, cx| { if now { this.toggle_play() } else { this.play_from(&TrackSource::Liked, i) } cx.notify(); }))
            .child(index_cell(i, now, playing, "track-row", accent))
            .when(!compact, |d| d
                .child(match thumb {
                    Some(p) => img(p).size(px(36.)).flex_none().rounded(px(6.)).object_fit(ObjectFit::Cover).into_any_element(),
                    None => div().size(px(36.)).flex_none().rounded(px(6.)).bg(rgba(theme::wash(0x0f))).into_any_element(),
                })
                .child(div().flex_1().min_w_0().flex().flex_col().child(title())
                    .child(div().text_xs().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(t.artist.clone()))))
            .when(compact, |d| d.child(div().flex_1().min_w_0().child(title())).child(column(t.artist.clone())))
            .child(column(t.album.clone()))
            .child(self.heart_button(("liked-heart", i), "track-row", &t, cx))
            .child(div().w(px(44.)).flex().justify_end().text_xs().text_color(rgba(theme::ink_3())).child(fmt(t.duration_ms)))
            .child(Self::more_button(("liked-more", i), "track-row", 28., MenuTarget::Track(t.clone()), cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{LikedOrder, LikedSort, arrange};
    use cyder_core::Track;

    fn track(name: &str, artist: &str) -> Track {
        Track { id: name.into(), uri: format!("spotify:track:{name}"), name: name.into(), artist: artist.into(), album: "Disco".into(), album_id: String::new(), duration_ms: 1, cover_small: None, cover_large: None, artists: Vec::new() }
    }

    #[test]
    fn sorts_and_filters() {
        let liked = [track("b", "Zé"), track("a", "Ana"), track("c", "ana")];
        let names = |v: Vec<Track>| v.into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert_eq!(names(arrange(&liked, "", LikedOrder::default())), ["b", "a", "c"]);
        assert_eq!(names(arrange(&liked, "", LikedOrder { reversed: true, ..Default::default() })), ["c", "a", "b"]);
        assert_eq!(names(arrange(&liked, "", LikedOrder { sort: LikedSort::Title, ..Default::default() })), ["a", "b", "c"]);
        // Mesmo artista (sem diferenciar maiúsculas): fica a ordem de curtida.
        assert_eq!(names(arrange(&liked, "", LikedOrder { sort: LikedSort::Artist, ..Default::default() })), ["a", "c", "b"]);
        assert_eq!(names(arrange(&liked, " ANA ", LikedOrder::default())), ["a", "c"]);
    }
}
