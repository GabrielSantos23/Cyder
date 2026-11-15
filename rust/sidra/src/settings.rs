//! Sidra's preferences, saved as `settings.json` in the data folder, and the Settings page.
//! The choices are the Electron build's Settings window, minus the Apple Music Classical
//! switch, which has no native pages.

use crate::pages::{Cx, Route};
use crate::style;
use crate::widgets::{heading, pill_button};
use egui::{Color32, CornerRadius, Frame, Margin, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use serde::{Deserialize, Serialize};
use sidra_core::lastfm::PendingScrobble;
use sidra_core::theme::{BUNDLED_THEMES, ThemeName, parse_custom_theme};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// `home`, `new`, `radio`, `all-playlists` or `last`.
    pub start_page: String,
    /// The page open when Sidra last closed, for the `last` start page.
    pub last_page: Option<Route>,
    /// A [`ThemeName`] as stored by the Electron build.
    pub theme: String,
    pub zoom_factor: f32,
    pub close_to_tray: bool,
    pub notifications: bool,
    pub discord: bool,
    pub lastfm: LastfmSettings,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LastfmSettings {
    pub enabled: bool,
    pub session_key: Option<String>,
    pub username: Option<String>,
    /// Plays whose request never reached Last.fm, sent with the next request that does.
    pub pending_scrobbles: Vec<PendingScrobble>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            start_page: "home".into(),
            last_page: None,
            theme: "apple-music".into(),
            zoom_factor: 1.0,
            close_to_tray: false,
            notifications: true,
            discord: false,
            lastfm: LastfmSettings::default(),
        }
    }
}

pub const START_PAGES: [(&str, &str); 5] = [
    ("home", "Home"),
    ("new", "New"),
    ("radio", "Radio"),
    ("all-playlists", "All Playlists"),
    ("last", "Last Opened Page"),
];

const ZOOMS: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

impl Settings {
    fn path(dir: &Path) -> PathBuf {
        dir.join("settings.json")
    }

    /// The saved settings, or the defaults when there are none or the file is unreadable.
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Written to a temporary file first, so a crash mid-write never leaves half a file.
    pub fn save(&self, dir: &Path) {
        let tmp = dir.join("settings.json.tmp");
        let result = serde_json::to_vec_pretty(self)
            .map_err(std::io::Error::other)
            .and_then(|body| {
                std::fs::create_dir_all(dir)?;
                std::fs::write(&tmp, body)?;
                std::fs::rename(&tmp, Self::path(dir))
            });
        if let Err(e) = result {
            eprintln!("sidra: could not save settings: {e}");
        }
    }

    /// The page Sidra opens on.
    pub fn start_route(&self) -> Route {
        match self.start_page.as_str() {
            "new" => Route::New,
            "radio" => Route::Radio,
            "all-playlists" => Route::Library(crate::pages::LibraryPage::Playlists),
            "last" => self.last_page.clone().unwrap_or(Route::Home),
            _ => Route::Home,
        }
    }

    /// The colours for the chosen theme; an unknown name or a broken custom theme is Apple Music.
    pub fn palette(&self, dir: &Path) -> style::Palette {
        match ThemeName::parse(&self.theme) {
            Some(ThemeName::Bundled(b)) => style::Palette::from_scheme(&b.theme().dark),
            Some(ThemeName::Custom) => {
                custom_theme(dir).and_then(|t| style::Palette::from_scheme(&t.dark))
            }
            _ => None,
        }
        .unwrap_or(style::APPLE_MUSIC)
    }
}

fn custom_theme(dir: &Path) -> Option<sidra_core::theme::ThemeDefinition<String>> {
    parse_custom_theme(&std::fs::read_to_string(dir.join("custom-theme.json")).ok()?)
}

/// The Settings page. Changes are applied by the app and saved at the end of the frame.
pub fn page(ui: &mut Ui, cx: &mut Cx) {
    heading(ui, "Settings", 30.0);
    ui.add_space(18.0);
    ui.set_max_width(560.0);
    let dir = cx.dir;
    let s = &mut *cx.settings;

    section(ui, "Player", |ui| {
        row(ui, "Start page", |ui| {
            let current = START_PAGES
                .iter()
                .find(|(id, _)| *id == s.start_page)
                .map_or("Home", |(_, l)| l);
            egui::ComboBox::from_id_salt("start-page")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (id, label) in START_PAGES {
                        ui.selectable_value(&mut s.start_page, id.to_owned(), label);
                    }
                });
        });
    });

    section(ui, "Style", |ui| {
        row(ui, "Style", |ui| {
            let mut themes: Vec<(String, String)> =
                vec![("apple-music".into(), "Apple Music".into())];
            themes.extend(
                BUNDLED_THEMES
                    .iter()
                    .map(|t| (t.name.to_owned(), t.label.to_owned())),
            );
            if custom_theme(dir).is_some() {
                themes.push(("custom".into(), "Custom Theme".into()));
            }
            let current = themes
                .iter()
                .find(|(n, _)| *n == s.theme)
                .map_or("Apple Music".to_owned(), |(_, l)| l.clone());
            egui::ComboBox::from_id_salt("theme")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (name, label) in themes {
                        ui.selectable_value(&mut s.theme, name, label);
                    }
                });
        });
        row(ui, "Zoom", |ui| {
            egui::ComboBox::from_id_salt("zoom")
                .selected_text(format!("{:.0}%", s.zoom_factor * 100.0))
                .show_ui(ui, |ui| {
                    for z in ZOOMS {
                        ui.selectable_value(&mut s.zoom_factor, z, format!("{:.0}%", z * 100.0));
                    }
                });
        });
    });

    section(ui, "Integrations", |ui| {
        let s = &mut *cx.settings;
        toggle(ui, "Close to tray", &mut s.close_to_tray);
        toggle(ui, "Notifications", &mut s.notifications);
        toggle(ui, "Discord Rich Presence", &mut s.discord);
        lastfm_rows(ui, cx);
    });
}

fn lastfm_rows(ui: &mut Ui, cx: &mut Cx) {
    let lastfm = &mut *cx.lastfm;
    let s = &mut cx.settings.lastfm;
    if !lastfm.configured() {
        row(ui, "Last.fm", |ui| {
            ui.label(RichText::new("Not set up").color(style::text_muted()));
        });
        ui.label(
            RichText::new(
                "Last.fm needs an API key. Set SIDRA_LASTFM_API_KEY and SIDRA_LASTFM_API_SECRET, \
                 or put {\"apiKey\": …, \"apiSecret\": …} in lastfm-credentials.json in Sidra's data folder, \
                 then restart Sidra.",
            )
            .font(style::small())
            .color(style::text_muted()),
        );
        return;
    }
    let connected = s.session_key.is_some();
    ui.add_enabled_ui(connected, |ui| toggle(ui, "Last.fm", &mut s.enabled));
    ui.horizontal(|ui| {
        let status = match (&s.username, lastfm.status()) {
            (Some(name), _) if connected => format!("Connected as {name}"),
            (_, Some(status)) => status.to_owned(),
            _ => "Not connected".into(),
        };
        ui.label(RichText::new(status).color(style::text_muted()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if connected {
                if pill_button(ui, icon::SIGN_OUT, "Disconnect", false, 120.0).clicked() {
                    lastfm.disconnect(s);
                }
            } else if lastfm.connecting() {
                ui.spinner();
            } else if pill_button(ui, icon::LINK, "Connect to Last.fm…", true, 180.0).clicked() {
                lastfm.connect();
            }
        });
    });
}

fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(
        RichText::new(title)
            .font(style::bold(15.0))
            .color(style::text()),
    );
    ui.add_space(6.0);
    Frame::new()
        .fill(style::field())
        .stroke(egui::Stroke::new(1.0, style::field_border()))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            add(ui)
        });
    ui.add_space(22.0);
}

fn row(ui: &mut Ui, label: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        ui.label(RichText::new(label).color(style::text()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

fn toggle(ui: &mut Ui, label: &str, on: &mut bool) {
    row(ui, label, |ui| {
        let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 22.0), egui::Sense::click());
        if resp.clicked() {
            *on = !*on;
        }
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label)
        });
        let t = ui.ctx().animate_bool(resp.id, *on);
        let off = if ui.is_enabled() {
            style::field_border()
        } else {
            style::row_hover()
        };
        let track = crate::theme::lerp_color(off, style::accent(), t);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(11), track);
        let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
        ui.painter()
            .circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
        style::focus_ring(ui, &resp, rect, 11);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_keeps_defaults_and_routes_round_trip() {
        let s: Settings = serde_json::from_str(r#"{"startPage":"last","zoomFactor":1.5}"#).unwrap();
        assert!(s.notifications);
        assert_eq!(s.theme, "apple-music");
        assert_eq!(s.start_route(), Route::Home);
        let s = Settings {
            last_page: Some(Route::Playlist("pl.1".into())),
            ..s
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.start_route(), Route::Playlist("pl.1".into()));
        assert_eq!(back.zoom_factor, 1.5);
    }
}
