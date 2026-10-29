//! Novidades: álbuns e singles recentes dos artistas que você segue, do mais novo para o mais velho.
use cyder_core::{Card, Cmd, NewRelease};
use gpui::{AnyElement, Context, FontWeight, ParentElement, Styled, div, prelude::*, px, rgba};

use super::{CARD_MIN, GRID_GAP, CardSource, Releases};
use crate::theme;
use crate::view::{Cyder, loading};

/// "hoje", "há 3 dias", "há 2 semanas", "há 1 mês".
fn ago(date: i64, now: i64) -> String {
    let days = (now - date).max(0) / 86_400;
    match days {
        0 => "hoje".into(),
        1 => "ontem".into(),
        2..=6 => format!("há {days} dias"),
        7..=13 => "há 1 semana".into(),
        14..=29 => format!("há {} semanas", days / 7),
        30..=59 => "há 1 mês".into(),
        _ => format!("há {} meses", days / 30),
    }
}

/// Cartões das Novidades: todos, só álbuns, só singles e EPs.
pub(in crate::view) fn cards(list: &[NewRelease]) -> [Vec<Card>; 3] {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let card = |r: &NewRelease| Card { subtitle: format!("{} · {}", r.card.subtitle, ago(r.date, now)), ..r.card.clone() };
    [
        list.iter().map(card).collect(),
        list.iter().filter(|r| !r.single).map(card).collect(),
        list.iter().filter(|r| r.single).map(card).collect(),
    ]
}

impl Cyder {
    /// Pede as Novidades na primeira vez que a tela abre.
    pub(super) fn load_news(&mut self) {
        if self.news.is_none() && !self.news_loading {
            self.news_loading = true;
            self.news_error = None;
            self.send(Cmd::NewReleases { force: false });
        }
    }

    pub(super) fn news_view(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        self.load_news();
        let counts = self.news.as_ref().map(|n| [n[0].len(), n[1].len(), n[2].len()]);
        let filters = [(Releases::All, "Tudo"), (Releases::Albums, "Álbuns"), (Releases::Singles, "Singles e EPs")];
        let chips: Vec<AnyElement> = filters.into_iter().enumerate().map(|(i, (filter, label))| {
            let on = self.release_filter == filter;
            Self::focusable(("news-filter", i))
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
        let refresh = Self::secondary_button("news-refresh", if self.news_loading { "Atualizando…" } else { "Atualizar" })
            .on_click(cx.listener(|t, _, _, cx| {
                if !t.news_loading {
                    t.news_loading = true;
                    t.news_error = None;
                    t.send(Cmd::NewReleases { force: true });
                }
                cx.notify();
            }));
        let subtitle = match counts {
            Some([n, ..]) if n > 0 => format!("{n} lançamentos dos últimos 4 meses de quem você segue"),
            _ => "Lançamentos dos últimos 4 meses de quem você segue".into(),
        };
        let header = div()
            .pb_4()
            .flex()
            .items_end()
            .gap_4()
            .child(
                div().flex_1().flex().flex_col().gap_0p5()
                    .child(div().text_size(px(24.)).font_weight(FontWeight::BOLD).child("Novidades"))
                    .child(div().text_sm().text_color(rgba(theme::ink_3())).child(subtitle)),
            )
            .child(refresh);
        let body = match (&self.news, &self.news_error) {
            (_, Some(e)) => div().pt_8().flex().flex_col().gap_1()
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Não deu para carregar as novidades."))
                .child(div().text_sm().text_color(rgba(theme::ink_3())).child(e.clone()))
                .into_any_element(),
            (None, _) => {
                let cols = (((width + GRID_GAP) / (CARD_MIN + GRID_GAP)).floor() as usize).max(2);
                loading::grid(cols, (width - GRID_GAP * (cols - 1) as f32) / cols as f32, false)
            }
            (Some(n), _) if n[0].is_empty() => {
                if self.library.as_ref().is_some_and(|l| l.artists.is_empty()) {
                    Self::empty_state(
                        "Você ainda não segue nenhum artista.",
                        "Siga artistas para ver os lançamentos deles aqui.",
                        Some(Self::primary_button("news-search", "Buscar artistas").on_click(cx.listener(|t, _, window, cx| t.open_search(window, cx)))),
                    )
                } else {
                    Self::empty_state("Nada novo por enquanto.", "Nenhum lançamento de quem você segue nos últimos 4 meses.", None)
                }
            }
            (Some(_), _) => self.card_grid(CardSource::News, width, cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            // Com um tipo só, os filtros não ajudam.
            .when(counts.is_some_and(|[_, a, s]| a > 0 && s > 0), |d| d.child(div().pb_4().flex().gap_2().children(chips)))
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn relative_dates() {
        let day = 86_400;
        assert_eq!(super::ago(10 * day, 10 * day + 5), "hoje");
        assert_eq!(super::ago(0, day), "ontem");
        assert_eq!(super::ago(0, 3 * day), "há 3 dias");
        assert_eq!(super::ago(0, 15 * day), "há 2 semanas");
        assert_eq!(super::ago(0, 95 * day), "há 3 meses");
    }
}
