//! Mini player: janela pequena por cima das outras com a capa, a faixa e os controles.
//! Lê tudo do `Cyder` (que avisa quando muda) e manda os cliques para ele.
use gpui::{
    AnyWindowHandle, App, Bounds, Context, Entity, FontWeight, MouseButton, ObjectFit, Render, Styled, Window, WindowBounds,
    WindowKind, WindowOptions, div, img, point, prelude::*, px, relative, rgba, size,
};

use super::{Cyder, Tip, icon};
use crate::theme;

const W: f32 = 380.;
const H: f32 = 84.;
/// Distância do canto da tela, acima da barra de tarefas.
const MARGIN: f32 = 24.;
const TASKBAR: f32 = 48.;

pub(super) struct MiniPlayer {
    cyder: Entity<Cyder>,
}

impl MiniPlayer {
    fn button(id: &'static str, label: &'static str, name: &'static str, size: f32) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(32.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .hover(|s| s.bg(rgba(theme::wash(0x1a))))
            .tip(label)
            // Não arrasta a janela ao clicar num botão.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(icon(name, size, theme::ink_1()))
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.cyder.read(cx);
        let track = c.tracks.get(c.current).cloned();
        let cover = track.as_ref().and_then(|t| t.cover_small.as_ref()).and_then(|u| c.thumbs.get(u)).cloned();
        let playing = c.playing;
        let progress = track.as_ref().map_or(0., |t| c.position() as f32 / t.duration_ms.max(1) as f32);
        let cyder = self.cyder.clone();
        let act = move |f: fn(&mut Cyder, &mut Context<Cyder>)| {
            let cyder = cyder.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| cyder.update(cx, |c, cx| { f(c, cx); cx.notify(); })
        };
        let (title, artist) = track.map_or(("Nada tocando".to_string(), String::new()), |t| (t.name, t.artist));
        div()
            .size_full()
            .relative()
            .flex()
            .items_center()
            .gap_3()
            .pl(px(12.))
            .pr_2()
            .rounded(px(14.))
            .overflow_hidden()
            .bg(rgba(theme::panel(0xff)))
            .border_1()
            .border_color(rgba(theme::wash(0x1f)))
            .text_color(rgba(theme::ink_1()))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .child(match cover {
                Some(p) => img(p).size(px(60.)).flex_none().rounded(px(8.)).object_fit(ObjectFit::Cover).into_any_element(),
                None => div().size(px(60.)).flex_none().rounded(px(8.)).bg(rgba(theme::wash(0x14))).flex().items_center().justify_center()
                    .child(icon("music", 22., theme::ink_3())).into_any_element(),
            })
            .child(
                div().flex_1().min_w_0().flex().flex_col()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(title))
                    .child(div().text_xs().text_color(rgba(theme::ink_2())).overflow_hidden().whitespace_nowrap().text_ellipsis().child(artist)),
            )
            .child(Self::button("mini-prev", "Anterior", "rewind", 15.).on_click(act(|c, _| c.prev_track())))
            .child(Self::button("mini-play", if playing { "Pausar" } else { "Tocar" }, if playing { "pause" } else { "play" }, 18.)
                .on_click(act(|c, _| c.toggle_play())))
            .child(Self::button("mini-next", "Próxima", "fast-forward", 15.).on_click(act(|c, _| c.next_track())))
            .child(Self::button("mini-expand", "Voltar à janela do Cyder", "maximize-2", 14.).on_click(act(|c, cx| {
                c.show_main();
                c.toggle_mini(cx);
            })))
            .child(div().absolute().left_0().bottom_0().h(px(3.)).w(relative(progress.clamp(0., 1.))).bg(rgba(theme::ink_1())))
    }
}

impl Cyder {
    /// Abre ou fecha o mini player. Fica para depois do update atual: a janela nova lê o `Cyder` ao
    /// renderizar (e fechar por dentro do mini mexeria na janela que está tratando o clique).
    pub(super) fn toggle_mini(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.mini.take() {
            cx.spawn(async move |_, cx| drop(handle.update(cx, |_, window, _| window.remove_window()))).detach();
            return;
        }
        let Some(display) = cx.primary_display() else { return };
        let screen = display.bounds();
        let origin = point(screen.origin.x + screen.size.width - px(W + MARGIN), screen.origin.y + screen.size.height - px(H + MARGIN + TASKBAR));
        let cyder = cx.entity();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin, size: size(px(W), px(H)) })),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_resizable: false,
            is_minimizable: false,
            focus: false,
            window_background: gpui::WindowBackgroundAppearance::Transparent,
            ..Default::default()
        };
        cx.spawn(async move |this, cx| {
            let opened = cx.update(|cx| cx.open_window(options, |window, cx| {
                window.set_rem_size(px(15.));
                cx.new(|cx| {
                    // Re-renderiza quando o player muda (faixa, play, o relógio da barra).
                    cx.observe(&cyder, |_, _, cx| cx.notify()).detach();
                    MiniPlayer { cyder }
                })
            }));
            match opened {
                Ok(handle) => drop(this.update(cx, |t, cx| { t.mini = Some(AnyWindowHandle::from(handle)); cx.notify(); })),
                Err(e) => log::warn!("mini player: {e}"),
            }
        })
        .detach();
    }

    /// Traz a janela principal de volta (escondida na bandeja ou minimizada).
    pub(super) fn show_main(&mut self) {
        #[cfg(windows)]
        super::tray::show(self.hwnd);
    }
}
