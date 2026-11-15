//! Colours and global egui style for Sidra's own chrome.
//!
//! The window background is the sidebar colour, and the content panel sits inset
//! inside it, so the sidebar reads as a frame around the whole UI.

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Galley, Rect, Response, Sense,
    Stroke, StrokeKind, TextStyle, Ui, Vec2, WidgetInfo, WidgetType,
};
use std::sync::Arc;

/// Sidra's colours. The default is the Apple Music look; a theme from Settings replaces it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Window frame and sidebar background.
    pub frame: Color32,
    /// The inset content panel and its hairline border.
    pub content: Color32,
    pub content_border: Color32,
    /// Primary text: 17:1 on the frame.
    pub text: Color32,
    /// Section labels and placeholders: 6.4:1 on the frame, above the 4.5:1 floor.
    pub text_muted: Color32,
    pub row_hover: Color32,
    pub row_selected: Color32,
    pub field: Color32,
    pub field_border: Color32,
    /// Keyboard focus ring.
    pub focus: Color32,
    /// Avatar placeholder behind the account initial.
    pub avatar: Color32,
    /// Key colour: the playing track, loved songs, LIVE.
    pub accent: Color32,
    /// The player bar.
    pub bar: Color32,
}

pub const APPLE_MUSIC: Palette = Palette {
    frame: Color32::from_rgb(11, 11, 11),
    content: Color32::from_rgb(18, 17, 16),
    content_border: Color32::from_rgb(40, 38, 36),
    text: Color32::from_rgb(232, 232, 232),
    text_muted: Color32::from_rgb(150, 150, 150),
    row_hover: Color32::from_rgb(24, 24, 24),
    row_selected: Color32::from_rgb(34, 34, 34),
    field: Color32::from_rgb(25, 25, 25),
    field_border: Color32::from_rgb(46, 46, 46),
    focus: Color32::from_rgb(190, 190, 190),
    avatar: Color32::from_rgb(74, 58, 48),
    accent: Color32::from_rgb(250, 45, 85),
    bar: Color32::from_rgb(26, 26, 26),
};

static PALETTE: std::sync::RwLock<Palette> = std::sync::RwLock::new(APPLE_MUSIC);

fn palette() -> Palette {
    *PALETTE.read().unwrap_or_else(|e| e.into_inner())
}

impl Palette {
    /// A theme's dark scheme on Sidra's slots, or `None` when a colour isn't `#rrggbb`.
    pub fn from_scheme<S: AsRef<str>>(c: &sidra_core::theme::SchemeColours<S>) -> Option<Self> {
        let hex = |s: &S| {
            let s = s.as_ref().trim_start_matches('#');
            let n = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6)?;
            Some(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
        };
        Some(Self {
            frame: hex(&c.crust)?,
            content: hex(&c.base)?,
            content_border: hex(&c.surface0)?,
            text: hex(&c.text)?,
            text_muted: hex(&c.subtext0)?,
            row_hover: hex(&c.mantle)?,
            row_selected: hex(&c.surface0)?,
            field: hex(&c.mantle)?,
            field_border: hex(&c.surface1)?,
            focus: hex(&c.overlay)?,
            avatar: hex(&c.surface2)?,
            accent: hex(&c.accent)?,
            bar: hex(&c.mantle)?,
        })
    }
}

/// Switches every colour and egui's own widget colours to `p`.
pub fn set_palette(ctx: &egui::Context, p: Palette) {
    *PALETTE.write().unwrap_or_else(|e| e.into_inner()) = p;
    visuals(ctx);
    ctx.request_repaint();
}

macro_rules! colours {
    ($($name:ident),*) => { $(pub fn $name() -> Color32 { palette().$name })* };
}
colours!(
    frame,
    content,
    content_border,
    text,
    text_muted,
    row_hover,
    row_selected,
    field,
    field_border,
    focus,
    avatar,
    accent,
    bar
);

const BOLD: &str = "bold";

pub const BODY_SIZE: f32 = 13.0;
pub const SMALL_SIZE: f32 = 11.5;
pub const ICON_SIZE: f32 = 16.0;

pub fn body() -> FontId {
    FontId::proportional(BODY_SIZE)
}

pub fn small() -> FontId {
    FontId::proportional(SMALL_SIZE)
}

/// Inter SemiBold, for headings and lyrics.
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

pub fn icon() -> FontId {
    FontId::proportional(ICON_SIZE)
}

/// The inset panel the Apple Music page will fill.
pub fn content_frame() -> Frame {
    Frame::new()
        .fill(content())
        .stroke(Stroke::new(1.0, content_border()))
        .corner_radius(CornerRadius::same(12))
}

/// Fonts (with the Phosphor icon glyphs merged in) and dark visuals.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // Inter (SIL OFL, assets/fonts/LICENSE-inter.txt) leads; egui's own fonts stay as fallbacks.
    for (name, bytes) in [
        (
            "inter-medium",
            &include_bytes!("../assets/fonts/Inter-Medium.ttf")[..],
        ),
        (
            "inter-semibold",
            &include_bytes!("../assets/fonts/Inter-SemiBold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(egui::FontData::from_static(bytes)));
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "inter-medium".into());
    let mut bold = proportional.clone();
    bold[0] = "inter-semibold".into();
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold);
    ctx.set_fonts(fonts);

    visuals(ctx);
    ctx.style_mut(|s| {
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        s.text_styles.insert(
            TextStyle::Body,
            FontId::new(BODY_SIZE, FontFamily::Proportional),
        );
        s.text_styles.insert(
            TextStyle::Small,
            FontId::new(SMALL_SIZE, FontFamily::Proportional),
        );
    });
}

fn visuals(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = frame();
    v.window_fill = frame();
    v.extreme_bg_color = field();
    v.override_text_color = Some(text());
    v.selection.stroke = Stroke::new(1.0, focus());
    v.selection.bg_fill = accent().gamma_multiply(0.6);
    v.hyperlink_color = accent();
    v.widgets.inactive.weak_bg_fill = field();
    v.widgets.inactive.bg_fill = field();
    v.widgets.hovered.weak_bg_fill = row_selected();
    v.widgets.hovered.bg_fill = row_selected();
    ctx.set_visuals(v);
}

/// A single line of text cut off with "…" when it would pass `max_width`.
pub fn truncated(ui: &Ui, text: &str, font: FontId, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, self::text());
    job.wrap = TextWrapping::truncate_at_width(max_width.max(0.0));
    ui.painter().layout_job(job)
}

pub fn focus_ring(ui: &Ui, response: &Response, rect: Rect, radius: u8) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(radius),
            Stroke::new(1.0, focus()),
            StrokeKind::Inside,
        );
    }
}

/// A square-ish glyph button. `selected` makes it a toggle and reports that state.
pub fn icon_button(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    size: Vec2,
    selected: Option<bool>,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| match selected {
        Some(on) => WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), on, label),
        None => WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label),
    });
    let on = selected == Some(true);
    // Translucent, so the states read on the frame and inside a pill alike.
    let fill = if on {
        Color32::from_white_alpha(24)
    } else if response.hovered() {
        Color32::from_white_alpha(12)
    } else {
        Color32::TRANSPARENT
    };
    let radius = (size.y / 2.0) as u8;
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius), fill);
    focus_ring(ui, &response, rect, radius);
    let colour = if on || response.hovered() {
        text()
    } else {
        text_muted()
    };
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, glyph, icon(), colour);
    response.on_hover_text(label)
}

/// The rounded group the toolbar's toggles sit in.
pub fn pill_frame() -> Frame {
    Frame::new()
        .fill(field())
        .stroke(Stroke::new(1.0, field_border()))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(egui::Margin::same(3))
}

/// A vertical gradient over `rect`, for fading content into the background at an edge.
pub fn vertical_fade(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    painter.add(mesh);
}
