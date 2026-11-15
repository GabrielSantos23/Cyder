//! The sidebar: search, browse pages, the Library, playlists and the account row.
//!
//! Rows are painted by hand rather than with `SelectableLabel` so the icon column,
//! trailing chevrons and selection shape match the design; each row still reports
//! itself to AccessKit as a selectable item and takes keyboard focus.

use crate::pages::{Cx, LibraryPage, Route};
use crate::store::{ROOT_FOLDER, Req, Res};
use crate::style::{self, focus_ring, truncated};
use egui::{
    Align, Align2, CornerRadius, Rect, Response, RichText, ScrollArea, Sense, Stroke, StrokeKind,
    TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use egui_phosphor::regular as icon;
use std::collections::HashSet;

pub const WIDTH: f32 = 220.0;
const ROW_HEIGHT: f32 = 28.0;
const HEADER_HEIGHT: f32 = 24.0;
const ROW_PAD: f32 = 8.0;
const ICON_GAP: f32 = 10.0;
const ROW_RADIUS: u8 = 8;
const SEARCH_HEIGHT: f32 = 30.0;
const ACCOUNT_HEIGHT: f32 = 40.0;
const SECTION_GAP: f32 = 14.0;
const INDENT: f32 = 14.0;

struct Item {
    icon: &'static str,
    label: &'static str,
    route: fn() -> Route,
    /// Shows a trailing chevron: the row opens more choices.
    more: bool,
}

const fn item(icon: &'static str, label: &'static str, route: fn() -> Route) -> Item {
    Item {
        icon,
        label,
        route,
        more: false,
    }
}

const BROWSE: [Item; 4] = [
    item(icon::HOUSE, "Home", || Route::Home),
    item(icon::SQUARES_FOUR, "New", || Route::Unavailable("New")),
    item(icon::BROADCAST, "Radio", || Route::Unavailable("Radio")),
    item(icon::TICKET, "Concerts", || Route::Unavailable("Concerts")),
];

const LIBRARY: [Item; 7] = [
    Item {
        more: true,
        ..item(icon::PUSH_PIN, "Pins", || Route::Unavailable("Pins"))
    },
    item(icon::CLOCK, "Recently Added", || {
        Route::Library(LibraryPage::RecentlyAdded)
    }),
    item(icon::MUSIC_NOTE, "Songs", || {
        Route::Library(LibraryPage::Songs)
    }),
    item(icon::STACK_SIMPLE, "Albums", || {
        Route::Library(LibraryPage::Albums)
    }),
    item(icon::USERS, "Artists", || {
        Route::Library(LibraryPage::Artists)
    }),
    item(icon::MONITOR_PLAY, "Music Videos", || {
        Route::Unavailable("Music Videos")
    }),
    item(icon::APPLE_PODCASTS_LOGO, "Podcasts", || {
        Route::Unavailable("Podcasts")
    }),
];

const ALL_PLAYLISTS: Item = item(icon::DOTS_NINE, "All Playlists", || {
    Route::Library(LibraryPage::Playlists)
});

#[derive(Default)]
pub struct Sidebar {
    search: String,
    library_closed: bool,
    playlists_closed: bool,
    /// Playlist folders the user has expanded.
    expanded: HashSet<String>,
    /// The name being typed for a new playlist, while that dialog is open.
    creating: Option<String>,
}

impl Sidebar {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
        self.search_field(ui, cx);
        ui.add_space(SECTION_GAP);

        let list_height = ui.available_height() - ACCOUNT_HEIGHT - 8.0;
        ScrollArea::vertical()
            .max_height(list_height.max(0.0))
            .auto_shrink([false, false])
            .show(ui, |ui| self.list(ui, cx));

        ui.add_space(8.0);
        account_row(ui, cx);
        self.create_dialog(ui, cx);
    }

    fn list(&mut self, ui: &mut Ui, cx: &mut Cx) {
        for item in &BROWSE {
            item_row(ui, cx, item);
        }

        ui.add_space(SECTION_GAP);
        if section_header(ui, "Library", !self.library_closed).clicked() {
            self.library_closed = !self.library_closed;
        }
        if !self.library_closed {
            for item in &LIBRARY {
                item_row(ui, cx, item);
            }
        }

        ui.add_space(SECTION_GAP);
        let apple = Route::Unavailable("Apple Music Playlists");
        if link_header(ui, "Apple Music Playlists", cx.nav.current() == &apple).clicked() {
            cx.nav.push(apple);
        }

        ui.add_space(SECTION_GAP);
        let (header, options) = playlists_header(ui, !self.playlists_closed);
        if header.clicked() {
            self.playlists_closed = !self.playlists_closed;
        }
        egui::Popup::menu(&options).show(|ui| {
            if ui.button("New Playlist…").clicked() {
                self.creating = Some(String::new());
            }
            if ui.button("Refresh Playlists").clicked() {
                cx.store
                    .refresh(cx.engine, &Req::Folder(ROOT_FOLDER.into()));
                for id in &self.expanded {
                    cx.store.refresh(cx.engine, &Req::Folder(id.clone()));
                }
            }
        });
        if !self.playlists_closed {
            if row(ui, icon::PLUS, "Create New", false, false).clicked() {
                self.creating = Some(String::new());
            }
            item_row(ui, cx, &ALL_PLAYLISTS);
            if cx.engine.authorized == Some(true) {
                self.folder(ui, cx, ROOT_FOLDER, 0);
            }
        }
    }

    /// A folder's playlists and subfolders, loading them on first view.
    fn folder(&mut self, ui: &mut Ui, cx: &mut Cx, id: &str, depth: usize) {
        let slot = cx.store.get(cx.engine, &Req::Folder(id.to_owned()));
        let Some(res) = slot.res.clone() else {
            if slot.loading {
                ui.horizontal(|ui| {
                    ui.add_space(ROW_PAD + depth as f32 * INDENT);
                    ui.spinner();
                });
            }
            return;
        };
        let Res::Entries(entries) = &*res else { return };
        for e in entries {
            let indent = depth as f32 * INDENT;
            if e.folder {
                let open = self.expanded.contains(&e.id);
                let glyph = if open {
                    icon::FOLDER_OPEN
                } else {
                    icon::FOLDER_SIMPLE
                };
                if indented(ui, indent, |ui| row(ui, glyph, &e.name, false, true)).clicked() {
                    if open {
                        self.expanded.remove(&e.id);
                    } else {
                        self.expanded.insert(e.id.clone());
                    }
                }
                if open {
                    self.folder(ui, cx, &e.id, depth + 1);
                }
            } else {
                let route = Route::Playlist(e.id.clone());
                let selected = cx.nav.current() == &route;
                let r = indented(ui, indent, |ui| {
                    row(ui, icon::PLAYLIST, &e.name, selected, false)
                });
                if r.clicked() {
                    cx.nav.push(route);
                }
            }
        }
    }

    fn search_field(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), SEARCH_HEIGHT), Sense::hover());
        let radius = CornerRadius::same(10);
        ui.painter().rect_filled(rect, radius, style::FIELD);

        let text_rect = Rect::from_min_max(rect.min + vec2(12.0, 0.0), rect.max - vec2(32.0, 0.0));
        let edit = TextEdit::singleline(&mut self.search)
            .hint_text(RichText::new("Search").color(style::TEXT_MUTED))
            .font(style::body())
            .text_color(style::TEXT)
            .frame(false)
            .vertical_align(Align::Center)
            .desired_width(text_rect.width());
        let response = ui.put(text_rect, edit);
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let term = self.search.trim();
            if !term.is_empty() {
                cx.nav.push(Route::Search(term.to_owned()));
            }
        }

        let border = if response.has_focus() {
            style::FOCUS
        } else {
            style::FIELD_BORDER
        };
        ui.painter()
            .rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
        ui.painter().text(
            pos2(rect.right() - 16.0, rect.center().y),
            Align2::CENTER_CENTER,
            icon::MAGNIFYING_GLASS,
            style::icon(),
            style::TEXT_MUTED,
        );
    }

    fn create_dialog(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let Some(name) = &mut self.creating else {
            return;
        };
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("new-playlist")).show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.label(
                RichText::new("New Playlist")
                    .font(style::bold(16.0))
                    .color(style::TEXT),
            );
            ui.add_space(8.0);
            let r = ui.add(
                TextEdit::singleline(name)
                    .hint_text("Playlist name")
                    .desired_width(f32::INFINITY),
            );
            r.request_focus();
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let ok = !name.trim().is_empty();
                if ui.add_enabled(ok, egui::Button::new("Create")).clicked() || (enter && ok) {
                    cx.store.create_playlist(cx.engine, name.trim());
                    cx.store
                        .refresh(cx.engine, &Req::Folder(ROOT_FOLDER.into()));
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.creating = None;
        }
    }
}

fn indented(ui: &mut Ui, indent: f32, add: impl FnOnce(&mut Ui) -> Response) -> Response {
    if indent == 0.0 {
        return add(ui);
    }
    ui.horizontal(|ui| {
        ui.add_space(indent);
        add(ui)
    })
    .inner
}

fn item_row(ui: &mut Ui, cx: &mut Cx, item: &Item) {
    let route = (item.route)();
    let selected = cx.nav.current() == &route;
    if row(ui, item.icon, item.label, selected, item.more).clicked() {
        cx.nav.push(route);
    }
}

fn account_row(ui: &mut Ui, cx: &mut Cx) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ACCOUNT_HEIGHT), Sense::click());
    let signed_in = cx.engine.authorized == Some(true);
    let label = if signed_in {
        "Apple Music"
    } else {
        "Not signed in"
    };
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let fill = if response.hovered() {
        style::ROW_SELECTED
    } else {
        style::FIELD
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    focus_ring(ui, &response, rect, 10);

    let avatar = pos2(rect.left() + 8.0 + 12.0, rect.center().y);
    painter.circle_filled(avatar, 12.0, style::AVATAR);
    painter.text(
        avatar,
        Align2::CENTER_CENTER,
        icon::USER,
        style::small(),
        style::TEXT,
    );

    let left = avatar.x + 12.0 + ICON_GAP;
    let width = rect.right() - ROW_PAD - left;
    let galley = truncated(ui, label, style::body(), width);
    let storefront = if signed_in {
        format!("Storefront: {}", cx.engine.storefront.to_uppercase())
    } else {
        "Click Sign In to start".to_owned()
    };
    let sub = truncated(ui, &storefront, style::small(), width);
    painter.galley(pos2(left, rect.center().y - 16.0), galley, style::TEXT);
    painter.galley(pos2(left, rect.center().y + 1.0), sub, style::TEXT_MUTED);

    egui::Popup::menu(&response).show(|ui| {
        if signed_in && ui.button("Sign Out").clicked() {
            cx.engine.command("signOut", serde_json::json!({}));
        }
        if ui.button("Clear Cache").clicked() {
            cx.store.clear_all();
            cx.store.toast("Cache cleared", cx.now);
        }
    });
}

/// A navigation row: icon, label (truncated with a tooltip), optional trailing chevron.
fn row(ui: &mut Ui, glyph: &str, label: &str, selected: bool, more: bool) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let fill = if selected {
        style::ROW_SELECTED
    } else if response.hovered() {
        style::ROW_HOVER
    } else {
        egui::Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(ROW_RADIUS), fill);
    focus_ring(ui, &response, rect, ROW_RADIUS);

    let icon_x = rect.left() + ROW_PAD + style::ICON_SIZE / 2.0;
    painter.text(
        pos2(icon_x, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        style::icon(),
        style::TEXT,
    );

    let left = rect.left() + ROW_PAD + style::ICON_SIZE + ICON_GAP;
    let trailing = if more { style::ICON_SIZE + 4.0 } else { 0.0 };
    let galley = truncated(
        ui,
        label,
        style::body(),
        rect.right() - ROW_PAD - trailing - left,
    );
    let elided = galley.elided;
    painter.galley(
        pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        style::TEXT,
    );

    if more {
        chevron(ui, rect, icon::CARET_RIGHT);
    }
    if elided {
        return response.on_hover_text(label);
    }
    response
}

fn chevron(ui: &Ui, rect: Rect, glyph: &str) {
    ui.painter().text(
        pos2(rect.right() - ROW_PAD - 6.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(13.0),
        style::TEXT_MUTED,
    );
}

fn header_rect(ui: &mut Ui, label: &str, selected: bool) -> (Rect, Response) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::click());
    if selected || response.hovered() {
        let fill = if selected {
            style::ROW_SELECTED
        } else {
            style::ROW_HOVER
        };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(ROW_RADIUS), fill);
    }
    focus_ring(ui, &response, rect, ROW_RADIUS);
    ui.painter().text(
        pos2(rect.left() + ROW_PAD, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        style::small(),
        style::TEXT_MUTED,
    );
    (rect, response)
}

/// A collapsible group label with a disclosure caret on the right.
fn section_header(ui: &mut Ui, label: &str, open: bool) -> Response {
    let (rect, response) = header_rect(ui, label, false);
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::CollapsingHeader, ui.is_enabled(), open, label)
    });
    chevron(
        ui,
        rect,
        if open {
            icon::CARET_DOWN
        } else {
            icon::CARET_RIGHT
        },
    );
    response
}

/// A label-styled row that opens a page, like "Apple Music Playlists".
fn link_header(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let (rect, response) = header_rect(ui, label, selected);
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    chevron(ui, rect, icon::CARET_RIGHT);
    response
}

/// The Playlists group label, with an options button beside the caret.
fn playlists_header(ui: &mut Ui, open: bool) -> (Response, Response) {
    let header = section_header(ui, "Playlists", open);
    let rect = header.rect;
    let button = Rect::from_center_size(
        pos2(rect.right() - ROW_PAD - 6.0 - 22.0, rect.center().y),
        vec2(20.0, 20.0),
    );
    let options = ui.interact(button, header.id.with("options"), Sense::click());
    options.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), "Playlist options")
    });
    if options.hovered() {
        ui.painter()
            .rect_filled(button, CornerRadius::same(6), style::ROW_SELECTED);
    }
    focus_ring(ui, &options, button, 6);
    ui.painter().text(
        button.center(),
        Align2::CENTER_CENTER,
        icon::DOTS_THREE,
        style::icon(),
        style::TEXT_MUTED,
    );
    (header, options.on_hover_text("Playlist options"))
}
