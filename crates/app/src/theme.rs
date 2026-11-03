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
    static COVER_FRAME: Cell<bool> = const { Cell::new(false) };
    static ACCENT: Cell<Option<u32>> = const { Cell::new(None) };
}

/// O verde do Spotify, o destaque padrão.
const GREEN: u32 = 0x1ed760;

/// Tema da tela que está sendo montada. A UI roda numa thread só, então basta um valor por thread.
pub fn set_light(on: bool) {
    LIGHT.with(|l| l.set(on));
}

pub fn light() -> bool {
    LIGHT.with(Cell::get)
}

/// A capa da música na moldura: ela e o fundo ficam transparentes e a capa (`Cyder::background`) aparece.
pub fn set_cover_frame(on: bool) {
    COVER_FRAME.with(|m| m.set(on));
}

pub fn cover_frame() -> bool {
    COVER_FRAME.with(Cell::get)
}

/// Destaque do sistema (0xRRGGBB) ou None para o verde.
pub fn set_accent(rgb: Option<u32>) {
    ACCENT.with(|a| a.set(rgb));
}

/// Cor de destaque (RGBA): ícones ativos, curtidas, interruptores. A do sistema vai clareando (no escuro) ou
/// escurecendo (no claro) até ter contraste AA de texto, 4,5:1, com o painel e a moldura.
pub fn accent() -> u32 {
    let rgb = match ACCENT.with(Cell::get) {
        Some(c) => readable(c, &[surface(), frame()], if light() { 0x000000 } else { 0xffffff }),
        None => GREEN,
    };
    (rgb << 8) | 0xff
}

/// Texto e ícones sobre um fundo da cor de destaque: preto ou branco, o que contrastar mais.
pub fn on_accent() -> u32 {
    let a = accent() >> 8;
    if contrast(a, 0x000000) >= contrast(a, 0xffffff) { 0x000000ff } else { 0xffffffff }
}

/// Luminância relativa do WCAG.
fn luminance(rgb: u32) -> f32 {
    let ch = |shift: u32| {
        let c = ((rgb >> shift) & 0xff) as f32 / 255.;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
}

fn contrast(a: u32, b: u32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Mistura `rgb` com `toward` em passos de 5% até ter 4,5:1 com todos os fundos.
fn readable(rgb: u32, backgrounds: &[u32], toward: u32) -> u32 {
    let mix = |t: f32| {
        let ch = |shift: u32| {
            let (a, b) = (((rgb >> shift) & 0xff) as f32, ((toward >> shift) & 0xff) as f32);
            ((a + (b - a) * t).round() as u32) << shift
        };
        ch(16) | ch(8) | ch(0)
    };
    (0..=20).map(|i| mix(i as f32 / 20.)).find(|&c| backgrounds.iter().all(|&bg| contrast(c, bg) >= 4.5)).unwrap_or(toward)
}

/// Fundo atrás de tudo, já com a opacidade (RGBA). Com a capa no fundo, transparente.
pub fn base_fill() -> u32 {
    if cover_frame() { 0 } else { (base() << 8) | 0xff }
}

/// Moldura (RGBA). Com a capa no fundo, transparente: uma moldura translúcida somaria com a faixa dos cantos e
/// desenharia uma linha mais escura em volta do painel.
pub fn frame_fill() -> u32 {
    if cover_frame() { 0 } else { (frame() << 8) | 0xff }
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

/// Fundo do vidro, para quem pinta direto (a trilha da barra de rolagem).
pub fn glass_fill() -> u32 {
    pick(0x14141899, 0xffffffb8)
}

/// Superfície de vidro: cartão de letras, barra do player, avisos.
pub fn glass(d: Div) -> Div {
    d.bg(rgba(glass_fill())).border_1().border_color(rgba(wash(0x17))).backdrop_blur(px(40.))
}

/// Texto puxado levemente para a cor da capa, como a letra do Cider: branco tingido no escuro,
/// a cor da capa escurecida no claro.
pub fn tinted_ink(accent: [u8; 3]) -> Hsla {
    let mix = |c: u8| if light() { (c as f32 * 0.45).round() as u32 } else { (255.0 - (255.0 - c as f32) * 0.22).round() as u32 };
    rgb((mix(accent[0]) << 16) | (mix(accent[1]) << 8) | mix(accent[2])).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_reaches_aa() {
        for (light_, accent_) in [(false, 0x0050a0), (true, 0x60cdff), (false, 0xffb900), (true, 0x1ed760)] {
            set_light(light_);
            set_accent(Some(accent_));
            let a = accent() >> 8;
            assert!(contrast(a, surface()) >= 4.5 && contrast(a, frame()) >= 4.5, "{accent_:06x} claro={light_}");
            assert!(contrast(a, on_accent() >> 8) >= 4.5);
        }
    }
}
