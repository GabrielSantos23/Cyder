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

/// Window frame and sidebar background.
pub const FRAME: Color32 = Color32::from_rgb(11, 11, 11);
/// The inset content panel and its hairline border.
pub const CONTENT: Color32 = Color32::from_rgb(18, 17, 16);
pub const CONTENT_BORDER: Color32 = Color32::from_rgb(40, 38, 36);
/// Primary text: 17:1 on FRAME.
pub const TEXT: Color32 = Color32::from_rgb(232, 232, 232);
/// Section labels and placeholders: 6.4:1 on FRAME, above the 4.5:1 floor.
pub const TEXT_MUTED: Color32 = Color32::from_rgb(150, 150, 150);
pub const ROW_HOVER: Color32 = Color32::from_rgb(24, 24, 24);
pub const ROW_SELECTED: Color32 = Color32::from_rgb(34, 34, 34);
pub const FIELD: Color32 = Color32::from_rgb(25, 25, 25);
pub const FIELD_BORDER: Color32 = Color32::from_rgb(46, 46, 46);
/// Keyboard focus ring.
pub const FOCUS: Color32 = Color32::from_rgb(190, 190, 190);
/// Avatar placeholder behind the account initial.
pub const AVATAR: Color32 = Color32::from_rgb(74, 58, 48);

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
        .fill(CONTENT)
        .stroke(Stroke::new(1.0, CONTENT_BORDER))
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

    ctx.set_visuals(egui::Visuals::dark());
    ctx.style_mut(|s| {
        s.visuals.panel_fill = FRAME;
        s.visuals.window_fill = FRAME;
        s.visuals.extreme_bg_color = FIELD;
        s.visuals.selection.stroke = Stroke::new(1.0, FOCUS);
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

/// A single line of text cut off with "…" when it would pass `max_width`.
pub fn truncated(ui: &Ui, text: &str, font: FontId, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, TEXT);
    job.wrap = TextWrapping::truncate_at_width(max_width.max(0.0));
    ui.painter().layout_job(job)
}

pub fn focus_ring(ui: &Ui, response: &Response, rect: Rect, radius: u8) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(radius),
            Stroke::new(1.0, FOCUS),
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
        TEXT
    } else {
        TEXT_MUTED
    };
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, glyph, icon(), colour);
    response.on_hover_text(label)
}

/// The rounded group the toolbar's toggles sit in.
pub fn pill_frame() -> Frame {
    Frame::new()
        .fill(FIELD)
        .stroke(Stroke::new(1.0, FIELD_BORDER))
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
