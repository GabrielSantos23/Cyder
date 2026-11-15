//! Pieces the pages share: artwork, cards and shelves, buttons, the track list and
//! the menus that act on songs, albums and playlists.

use crate::pages::{Cx, Route};
use crate::style::{self, truncated};
use egui::{
    Align2, Color32, CornerRadius, Image, Rect, Response, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use egui_phosphor::regular as icon;
use serde_json::json;
use sidra_core::apple::{Artwork, Card, CardKind, Track};

/// Artwork in `rect`, or a neutral tile while it loads or when there is none.
pub fn paint_art(ui: &Ui, rect: Rect, art: Option<&Artwork>, radius: u8, round: bool) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let radius = if round {
        (rect.width() / 2.0) as u8
    } else {
        radius
    };
    let fill = art
        .and_then(|a| a.bg)
        .map_or(style::field(), |[r, g, b]| Color32::from_rgb(r, g, b));
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius), fill);
    let px = (rect.width() * ui.ctx().pixels_per_point()).ceil() as u32;
    // Apple renders sizes in steps; asking for a common one shares cache entries.
    let px = [64, 128, 256, 400, 600, 1000]
        .into_iter()
        .find(|s| *s >= px)
        .unwrap_or(1200);
    if let Some(url) = art.and_then(|a| a.url(px)) {
        Image::new(url)
            .corner_radius(CornerRadius::same(radius))
            .show_loading_spinner(false)
            .paint_at(ui, rect);
    } else {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon::MUSIC_NOTE,
            egui::FontId::proportional((rect.width() * 0.35).clamp(12.0, 64.0)),
            style::text_muted(),
        );
    }
}

/// A rounded button with an icon and label; `primary` is the white Play button.
pub fn pill_button(ui: &mut Ui, glyph: &str, label: &str, primary: bool, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let hovered = response.hovered();
    let (fill, text) = match (primary, hovered) {
        (true, false) => (Color32::WHITE, Color32::BLACK),
        (true, true) => (Color32::from_gray(225), Color32::BLACK),
        (false, false) => (Color32::from_white_alpha(18), style::text()),
        (false, true) => (Color32::from_white_alpha(30), style::text()),
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(17), fill);
    if !primary {
        p.rect_stroke(
            rect,
            CornerRadius::same(17),
            Stroke::new(1.0, Color32::from_white_alpha(20)),
            StrokeKind::Inside,
        );
    }
    style::focus_ring(ui, &response, rect, 17);
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), style::bold(13.0), text);
    let total = 16.0 + 8.0 + galley.size().x;
    let x = rect.center().x - total / 2.0;
    p.text(
        pos2(x + 8.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        style::icon(),
        text,
    );
    p.galley(
        pos2(x + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        text,
    );
    response
}

/// A round icon button on a translucent disc, like the "…" beside Add.
pub fn disc_button(ui: &mut Ui, glyph: &str, label: &str, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let fill = if response.hovered() { 30 } else { 18 };
    ui.painter()
        .circle_filled(rect.center(), size / 2.0, Color32::from_white_alpha(fill));
    style::focus_ring(ui, &response, rect, (size / 2.0) as u8);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        style::icon(),
        style::text(),
    );
    response.on_hover_text(label)
}

pub fn chip(ui: &mut Ui, glyph: &str, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), style::bold(11.0), style::text());
    let size = vec2(galley.size().x + 32.0, 22.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(11), Color32::from_white_alpha(14));
    p.rect_stroke(
        rect,
        CornerRadius::same(11),
        Stroke::new(1.0, Color32::from_white_alpha(18)),
        StrokeKind::Inside,
    );
    p.text(
        pos2(rect.left() + 13.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(12.0),
        style::text_muted(),
    );
    p.galley(
        pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        style::text(),
    );
}

pub fn heading(ui: &mut Ui, text: &str, size: f32) {
    ui.label(
        RichText::new(text)
            .font(style::bold(size))
            .color(style::text()),
    );
}

/// The queue options for a card, or `None` for an artist, which has nothing to play directly.
pub fn queue_opts(kind: CardKind, id: &str) -> Option<serde_json::Value> {
    Some(match kind {
        CardKind::Album => json!({"album": id}),
        CardKind::Playlist => json!({"playlist": id}),
        CardKind::Station => json!({"station": id}),
        CardKind::Song => json!({"song": id}),
        CardKind::MusicVideo => json!({"musicVideo": id}),
        CardKind::Artist => return None,
    })
}

fn library_kind(kind: CardKind) -> Option<&'static str> {
    match kind {
        CardKind::Album => Some("albums"),
        CardKind::Playlist => Some("playlists"),
        CardKind::Song => Some("songs"),
        CardKind::MusicVideo => Some("music-videos"),
        _ => None,
    }
}

pub fn open_card(cx: &mut Cx, card: &Card) {
    match card.kind {
        CardKind::Album => cx.nav.push(Route::Album(card.id.clone())),
        CardKind::Playlist => cx.nav.push(Route::Playlist(card.id.clone())),
        CardKind::Artist => cx.nav.push(Route::Artist(card.id.clone())),
        CardKind::Station | CardKind::Song | CardKind::MusicVideo => {
            if let Some(opts) = queue_opts(card.kind, &card.id) {
                cx.engine.command("setQueue", json!({"opts": opts}));
            }
        }
    }
}

/// The right-click menu of a card.
fn card_menu(ui: &mut Ui, cx: &mut Cx, card: &Card) {
    if let Some(opts) = queue_opts(card.kind, &card.id) {
        if ui.button("Play").clicked() {
            cx.engine.command("setQueue", json!({"opts": opts}));
        }
        if card.kind != CardKind::Station {
            if ui.button("Shuffle").clicked() {
                cx.engine
                    .command("setQueue", json!({"opts": opts, "shuffle": true}));
            }
            if ui.button("Play Next").clicked() {
                cx.engine.command("playNext", json!({"opts": opts}));
            }
            if ui.button("Play Later").clicked() {
                cx.engine.command("playLater", json!({"opts": opts}));
            }
        }
    }
    if let Some(kind) = library_kind(card.kind)
        && !is_library_id(&card.id)
        && ui.button("Add to Library").clicked()
    {
        cx.store.add_to_library(cx.engine, kind, &card.id);
    }
    if card.kind != CardKind::Station && ui.button("Open").clicked() {
        open_card(cx, card);
    }
}

pub fn is_library_id(id: &str) -> bool {
    ["l.", "p.", "i.", "r."].iter().any(|p| id.starts_with(p))
}

/// A tile: artwork, then title and subtitle. Click opens; right-click offers more.
pub fn card(ui: &mut Ui, cx: &mut Cx, card: &Card, width: f32) {
    let round = card.kind == CardKind::Artist;
    let height = width + 42.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            ui.is_enabled(),
            format!("{}, {}", card.title, card.subtitle),
        )
    });
    if ui.is_rect_visible(rect) {
        let art = Rect::from_min_size(rect.min, vec2(width, width));
        paint_art(ui, art, card.artwork.as_ref(), 10, round);
        if response.hovered() {
            ui.painter().rect_filled(
                art,
                CornerRadius::same(if round { (width / 2.0) as u8 } else { 10 }),
                Color32::from_black_alpha(50),
            );
            if card.kind != CardKind::Artist {
                let play =
                    Rect::from_center_size(art.left_bottom() + vec2(24.0, -24.0), vec2(32.0, 32.0));
                ui.painter()
                    .circle_filled(play.center(), 16.0, Color32::from_white_alpha(230));
                ui.painter().text(
                    play.center() + vec2(1.0, 0.0),
                    Align2::CENTER_CENTER,
                    icon::PLAY,
                    style::icon(),
                    Color32::BLACK,
                );
            }
        }
        style::focus_ring(ui, &response, art, 10);
        let align = if round { 0.5 } else { 0.0 };
        let title = truncated(ui, &card.title, style::body(), width);
        let sub = truncated(ui, &card.subtitle, style::small(), width);
        let tx = |w: f32| rect.left() + (width - w) * align;
        let (tw, sw) = (title.size().x, sub.size().x);
        ui.painter()
            .galley(pos2(tx(tw), art.bottom() + 6.0), title, style::text());
        ui.painter()
            .galley(pos2(tx(sw), art.bottom() + 24.0), sub, style::text_muted());
    }
    // A click on the play disc plays; anywhere else opens.
    let play_hit = response.hover_pos().is_some_and(|p| {
        p.distance(rect.min + vec2(24.0, width - 24.0)) < 18.0 && card.kind != CardKind::Artist
    });
    if response.clicked() {
        match (play_hit, queue_opts(card.kind, &card.id)) {
            (true, Some(opts)) => cx.engine.command("setQueue", json!({"opts": opts})),
            _ => open_card(cx, card),
        }
    }
    response.context_menu(|ui| card_menu(ui, cx, card));
}

/// The loaded image's size, or `None` while it loads.
fn image_size(ui: &Ui, url: &str) -> Option<egui::Vec2> {
    match egui::Image::new(url.to_owned()).load_for_size(ui.ctx(), vec2(1.0, 1.0)) {
        Ok(egui::load::TexturePoll::Ready { texture }) => Some(texture.size),
        _ => None,
    }
}

/// Fills `rect` with the image cropped to its shape, the way CSS `object-fit: cover` does,
/// so an image of another shape is never stretched.
pub fn paint_cover(ui: &Ui, url: &str, rect: Rect, corners: CornerRadius) {
    let Some(size) = image_size(ui, url) else {
        return;
    };
    let (img, want) = (size.x / size.y, rect.width() / rect.height());
    let uv = if img > want {
        let w = want / img;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    } else {
        // Taller than the box: keep the top, where faces usually are.
        let h = img / want;
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, h))
    };
    Image::new(url.to_owned())
        .uv(uv)
        .corner_radius(corners)
        .show_loading_spinner(false)
        .paint_at(ui, rect);
}

/// Draws the image as large as fits in `bounds`, left and bottom aligned. False while it loads.
pub fn paint_fit(ui: &Ui, url: &str, bounds: Rect) -> bool {
    let Some(size) = image_size(ui, url) else {
        return false;
    };
    let scale = (bounds.width() / size.x).min(bounds.height() / size.y);
    let s = size * scale;
    let rect = Rect::from_min_size(pos2(bounds.left(), bounds.bottom() - s.y), s);
    Image::new(url.to_owned())
        .show_loading_spinner(false)
        .paint_at(ui, rect);
    true
}

/// One Top Picks card: Apple's tall editorial image, or the cover on its colour, with the
/// reason, title and note along the bottom.
fn hero_card(ui: &mut Ui, cx: &mut Cx, card: &Card, size: egui::Vec2) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            ui.is_enabled(),
            format!("{}, {}", card.title, card.subtitle),
        )
    });
    if ui.is_rect_visible(rect) {
        let r = CornerRadius::same(12);
        let bg = card
            .tall
            .as_ref()
            .and_then(|t| t.bg)
            .or_else(|| card.artwork.as_ref().and_then(|a| a.bg))
            .map_or(style::field(), |[r, g, b]| Color32::from_rgb(r, g, b));
        let p = ui.painter();
        p.rect_filled(rect, r, bg);
        let px = (size.x * ui.ctx().pixels_per_point()).ceil() as u32;
        match card.tall.as_ref().and_then(|t| t.url_wh(px, px * 4 / 3)) {
            Some(url) => paint_cover(ui, &url, rect, r),
            None => {
                // No tall image (an artist station): the cover, centred on its own colour.
                let side = size.x * 0.62;
                let art = Rect::from_center_size(
                    rect.center() - vec2(0.0, size.y * 0.1),
                    vec2(side, side),
                );
                paint_art(
                    ui,
                    art,
                    card.artwork.as_ref(),
                    8,
                    card.kind == CardKind::Station,
                );
            }
        }
        // A dark wash under the text so it reads on any image.
        let wash = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 110.0), rect.max);
        style::vertical_fade(
            ui.painter(),
            wash,
            Color32::TRANSPARENT,
            Color32::from_black_alpha(170),
        );
        let w = size.x - 32.0;
        let mut y = rect.bottom() - 16.0;
        let lines = [
            (
                card.note.as_deref().unwrap_or(&card.subtitle),
                style::small(),
                Color32::from_white_alpha(200),
            ),
            (card.title.as_str(), style::bold(15.0), Color32::WHITE),
            (
                card.reason.as_deref().unwrap_or(""),
                style::bold(11.5),
                Color32::from_white_alpha(220),
            ),
        ];
        for (text, font, colour) in lines {
            if text.is_empty() {
                continue;
            }
            let g = truncated(ui, text, font, w);
            y -= g.size().y + 2.0;
            ui.painter().galley(pos2(rect.left() + 16.0, y), g, colour);
        }
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, r, Color32::from_black_alpha(30));
        }
        style::focus_ring(ui, &response, rect, 12);
    }
    if response.clicked() {
        open_card(cx, card);
    }
    response.context_menu(|ui| card_menu(ui, cx, card));
}

/// Home's Top Picks: a row of tall editorial cards.
pub fn hero_shelf(ui: &mut Ui, cx: &mut Cx, id: &str, title: &str, cards: &[Card]) {
    heading(ui, title, 17.0);
    ui.add_space(8.0);
    let w = 260.0;
    ScrollArea::horizontal()
        .id_salt(("hero", id))
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for c in cards {
                    hero_card(ui, cx, c, vec2(w, w * 4.0 / 3.0));
                }
            });
        });
    ui.add_space(24.0);
}

/// A titled horizontal row of cards with arrows to page through it.
pub fn shelf(ui: &mut Ui, cx: &mut Cx, id: &str, title: &str, cards: &[Card], width: f32) {
    if cards.is_empty() {
        return;
    }
    let scroll_id = ui.id().with(("shelf", id));
    let mut step = 0.0;
    ui.horizontal(|ui| {
        heading(ui, title, 17.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if disc_button(ui, icon::CARET_RIGHT, "Scroll right", 28.0).clicked() {
                step = 1.0;
            }
            if disc_button(ui, icon::CARET_LEFT, "Scroll left", 28.0).clicked() {
                step = -1.0;
            }
        });
    });
    ui.add_space(8.0);
    let gap = 16.0;
    let mut area = ScrollArea::horizontal()
        .id_salt(scroll_id)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .scroll_source(
            egui::scroll_area::ScrollSource::SCROLL_BAR
                | egui::scroll_area::ScrollSource::MOUSE_WHEEL,
        );
    if step != 0.0 {
        let current = egui::scroll_area::State::load(ui.ctx(), ui.id().with(scroll_id))
            .map_or(0.0, |s| s.offset.x);
        area =
            area.horizontal_scroll_offset((current + step * ui.available_width() * 0.8).max(0.0));
    }
    area.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for c in cards {
                card(ui, cx, c, width);
            }
        });
    });
    ui.add_space(24.0);
}

/// Cards wrapped into as many columns as fit.
pub fn grid(ui: &mut Ui, cx: &mut Cx, cards: &[Card]) {
    let gap = 18.0;
    let cols = ((ui.available_width() + gap) / (180.0 + gap))
        .floor()
        .max(2.0);
    let width = (ui.available_width() - gap * (cols - 1.0)) / cols;
    for row in cards.chunks(cols as usize) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for c in row {
                card(ui, cx, c, width);
            }
        });
        ui.add_space(gap);
    }
}

pub fn spinner(ui: &mut Ui, text: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.spinner();
        ui.label(RichText::new(text).color(style::text_muted()));
    });
}

pub fn empty(ui: &mut Ui, title: &str, detail: &str) {
    ui.add_space(80.0);
    ui.vertical_centered(|ui| {
        heading(ui, title, 18.0);
        ui.add_space(4.0);
        ui.label(RichText::new(detail).color(style::text_muted()));
    });
}

pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// How a track list is ordered.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Default,
    Title,
    Artist,
    Album,
    Duration,
}

impl Sort {
    const ALL: [(Sort, &'static str); 5] = [
        (Sort::Default, "Default order"),
        (Sort::Title, "Title"),
        (Sort::Artist, "Artist"),
        (Sort::Album, "Album"),
        (Sort::Duration, "Duration"),
    ];
}

/// Filter and sort state for the list on the current page.
#[derive(Default)]
pub struct ListState {
    pub filter: String,
    pub searching: bool,
    pub sort: Sort,
}

/// Where a track list's songs come from, which decides how playing one queues the rest.
pub enum Source<'a> {
    Album(&'a str),
    Playlist(&'a str),
    /// A loose list (top songs, search results, library songs): queued as songs.
    Songs,
}

const ROW: f32 = 46.0;

/// The track list: header with filter and sort, then virtualised rows.
pub fn track_list(ui: &mut Ui, cx: &mut Cx, tracks: &[Track], source: Source, numbered: bool) {
    // Header: filter toggle and sort menu.
    ui.horizontal(|ui| {
        let searching = cx.list.searching;
        if style::icon_button(
            ui,
            icon::MAGNIFYING_GLASS,
            "Filter songs",
            vec2(30.0, 28.0),
            Some(searching),
        )
        .clicked()
        {
            cx.list.searching = !searching;
            if searching {
                cx.list.filter.clear();
            }
        }
        if cx.list.searching {
            let r = ui.add(
                TextEdit::singleline(&mut cx.list.filter)
                    .hint_text("Filter")
                    .desired_width(220.0),
            );
            if !searching {
                r.request_focus();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = style::icon_button(
                ui,
                icon::SORT_ASCENDING,
                "Sort",
                vec2(30.0, 28.0),
                Some(cx.list.sort != Sort::Default),
            );
            egui::Popup::menu(&r).show(|ui| {
                for (sort, label) in Sort::ALL {
                    if ui.radio(cx.list.sort == sort, label).clicked() {
                        cx.list.sort = sort;
                    }
                }
            });
        });
    });
    ui.add_space(6.0);

    let needle = cx.list.filter.to_lowercase();
    let mut order: Vec<usize> = (0..tracks.len())
        .filter(|&i| {
            needle.is_empty() || {
                let t = &tracks[i];
                t.title.to_lowercase().contains(&needle)
                    || t.artist.to_lowercase().contains(&needle)
                    || t.album.to_lowercase().contains(&needle)
            }
        })
        .collect();
    match cx.list.sort {
        Sort::Default => {}
        Sort::Title => order.sort_by_key(|&i| tracks[i].title.to_lowercase()),
        Sort::Artist => order.sort_by_key(|&i| tracks[i].artist.to_lowercase()),
        Sort::Album => order.sort_by_key(|&i| tracks[i].album.to_lowercase()),
        Sort::Duration => order.sort_by_key(|&i| tracks[i].duration_ms),
    }
    if order.is_empty() {
        empty(
            ui,
            "No songs",
            if needle.is_empty() {
                "There's nothing here yet."
            } else {
                "Nothing matches that filter."
            },
        );
        return;
    }
    let reordered = cx.list.sort != Sort::Default || !needle.is_empty();

    let ids: Vec<&str> = tracks.iter().filter_map(|t| t.catalog()).collect();
    let loved = cx.store.loved(cx.engine, &ids);
    let playing = cx.engine.playback.now.clone();
    let loading = cx.engine.loading.as_ref().and_then(|l| l.track.clone());

    // Only rows inside the clip rect are laid out; the rest is empty space.
    let top = ui.cursor().top();
    let clip = ui.clip_rect();
    let first = (((clip.top() - top) / ROW).floor().max(0.0) as usize).min(order.len());
    let last = ((((clip.bottom() - top) / ROW).ceil().max(0.0)) as usize + 1).min(order.len());
    ui.add_space(first as f32 * ROW);
    for (pos, &i) in order.iter().enumerate().take(last).skip(first) {
        let t = &tracks[i];
        let is_playing = playing
            .as_ref()
            .is_some_and(|n| n.id == t.id || (n.catalog().is_some() && n.catalog() == t.catalog()));
        let love = t
            .catalog()
            .and_then(|c| loved.get(c).copied())
            .unwrap_or(false);
        let is_loading = loading.as_ref() == Some(&t.id);
        let response = track_row(ui, t, pos, numbered, is_playing, is_loading, love);
        if response.double_clicked() {
            play_from(cx, tracks, &order, &source, pos, reordered);
        }
        let dots = Rect::from_center_size(
            pos2(response.rect.right() - 22.0, response.rect.center().y),
            vec2(26.0, 26.0),
        );
        let dots = ui.interact(dots, response.id.with("dots"), Sense::click());
        dots.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "More"));
        egui::Popup::menu(&dots).show(|ui| track_menu(ui, cx, t, love));
        response.context_menu(|ui| track_menu(ui, cx, t, love));
    }
    ui.add_space((order.len() - last) as f32 * ROW);
}

fn play_from(
    cx: &mut Cx,
    tracks: &[Track],
    order: &[usize],
    source: &Source,
    pos: usize,
    reordered: bool,
) {
    let opts = match source {
        // In the container's own order, queue the container so it keeps its name and autoplay.
        Source::Album(id) if !reordered => json!({"album": id, "startWith": order[pos]}),
        Source::Playlist(id) if !reordered => json!({"playlist": id, "startWith": order[pos]}),
        _ => {
            // shortcut: a loose queue takes the next 300 songs; MusicKit slows down on longer item lists.
            let ids: Vec<&str> = order[pos..]
                .iter()
                .take(300)
                .map(|&i| tracks[i].id.as_str())
                .collect();
            json!({"songs": ids})
        }
    };
    cx.engine.command("setQueue", json!({"opts": opts}));
    if let Some(l) = &mut cx.engine.loading {
        l.track = Some(tracks[order[pos]].id.clone());
    }
}

fn track_row(
    ui: &mut Ui,
    t: &Track,
    pos: usize,
    numbered: bool,
    playing: bool,
    loading: bool,
    loved: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            ui.is_enabled(),
            format!("{}, {}, {}", t.title, t.artist, duration(t.duration_ms)),
        )
    });
    let p = ui.painter();
    let fill = if response.hovered() {
        Color32::from_white_alpha(14)
    } else if pos.is_multiple_of(2) {
        Color32::from_white_alpha(6)
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, CornerRadius::same(8), fill);
    style::focus_ring(ui, &response, rect, 8);
    let dim = if t.playable { 255 } else { 110 };
    let text = Color32::from_rgba_unmultiplied(232, 232, 232, dim);

    let lead = rect.left() + 12.0;
    let mut left = lead + 32.0;
    // Playlists show artwork in the number column; the playing marker sits over it.
    if loading && numbered {
        let r = Rect::from_center_size(pos2(lead + 10.0, rect.center().y), vec2(16.0, 16.0));
        egui::Spinner::new()
            .size(16.0)
            .color(style::accent())
            .paint_at(ui, r);
    } else if playing && numbered {
        p.text(
            pos2(lead + 10.0, rect.center().y),
            Align2::CENTER_CENTER,
            icon::WAVEFORM,
            style::icon(),
            style::accent(),
        );
    } else if numbered {
        let n = t
            .number
            .map_or_else(|| (pos + 1).to_string(), |n| n.to_string());
        p.text(
            pos2(lead + 10.0, rect.center().y),
            Align2::CENTER_CENTER,
            n,
            style::small(),
            style::text_muted(),
        );
    }
    if !numbered {
        let art = Rect::from_min_size(pos2(lead, rect.center().y - 17.0), vec2(34.0, 34.0));
        paint_art(ui, art, t.artwork.as_ref(), 4, false);
        if loading {
            p.rect_filled(art, CornerRadius::same(4), Color32::from_black_alpha(150));
            egui::Spinner::new()
                .size(16.0)
                .color(style::accent())
                .paint_at(ui, Rect::from_center_size(art.center(), vec2(16.0, 16.0)));
        } else if playing {
            p.rect_filled(art, CornerRadius::same(4), Color32::from_black_alpha(150));
            p.text(
                art.center(),
                Align2::CENTER_CENTER,
                icon::WAVEFORM,
                style::icon(),
                style::accent(),
            );
        }
        left = art.right() + 12.0;
    }
    let right = rect.right() - 100.0;
    let title = truncated(ui, &t.title, style::bold(13.0), right - left - 40.0);
    let tw = title.size().x;
    let detail = if numbered {
        t.artist.clone()
    } else {
        format!("{} – {}", t.artist, t.album)
    };
    let sub = truncated(ui, &detail, style::small(), right - left);
    let has_sub = !detail.trim_matches(['–', ' ']).is_empty();
    let y = if has_sub {
        rect.center().y - 15.0
    } else {
        rect.center().y - 8.0
    };
    let title_colour = if playing { style::accent() } else { text };
    p.galley(pos2(left, y), title, title_colour);
    let mut badge_x = left + tw + 8.0;
    if t.explicit {
        let b = Rect::from_min_size(pos2(badge_x, y + 2.0), vec2(13.0, 13.0));
        p.rect_filled(b, CornerRadius::same(2), style::text_muted());
        p.text(
            b.center(),
            Align2::CENTER_CENTER,
            "E",
            style::bold(9.0),
            Color32::BLACK,
        );
        badge_x += 19.0;
    }
    if loved {
        p.text(
            pos2(badge_x, y + 8.0),
            Align2::LEFT_CENTER,
            icon::STAR,
            egui::FontId::proportional(12.0),
            style::accent(),
        );
    }
    if has_sub {
        p.galley(pos2(left, y + 17.0), sub, style::text_muted());
    }
    p.text(
        pos2(rect.right() - 48.0, rect.center().y),
        Align2::RIGHT_CENTER,
        duration(t.duration_ms),
        style::small(),
        style::text_muted(),
    );
    p.text(
        pos2(rect.right() - 22.0, rect.center().y),
        Align2::CENTER_CENTER,
        icon::DOTS_THREE,
        style::icon(),
        style::text_muted(),
    );
    response
}

/// Everything that can be done to one song.
pub fn track_menu(ui: &mut Ui, cx: &mut Cx, t: &Track, loved: bool) {
    let opts = json!({"song": t.id});
    if ui.button("Play").clicked() {
        cx.engine.command("setQueue", json!({"opts": opts}));
    }
    if ui.button("Play Next").clicked() {
        cx.engine.command("playNext", json!({"opts": opts}));
    }
    if ui.button("Play Later").clicked() {
        cx.engine.command("playLater", json!({"opts": opts}));
    }
    ui.separator();
    if let Some(catalog) = t.catalog().map(str::to_owned) {
        if !t.id.starts_with("i.") && ui.button("Add to Library").clicked() {
            cx.store.add_to_library(cx.engine, "songs", &catalog);
        }
        let label = if loved {
            "Remove from Favourites"
        } else {
            "Favourite"
        };
        if ui.button(label).clicked() {
            cx.store.set_loved(cx.engine, &catalog, !loved);
        }
    }
    playlist_submenu(ui, cx, t);
    ui.separator();
    if let Some(catalog) = t.catalog().map(str::to_owned) {
        if ui.button("Go to Album").clicked() {
            match &t.album_id {
                Some(id) => cx.nav.push(Route::Album(id.clone())),
                None => cx.store.goto_from_song(cx.engine, &catalog, false),
            }
        }
        if ui.button("Go to Artist").clicked() {
            cx.store.goto_from_song(cx.engine, &catalog, true);
        }
    }
    if let Some(url) = &t.url
        && ui.button("Copy Link").clicked()
    {
        ui.ctx().copy_text(url.clone());
        cx.store.toast("Link copied", cx.now);
    }
}

fn playlist_submenu(ui: &mut Ui, cx: &mut Cx, t: &Track) {
    ui.menu_button("Add to Playlist", |ui| {
        let res = cx
            .store
            .res(cx.engine, &crate::store::Req::LibraryPlaylists);
        let Some(res) = res else {
            ui.spinner();
            return;
        };
        let crate::store::Res::Cards(cards) = &*res else {
            return;
        };
        ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for c in cards {
                if ui.button(&c.title).clicked() {
                    let (id, kind) = if t.id.starts_with("i.") {
                        (t.id.clone(), "library-songs")
                    } else {
                        (t.catalog().unwrap_or(&t.id).to_owned(), "songs")
                    };
                    cx.store.add_to_playlist(cx.engine, &c.id, &id, kind);
                }
            }
        });
    });
}
