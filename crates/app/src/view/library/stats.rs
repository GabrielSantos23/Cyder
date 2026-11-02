//! Estatísticas: um "Wrapped" do que tocou pelo Cyder neste computador, por mês, ano ou desde sempre.
use cyder_core::{Card, Cmd, Kind, stats::{Range, Ranked}};
use gpui::{AnyElement, Context, FontWeight, ObjectFit, ParentElement, Styled, div, img, prelude::*, px, relative, rgba};

use super::{Screen, local_offset_secs};
use crate::theme;
use crate::view::{Cyder, icon};

const MAX_W: f32 = 1100.;
const MONTHS: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];

/// "1.234" com ponto de milhar, como em português.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{} {many}", thousands(n as u64)) }
}

impl Cyder {
    pub(super) fn stats_view(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        if self.stats.is_none() && !self.stats_loading {
            self.stats_loading = true;
            self.send(Cmd::Stats { range: self.stats_range, offset_secs: local_offset_secs() });
        }
        let ranges = [(Range::Month, "Este mês"), (Range::Year, "Este ano"), (Range::All, "Desde sempre")];
        let chips: Vec<AnyElement> = ranges.into_iter().enumerate().map(|(i, (range, label))| {
            let on = self.stats_range == range;
            Self::focusable(("stats-range", i))
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
                .on_click(cx.listener(move |t, _, _, cx| {
                    if t.stats_range != range {
                        t.stats_range = range;
                        t.stats = None;
                        t.stats_loading = false;
                    }
                    cx.notify();
                }))
                .child(label)
                .into_any_element()
        }).collect();
        let header = div().pb_6().flex().flex_col().gap_4()
            .child(div().flex().flex_col().gap_0p5()
                .child(div().text_size(px(24.)).font_weight(FontWeight::BOLD).child("Estatísticas"))
                .child(div().text_sm().text_color(rgba(theme::ink_3())).child("O que você ouviu pelo Cyder neste computador.")))
            .child(div().flex().gap_2().children(chips));
        let body = match self.stats.clone() {
            None => div().pt_4().text_color(rgba(theme::ink_3())).child("Calculando…").into_any_element(),
            Some(s) if s.plays == 0 => Self::empty_state(
                if self.stats_range == Range::All { "Ainda não há nada aqui." } else { "Nada ouvido neste período ainda." },
                "Cada música ouvida por mais de 30 segundos no Cyder entra aqui.",
                Some(Self::primary_button("stats-home", "Escolher o que ouvir").on_click(cx.listener(|t, _, _, cx| {
                    t.go(super::Screen::Section(super::Section::Home));
                    cx.notify();
                }))),
            ),
            Some(s) => {
                let tile = |value: String, label: &'static str| {
                    div().flex_1().min_w_0().px_5().py_4().rounded(px(16.)).bg(rgba(theme::wash(0x0a))).border_1().border_color(rgba(theme::wash(0x12)))
                        .flex().flex_col().gap_1()
                        .child(div().text_size(px(30.)).line_height(px(36.)).font_weight(FontWeight::BOLD).child(value))
                        .child(div().text_sm().text_color(rgba(theme::ink_2())).child(label))
                };
                let tiles = div().flex().gap_4()
                    .child(tile(thousands(s.minutes), "minutos ouvidos"))
                    .child(tile(thousands(s.plays as u64), "audições"))
                    .child(tile(thousands(s.tracks as u64), "músicas diferentes"))
                    .child(tile(thousands(s.artists as u64), "artistas"));
                let wide = width >= 820.;
                let lists = div().flex().when(!wide, |d| d.flex_col()).gap_8()
                    .child(self.ranked_list("Artistas mais ouvidos", &s.top_artists, true, cx))
                    .child(self.ranked_list("Músicas mais ouvidas", &s.top_tracks, false, cx));
                let fact = s.top_hour.map(|h| {
                    let when = match h { 5..=11 => "de manhã", 12..=17 => "à tarde", 18..=23 => "à noite", _ => "de madrugada" };
                    div().flex().items_center().gap_3().px_5().py_4().rounded(px(16.)).bg(rgba(theme::wash(0x0a))).border_1().border_color(rgba(theme::wash(0x12)))
                        .child(icon("clock", 18., theme::ink_2()))
                        .child(div().text_sm().child(format!("Você ouve mais {when}, por volta das {h}h.")))
                });
                div().flex().flex_col().gap_8()
                    .child(tiles)
                    .children(self.timeline(&s.timeline))
                    .child(lists)
                    .children(fact)
                    .into_any_element()
            }
        };
        let handle = self.scroll_handle("stats");
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(super::super::scrollbar::scrolled(
                div().id("stats-scroll").flex_1().min_h_0().overflow_y_scroll().track_scroll(&handle).pb(px(super::LIST_BOTTOM))
                    .child(div().w_full().max_w(px(MAX_W)).child(header).child(body)),
                super::panel_bar("stats-bar", handle, false),
            ))
            .into_any_element()
    }

    /// Minutos por mês (no ano) ou por dia (no mês), em barras.
    fn timeline(&self, values: &[u64]) -> Option<AnyElement> {
        let max = values.iter().copied().max().filter(|m| *m > 0)?;
        let by_month = values.len() == 12;
        const H: f32 = 140.;
        let bars = values.iter().enumerate().map(|(i, &v)| {
            let label = if by_month { MONTHS[i].to_string() } else if (i + 1) % 5 == 0 || i == 0 { (i + 1).to_string() } else { String::new() };
            div().flex_1().min_w_0().h_full().flex().flex_col().justify_end().items_center().gap_1()
                .child(div().w_full().max_w(px(36.)).h(relative((v as f32 / max as f32).max(if v > 0 { 0.02 } else { 0. }) * 0.85)).rounded_t(px(4.))
                    .bg(rgba(if v > 0 { theme::ink_1() } else { theme::wash(0x14) })))
                .child(div().h(px(14.)).text_xs().text_color(rgba(theme::ink_3())).child(label))
        });
        Some(
            div().flex().flex_col().gap_3()
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(if by_month { "Minutos por mês" } else { "Minutos por dia" }))
                .child(div().h(px(H)).flex().items_end().gap(px(if by_month { 10. } else { 4. })).children(bars))
                .into_any_element(),
        )
    }

    fn ranked_list(&mut self, title: &'static str, rows: &[Ranked], artists: bool, cx: &mut Context<Self>) -> AnyElement {
        let items: Vec<AnyElement> = rows.iter().enumerate().map(|(i, r)| {
            let thumb = r.image.as_ref().and_then(|u| self.thumbs_small.get(u).or_else(|| self.thumbs.get(u))).cloned();
            let art = match thumb {
                Some(p) => img(p).size(px(40.)).flex_none().object_fit(ObjectFit::Cover).when(artists, |i| i.rounded_full()).when(!artists, |i| i.rounded(px(6.))).into_any_element(),
                None => div().size(px(40.)).flex_none().bg(rgba(theme::wash(0x0f))).when(artists, |d| d.rounded_full()).when(!artists, |d| d.rounded(px(6.))).into_any_element(),
            };
            let detail = if artists {
                format!("{} · {} min", plural(r.plays, "audição", "audições"), thousands(r.minutes))
            } else {
                format!("{} · {}", r.detail, plural(r.plays, "vez", "vezes"))
            };
            // Artista com id abre a página dele.
            let card = (artists && !r.id.is_empty()).then(|| Card { kind: Kind::Artist, id: r.id.clone(), uri: format!("spotify:artist:{}", r.id), title: r.name.clone(), subtitle: "Artista".into(), image: None });
            Self::focusable((if artists { "stats-artist" } else { "stats-track" }, i))
                .h(px(56.))
                .px_2()
                .rounded(px(10.))
                .flex()
                .items_center()
                .gap_3()
                .when(card.is_some(), |d| d.hover(|s| s.bg(rgba(theme::wash(0x0f)))))
                .when_some(card, |d, card| d.on_click(cx.listener(move |t, _, _, cx| { t.go(Screen::Page(card.clone())); cx.notify(); })))
                .child(div().w(px(22.)).flex_none().flex().justify_end().text_sm().font_weight(FontWeight::BOLD).text_color(rgba(theme::ink_3())).child((i + 1).to_string()))
                .child(art)
                .child(div().flex_1().min_w_0().flex().flex_col()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(r.name.clone()))
                    .child(div().text_xs().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(detail)))
                .into_any_element()
        }).collect();
        div().flex_1().min_w_0().flex().flex_col().gap_2()
            .child(div().text_lg().font_weight(FontWeight::BOLD).child(title))
            .child(div().flex().flex_col().children(items))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn numbers_in_portuguese() {
        assert_eq!(super::thousands(7), "7");
        assert_eq!(super::thousands(1234), "1.234");
        assert_eq!(super::thousands(1234567), "1.234.567");
        assert_eq!(super::plural(1, "vez", "vezes"), "1 vez");
    }
}
