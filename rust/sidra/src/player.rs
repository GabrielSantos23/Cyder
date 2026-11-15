//! The floating player bar at the bottom of the content panel.

use crate::pages::{Cx, Route};
use crate::style::{self, icon_button, truncated};
use crate::widgets::duration;
use egui::{
    Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};
use egui_phosphor::regular as icon;
use serde_json::json;
use sidra_core::apple::Artwork;

pub const HEIGHT: f32 = 92.0;

pub fn bar(ui: &mut Ui, cx: &mut Cx, rect: Rect) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(16), style::bar());
    p.rect_stroke(
        rect,
        CornerRadius::same(16),
        Stroke::new(1.0, Color32::from_white_alpha(22)),
        StrokeKind::Inside,
    );
    // Swallow clicks on the bar's background so they don't reach the page beneath.
    ui.interact(rect, ui.id().with("player-bg"), Sense::click());

    let pb = cx.engine.playback.clone();
    let pos = cx.engine.position(cx.now);
    let inner = rect.shrink2(vec2(18.0, 10.0));

    // Seek row.
    let seek_row = Rect::from_min_size(inner.min, vec2(inner.width(), 22.0));
    let time_w = 44.0;
    let track = Rect::from_min_max(
        pos2(seek_row.left() + time_w, seek_row.center().y - 8.0),
        pos2(seek_row.right() - time_w, seek_row.center().y + 8.0),
    );
    let seekable = pb.can_seek && pb.duration > 0.0;
    let sense = if seekable {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let seek = ui.interact(track, ui.id().with("seek"), sense);
    seek.widget_info(|| egui::WidgetInfo::slider(true, pos, "Playback position"));
    let frac_at = |x: f32| ((x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
    let mut shown = if pb.live {
        1.0
    } else if pb.duration > 0.0 {
        pos / pb.duration
    } else {
        0.0
    };
    if let Some(p) = seek.interact_pointer_pos() {
        shown = frac_at(p.x);
    }
    if (seek.drag_stopped() || seek.clicked())
        && seekable
        && let Some(p) = seek.interact_pointer_pos().or(seek.hover_pos())
    {
        cx.engine
            .command("seek", json!({"t": frac_at(p.x) * pb.duration}));
    }
    let line = Rect::from_center_size(
        track.center(),
        vec2(
            track.width(),
            if seekable && (seek.hovered() || seek.dragged()) {
                6.0
            } else {
                4.0
            },
        ),
    );
    let p = ui.painter();
    p.rect_filled(line, CornerRadius::same(3), Color32::from_white_alpha(40));
    let filled = Rect::from_min_size(line.min, vec2(line.width() * shown as f32, line.height()));
    p.rect_filled(
        filled,
        CornerRadius::same(3),
        Color32::from_white_alpha(if pb.live { 90 } else { 230 }),
    );
    if pb.live {
        p.text(
            pos2(seek_row.left(), seek_row.center().y),
            Align2::LEFT_CENTER,
            "LIVE",
            style::bold(11.0),
            style::accent(),
        );
    } else {
        let shown_secs = shown * pb.duration;
        p.text(
            pos2(seek_row.left(), seek_row.center().y),
            Align2::LEFT_CENTER,
            duration((shown_secs * 1000.0) as u64),
            style::small(),
            style::text_muted(),
        );
        let left = ((pb.duration - shown_secs).max(0.0) * 1000.0) as u64;
        p.text(
            pos2(seek_row.right(), seek_row.center().y),
            Align2::RIGHT_CENTER,
            format!("-{}", duration(left)),
            style::small(),
            style::text_muted(),
        );
    }

    // Control row.
    let row = Rect::from_min_max(pos2(inner.left(), seek_row.bottom() + 6.0), inner.max);
    let catalog = pb
        .now
        .as_ref()
        .filter(|_| !pb.live)
        .and_then(|n| n.catalog().map(str::to_owned));
    let loved = catalog.as_deref().is_some_and(|c| {
        cx.store
            .loved(cx.engine, &[c])
            .get(c)
            .copied()
            .unwrap_or(false)
    });
    let in_library = pb.now.as_ref().is_some_and(|n| n.id.starts_with("i."));
    let size = vec2(34.0, 34.0);

    ui.scope_builder(
        UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let playing_something = pb.now.is_some();
            let enabled = |ui: &mut Ui, on: bool, add: &mut dyn FnMut(&mut Ui)| {
                ui.add_enabled_ui(on && playing_something, |ui| add(ui));
            };
            enabled(ui, pb.can_edit, &mut |ui| {
                if icon_button(
                    ui,
                    icon::SHUFFLE,
                    "Shuffle",
                    size,
                    Some(pb.shuffle && pb.can_edit),
                )
                .clicked()
                {
                    cx.engine.command("shuffle", json!({"on": !pb.shuffle}));
                }
            });
            enabled(ui, pb.can_prev, &mut |ui| {
                if icon_button(ui, icon::SKIP_BACK, "Previous", size, None).clicked() {
                    cx.engine.command("prev", json!({}));
                }
            });
            // Live radio can't pause: Apple's player offers Stop in its place.
            let (glyph, label) = match (pb.playing, pb.can_pause) {
                (true, true) => (icon::PAUSE, "Pause"),
                (true, false) => (icon::STOP, "Stop"),
                _ => (icon::PLAY, "Play"),
            };
            if cx.engine.loading.is_some() {
                let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                egui::Spinner::new()
                    .size(20.0)
                    .color(style::text())
                    .paint_at(ui, Rect::from_center_size(r.center(), vec2(20.0, 20.0)));
                ui.ctx().request_repaint();
            } else {
                enabled(ui, true, &mut |ui| {
                    if icon_button(ui, glyph, label, vec2(40.0, 40.0), None).clicked() {
                        cx.engine.command("toggle", json!({}));
                    }
                });
            }
            enabled(ui, pb.can_next, &mut |ui| {
                if icon_button(ui, icon::SKIP_FORWARD, "Next", size, None).clicked() {
                    cx.engine.command("next", json!({}));
                }
            });
            // MusicKit's repeat modes: 0 off, 1 one, 2 all. The button cycles off, all, one.
            let (glyph, label) = match pb.repeat {
                1 => (icon::REPEAT_ONCE, "Repeat one"),
                2 => (icon::REPEAT, "Repeat all"),
                _ => (icon::REPEAT, "Repeat"),
            };
            enabled(ui, pb.can_edit, &mut |ui| {
                if icon_button(ui, glyph, label, size, Some(pb.repeat != 0 && pb.can_edit))
                    .clicked()
                {
                    let next = match pb.repeat {
                        0 => 2,
                        2 => 1,
                        _ => 0,
                    };
                    cx.engine.command("repeat", json!({"mode": next}));
                }
            });
            if pb.now.as_ref().is_some_and(|n| n.is_video()) {
                let open = cx.engine.video_open;
                let label = if open { "Hide Video" } else { "Show Video" };
                if icon_button(ui, icon::MONITOR_PLAY, label, size, Some(open)).clicked() {
                    cx.engine.video_open = !open;
                }
            }
            ui.add_space(8.0);
            ui.add_enabled_ui(catalog.is_some() && !in_library, |ui| {
                let label = if in_library {
                    "In your library"
                } else {
                    "Add to Library"
                };
                let glyph = if in_library {
                    icon::CHECK_CIRCLE
                } else {
                    icon::PLUS_CIRCLE
                };
                if icon_button(ui, glyph, label, size, None).clicked()
                    && let Some(c) = &catalog
                {
                    cx.store.add_to_library(cx.engine, "songs", c);
                }
            });
            ui.add_enabled_ui(catalog.is_some(), |ui| {
                let r = icon_button(
                    ui,
                    icon::STAR,
                    if loved {
                        "Remove from Favourites"
                    } else {
                        "Favourite"
                    },
                    size,
                    Some(loved),
                );
                if loved {
                    ui.painter().text(
                        r.rect.center(),
                        Align2::CENTER_CENTER,
                        icon::STAR,
                        style::icon(),
                        style::accent(),
                    );
                }
                if r.clicked()
                    && let Some(c) = &catalog
                {
                    cx.store.set_loved(cx.engine, c, !loved);
                }
            });
        },
    );

    // Now playing, centred.
    let card_w = (row.width() * 0.4).clamp(240.0, 460.0);
    let card = Rect::from_center_size(
        pos2(row.center().x + 30.0, row.center().y),
        vec2(card_w, 44.0),
    );
    if let Some(now) = &pb.now {
        let r = ui.interact(card, ui.id().with("now-card"), Sense::click());
        r.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Now playing: {}, {}", now.title, now.artist),
            )
        });
        let p = ui.painter();
        p.rect_filled(
            card,
            CornerRadius::same(8),
            Color32::from_white_alpha(if r.hovered() { 22 } else { 12 }),
        );
        let art = Rect::from_min_size(card.min + vec2(4.0, 4.0), vec2(36.0, 36.0));
        crate::widgets::paint_art(
            ui,
            art,
            now.artwork
                .as_ref()
                .map(|t| Artwork {
                    template: t.clone(),
                    bg: None,
                })
                .as_ref(),
            5,
            false,
        );
        let w = card.right() - art.right() - 16.0;
        let title = truncated(ui, &now.title, style::bold(13.0), w);
        let sub = truncated(
            ui,
            &format!("{} – {}", now.album, now.artist),
            style::small(),
            w,
        );
        ui.painter().galley(
            pos2(art.right() + 10.0, card.top() + 5.0),
            title,
            style::text(),
        );
        ui.painter().galley(
            pos2(art.right() + 10.0, card.top() + 23.0),
            sub,
            style::text_muted(),
        );
        if r.on_hover_text("Go to album").clicked() {
            match (now.album_id(), now.catalog()) {
                (Some(id), _) => cx.nav.push(Route::Album(id)),
                (None, Some(c)) => cx.store.goto_from_song(cx.engine, c, false),
                _ => {}
            }
        }
    } else {
        ui.painter().text(
            card.center(),
            Align2::CENTER_CENTER,
            "Not playing",
            style::body(),
            style::text_muted(),
        );
    }

    // Volume, right.
    let vol_w = 150.0;
    let vol = Rect::from_min_max(pos2(row.right() - vol_w, row.top()), row.max);
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(vol)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            let muted = pb.volume <= 0.001;
            let glyph = if muted {
                icon::SPEAKER_X
            } else if pb.volume < 0.5 {
                icon::SPEAKER_LOW
            } else {
                icon::SPEAKER_HIGH
            };
            if icon_button(ui, glyph, if muted { "Unmute" } else { "Mute" }, size, None).clicked() {
                let id = ui.id().with("unmuted");
                let restore: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(1.0);
                if !muted {
                    ui.data_mut(|d| d.insert_temp(id, pb.volume));
                }
                cx.engine
                    .command("volume", json!({"v": if muted { restore } else { 0.0 }}));
            }
            let (r, resp) = ui.allocate_exact_size(
                vec2(ui.available_width() - 4.0, 20.0),
                Sense::click_and_drag(),
            );
            resp.widget_info(|| egui::WidgetInfo::slider(true, pb.volume as f64, "Volume"));
            let mut v = pb.volume;
            if let Some(p) = resp.interact_pointer_pos() {
                v = ((p.x - r.left()) / r.width()).clamp(0.0, 1.0);
                if resp.dragged() || resp.clicked() {
                    cx.engine.playback.volume = v;
                    cx.engine
                        .command("volume", json!({"v": (v * 100.0).round() / 100.0}));
                }
            }
            let line = Rect::from_center_size(r.center(), vec2(r.width(), 4.0));
            ui.painter()
                .rect_filled(line, CornerRadius::same(2), Color32::from_white_alpha(40));
            ui.painter().rect_filled(
                Rect::from_min_size(line.min, vec2(line.width() * v, 4.0)),
                CornerRadius::same(2),
                Color32::from_white_alpha(220),
            );
        },
    );
}
