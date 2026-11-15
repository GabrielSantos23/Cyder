//! Native pages over Apple Music data: Home, album, playlist, artist, library lists
//! and search, plus the navigation history behind the back and forward buttons.

use crate::engine::Engine;
use crate::store::{Req, Res, Store};
use crate::style;
use crate::widgets::{self, ListState, Source, chip, disc_button, heading, paint_art, pill_button};
use egui::{Align, Color32, Layout, Rect, RichText, ScrollArea, Ui, pos2, vec2};
use egui_phosphor::regular as icon;
use serde_json::json;
use sidra_core::apple::{Artwork, Card, CardKind};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LibraryPage {
    RecentlyAdded,
    Albums,
    Artists,
    Songs,
    Playlists,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    Home,
    Album(String),
    Playlist(String),
    Artist(String),
    Library(LibraryPage),
    Search(String),
    /// A sidebar destination with no native page yet.
    Unavailable(&'static str),
}

/// Back and forward history.
pub struct Nav {
    history: Vec<Route>,
    index: usize,
}

impl Default for Nav {
    fn default() -> Self {
        Self {
            history: vec![Route::Home],
            index: 0,
        }
    }
}

impl Nav {
    pub fn current(&self) -> &Route {
        &self.history[self.index]
    }

    pub fn push(&mut self, route: Route) {
        if *self.current() == route {
            return;
        }
        self.history.truncate(self.index + 1);
        self.history.push(route);
        self.index += 1;
    }

    pub fn can_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.history.len()
    }

    pub fn back(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn forward(&mut self) {
        if self.can_forward() {
            self.index += 1;
        }
    }
}

/// Everything a page or widget needs to read data and act on it.
pub struct Cx<'a> {
    pub engine: &'a mut Engine,
    pub store: &'a mut Store,
    pub nav: &'a mut Nav,
    pub list: &'a mut ListState,
    pub now: f64,
}

pub const PAD_X: f32 = 32.0;
/// Room for the floating navigation bar above and the player bar below.
pub const PAD_TOP: f32 = 60.0;
pub const PAD_BOTTOM: f32 = 130.0;

/// The page for the current route, in a scroll area filling `ui`.
pub fn show(ui: &mut Ui, cx: &mut Cx) {
    let route = cx.nav.current().clone();
    ScrollArea::vertical()
        .id_salt(("page", format!("{route:?}")))
        .auto_shrink([false, false])
        .scroll_source(
            egui::scroll_area::ScrollSource::SCROLL_BAR
                | egui::scroll_area::ScrollSource::MOUSE_WHEEL,
        )
        .show(ui, |ui| {
            let full = ui.max_rect();
            match &route {
                Route::Home => padded(ui, |ui| home(ui, cx)),
                Route::Album(id) => album(ui, cx, id, full),
                Route::Playlist(id) => playlist(ui, cx, id, full),
                Route::Artist(id) => artist(ui, cx, id, full),
                Route::Library(page) => padded(ui, |ui| library(ui, cx, *page)),
                Route::Search(term) => padded(ui, |ui| search(ui, cx, term)),
                Route::Unavailable(name) => padded(ui, |ui| {
                    widgets::empty(ui, name, "This page isn't built into Sidra yet.")
                }),
            }
        });
}

fn padded(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: PAD_X as i8,
            right: PAD_X as i8,
            top: PAD_TOP as i8,
            bottom: PAD_BOTTOM as i8,
        })
        .show(ui, add);
}

/// Loads `req`, showing a spinner or the error while there is nothing to draw.
fn load(ui: &mut Ui, cx: &mut Cx, req: &Req, what: &str) -> Option<Arc<Res>> {
    let slot = cx.store.get(cx.engine, req);
    if let Some(res) = &slot.res {
        return Some(res.clone());
    }
    if let Some(e) = slot.error.clone() {
        widgets::empty(ui, &format!("Couldn't load {what}"), &e);
        if ui.button("Try again").clicked() {
            cx.store.refresh(cx.engine, req);
        }
    } else if cx.engine.authorized == Some(false) {
        widgets::empty(
            ui,
            "Sign in to Apple Music",
            "Your library appears once you're signed in.",
        );
    } else {
        widgets::spinner(ui, &format!("Loading {what}…"));
    }
    None
}

/// The artwork's colour washed down the top of the page.
fn tint(ui: &Ui, full: Rect, art: Option<&Artwork>) {
    let Some([r, g, b]) = art.and_then(|a| a.bg) else {
        return;
    };
    let top = Color32::from_rgb(r, g, b).gamma_multiply(0.45);
    let rect = Rect::from_min_size(full.min, vec2(full.width(), 520.0));
    style::vertical_fade(ui.painter(), rect, top, Color32::TRANSPARENT);
}

fn home(ui: &mut Ui, cx: &mut Cx) {
    ui.horizontal(|ui| {
        heading(ui, "Home", 30.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let loading = cx.store.is_loading(&Req::Recommendations);
            if disc_button(ui, icon::ARROW_CLOCKWISE, "Refresh", 32.0).clicked() && !loading {
                cx.store.refresh(cx.engine, &Req::Recommendations);
            }
            if loading {
                ui.spinner();
            }
        });
    });
    ui.add_space(18.0);
    let width = 180.0;
    // Recommendations already carry a recently played group, so it isn't fetched separately.
    let Some(res) = load(ui, cx, &Req::Recommendations, "recommendations") else {
        return;
    };
    if let Res::Shelves(shelves) = &*res {
        for (i, s) in shelves.iter().enumerate() {
            widgets::shelf(ui, cx, &format!("rec{i}"), &s.title, &s.cards, width);
        }
    }
}

/// The album and playlist header: cover, title, byline, chips and actions.
struct Header<'a> {
    art: Option<&'a Artwork>,
    title: &'a str,
    byline: &'a str,
    byline_art: Option<&'a Artwork>,
    byline_route: Option<Route>,
    chips: Vec<(&'static str, String)>,
    description: Option<&'a str>,
}

/// Returns (play, shuffle) clicks and leaves room for the caller's extra buttons.
fn header(
    ui: &mut Ui,
    cx: &mut Cx,
    h: &Header,
    extra: impl FnOnce(&mut Ui, &mut Cx),
) -> (bool, bool) {
    let (mut play, mut shuffle) = (false, false);
    let wide = ui.available_width() > 720.0;
    let cover = if wide { 300.0 } else { 200.0 };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(cover, cover), egui::Sense::hover());
        paint_art(ui, rect, h.art, 14, false);
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.set_min_height(cover);
            ui.add_space((cover - 190.0).max(0.0));
            ui.add(
                egui::Label::new(
                    RichText::new(h.title)
                        .font(style::bold(30.0))
                        .color(style::TEXT),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if let Some(art) = h.byline_art {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), egui::Sense::hover());
                    paint_art(ui, r, Some(art), 0, true);
                }
                let label = egui::Label::new(
                    RichText::new(h.byline)
                        .font(style::bold(15.0))
                        .color(style::TEXT_MUTED),
                )
                .sense(egui::Sense::click());
                let r = ui.add(label);
                if let Some(route) = &h.byline_route
                    && r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                {
                    cx.nav.push(route.clone());
                }
            });
            if let Some(d) = h.description {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(RichText::new(strip_html(d)).color(style::TEXT_MUTED))
                        .wrap()
                        .truncate(),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (glyph, text) in &h.chips {
                    chip(ui, glyph, text);
                }
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                play = pill_button(ui, icon::PLAY, "Play", true, 128.0).clicked();
                shuffle = pill_button(ui, icon::SHUFFLE, "Shuffle", false, 128.0).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| extra(ui, cx));
            });
        });
    });
    ui.add_space(28.0);
    (play, shuffle)
}

fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// "…" menu items shared by albums and playlists.
fn collection_menu(
    ui: &mut Ui,
    cx: &mut Cx,
    opts: &serde_json::Value,
    kind: &str,
    id: &str,
    url: Option<&str>,
) {
    if ui.button("Play Next").clicked() {
        cx.engine.command("playNext", json!({"opts": opts}));
    }
    if ui.button("Play Later").clicked() {
        cx.engine.command("playLater", json!({"opts": opts}));
    }
    if !widgets::is_library_id(id) && ui.button("Add to Library").clicked() {
        cx.store.add_to_library(cx.engine, kind, id);
    }
    if let Some(url) = url
        && ui.button("Copy Link").clicked()
    {
        ui.ctx().copy_text(url.to_owned());
        cx.store.toast("Link copied", cx.now);
    }
}

fn album(ui: &mut Ui, cx: &mut Cx, id: &str, full: Rect) {
    let req = Req::Album(id.to_owned());
    let res = cx.store.res(cx.engine, &req);
    let art = res.as_deref().and_then(|r| match r {
        Res::Album(a) => a.artwork.clone(),
        _ => None,
    });
    tint(ui, full, art.as_ref());
    padded(ui, |ui| {
        let Some(res) = load(ui, cx, &req, "this album") else {
            return;
        };
        let Res::Album(a) = &*res else {
            widgets::empty(
                ui,
                "Album not found",
                "Apple Music has nothing at this address.",
            );
            return;
        };
        let mut chips = Vec::new();
        if let Some(g) = &a.genre {
            chips.push((icon::WAVEFORM, g.clone()));
        }
        if let Some(y) = &a.year {
            chips.push((icon::CALENDAR, y.clone()));
        }
        chips.push((icon::MUSIC_NOTE, format!("{} tracks", a.track_count)));
        let opts = json!({"album": a.id});
        let (play, shuffle) = header(
            ui,
            cx,
            &Header {
                art: a.artwork.as_ref(),
                title: &a.title,
                byline: &a.artist,
                byline_art: a.artist_artwork.as_ref(),
                byline_route: a.artist_id.clone().map(Route::Artist),
                chips,
                description: None,
            },
            |ui, cx| {
                let more = disc_button(ui, icon::DOTS_THREE, "More", 34.0);
                egui::Popup::menu(&more).show(|ui| {
                    collection_menu(ui, cx, &opts, "albums", &a.id, a.url.as_deref());
                    if let Some(artist) = &a.artist_id
                        && ui.button("Go to Artist").clicked()
                    {
                        cx.nav.push(Route::Artist(artist.clone()));
                    }
                });
                if !a.library && pill_button(ui, icon::PLUS, "Add", false, 84.0).clicked() {
                    cx.store.add_to_library(cx.engine, "albums", &a.id);
                }
            },
        );
        if play || shuffle {
            cx.engine
                .command("setQueue", json!({"opts": opts, "shuffle": shuffle}));
        }
        widgets::track_list(ui, cx, &a.tracks, Source::Album(&a.id), true);
        if let Some(c) = &a.copyright {
            ui.add_space(16.0);
            ui.label(
                RichText::new(c)
                    .font(style::small())
                    .color(style::TEXT_MUTED),
            );
        }
    });
}

fn playlist(ui: &mut Ui, cx: &mut Cx, id: &str, full: Rect) {
    let req = Req::Playlist(id.to_owned());
    let res = cx.store.res(cx.engine, &req);
    let art = res.as_deref().and_then(|r| match r {
        Res::Playlist(p) => p.artwork.clone(),
        _ => None,
    });
    tint(ui, full, art.as_ref());
    padded(ui, |ui| {
        let Some(res) = load(ui, cx, &req, "this playlist") else {
            return;
        };
        let Res::Playlist(p) = &*res else {
            widgets::empty(
                ui,
                "Playlist not found",
                "Apple Music has nothing at this address.",
            );
            return;
        };
        let total: u64 = p.tracks.iter().map(|t| t.duration_ms).sum();
        let mut chips = vec![(icon::MUSIC_NOTE, format!("{} songs", p.tracks.len()))];
        if total > 0 {
            let mins = total / 60_000;
            chips.push((
                icon::CLOCK,
                if mins >= 60 {
                    format!("{} h {} min", mins / 60, mins % 60)
                } else {
                    format!("{mins} min")
                },
            ));
        }
        if let Some(m) = &p.last_modified {
            chips.push((icon::CALENDAR, format!("Updated {}", &m[..m.len().min(10)])));
        }
        let opts = json!({"playlist": p.id});
        // A playlist without its own cover borrows its first song's, as Apple's apps do.
        let cover = p
            .artwork
            .as_ref()
            .or_else(|| p.tracks.iter().find_map(|t| t.artwork.as_ref()));
        let (play, shuffle) = header(
            ui,
            cx,
            &Header {
                art: cover,
                title: &p.name,
                byline: &p.curator,
                byline_art: None,
                byline_route: None,
                chips,
                description: p.description.as_deref(),
            },
            |ui, cx| {
                let more = disc_button(ui, icon::DOTS_THREE, "More", 34.0);
                egui::Popup::menu(&more).show(|ui| {
                    collection_menu(ui, cx, &opts, "playlists", &p.id, p.url.as_deref());
                    if ui.button("Refresh").clicked() {
                        cx.store.refresh(cx.engine, &Req::Playlist(p.id.clone()));
                    }
                });
                if !p.library && pill_button(ui, icon::PLUS, "Add", false, 84.0).clicked() {
                    cx.store.add_to_library(cx.engine, "playlists", &p.id);
                }
            },
        );
        if play || shuffle {
            cx.engine
                .command("setQueue", json!({"opts": opts, "shuffle": shuffle}));
        }
        widgets::track_list(ui, cx, &p.tracks, Source::Playlist(&p.id), false);
    });
}

fn artist(ui: &mut Ui, cx: &mut Cx, id: &str, full: Rect) {
    let req = Req::Artist(id.to_owned());
    let Some(res) = cx.store.res(cx.engine, &req) else {
        padded(ui, |ui| {
            load(ui, cx, &req, "this artist");
        });
        return;
    };
    let a = match &*res {
        Res::Artist(a) => a,
        Res::Redirect(route) => {
            // A library artist: show its catalogue page in place of this one.
            let route = route.clone();
            cx.nav.back();
            cx.nav.push(route);
            return;
        }
        _ => {
            padded(ui, |ui| {
                widgets::empty(
                    ui,
                    "Artist not found",
                    "Apple Music has nothing at this address.",
                )
            });
            return;
        }
    };

    // Hero: the editorial image (or the portrait) under the name, fading into the page.
    let hero_h = (full.width() * 0.42).clamp(300.0, 460.0);
    let hero = Rect::from_min_size(full.min, vec2(full.width(), hero_h));
    let (_, _) = ui.allocate_exact_size(hero.size(), egui::Sense::hover());
    if let Some(art) = a.hero.as_ref().or(a.artwork.as_ref()) {
        let px = (hero.width() * ui.ctx().pixels_per_point()) as u32;
        let url = if a.hero.is_some() {
            art.url_wh(px, (px as f32 * hero_h / hero.width()) as u32)
        } else {
            art.url(px)
        };
        if let Some(url) = url {
            egui::Image::new(url)
                .fit_to_exact_size(hero.size())
                .maintain_aspect_ratio(false)
                .corner_radius(egui::CornerRadius {
                    nw: 12,
                    ne: 12,
                    sw: 0,
                    se: 0,
                })
                .show_loading_spinner(false)
                .paint_at(ui, hero);
        }
    }
    let fade = Rect::from_min_max(pos2(hero.left(), hero.bottom() - hero_h * 0.6), hero.max);
    style::vertical_fade(ui.painter(), fade, Color32::TRANSPARENT, style::CONTENT);
    let name_pos = pos2(hero.left() + PAD_X, hero.bottom() - 100.0);
    ui.painter().text(
        name_pos,
        egui::Align2::LEFT_BOTTOM,
        &a.name,
        style::bold(46.0),
        Color32::WHITE,
    );
    let buttons = Rect::from_min_size(
        pos2(hero.left() + PAD_X, hero.bottom() - 84.0),
        vec2(300.0, 48.0),
    );
    let top_ids: Vec<&str> = a.top_songs.iter().map(|t| t.id.as_str()).collect();
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(buttons)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let more = disc_button(ui, icon::DOTS_THREE, "More", 34.0);
            egui::Popup::menu(&more).show(|ui| {
                if !top_ids.is_empty() && ui.button("Shuffle Top Songs").clicked() {
                    cx.engine.command(
                        "setQueue",
                        json!({"opts": {"songs": top_ids}, "shuffle": true}),
                    );
                }
                if let Some(url) = &a.url
                    && ui.button("Copy Link").clicked()
                {
                    ui.ctx().copy_text(url.clone());
                    cx.store.toast("Link copied", cx.now);
                }
                if ui.button("Refresh").clicked() {
                    cx.store.refresh(cx.engine, &Req::Artist(a.id.clone()));
                }
            });
            if !top_ids.is_empty() && disc_button(ui, icon::PLAY, "Play top songs", 48.0).clicked()
            {
                cx.engine
                    .command("setQueue", json!({"opts": {"songs": top_ids}}));
            }
        },
    );

    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: PAD_X as i8,
            right: PAD_X as i8,
            top: 0,
            bottom: PAD_BOTTOM as i8,
        })
        .show(ui, |ui| {
            if let Some(latest) = &a.latest {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(220.0);
                        heading(ui, "Latest Release", 17.0);
                        ui.add_space(8.0);
                        widgets::card(ui, cx, latest, 200.0);
                    });
                    ui.add_space(24.0);
                    ui.vertical(|ui| top_songs(ui, cx, a));
                });
                ui.add_space(24.0);
            } else {
                top_songs(ui, cx, a);
                ui.add_space(24.0);
            }
            for (i, s) in a.shelves.iter().enumerate() {
                widgets::shelf(ui, cx, &format!("artist{i}"), &s.title, &s.cards, 170.0);
            }
        });
}

/// Top songs in compact columns, like Apple's artist page.
fn top_songs(ui: &mut Ui, cx: &mut Cx, a: &sidra_core::apple::Artist) {
    if a.top_songs.is_empty() {
        return;
    }
    heading(ui, "Top Songs", 17.0);
    ui.add_space(8.0);
    let cols = ((ui.available_width() / 260.0).floor() as usize).clamp(1, 3);
    let col_w = ui.available_width() / cols as f32 - 8.0;
    let ids: Vec<&str> = a.top_songs.iter().map(|t| t.id.as_str()).collect();
    let rows = a.top_songs.len().min(12).div_ceil(cols);
    egui::Grid::new(("top", &a.id))
        .spacing(vec2(8.0, 4.0))
        .show(ui, |ui| {
            for r in 0..rows {
                for c in 0..cols {
                    let i = c * rows + r;
                    let Some(t) = a.top_songs.get(i) else {
                        continue;
                    };
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(col_w, 52.0), egui::Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(8),
                            Color32::from_white_alpha(12),
                        );
                    }
                    let art = Rect::from_min_size(rect.min + vec2(6.0, 8.0), vec2(36.0, 36.0));
                    paint_art(ui, art, t.artwork.as_ref(), 4, false);
                    let w = rect.width() - 60.0;
                    let title = style::truncated(ui, &t.title, style::bold(13.0), w);
                    let sub = style::truncated(
                        ui,
                        &format!("{} · {}", t.album, t.artist),
                        style::small(),
                        w,
                    );
                    ui.painter().galley(
                        pos2(art.right() + 10.0, rect.top() + 9.0),
                        title,
                        style::TEXT,
                    );
                    ui.painter().galley(
                        pos2(art.right() + 10.0, rect.top() + 27.0),
                        sub,
                        style::TEXT_MUTED,
                    );
                    if resp.double_clicked() || resp.clicked() {
                        cx.engine
                            .command("setQueue", json!({"opts": {"songs": ids[i..].to_vec()}}));
                    }
                    resp.context_menu(|ui| widgets::track_menu(ui, cx, t, false));
                }
                ui.end_row();
            }
        });
}

fn library(ui: &mut Ui, cx: &mut Cx, page: LibraryPage) {
    let (title, req) = match page {
        LibraryPage::RecentlyAdded => ("Recently Added", Req::RecentlyAdded),
        LibraryPage::Albums => ("Albums", Req::LibraryAlbums),
        LibraryPage::Artists => ("Artists", Req::LibraryArtists),
        LibraryPage::Songs => ("Songs", Req::LibrarySongs),
        LibraryPage::Playlists => ("All Playlists", Req::LibraryPlaylists),
    };
    ui.horizontal(|ui| {
        heading(ui, title, 30.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if disc_button(ui, icon::ARROW_CLOCKWISE, "Refresh", 32.0).clicked() {
                cx.store.refresh(cx.engine, &req);
            }
            if cx.store.is_loading(&req) {
                ui.spinner();
            }
        });
    });
    ui.add_space(18.0);
    let Some(res) = load(ui, cx, &req, &title.to_lowercase()) else {
        return;
    };
    match &*res {
        Res::Tracks(tracks) => widgets::track_list(ui, cx, tracks, Source::Songs, false),
        Res::Cards(cards) if cards.is_empty() => widgets::empty(
            ui,
            "Nothing here yet",
            "Add music to your library and it shows up here.",
        ),
        Res::Cards(cards) => widgets::grid(ui, cx, cards),
        _ => {}
    }
}

fn search(ui: &mut Ui, cx: &mut Cx, term: &str) {
    heading(ui, &format!("Results for \u{201c}{term}\u{201d}"), 26.0);
    ui.add_space(16.0);
    let req = Req::Search(term.to_owned());
    let Some(res) = load(ui, cx, &req, "results") else {
        return;
    };
    let Res::Search { songs, cards } = &*res else {
        return;
    };
    if songs.is_empty() && cards.is_empty() {
        widgets::empty(ui, "No results", "Try a different spelling or fewer words.");
        return;
    }
    let by = |k: CardKind| {
        cards
            .iter()
            .filter(|c| c.kind == k)
            .cloned()
            .collect::<Vec<Card>>()
    };
    widgets::shelf(ui, cx, "s-artists", "Artists", &by(CardKind::Artist), 150.0);
    widgets::shelf(ui, cx, "s-albums", "Albums", &by(CardKind::Album), 170.0);
    if !songs.is_empty() {
        heading(ui, "Songs", 17.0);
        ui.add_space(8.0);
        widgets::track_list(ui, cx, songs, Source::Songs, false);
        ui.add_space(24.0);
    }
    widgets::shelf(
        ui,
        cx,
        "s-playlists",
        "Playlists",
        &by(CardKind::Playlist),
        170.0,
    );
}
