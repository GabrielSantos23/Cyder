//! The right sidebar: the play queue or the lyrics, opened from the content toolbar.
//! It sits on the frame colour like the left sidebar, so the frame wraps both sides.

use crate::chrome;
use crate::engine::Item;
use crate::pages::Cx;
use crate::store::{Req, Res};
use crate::style::{self, icon_button, truncated};
use crate::{karaoke, preset};
use egui::{
    Align, Color32, CornerRadius, Layout, Rect, RichText, ScrollArea, Sense, Ui, WidgetInfo,
    WidgetType, pos2, vec2,
};
use egui_phosphor::regular as icon;
use serde_json::json;
use sidra_core::apple::Artwork;

pub const WIDTH: f32 = 290.0;
const TRACK_HEIGHT: f32 = 48.0;
const ART: f32 = 36.0;
const PAD: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Queue,
    Lyrics,
}

#[derive(Default)]
pub struct RightPanel {
    pub view: Option<View>,
    lyric_view: karaoke::View,
}

impl RightPanel {
    /// Opens `view`, or closes the panel when it is already showing.
    pub fn toggle(&mut self, view: View) {
        self.view = if self.view == Some(view) {
            None
        } else {
            Some(view)
        };
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
        chrome::window_controls(ui);
        ui.add_space(8.0);
        // Frees the lyrics' glow textures while they aren't on screen.
        self.lyric_view
            .release_hidden(ui.ctx().cumulative_pass_nr());
        match self.view {
            Some(View::Queue) => queue(ui, cx),
            Some(View::Lyrics) => self.lyrics(ui, cx),
            None => {}
        }
    }

    fn lyrics(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let Some(now) = cx.engine.playback.now.clone() else {
            return note(ui, "Nothing is playing.");
        };
        let Some(catalog) = now.catalog().map(str::to_owned) else {
            return note(
                ui,
                "Lyrics aren't available for songs uploaded to your library.",
            );
        };
        // Syllable lyrics first; a song without them falls back to line lyrics.
        let mut found = None;
        for req in [
            Req::Lyrics(catalog.clone()),
            Req::LineLyrics(catalog.clone()),
        ] {
            let slot = cx.store.get(cx.engine, &req);
            match slot.res.clone() {
                Some(res) if matches!(&*res, Res::Lyrics(_)) => {
                    found = Some(res);
                    break;
                }
                None if slot.error.is_none() => {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| ui.spinner());
                    return;
                }
                _ => {}
            }
        }
        let Some(res) = found else {
            return note(ui, "No lyrics for this song.");
        };
        let Res::Lyrics(lyrics) = &*res else { return };

        // ytm-rs's renderer, in its Apple Music look.
        let n = lyrics.lines.len();
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            catalog.hash(&mut h);
            h.finish()
        };
        let frame = karaoke::Frame {
            lyrics,
            key,
            roman: (0..n).map(|_| None).collect(),
            trans: vec![None; n],
            t: cx.engine.position(cx.now),
            playing: cx.engine.playback.playing,
            now: cx.now,
            rect: ui.available_rect_before_wrap(),
            style: &preset::get("apple-music").lyrics,
            accent: Color32::WHITE,
            centered: false,
            scale: 1.0,
            fullscreen: false,
            glass_chip: None,
        };
        let out = self.lyric_view.show(ui, frame);
        if let Some(t) = out.seek {
            cx.engine.command("seek", json!({ "t": t }));
        }
        if out.animating {
            ui.ctx().request_repaint();
        }
    }
}

fn note(ui: &mut Ui, text: &str) {
    ui.add_space(24.0);
    ui.label(RichText::new(text).color(style::text_muted()));
}

fn queue(ui: &mut Ui, cx: &mut Cx) {
    let pb = cx.engine.playback.clone();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let size = vec2(30.0, 28.0);
        if icon_button(ui, icon::SHUFFLE, "Shuffle", size, Some(pb.shuffle)).clicked() {
            cx.engine.command("shuffle", json!({"on": !pb.shuffle}));
        }
        let repeat_glyph = if pb.repeat == 1 {
            icon::REPEAT_ONCE
        } else {
            icon::REPEAT
        };
        if icon_button(ui, repeat_glyph, "Repeat", size, Some(pb.repeat != 0)).clicked() {
            let next = match pb.repeat {
                0 => 2,
                2 => 1,
                _ => 0,
            };
            cx.engine.command("repeat", json!({"mode": next}));
        }
        if icon_button(ui, icon::INFINITY, "Autoplay", size, Some(pb.autoplay)).clicked() {
            cx.engine.command("autoplay", json!({"on": !pb.autoplay}));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} items", pb.queue.len()))
                    .font(style::small())
                    .color(style::text_muted()),
            );
        });
    });
    ui.add_space(14.0);

    let Some(now) = pb.queue.get(pb.position).or(pb.now.as_ref()) else {
        return note(ui, "The queue is empty. Play something to fill it.");
    };
    heading(ui, "Now Playing");
    ui.add_space(4.0);
    track_row(ui, now, true);
    ui.add_space(16.0);

    let next = pb.queue.len().saturating_sub(pb.position + 1);
    ui.horizontal(|ui| {
        heading(ui, "Playing Next");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !pb.queue.is_empty() {
                ui.label(
                    RichText::new(format!("{} of {}", pb.position + 1, pb.queue.len()))
                        .font(style::small())
                        .color(style::text_muted()),
                );
            }
        });
    });
    ui.add_space(6.0);
    if next == 0 {
        let text = if pb.autoplay {
            "Autoplay picks what comes next."
        } else {
            "Nothing else is queued."
        };
        return note(ui, text);
    }
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, TRACK_HEIGHT + 2.0, next, |ui, range| {
            for i in range {
                let index = pb.position + 1 + i;
                if track_row(ui, &pb.queue[index], false).clicked() {
                    cx.engine.command("jump", json!({"index": index}));
                }
            }
        });
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(style::bold(15.0))
            .color(style::text()),
    );
}

/// A queue entry: artwork, title, then "artist – album".
fn track_row(ui: &mut Ui, track: &Item, current: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), TRACK_HEIGHT), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            ui.is_enabled(),
            current,
            format!("{}, {}", track.title, track.artist),
        )
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let fill = if current {
        style::row_selected()
    } else if response.hovered() {
        style::row_hover()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    style::focus_ring(ui, &response, rect, 8);

    let art = Rect::from_min_size(
        pos2(rect.left() + PAD, rect.center().y - ART / 2.0),
        vec2(ART, ART),
    );
    let artwork = track.artwork.as_ref().map(|t| Artwork {
        template: t.clone(),
        bg: None,
    });
    crate::widgets::paint_art(ui, art, artwork.as_ref(), 4, false);

    let left = art.right() + 10.0;
    let width = rect.right() - PAD - left;
    let title = truncated(ui, &track.title, style::body(), width);
    let detail = truncated(
        ui,
        &format!("{} – {}", track.artist, track.album),
        style::small(),
        width,
    );
    let top = rect.center().y - (title.size().y + detail.size().y + 2.0) / 2.0;
    let title_h = title.size().y;
    ui.painter().galley(pos2(left, top), title, style::text());
    ui.painter()
        .galley(pos2(left, top + title_h + 2.0), detail, style::text_muted());
    response.on_hover_text(format!("{} – {}", track.title, track.artist))
}
