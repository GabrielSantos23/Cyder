//! Tokens do design (doc 05). As cores vêm de funções porque mudam com o tema: claro ou escuro, escolhido
//! nas configurações (ou o do sistema). O render da janela define o tema antes de montar a tela.
use std::cell::Cell;

use gpui::{Div, Hsla, Styled, px, rgb, rgba};

pub const FONT: &str = "Segoe UI";
pub const DANGER: u32 = 0xe81123;

pub const RADIUS_CARD: f32 = 28.;
pub const TITLEBAR_H: f32 = 40.;

thread_local! {
    static LIGHT: Cell<bool> = const { Cell::new(false) };
}

/// Tema da tela que está sendo montada. A UI roda numa thread só, então basta um valor por thread.
pub fn set_light(on: bool) {
    LIGHT.with(|l| l.set(on));
}

pub fn light() -> bool {
    LIGHT.with(Cell::get)
}

/// Escolhe a cor do tema (RGBA).
fn pick(dark: u32, light_: u32) -> u32 {
    if light() { light_ } else { dark }
}

/// Fundo atrás de tudo.
pub fn base() -> u32 {
    pick(0x0b0b0e, 0xe9e9ee)
}

/// Fundo fixo das telas principais, o cinza-grafite do Cider (no claro, quase branco).
pub fn surface() -> u32 {
    pick(0x1c1c20, 0xf7f7f9)
}

/// Moldura em volta do painel principal.
pub fn frame() -> u32 {
    pick(0x08080a, 0xe3e3e8)
}

/// Texto e ícones: principal, secundário e terciário.
pub fn ink_1() -> u32 {
    pick(0xffffffff, 0x18181bff)
}

pub fn ink_2() -> u32 {
    pick(0xffffffb3, 0x18181bb3)
}

pub fn ink_3() -> u32 {
    pick(0xffffff80, 0x18181b8c)
}

/// Véu da cor do texto com a opacidade `alpha` (0..=0xff): hover, linhas zebradas, bordas, trilhos.
pub fn wash(alpha: u32) -> u32 {
    pick(0xffffff00, 0x18181b00) | (alpha & 0xff)
}

/// O contrário de `wash`, na direção do fundo: campos de texto e vidro sobre fotos.
pub fn shade(alpha: u32) -> u32 {
    pick(0x00000000, 0xffffff00) | (alpha & 0xff)
}

/// Texto sobre um botão cheio da cor do texto (o "Tocar" branco no escuro, preto no claro).
pub fn on_ink() -> u32 {
    pick(0x000000ff, 0xffffffff)
}

/// Cartão de menus, janelas e do mini player, com a opacidade `alpha`.
pub fn panel(alpha: u32) -> u32 {
    pick(0x1e1e2300, 0xffffff00) | (alpha & 0xff)
}

pub fn hover() -> u32 {
    wash(0x14)
}

/// Superfície de vidro: cartão de letras, barra do player, avisos.
pub fn glass(d: Div) -> Div {
    d.bg(rgba(pick(0x14141899, 0xffffffb8))).border_1().border_color(rgba(wash(0x17))).backdrop_blur(px(40.))
}

/// Texto puxado levemente para a cor da capa, como a letra do Cider: branco tingido no escuro,
/// a cor da capa escurecida no claro.
pub fn tinted_ink(accent: [u8; 3]) -> Hsla {
    let mix = |c: u8| if light() { (c as f32 * 0.45).round() as u32 } else { (255.0 - (255.0 - c as f32) * 0.22).round() as u32 };
    rgb((mix(accent[0]) << 16) | (mix(accent[1]) << 8) | mix(accent[2])).into()
}
