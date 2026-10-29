//! Primeira abertura: boas-vindas, criar o app no painel do Spotify e colar o Client ID.
//! Depois do primeiro login, um cartão mostra onde ficam as funções principais.
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, ClipboardItem, Context, Div, FontWeight, SpringAnimation, Window, div, ease_out_quint, point,
    prelude::*, px, rgba,
};

use super::{Cyder, Dialog, Phase, REDIRECT_URI, SPRING, icon};
use crate::theme::{self, glass};

const GREEN: u32 = 0x1ed760;
const DASHBOARD: &str = "https://developer.spotify.com/dashboard";
const STEPS: usize = 3;
pub(super) const STEP_CLIENT_ID: usize = 2;
/// Só para a ilustração: parece um Client ID, mas não é de ninguém.
const SAMPLE_ID: &str = "7f3e91c2d84b4a06…";

fn green(alpha: u32) -> u32 {
    (GREEN << 8) | alpha
}

/// Sobe e desce: 0 → 1 → 0.
fn wave(t: f32) -> f32 {
    1. - (t * 2. - 1.).abs()
}

impl Cyder {
    pub(super) fn onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let step = self.onboard_step.min(STEPS - 1);
        if step == STEP_CLIENT_ID && std::mem::take(&mut self.focus_client_id) {
            window.focus(&self.client_id_focus, cx);
        }
        let (visual, content) = match step {
            0 => (welcome_visual(), self.welcome_step(cx)),
            1 => (dashboard_visual(), self.create_app_step(cx)),
            _ => (client_id_visual(), self.client_id_step(window, cx)),
        };
        // Cada passo entra deslizando um pouco; a ilustração só aparece.
        let enter = || Animation::new(Duration::from_millis(420)).with_easing(ease_out_quint());
        div()
            .absolute()
            .inset_0()
            .pt(px(theme::TITLEBAR_H))
            .px_8()
            .pb_8()
            .flex()
            .items_center()
            .justify_center()
            .child(
                glass(div())
                    .w_full()
                    .max_w(px(1000.))
                    .h(px(580.))
                    .rounded(px(theme::RADIUS_CARD))
                    .overflow_hidden()
                    .flex()
                    .child(
                        // O GPUI recorta os filhos num retângulo: sem os cantos próprios, o fundo vaza pela curva do cartão.
                        div().w(px(420.)).flex_none().h_full().relative().overflow_hidden().rounded_l(px(theme::RADIUS_CARD)).bg(rgba(theme::shade(0x4d)))
                            .child(visual.with_animation(("onb-visual", step), enter(), |d, t| d.opacity(t))),
                    )
                    .child(
                        div().flex_1().min_w_0().h_full().px_10().py_8().flex().flex_col()
                            .child(progress(step))
                            .child(content.with_animation(("onb-step", step), enter(), |d, t| {
                                d.opacity(t).layer_translate(point(px(20. * (1. - t)), px(0.)))
                            })),
                    ),
            )
            .into_any_element()
    }

    fn go_to(&mut self, step: usize) {
        // Os botões de avançar ficam no mesmo lugar em todos os passos: um clique repetido pularia vários
        // de uma vez (e chegaria a "Salvar e entrar"). Mesma defesa do pular faixa.
        if self.skip_bounced() {
            return;
        }
        self.onboard_step = step;
        self.focus_client_id = step == STEP_CLIENT_ID;
    }

    fn welcome_step(&mut self, cx: &mut Context<Self>) -> Div {
        let need = |name: &'static str, title: &'static str, detail: &'static str| {
            div().flex().gap_3()
                .child(div().size(px(36.)).flex_none().rounded(px(10.)).bg(rgba(green(0x1f))).flex().items_center().justify_center()
                    .child(icon(name, 18., green(0xff))))
                .child(div().flex_1().min_w_0().flex().flex_col().gap_0p5()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().text_sm().text_color(rgba(theme::ink_3())).child(detail)))
        };
        step_column()
            .child(title("Bem-vindo ao Cyder"))
            .child(lead("Um player para a sua conta do Spotify, com letra sincronizada e mini player."))
            .child(div().mt_8().mb_3().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_3())).child("Você vai precisar de"))
            .child(div().flex().flex_col().gap_4()
                .child(need("user-round", "Spotify Premium", "O Spotify só toca em apps como este para contas Premium."))
                .child(need("laptop", "Um app grátis no painel do Spotify", "Leva uns cinco minutos."))
                .child(need("log-in", "Entrar pelo navegador", "O Cyder não vê a sua senha.")))
            .child(div().flex_1())
            .child(div().flex().items_center().justify_between()
                .child(Self::text_button("onb-have-id", "Já tenho um Client ID")
                    .on_click(cx.listener(|t, _, _, cx| { t.go_to(STEP_CLIENT_ID); cx.notify(); })))
                .child(Self::primary_button("onb-start", "Criar meu app")
                    .on_click(cx.listener(|t, _, _, cx| { t.go_to(1); cx.notify(); }))))
    }

    fn create_app_step(&mut self, cx: &mut Context<Self>) -> Div {
        let copied = self.uri_copied;
        // "Create app", "Redirect URIs", "Web API" e "Save" ficam em inglês: são os rótulos exatos do painel do Spotify.
        step_column()
            .child(title("Crie seu app no Spotify"))
            .child(lead("Faça no navegador e volte aqui."))
            .child(div().mt_6().flex().flex_col().gap_4()
                .child(numbered(1, "Abra o painel, entre na sua conta e clique em Create app.")
                    .child(div().mt_2().child(Self::secondary_button("onb-dashboard", "Abrir o painel do Spotify")
                        .flex().items_center().gap_2().child(icon("external-link", 14., theme::ink_1()))
                        .on_click(|_, _, cx| cx.open_url(DASHBOARD)))))
                .child(numbered(2, "Dê qualquer nome e descrição, como Cyder."))
                .child(numbered(3, "Em Redirect URIs, cole este endereço e clique em Add:")
                    .child(div().mt_2().flex().items_center().gap_2()
                        .child(div().flex_1().min_w_0().px_3().py_2().rounded(px(8.)).bg(rgba(theme::wash(0x14))).font_family("Consolas").text_sm().child(REDIRECT_URI))
                        .child(Self::secondary_button("onb-copy-uri", if copied { "Copiado" } else { "Copiar" })
                            .flex().items_center().gap_2()
                            .child(icon(if copied { "check" } else { "copy" }, 14., if copied { green(0xff) } else { theme::ink_1() }))
                            .on_click(cx.listener(|t, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(REDIRECT_URI.into()));
                                t.uri_copied = true;
                                cx.notify();
                            })))))
                .child(numbered(4, "Marque Web API, aceite os termos e clique em Save.")))
            .child(div().flex_1())
            .child(self.footer("onb-back-1", 0, Self::primary_button("onb-created", "Já criei o app")
                .on_click(cx.listener(|t, _, _, cx| { t.go_to(STEP_CLIENT_ID); cx.notify(); })), cx))
    }

    fn client_id_step(&mut self, window: &Window, cx: &mut Context<Self>) -> Div {
        step_column()
            .child(title("Cole o Client ID"))
            .child(lead("No app que você criou, clique em Settings. O Client ID é o primeiro item de Basic Information."))
            .child(div().mt_8().mb_2().text_sm().font_weight(FontWeight::SEMIBOLD).child("Client ID"))
            .child(self.client_id_field(window, cx))
            // Linha do erro sempre reservada: a tela não pula quando ele aparece.
            .child(div().mt_2().h(px(20.)).text_sm().text_color(rgba((theme::DANGER << 8) | 0xff)).children(self.client_id_error.clone()))
            .child(div().mt_2().flex().gap_2().text_sm().text_color(rgba(theme::ink_3()))
                .child(div().pt(px(2.)).child(icon("info", 14., theme::ink_3())))
                .child(div().flex_1().min_w_0().child("Ao salvar, o navegador abre para você entrar no Spotify.")))
            .child(div().flex_1())
            .child(self.footer("onb-back-2", 1, Self::primary_button("onb-save", "Salvar e entrar")
                .on_click(cx.listener(|t, _, window, cx| {
                    if !t.skip_bounced() {
                        t.submit_client_id(window, cx);
                    }
                    cx.notify();
                })), cx))
    }

    /// Voltar à esquerda; Cancelar (vindo das configurações) e a ação principal à direita.
    fn footer(&self, back_id: &'static str, back_to: usize, primary: gpui::Stateful<Div>, cx: &mut Context<Self>) -> Div {
        div().flex().items_center().justify_between()
            .child(Self::text_button(back_id, "Voltar").on_click(cx.listener(move |t, _, _, cx| { t.go_to(back_to); cx.notify(); })))
            .child(div().flex().gap_2()
                .when(self.setup_return, |d| d.child(Self::secondary_button("onb-cancel", "Cancelar")
                    .on_click(cx.listener(|t, _, window, cx| {
                        t.setup_return = false;
                        t.client_id_error = None;
                        t.phase = Phase::Ready;
                        window.focus(&t.focus, cx);
                        cx.notify();
                    }))))
                .child(primary))
    }

    /// Cartão de depois do primeiro login: o que dá para fazer e onde fica.
    pub(super) fn welcome_dialog(&self, card: Div, cx: &mut Context<Self>) -> Div {
        let row = |name: &'static str, what: &'static str, keys: Option<&'static str>| {
            div().flex().items_center().gap_3().text_sm()
                .child(div().size(px(32.)).flex_none().rounded(px(9.)).bg(rgba(theme::wash(0x14))).flex().items_center().justify_center()
                    .child(icon(name, 16., theme::ink_1())))
                .child(div().flex_1().min_w_0().text_color(rgba(theme::ink_2())).child(what))
                .children(keys.map(|k| div().px_2().py_0p5().rounded(px(6.)).bg(rgba(theme::wash(0x14))).text_xs().font_weight(FontWeight::SEMIBOLD).child(k)))
        };
        let badge = div().size(px(44.)).rounded_full().bg(rgba(green(0xff))).flex().items_center().justify_center()
            .child(icon("check", 22., 0x000000ff))
            .with_animation("onb-done", Animation::new(Duration::from_millis(500)).with_easing(ease_out_quint()), |d, t| d.opacity(t).layer_scale(0.5 + 0.5 * t));
        card.w(px(460.))
            .child(div().flex().items_center().gap_3()
                .child(badge)
                .child(div().flex().flex_col()
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child("Tudo pronto"))
                    .child(div().text_sm().text_color(rgba(theme::ink_2())).child("Sua biblioteca já está aqui."))))
            .child(div().flex().flex_col().gap_3()
                .child(row("search", "Buscar músicas, álbuns e artistas", Some("Ctrl + K")))
                .child(row("play", "Tocar ou pausar de qualquer tela", Some("Espaço")))
                .child(row("mic-vocal", "Letra sincronizada em tela cheia", Some("L")))
                .child(row("picture-in-picture-2", "Mini player: botão no topo da janela, à direita", None))
                .child(row("speaker", "O Cyder aparece no Spotify Connect do celular", None))
                .child(row("settings", "Last.fm, Discord e equalizador: Configurações", None)))
            .child(div().flex().items_center().justify_between()
                .child(Self::text_button("onb-shortcuts", "Ver todos os atalhos")
                    .on_click(cx.listener(|t, _, _, cx| { t.dialog = Some(Dialog::Shortcuts); cx.notify(); })))
                .child(Self::primary_button("onb-listen", "Começar a ouvir")
                    .on_click(cx.listener(|t, _, window, cx| { t.dialog = None; window.focus(&t.focus, cx); cx.notify(); }))))
    }
}

fn step_column() -> Div {
    div().flex_1().min_h_0().flex().flex_col()
}

fn title(text: &'static str) -> Div {
    div().text_size(px(30.)).font_weight(FontWeight::BOLD).child(text)
}

fn lead(text: &'static str) -> Div {
    div().mt_2().max_w(px(460.)).text_color(rgba(theme::ink_2())).child(text)
}

fn numbered(n: usize, text: &'static str) -> Div {
    div().flex().gap_3()
        .child(div().size(px(24.)).flex_none().rounded_full().bg(rgba(green(0x26))).flex().items_center().justify_center()
            .text_xs().font_weight(FontWeight::BOLD).text_color(rgba(green(0xff))).child(n.to_string()))
        .child(div().flex_1().min_w_0().pt(px(2.)).flex().flex_col().text_sm().text_color(rgba(theme::ink_2())).child(text))
}

/// Traços do topo: o passo atual é o mais comprido e cresce com mola.
fn progress(step: usize) -> Div {
    div().mb_8().flex().items_center().gap_3()
        .child(div().flex().gap_1p5().children((0..STEPS).map(|i| {
            let color = if i <= step { green(0xff) } else { theme::wash(0x33) };
            div().h(px(6.)).rounded_full().bg(rgba(color))
                .with_spring(("onb-progress", i), SpringAnimation::new(SPRING).to(px(if i == step { 28. } else { 8. })), |d, w| d.w(w))
        })))
        .child(div().text_xs().text_color(rgba(theme::ink_3())).child(format!("Passo {} de {STEPS}", step + 1)))
}

/// Boas-vindas: um disco com ondas saindo e um equalizador dançando.
fn welcome_visual() -> Div {
    let ring = |i: usize| {
        div().absolute().size(px(132.)).rounded_full().border_2().border_color(rgba(green(0xff)))
            .with_animation(("onb-ring", i), Animation::new(Duration::from_millis(2600)).repeat(), move |d, t| {
                // Os anéis saem defasados, um depois do outro.
                let t = (t + i as f32 / 3.).fract();
                d.opacity(0.55 * (1. - t)).layer_scale(1. + 1.1 * t)
            })
    };
    let bar = |i: usize| {
        let phase = [0., 0.35, 0.7, 0.15, 0.55, 0.9, 0.25, 0.6, 0.05][i];
        div().w(px(6.)).rounded_full().bg(rgba(green(0x99 + (i as u32 % 3) * 0x22)))
            .with_animation(("onb-bar", i), Animation::new(Duration::from_millis(1200)).repeat(), move |d, t| {
                d.h(px(10. + 46. * wave((t + phase).fract())))
            })
    };
    div().size_full().flex().flex_col().items_center().justify_center().gap_10()
        .child(div().relative().size(px(132.)).flex().items_center().justify_center()
            .children((0..3).map(ring))
            .child(div().size(px(132.)).rounded_full().bg(rgba(green(0xff))).shadow_2xl().flex().items_center().justify_center()
                .child(icon("headphones", 56., 0x000000ff))))
        .child(div().h(px(56.)).flex().items_center().gap(px(5.)).children((0..9).map(bar)))
}

/// Linha de formulário do painel do Spotify, desenhada em miniatura.
fn mock_field(label: &'static str, value: Div) -> Div {
    div().flex().flex_col().gap_1()
        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child(label))
        .child(value.h(px(30.)).px_2().flex().items_center().rounded(px(6.)).border_1().border_color(rgba(theme::wash(0x29)))
            .bg(rgba(theme::shade(0x59))).text_xs().overflow_hidden().whitespace_nowrap())
}

/// O campo que importa naquele passo pisca uma borda verde.
fn highlight(id: &'static str, d: Div) -> impl IntoElement {
    d.with_animation(id, Animation::new(Duration::from_millis(1600)).repeat(), |d, t| {
        d.border_2().border_color(rgba(green(0x55 + (0xaa as f32 * wave(t)) as u32)))
    })
}

fn mock_window(heading: &'static str) -> Div {
    let dot = || div().size(px(8.)).rounded_full().bg(rgba(theme::wash(0x33)));
    div().w(px(320.)).rounded(px(14.)).overflow_hidden().border_1().border_color(rgba(theme::wash(0x1f))).bg(rgba(theme::panel(0xf2))).shadow_2xl()
        .child(div().h(px(28.)).px_3().flex().items_center().gap_1p5().bg(rgba(theme::wash(0x0d))).child(dot()).child(dot()).child(dot())
            .child(div().ml_2().text_xs().text_color(rgba(theme::ink_3())).child("developer.spotify.com")))
        .child(div().p_4().flex().flex_col().gap_3().child(div().text_sm().font_weight(FontWeight::BOLD).child(heading)))
}

/// Passo 2: o formulário "Create app", com o Redirect URI sendo digitado.
fn dashboard_visual() -> Div {
    let typed = div().font_family("Consolas")
        .with_animation("onb-typing", Animation::new(Duration::from_millis(1800)).with_easing(ease_out_quint()), |d, t| {
            d.child(&REDIRECT_URI[..(REDIRECT_URI.len() as f32 * t).round() as usize])
        });
    let check = div().size(px(14.)).rounded(px(3.)).bg(rgba(green(0xff))).flex().items_center().justify_center().child(icon("check", 10., 0x000000ff));
    let window = mock_window("Create app");
    let window = window.child(
        div().px_4().pb_4().flex().flex_col().gap_3()
            .child(mock_field("App name", div().child("Cyder")))
            .child(mock_field("App description", div().text_color(rgba(theme::ink_3())).child("Meu player")))
            .child(div().flex().flex_col().gap_1()
                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child("Redirect URIs"))
                .child(div().flex().gap_2()
                    .child(highlight("onb-uri", div().flex_1().min_w_0().h(px(30.)).px_2().flex().items_center().rounded(px(6.))
                        .bg(rgba(theme::shade(0x59))).text_xs().overflow_hidden().whitespace_nowrap().child(typed)))
                    .child(div().h(px(30.)).px_3().flex().items_center().rounded_full().bg(rgba(theme::wash(0x1f))).text_xs().child("Add"))))
            .child(div().flex().items_center().gap_2().text_xs().child(check).child("Web API"))
            .child(div().flex().justify_end().child(div().px_3().py_1().rounded_full().bg(rgba(green(0xff))).text_color(rgba(0x000000ff)).text_xs()
                .font_weight(FontWeight::SEMIBOLD).child("Save"))),
    );
    div().size_full().flex().items_center().justify_center().child(window)
}

/// Passo 3: "Basic Information", com o Client ID em destaque e uma etiqueta apontando.
fn client_id_visual() -> Div {
    let id_row = div().h(px(30.)).px_2().flex().items_center().justify_between().rounded(px(6.)).bg(rgba(theme::shade(0x59))).text_xs()
        .child(div().font_family("Consolas").child(SAMPLE_ID))
        .child(icon("copy", 12., theme::ink_2()));
    let tag = div().absolute().top(px(-30.)).right(px(0.)).px_2().py_1().rounded(px(6.)).bg(rgba(green(0xff))).text_color(rgba(0x000000ff))
        .text_xs().font_weight(FontWeight::SEMIBOLD).child("Copie este")
        .with_animation("onb-tag", Animation::new(Duration::from_millis(1400)).repeat(), |d, t| d.mt(px(-3. * wave(t))));
    let window = mock_window("Basic Information").child(
        div().px_4().pb_4().flex().flex_col().gap_3()
            .child(div().flex().flex_col().gap_1()
                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgba(theme::ink_2())).child("Client ID"))
                .child(div().relative().child(highlight("onb-id", id_row)).child(tag)))
            .child(div().opacity(0.45).flex().flex_col().gap_3()
                .child(mock_field("Client secret", div().child("View client secret")))
                .child(mock_field("Redirect URIs", div().font_family("Consolas").child(REDIRECT_URI)))),
    );
    div().size_full().flex().items_center().justify_center().child(window)
}
