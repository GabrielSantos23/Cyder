//! Esqueletos de carregamento: o formato da tela que vai chegar, pulsando de leve.
//! Só animam enquanto estão na tela, então não custam nada depois que o conteúdo chega.
use std::time::Duration;

use gpui::{Animation, AnimationExt, AnyElement, IntoElement, ParentElement, Styled, div, prelude::*, px, rgba};
use crate::theme;

/// Bloco cinza arredondado.
fn block(w: Option<f32>, h: f32, r: f32) -> gpui::Div {
    let d = div().h(px(h)).rounded(px(r)).bg(rgba(theme::wash(0x14)));
    match w {
        Some(w) => d.w(px(w)).flex_none(),
        None => d.w_full(),
    }
}

/// Pulsa a opacidade de todo o esqueleto, num ciclo lento.
fn pulse(id: &'static str, el: gpui::Div) -> AnyElement {
    el.with_animation(id, Animation::new(Duration::from_millis(1400)).repeat(), |d, t| {
        // Sobe e desce: 0 → 1 → 0.
        let wave = 1. - (t * 2. - 1.).abs();
        d.opacity(0.45 + 0.4 * wave)
    })
    .into_any_element()
}

/// Uma linha de faixa: número, capa (opcional), título e artista, duração.
fn track_row(cover: bool, i: usize) -> gpui::Div {
    let widths = [0.42, 0.3, 0.5, 0.36, 0.46, 0.28];
    let w = widths[i % widths.len()];
    div()
        .h(px(48.))
        .px_3()
        .flex()
        .items_center()
        .gap_3()
        .child(block(Some(18.), 10., 4.))
        .when(cover, |d| d.child(block(Some(36.), 36., 6.)))
        .child(div().flex_1().flex().flex_col().gap_1p5()
            .child(div().w(gpui::relative(w)).child(block(None, 11., 4.)))
            .child(div().w(gpui::relative(w * 0.6)).child(block(None, 9., 4.))))
        .child(block(Some(32.), 10., 4.))
}

/// Tela inicial: título e três prateleiras de cartões.
pub fn home(card: f32) -> AnyElement {
    let shelf = |n: usize| {
        div().flex().flex_col().gap_3()
            .child(block(Some(180.), 16., 6.))
            .child(div().flex().gap_4().overflow_hidden().children((0..n).map(|_| {
                div().flex().flex_col().gap_2().child(block(Some(card), card, 12.)).child(block(Some(card * 0.7), 11., 4.)).child(block(Some(card * 0.45), 9., 4.))
            })))
    };
    pulse("loading-home", div().flex().flex_col().gap(px(28.)).pt_2().child(shelf(6)).child(shelf(6)).child(shelf(6)))
}

/// Grade de álbuns, artistas ou playlists.
pub fn grid(cols: usize, card: f32, round: bool) -> AnyElement {
    let row = || {
        div().flex().gap(px(20.)).children((0..cols).map(|_| {
            div().flex().flex_col().gap_2()
                .when(round, |d| d.items_center())
                .child(block(Some(card), card, if round { card / 2. } else { 12. }))
                .child(block(Some(card * 0.7), 11., 4.))
                .child(block(Some(card * 0.4), 9., 4.))
        }))
    };
    pulse("loading-grid", div().flex().flex_col().gap(px(20.)).child(row()).child(row()))
}

/// Lista de faixas (Músicas).
pub fn tracks(cover: bool) -> AnyElement {
    pulse("loading-tracks", div().flex().flex_col().children((0..10).map(|i| track_row(cover, i))))
}

/// Álbum ou playlist: capa grande, título, chips, botões e as faixas.
pub fn media_page(playlist: bool) -> AnyElement {
    pulse(
        "loading-media",
        div()
            .flex()
            .flex_col()
            .child(
                div().pt_2().pb_8().flex().items_center().gap_8()
                    .child(block(Some(220.), 220., 14.))
                    .child(div().flex_1().flex().flex_col().gap_3()
                        .child(block(Some(70.), 10., 4.))
                        .child(block(Some(320.), 30., 8.))
                        .child(block(Some(150.), 14., 6.))
                        .child(div().flex().gap_2().child(block(Some(70.), 24., 12.)).child(block(Some(80.), 24., 12.)).child(block(Some(70.), 24., 12.)))
                        .child(div().mt_2().flex().gap_3().child(block(Some(100.), 34., 17.)).child(block(Some(120.), 34., 17.)))),
            )
            .children((0..8).map(|i| track_row(playlist, i))),
    )
}

/// Artista: a foto grande, o nome e as linhas de destaque.
pub fn artist(hero_h: f32, pad: f32) -> AnyElement {
    pulse(
        "loading-artist",
        div()
            .flex()
            .flex_col()
            .child(
                div().relative().w_full().h(px(hero_h)).bg(rgba(theme::wash(0x0a)))
                    .child(div().absolute().left(px(pad)).bottom(px(24.)).flex().flex_col().gap_4()
                        .child(block(Some(300.), 40., 10.))
                        .child(div().flex().gap_3().child(block(Some(52.), 52., 26.)).child(block(Some(40.), 40., 20.)))),
            )
            .child(div().px(px(pad)).pt_6().flex().gap_8()
                .child(div().w(px(280.)).flex().flex_col().gap_3().child(block(Some(180.), 16., 6.)).child(div().flex().gap_4().child(block(Some(110.), 110., 10.)).child(div().flex_1().flex().flex_col().gap_2().child(block(None, 10., 4.)).child(block(None, 14., 4.)).child(block(Some(80.), 10., 4.)))))
                .child(div().flex_1().flex().flex_col().gap_1().child(block(Some(140.), 16., 6.)).children((0..4).map(|i| track_row(true, i))))),
    )
}

/// Letra: linhas largas de tamanhos diferentes.
pub fn lyrics(line_h: f32, pad_x: f32) -> AnyElement {
    let widths = [0.7, 0.55, 0.8, 0.45, 0.65, 0.5];
    pulse(
        "loading-lyrics",
        div().size_full().px(px(pad_x)).pt(px(line_h * 2.)).flex().flex_col().gap(px(line_h * 0.6))
            .children(widths.iter().map(|w| div().w(gpui::relative(*w)).child(block(None, line_h * 0.7, 8.)))),
    )
}
