//! Window chrome for the frameless window: moving, edge resizing and the
//! minimise / maximise / close buttons Sidra draws itself.

use crate::style;
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Layout, ResizeDirection, Sense, Ui, ViewportCommand,
    WidgetInfo, WidgetType, vec2,
};
use egui_phosphor::regular as icon;

/// How far into the window an edge or corner can be grabbed for resizing.
const GRAB: f32 = 6.0;
const BUTTON: egui::Vec2 = vec2(36.0, 28.0);
pub const CONTROLS_HEIGHT: f32 = 28.0;
/// Windows' own close-button hover colour.
const CLOSE_HOVER: Color32 = Color32::from_rgb(196, 43, 28);

fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

/// Resizes from the window edges. Returns true while the pointer is over a grab zone,
/// so the background does not also start a move.
pub fn edge_resize(ctx: &egui::Context) -> bool {
    if maximized(ctx) {
        return false;
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return false;
    };
    let r = ctx.content_rect();
    let (l, rt) = (pos.x - r.left() < GRAB, r.right() - pos.x < GRAB);
    let (t, b) = (pos.y - r.top() < GRAB, r.bottom() - pos.y < GRAB);
    let (dir, cursor) = match (l, rt, t, b) {
        (true, _, true, _) => (ResizeDirection::NorthWest, CursorIcon::ResizeNwSe),
        (_, true, _, true) => (ResizeDirection::SouthEast, CursorIcon::ResizeNwSe),
        (_, true, true, _) => (ResizeDirection::NorthEast, CursorIcon::ResizeNeSw),
        (true, _, _, true) => (ResizeDirection::SouthWest, CursorIcon::ResizeNeSw),
        (true, ..) => (ResizeDirection::West, CursorIcon::ResizeHorizontal),
        (_, true, ..) => (ResizeDirection::East, CursorIcon::ResizeHorizontal),
        (_, _, true, _) => (ResizeDirection::North, CursorIcon::ResizeVertical),
        (.., true) => (ResizeDirection::South, CursorIcon::ResizeVertical),
        _ => return false,
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
    }
    true
}

/// Makes every empty stretch of the window frame a title bar: drag to move,
/// double-click to maximise. Call before the panels so they sit on top of it.
pub fn drag_background(ctx: &egui::Context, ui: &mut Ui, resizing: bool) {
    let response = ui.interact(
        ctx.content_rect(),
        ui.id().with("titlebar"),
        Sense::click_and_drag(),
    );
    if resizing {
        return;
    }
    if response.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized(ctx)));
    } else if response.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// Minimise, maximise or restore, and close, right-aligned in `ui`.
pub fn window_controls(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let max = maximized(&ctx);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), CONTROLS_HEIGHT),
        Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if control(ui, icon::X, "Close", true) {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            let (glyph, label) = if max {
                (icon::COPY, "Restore")
            } else {
                (icon::SQUARE, "Maximise")
            };
            if control(ui, glyph, label, false) {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!max));
            }
            if control(ui, icon::MINUS, "Minimise", false) {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }
        },
    );
}

fn control(ui: &mut Ui, glyph: &str, label: &str, close: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(BUTTON, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let hovered = response.hovered();
    if hovered {
        let fill = if close {
            CLOSE_HOVER
        } else {
            style::ROW_SELECTED
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    }
    style::focus_ring(ui, &response, rect, 6);
    let colour = if hovered {
        Color32::WHITE
    } else {
        style::TEXT_MUTED
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(14.0),
        colour,
    );
    response.on_hover_text(label).clicked()
}
