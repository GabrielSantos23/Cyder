//! Novidades: álbuns e singles recentes dos artistas que você segue, do mais novo para o mais velho.
use cyder_core::{i18n::t, tr};
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
        2..=6 => tr!("há {days} dias", "{days} days ago"),
        7..=13 => t("há 1 semana").into(),
        14..=29 => tr!("há {} semanas", "{} weeks ago", days / 7),
        30..=59 => t("há 1 mês").into(),
        _ => tr!("há {} meses", "{} months ago", days / 30),
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
    /// Pede as Novidades na primeira vez que a tela abre. Depois de um erro, só pelo "Tentar de novo": pedir de
    /// novo a cada render repetia o pedido sem parar e a tela nunca saía do esqueleto.
    pub(super) fn load_news(&mut self) {
        if self.news.is_none() && !self.news_loading && self.news_error.is_none() {
            self.news_loading = true;
            self.news_error = None;
            self.send(Cmd::NewReleases { force: false });
        }
    }

    pub(super) fn news_view(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        self.load_news();
        let counts = self.news.as_ref().map(|n| [n[0].len(), n[1].len(), n[2].len()]);
        let filters = [(Releases::All, t("Tudo")), (Releases::Albums, t("Álbuns")), (Releases::Singles, t("Singles e EPs"))];
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
        let refresh = Self::secondary_button("news-refresh", if self.news_loading { t("Atualizando…") } else { t("Atualizar") })
            .on_click(cx.listener(|t, _, _, cx| {
                if !t.news_loading {
                    t.news_loading = true;
                    t.news_error = None;
                    t.send(Cmd::NewReleases { force: true });
                }
                cx.notify();
            }));
        let subtitle = match counts {
            Some([n, ..]) if n > 0 => tr!("{n} lançamentos dos últimos 4 meses de quem você segue", "{n} releases from the last 4 months by artists you follow"),
            _ => t("Lançamentos dos últimos 4 meses de quem você segue").into(),
        };
        let header = div()
            .pb_4()
            .flex()
            .items_end()
            .gap_4()
            .child(
                div().flex_1().flex().flex_col().gap_0p5()
                    .child(div().text_size(px(28.)).font_weight(FontWeight::EXTRA_BOLD).child(t("Novidades")))
                    .child(div().text_sm().text_color(rgba(theme::ink_3())).child(subtitle)),
            )
            .child(refresh);
        let body = match (&self.news, &self.news_error) {
            (_, Some(e)) => {
                let e = e.clone();
                self.error_state("news-retry", t("Não deu para carregar as novidades"), &e, |t, _| {
                    t.news_error = None;
                    t.news_loading = true;
                    t.send(Cmd::NewReleases { force: true });
                }, cx)
            }
            (None, _) => {
                let cols = (((width + GRID_GAP) / (CARD_MIN + GRID_GAP)).floor() as usize).max(2);
                loading::grid(cols, (width - GRID_GAP * (cols - 1) as f32) / cols as f32, false)
            }
            (Some(n), _) if n[0].is_empty() => {
                if self.library.as_ref().is_some_and(|l| l.artists.is_empty()) {
                    Self::empty_state(
                        t("Você ainda não segue nenhum artista."),
                        t("Siga artistas para ver os lançamentos deles aqui."),
                        Some(Self::primary_button("news-search", t("Buscar artistas")).on_click(cx.listener(|t, _, window, cx| t.open_search(window, cx)))),
                    )
                } else {
                    Self::empty_state(t("Nada novo por enquanto."), t("Nenhum lançamento de quem você segue nos últimos 4 meses."), None)
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
