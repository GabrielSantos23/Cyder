//! The theme helpers `karaoke.rs` and `preset.rs` call, adapted from ytm-rs's `theme.rs`
//! (GPL-3.0, https://github.com/grapedjuice/ytm-rs) onto Sidra's fonts and dark look.

use crate::preset::Weight;
use egui::{Color32, FontId};

const SPEED: f32 = 0.65;

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn anim(
    ctx: &egui::Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    target: f32,
    secs: f32,
) -> f32 {
    ctx.animate_value_with_time(egui::Id::new(id), target, secs * SPEED)
}

pub fn anim_bool(
    ctx: &egui::Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    on: bool,
    secs: f32,
) -> f32 {
    ease_out(ctx.animate_bool_with_time(egui::Id::new(id), on, secs * SPEED))
}

pub fn anim_bool_timed(
    ctx: &egui::Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    on: bool,
    secs: f32,
) -> f32 {
    anim_bool(ctx, id, on, secs)
}

/// Sidra is dark only.
pub fn light() -> bool {
    false
}

pub fn bg() -> Color32 {
    crate::style::FRAME
}

pub fn system_accent() -> Option<Color32> {
    None
}

pub fn glass(a: f32) -> Color32 {
    Color32::from_white_alpha((a.min(1.0) * 255.0) as u8)
}

pub fn shade(a: f32) -> Color32 {
    Color32::from_black_alpha((a * 255.0) as u8)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

// Sidra bundles Inter Medium and SemiBold only, so every lyric font and weight maps onto them.

pub fn bold(size: f32) -> FontId {
    crate::style::bold(size)
}

pub fn weighted(w: Weight, size: f32) -> FontId {
    match w {
        Weight::Regular | Weight::Medium | Weight::Light => FontId::proportional(size),
        _ => bold(size),
    }
}

pub fn lyric_font_for(_name: &str, weight: Weight, size: f32, _text: &str) -> FontId {
    weighted(weight, size)
}

pub fn effective_font(theme_font: &str) -> String {
    theme_font.to_owned()
}

pub fn font_choice() -> String {
    String::new()
}

pub fn font_choice_is(name: &str) -> bool {
    name.is_empty()
}

/// Sidra's fonts never change after start-up.
pub fn fonts_generation() -> u32 {
    0
}

/// The Phosphor icon font is merged into the proportional family.
pub fn icon_font(size: f32) -> FontId {
    FontId::proportional(size)
}

pub mod icon {
    /// Phosphor's music notes, standing in for ytm-rs's Lucide glyph.
    pub const MUSIC: char = '\u{E340}';
}
