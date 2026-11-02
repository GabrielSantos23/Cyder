//! Barra de rolagem fina por cima das listas, e o autoscroll do botão do meio, como no Windows e no Chrome:
//! o clique do meio deixa uma âncora, e a lista rola sozinha mais rápido quanto mais longe o mouse fica dela.
//! Um novo clique ou o Esc encerra.
use std::time::Instant;

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Div, ElementId, Hitbox, HitboxBehavior, IntoElement, ListState, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, ScrollHandle, ScrollWheelEvent, Styled, Subscription, TransformationMatrix,
    UniformListScrollHandle, Window, canvas, div, point, px, quad, rgba, size,
};

use crate::theme;

/// Largura da faixa que pega o mouse; o polegar desenhado é mais fino.
const HIT_W: f32 = 12.;
const MIN_THUMB: f32 = 32.;
/// Raio da âncora em que o autoscroll fica parado.
const DEAD: f32 = 12.;
/// Pixels por segundo a cada pixel de distância da âncora.
const SPEED: f32 = 8.;
const MAX_SPEED: f32 = 6000.;

/// O que a barra lê e move. `scrolled` é quanto já desceu (positivo) e vai de 0 a `max`.
pub(super) trait Scrolls: Clone + 'static {
    fn area(&self) -> Bounds<Pixels>;
    fn max(&self) -> f32;
    fn scrolled(&self) -> f32;
    fn scroll_to(&self, y: f32);
    fn dragging(&self, _on: bool) {}
}

impl Scrolls for ScrollHandle {
    fn area(&self) -> Bounds<Pixels> {
        self.bounds()
    }
    fn max(&self) -> f32 {
        self.max_offset().y.into()
    }
    fn scrolled(&self) -> f32 {
        -f32::from(self.offset().y)
    }
    fn scroll_to(&self, y: f32) {
        self.set_offset(point(self.offset().x, px(-y.clamp(0., self.max()))));
    }
}

impl Scrolls for UniformListScrollHandle {
    fn area(&self) -> Bounds<Pixels> {
        self.0.borrow().base_handle.area()
    }
    fn max(&self) -> f32 {
        self.0.borrow().base_handle.max()
    }
    fn scrolled(&self) -> f32 {
        self.0.borrow().base_handle.scrolled()
    }
    fn scroll_to(&self, y: f32) {
        let base = self.0.borrow().base_handle.clone();
        base.scroll_to(y);
    }
}

impl Scrolls for ListState {
    fn area(&self) -> Bounds<Pixels> {
        self.viewport_bounds()
    }
    fn max(&self) -> f32 {
        self.max_offset_for_scrollbar().y.into()
    }
    fn scrolled(&self) -> f32 {
        -f32::from(self.scroll_px_offset_for_scrollbar().y)
    }
    fn scroll_to(&self, y: f32) {
        self.set_offset_from_scrollbar(point(px(0.), px(-y)));
    }
    fn dragging(&self, on: bool) {
        if on { self.scrollbar_drag_started() } else { self.scrollbar_drag_ended() }
    }
}

#[derive(Default)]
struct State {
    /// Arrastando o polegar: distância do topo dele ao ponto pego.
    grab: Option<f32>,
    /// Autoscroll ligado: onde foi o clique do meio, e quando rolou pela última vez.
    anchor: Option<(Point<Pixels>, Instant)>,
    /// Pega o Esc enquanto o autoscroll está ligado (a barra não tem foco de teclado).
    esc: Option<Subscription>,
}

/// A lista com a barra por cima, na borda direita. `bar` é a de `scrollbar`, já posicionada.
pub(super) fn scrolled(list: impl IntoElement, bar: impl IntoElement) -> Div {
    div().relative().flex_1().min_h_0().flex().flex_col().child(list).child(bar)
}

/// A barra ocupa a borda direita do pai (`scrolled`); `.right(..)` e `.bottom(..)` a afastam de cantos e do player.
pub(super) fn scrollbar(id: impl Into<ElementId>, handle: impl Scrolls) -> impl IntoElement + Styled {
    let id = id.into();
    let prep = handle.clone();
    canvas(
        move |bar, window, cx| {
            let state = window.use_keyed_state(id, cx, |_, _| State::default());
            let area = prep.area();
            if prep.max() < 1. || area.size.height <= px(0.) || bar.size.height <= px(0.) {
                return None;
            }
            // A faixa deixa a roda passar para a lista; a área inteira só escuta o clique do meio.
            let hit = window.insert_hitbox(bar, HitboxBehavior::BlockMouseExceptScroll);
            let whole = window.insert_hitbox(area, HitboxBehavior::Normal);
            Some((state, hit, whole))
        },
        move |bar, frame, window, cx| {
            let Some((state, hit, whole)) = frame else { return };
            paint(&handle, bar, state, hit, whole, window, cx);
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .right_0()
    .w(px(HIT_W))
}

fn paint(h: &impl Scrolls, bar: Bounds<Pixels>, state: gpui::Entity<State>, hit: Hitbox, whole: Hitbox, window: &mut Window, cx: &mut App) {
    let (max, view) = (h.max(), f32::from(h.area().size.height));
    let track_h = f32::from(bar.size.height);
    let thumb_h = (track_h * view / (view + max)).clamp(MIN_THUMB.min(track_h), track_h);
    let travel = (track_h - thumb_h).max(1.);
    let top = f32::from(bar.top());
    let thumb_y = top + travel * (h.scrolled() / max).clamp(0., 1.);
    let (grab, anchor) = {
        let s = state.read(cx);
        (s.grab, s.anchor)
    };
    let hovered = hit.is_hovered(window);
    let open = hovered || grab.is_some();

    // Fina e discreta em repouso; com o mouse em cima, a trilha de vidro aparece e o polegar engorda.
    if open {
        let track = Bounds::new(point(bar.left() + px(1.), bar.top()), size(px(HIT_W - 2.), bar.size.height));
        window.paint_quad(quad(track, px((HIT_W - 2.) / 2.), rgba(theme::glass_fill()), px(1.), rgba(theme::wash(0x17)), Default::default()));
    }
    let (w, alpha) = match (grab.is_some(), open) {
        (true, _) => (8., 0xa6),
        (_, true) => (8., 0x80),
        _ => (4., 0x40),
    };
    let x = bar.right() - px((HIT_W - w) / 2.);
    let thumb = Bounds::new(point(x - px(w), px(thumb_y)), size(px(w), px(thumb_h)));
    window.paint_quad(quad(thumb, px(w / 2.), rgba(theme::wash(alpha)), px(0.), rgba(0), Default::default()));
    if open {
        window.set_cursor_style(CursorStyle::Arrow, &hit);
    }

    let set = {
        let h = h.clone();
        move |mouse_y: f32, grab: f32| h.scroll_to((mouse_y - top - grab) / travel * max)
    };

    // Polegar: pega onde clicou. Trilha: o polegar pula para o clique e continua arrastando.
    window.on_mouse_event({
        let (state, h, set, hit) = (state.clone(), h.clone(), set.clone(), hit.clone());
        move |e: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || e.button != MouseButton::Left || !hit.is_hovered(window) {
                return;
            }
            let y = f32::from(e.position.y);
            let g = if (thumb_y..thumb_y + thumb_h).contains(&y) { y - thumb_y } else { thumb_h / 2. };
            if g == thumb_h / 2. {
                set(y, g);
            }
            state.update(cx, |s, _| s.grab = Some(g));
            h.dragging(true);
            window.capture_pointer(hit.id);
            cx.stop_propagation();
            window.refresh();
        }
    });
    window.on_mouse_event({
        let (state, h, hit) = (state.clone(), h.clone(), hit.clone());
        move |e: &MouseMoveEvent, _, window, cx| {
            match grab {
                Some(g) if e.pressed_button == Some(MouseButton::Left) => set(f32::from(e.position.y), g),
                // Soltou fora da janela: o MouseUp não veio.
                Some(_) => {
                    state.update(cx, |s, _| s.grab = None);
                    h.dragging(false);
                }
                None if hit.is_hovered(window) == hovered => return,
                None => {}
            }
            window.refresh();
        }
    });
    if grab.is_some() {
        window.on_mouse_event({
            let (state, h) = (state.clone(), h.clone());
            move |e: &MouseUpEvent, _, window, cx| {
                if e.button == MouseButton::Left {
                    state.update(cx, |s, _| s.grab = None);
                    h.dragging(false);
                    window.refresh();
                }
            }
        });
    }
    // A barra fica na margem, fora da lista: a roda em cima dela também rola.
    window.on_mouse_event({
        let (h, hit) = (h.clone(), hit.clone());
        move |e: &ScrollWheelEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble && hit.is_hovered(window) && !h.area().contains(&e.position) {
                h.scroll_to(h.scrolled() - f32::from(e.delta.pixel_delta(window.line_height()).y));
                window.refresh();
            }
        }
    });

    autoscroll(h, state, whole, anchor, window, cx);
}

fn autoscroll(h: &impl Scrolls, state: gpui::Entity<State>, whole: Hitbox, anchor: Option<(Point<Pixels>, Instant)>, window: &mut Window, cx: &mut App) {
    let Some((at, last)) = anchor else {
        if state.read(cx).esc.is_some() {
            state.update(cx, |s, _| s.esc = None);
        }
        window.on_mouse_event({
            let whole = whole.clone();
            move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || e.button != MouseButton::Middle || !whole.is_hovered(window) {
                    return;
                }
                let esc = cx.intercept_keystrokes({
                    let state = state.clone();
                    move |k, window, cx| {
                        if k.keystroke.key == "escape" && state.read(cx).anchor.is_some() {
                            state.update(cx, |s, _| s.anchor = None);
                            cx.stop_propagation();
                            window.refresh();
                        }
                    }
                });
                state.update(cx, |s, _| {
                    s.anchor = Some((e.position, Instant::now()));
                    s.esc = Some(esc);
                });
                cx.stop_propagation();
                window.refresh();
            }
        });
        return;
    };

    // Rola pelo tempo desde o último quadro, para a velocidade não depender da taxa da tela.
    let now = Instant::now();
    let dt = now.duration_since(last).as_secs_f32().min(0.05);
    let dy = f32::from(window.mouse_position().y - at.y);
    let speed = ((dy.abs() - DEAD).max(0.) * SPEED).min(MAX_SPEED) * dy.signum();
    if speed != 0. {
        h.scroll_to(h.scrolled() + speed * dt);
    }
    state.update(cx, |s, _| s.anchor = Some((at, now)));
    window.request_animation_frame();

    // Âncora: um disco de vidro com setas para cima e para baixo; a seta apagada é a do lado que já acabou.
    let r = 14.;
    let disc = Bounds::new(at - point(px(r), px(r)), size(px(r * 2.), px(r * 2.)));
    window.paint_quad(quad(disc, px(r), rgba(theme::glass_fill()), px(1.), rgba(theme::wash(0x40)), Default::default()));
    window.paint_quad(quad(Bounds::new(at - point(px(2.), px(2.)), size(px(4.), px(4.))), px(2.), rgba(theme::ink_1()), px(0.), rgba(0), Default::default()));
    let (up, down) = (h.scrolled() > 0.5, h.scrolled() < h.max() - 0.5);
    for (name, dir, on) in [("chevron-up", -1., up), ("chevron-down", 1., down)] {
        let b = Bounds::new(at + point(px(-5.), px(dir * 7. - 5.)), size(px(10.), px(10.)));
        let color = if on { theme::ink_1() } else { theme::ink_3() };
        let _ = window.paint_svg(b, format!("icons/{name}.svg").into(), None, TransformationMatrix::unit(), rgba(color).into(), cx);
    }
    window.set_cursor_style(CursorStyle::ResizeUpDown, &whole);

    // Qualquer clique encerra, e não chega a quem está embaixo.
    window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Capture {
            state.update(cx, |s, _| s.anchor = None);
            cx.stop_propagation();
            window.refresh();
        }
    });
}
