//! Sidra's native shell. The window is frameless: its background is the sidebar
//! colour, the sidebars sit on it and the content panel is inset, so the frame
//! wraps the whole UI. Pages are drawn natively from Apple Music data, which the
//! engine's hidden Apple Music page fetches and plays.

mod art;
mod bidi;
mod chrome;
mod engine;
mod integrations;
mod karaoke;
mod lastfm;
mod lyrics;
mod pages;
mod player;
mod preset;
mod right_panel;
mod settings;
mod shader;
mod shell;
mod sidebar;
mod store;
mod style;
mod theme;
mod widgets;

use egui::{
    Align, CentralPanel, Color32, Frame, Layout, Margin, Rect, SidePanel, Ui, UiBuilder, pos2, vec2,
};
use egui_phosphor::regular as icon;
use pages::{Cx, Nav};
use right_panel::View;
use std::path::PathBuf;
use std::sync::Arc;

const FRAME_INSET: i8 = 8;
const NAV_HEIGHT: f32 = 52.0;
const TOAST_SECS: f64 = 3.0;

/// Sidra's own folder: `%LOCALAPPDATA%\Sidra` on Windows, the XDG data folder elsewhere.
fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("Sidra")
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sidra")
            .with_decorations(false)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Sidra",
        options,
        Box::new(|cc| {
            style::install(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let dir = data_dir();
            cc.egui_ctx
                .add_bytes_loader(Arc::new(art::ArtLoader::new(dir.join("cache").join("art"))));
            Ok(Box::new(App::new(dir)))
        }),
    )
}

struct App {
    dir: PathBuf,
    engine: engine::Engine,
    store: store::Store,
    nav: Nav,
    list: widgets::ListState,
    sidebar: sidebar::Sidebar,
    right: right_panel::RightPanel,
    sidebar_hidden: bool,
    signed_in: bool,
    /// The storefront pages were last loaded in.
    saved_storefront: String,
    /// The route the list filter belongs to; a new page starts unfiltered.
    list_route: pages::Route,
    settings: settings::Settings,
    /// The settings as last saved, so a change is written once.
    saved_settings: settings::Settings,
    /// The theme and zoom egui is drawing with.
    applied: Option<(String, f32)>,
    lastfm: lastfm::Lastfm,
    notifier: integrations::Notifier,
    discord: integrations::Discord,
    #[cfg(windows)]
    tray: integrations::tray::Tray,
}

impl App {
    fn new(dir: PathBuf) -> Self {
        // The account's storefront from last time, so the first load needs no redirect.
        let storefront = std::fs::read_to_string(dir.join("storefront"))
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| s.len() == 2 && s.chars().all(|c| c.is_ascii_lowercase()))
            .unwrap_or_else(|| "us".into());
        let settings = settings::Settings::load(&dir);
        #[cfg(windows)]
        integrations::register_app_id(&dir);
        Self {
            nav: Nav::new(settings.start_route()),
            lastfm: lastfm::Lastfm::new(&dir, &settings.lastfm),
            saved_settings: settings.clone(),
            settings,
            applied: None,
            notifier: Default::default(),
            discord: Default::default(),
            #[cfg(windows)]
            tray: Default::default(),
            saved_storefront: storefront.clone(),
            // SIDRA_WEBVIEW_DIR points the engine at another WebView2 profile, for diagnosing playback.
            engine: engine::Engine::new(
                &std::env::var_os("SIDRA_WEBVIEW_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dir.join("webview")),
                storefront,
            ),
            store: store::Store::new(dir.join("cache").join("api")),
            dir,
            list: Default::default(),
            sidebar: Default::default(),
            right: Default::default(),
            sidebar_hidden: false,
            signed_in: false,
            list_route: pages::Route::Home,
        }
    }

    fn handle_events(&mut self, now: f64) {
        for event in self.engine.poll(now) {
            match event {
                engine::Event::Auth {
                    authorized,
                    storefront,
                } => {
                    if !authorized && self.signed_in {
                        // Signed out: the cached library belongs to the account that left.
                        self.store.clear_library();
                    }
                    self.signed_in = authorized;
                    if authorized && !storefront.is_empty() && storefront != self.saved_storefront {
                        // Catalogue ids belong to one storefront: reload pages in the new one.
                        self.store.forget();
                        self.nav = Nav::default();
                        self.saved_storefront = storefront.clone();
                    }
                    if authorized && !storefront.is_empty() {
                        let _ = std::fs::create_dir_all(&self.dir);
                        let _ = std::fs::write(self.dir.join("storefront"), &storefront);
                    }
                }
                engine::Event::Response { id, result } => {
                    self.store.respond(&self.engine, id, result, now);
                }
                engine::Event::Error(e) => {
                    eprintln!("sidra: {e}");
                    self.store.toast(e, now);
                }
                engine::Event::Changed => {}
            }
        }
        if let Some(route) = self.store.goto.take() {
            self.nav.push(route);
        }
    }
}

impl App {
    /// Theme, zoom, Last.fm, notifications, Discord and the tray, then saving any change.
    fn integrations(&mut self, ctx: &egui::Context, frame: &eframe::Frame, now: f64) {
        let s = &mut self.settings;
        let want = (s.theme.clone(), s.zoom_factor);
        if self.applied.as_ref() != Some(&want) {
            style::set_palette(ctx, s.palette(&self.dir));
            ctx.set_zoom_factor(s.zoom_factor);
            self.engine.set_zoom(s.zoom_factor);
            self.applied = Some(want);
        }
        let pb = &self.engine.playback;
        self.lastfm.tick(ctx, &mut s.lastfm, pb, now);
        self.notifier.tick(s.notifications, pb);
        self.discord.tick(s.discord, pb, self.engine.position(now));
        #[cfg(windows)]
        {
            use integrations::tray;
            self.tray.tick(s.close_to_tray, frame, ctx);
            let quitting = tray::QUITTING.load(std::sync::atomic::Ordering::Relaxed);
            if s.close_to_tray && !quitting && ctx.input(|i| i.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                tray::hide();
            }
        }
        #[cfg(not(windows))]
        let _ = frame;
        if s.last_page.as_ref() != Some(self.nav.current()) {
            s.last_page = Some(self.nav.current().clone());
        }
        if *s != self.saved_settings {
            s.save(&self.dir);
            self.saved_settings = s.clone();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        self.engine.start(frame, ctx);
        self.handle_events(now);
        if self.nav.current() != &self.list_route {
            self.list_route = self.nav.current().clone();
            self.list = Default::default();
        }
        self.integrations(ctx, frame, now);
        if self.engine.playback.playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        let resizing = chrome::edge_resize(ctx);
        let App {
            dir,
            engine,
            store,
            nav,
            list,
            sidebar,
            right,
            sidebar_hidden,
            settings,
            lastfm,
            ..
        } = self;
        let mut cx = Cx {
            engine,
            store,
            nav,
            list,
            settings,
            lastfm,
            dir,
            now,
        };

        CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(style::frame())
                    .inner_margin(Margin::same(FRAME_INSET)),
            )
            .show(ctx, |ui| {
                chrome::drag_background(ctx, ui, resizing);

                if !*sidebar_hidden {
                    SidePanel::left("sidebar")
                        .frame(Frame::NONE.inner_margin(Margin {
                            right: FRAME_INSET,
                            ..Margin::ZERO
                        }))
                        .resizable(false)
                        .show_separator_line(false)
                        .exact_width(sidebar::WIDTH)
                        .show_inside(ui, |ui| sidebar.ui(ui, &mut cx));
                }

                if right.view.is_some() {
                    SidePanel::right("right-panel")
                        .frame(Frame::NONE.inner_margin(Margin {
                            left: FRAME_INSET,
                            ..Margin::ZERO
                        }))
                        .resizable(false)
                        .show_separator_line(false)
                        .exact_width(right_panel::WIDTH)
                        .show_inside(ui, |ui| right.ui(ui, &mut cx));
                }

                CentralPanel::default()
                    .frame(style::content_frame())
                    .show_inside(ui, |ui| {
                        let full = ui.max_rect();
                        content(ui, &mut cx, full, right, sidebar_hidden);
                    });
            });
    }
}

/// The content panel: the page, then the navigation bar, player bar and toasts over it.
fn content(
    ui: &mut Ui,
    cx: &mut Cx,
    full: Rect,
    right: &mut right_panel::RightPanel,
    sidebar_hidden: &mut bool,
) {
    // Signed out: Sidra's sign-in screen, or Apple's whole page when the user asks for it.
    let sign_in = cx.engine.authorized == Some(false);
    let below_nav = Rect::from_min_max(pos2(full.left(), full.top() + NAV_HEIGHT), full.max);
    let show_page = sign_in && cx.engine.show_page;
    // A playing music video sits between the navigation and player bars, in the webview itself.
    let video = (!sign_in
        && cx.engine.video_open
        && cx
            .engine
            .playback
            .now
            .as_ref()
            .is_some_and(|n| n.is_video()))
    .then(|| {
        Rect::from_min_max(
            pos2(full.left() + 16.0, below_nav.top() + 4.0),
            pos2(full.right() - 16.0, full.bottom() - player::HEIGHT - 28.0),
        )
    });
    cx.engine.set_video(video.is_some());
    cx.engine.place(if show_page {
        Some(below_nav.shrink(1.0))
    } else {
        video
    });
    if sign_in && !show_page {
        ui.scope_builder(UiBuilder::new().max_rect(below_nav), |ui| {
            sign_in_screen(ui, cx)
        });
    }

    if !sign_in {
        ui.scope_builder(UiBuilder::new().max_rect(full), |ui| {
            ui.set_clip_rect(full);
            pages::show(ui, cx);
        });
        if let Some(r) = video {
            ui.painter()
                .rect_filled(r, egui::CornerRadius::same(12), Color32::BLACK);
        }
        if cx.engine.authorized.is_none() {
            ui.scope_builder(UiBuilder::new().max_rect(below_nav), |ui| {
                widgets::spinner(ui, "Connecting to Apple Music…")
            });
        }
    }

    // A wash behind the navigation bar so scrolled content doesn't fight the buttons.
    let wash = Rect::from_min_size(full.min, vec2(full.width(), NAV_HEIGHT + 8.0));
    style::vertical_fade(
        &ui.painter_at(full),
        wash,
        style::content().gamma_multiply(0.9),
        Color32::TRANSPARENT,
    );
    nav_bar(
        ui,
        cx,
        Rect::from_min_size(full.min, vec2(full.width(), NAV_HEIGHT)),
        right,
        sidebar_hidden,
    );
    if show_page {
        // Apple's page covers the panel; this is the way back to Sidra's own screen.
        let cancel = Rect::from_center_size(
            pos2(full.center().x, full.top() + NAV_HEIGHT / 2.0),
            vec2(170.0, 34.0),
        );
        ui.scope_builder(UiBuilder::new().max_rect(cancel), |ui| {
            if widgets::pill_button(ui, icon::X, "Cancel Sign In", false, 170.0).clicked() {
                cx.engine.show_page = false;
            }
        });
    }

    if !sign_in {
        let bar = Rect::from_min_max(
            pos2(full.left() + 16.0, full.bottom() - player::HEIGHT - 14.0),
            pos2(full.right() - 16.0, full.bottom() - 14.0),
        );
        ui.scope_builder(UiBuilder::new().max_rect(bar), |ui| {
            player::bar(ui, cx, bar)
        });
    }
    let toast_bottom = if sign_in {
        full.bottom() - 24.0
    } else {
        full.bottom() - player::HEIGHT - 24.0
    };
    toasts(
        ui,
        cx,
        Rect::from_min_max(full.min, pos2(full.right(), toast_bottom)),
    );
}

/// Shown while signed out. The button opens only Apple's sign-in window; Sidra never sees the password.
fn sign_in_screen(ui: &mut Ui, cx: &mut Cx) {
    ui.add_space((ui.available_height() / 2.0 - 140.0).max(24.0));
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(icon::MUSIC_NOTES)
                .size(44.0)
                .color(style::text()),
        );
        ui.add_space(12.0);
        widgets::heading(ui, "Sign in to Apple Music", 24.0);
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Apple's own sign-in box opens. Sidra never sees your password.")
                .color(style::text_muted()),
        );
        ui.add_space(20.0);
        if widgets::pill_button(ui, icon::SIGN_IN, "Sign In", true, 150.0).clicked() {
            cx.engine.authorize();
        }
        ui.add_space(14.0);
        let alt = ui.add(
            egui::Label::new(
                egui::RichText::new("Sign in on Apple's page instead")
                    .font(style::small())
                    .color(style::text_muted())
                    .underline(),
            )
            .sense(egui::Sense::click()),
        );
        if alt
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            cx.engine.show_page = true;
        }
    });
}

fn nav_bar(
    ui: &mut Ui,
    cx: &mut Cx,
    rect: Rect,
    right: &mut right_panel::RightPanel,
    sidebar_hidden: &mut bool,
) {
    let inner = rect.shrink2(vec2(12.0, 10.0));
    let layout = Layout::left_to_right(Align::Center);
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(layout), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let size = vec2(32.0, 30.0);
        style::pill_frame().show(ui, |ui| {
            let more = style::icon_button(ui, icon::DOTS_THREE, "Menu", size, None);
            egui::Popup::menu(&more).show(|ui| {
                if ui.button("Refresh Page").clicked() {
                    refresh_current(cx);
                }
                if ui.button("Settings").clicked() {
                    cx.nav.push(pages::Route::Settings);
                }
                if ui.button("Clear Cache").clicked() {
                    cx.store.clear_all();
                    cx.store.toast("Cache cleared", cx.now);
                }
                if cx.engine.authorized == Some(true) && ui.button("Sign Out").clicked() {
                    cx.engine.command("signOut", serde_json::json!({}));
                }
                ui.separator();
                if ui.button("Quit Sidra").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
        style::pill_frame().show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_enabled_ui(cx.nav.can_back(), |ui| {
                if style::icon_button(ui, icon::CARET_LEFT, "Back", size, None).clicked() {
                    cx.nav.back();
                }
            });
            ui.add_enabled_ui(cx.nav.can_forward(), |ui| {
                if style::icon_button(ui, icon::CARET_RIGHT, "Forward", size, None).clicked() {
                    cx.nav.forward();
                }
            });
        });
        style::pill_frame().show(ui, |ui| {
            let label = if *sidebar_hidden {
                "Show Sidebar"
            } else {
                "Hide Sidebar"
            };
            if style::icon_button(ui, icon::SIDEBAR, label, size, Some(!*sidebar_hidden)).clicked()
            {
                *sidebar_hidden = !*sidebar_hidden;
            }
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if right.view.is_none() {
                ui.allocate_ui(
                    vec2(112.0, chrome::CONTROLS_HEIGHT),
                    chrome::window_controls,
                );
                ui.add_space(8.0);
            }
            // Stations have no queue Sidra can show and no lyrics.
            let radio = cx.engine.playback.radio;
            if radio {
                right.view = None;
            }
            style::pill_frame().show(ui, |ui| {
                if radio {
                    ui.disable();
                }
                ui.spacing_mut().item_spacing.x = 2.0;
                for (view, glyph, label) in [
                    (View::Queue, icon::QUEUE, "Playing Next"),
                    (View::Lyrics, icon::CHAT_CENTERED_TEXT, "Lyrics"),
                ] {
                    let on = right.view == Some(view);
                    if style::icon_button(ui, glyph, label, vec2(34.0, 30.0), Some(on)).clicked() {
                        right.toggle(view);
                    }
                }
            });
        });
    });
}

fn refresh_current(cx: &mut Cx) {
    if let Some(req) = cx.nav.current().req() {
        cx.store.refresh(cx.engine, &req);
    }
}

fn toasts(ui: &mut Ui, cx: &mut Cx, area: Rect) {
    cx.store.toasts.retain(|(_, at)| cx.now - at < TOAST_SECS);
    let mut y = area.bottom();
    for (text, _) in cx.store.toasts.iter().rev() {
        let galley = ui
            .painter()
            .layout_no_wrap(text.clone(), style::body(), style::text());
        let size = galley.size() + vec2(28.0, 16.0);
        let rect = Rect::from_center_size(pos2(area.center().x, y - size.y / 2.0), size);
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(10),
            Color32::from_rgb(44, 44, 44),
        );
        ui.painter()
            .galley(rect.min + vec2(14.0, 8.0), galley, style::text());
        y = rect.top() - 6.0;
    }
    if !cx.store.toasts.is_empty() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(300));
    }
}
