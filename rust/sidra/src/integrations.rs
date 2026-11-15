//! Desktop notifications, Discord Rich Presence and the tray icon behind "Close to tray".

use crate::engine::{Item, Playback};
use sidra_core::apple::Artwork;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

fn artwork_url(item: &Item, px: u32) -> Option<String> {
    Artwork {
        template: item.artwork.clone()?,
        bg: None,
    }
    .url(px)
}

/// The AppUserModelID Windows files Sidra's notifications under.
#[cfg(windows)]
const APP_ID: &str = "Sidra.Native";

/// Registers Sidra's name and icon for its notifications, which Windows otherwise
/// labels with the id of the process that showed them (PowerShell's, via notify-rust).
/// Per-user and idempotent: it rewrites the same two values on every launch.
#[cfg(windows)]
pub fn register_app_id(dir: &std::path::Path) {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
    let icon = dir.join("sidra.png");
    if std::fs::create_dir_all(dir).is_err()
        || std::fs::write(&icon, include_bytes!("../../../assets/sidra-logo.png")).is_err()
    {
        return;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let key = wide(&format!("Software\\Classes\\AppUserModelId\\{APP_ID}"));
    for (name, value) in [
        ("DisplayName", "Sidra".to_owned()),
        ("IconUri", icon.display().to_string()),
    ] {
        let (name, value) = (wide(name), wide(&value));
        // SAFETY: every pointer is a NUL-terminated UTF-16 buffer that outlives the call.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        };
        if status != 0 {
            eprintln!("sidra: could not register the notification name ({status})");
        }
    }
}

/// Shows a notification when a new song starts playing.
#[derive(Default)]
pub struct Notifier {
    last: Option<String>,
}

impl Notifier {
    pub fn tick(&mut self, enabled: bool, pb: &Playback) {
        let Some(item) = pb.now.as_ref().filter(|_| pb.playing) else {
            return;
        };
        if self.last.as_ref() == Some(&item.id) {
            return;
        }
        self.last = Some(item.id.clone());
        if !enabled || item.title.is_empty() {
            return;
        }
        let summary = item.title.clone();
        let body = [item.artist.as_str(), item.album.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" — ");
        // Showing a toast can block for a moment, so it never runs on the UI thread.
        std::thread::spawn(move || {
            let mut n = notify_rust::Notification::new();
            #[cfg(windows)]
            n.app_id(APP_ID);
            if let Err(e) = n.appname("Sidra").summary(&summary).body(&body).show() {
                eprintln!("sidra: notification failed: {e}");
            }
        });
    }
}

/// What Discord shows: one song, with its start and end so Discord draws the progress bar.
#[derive(Clone, PartialEq)]
struct Presence {
    title: String,
    artist: String,
    album: String,
    art: Option<String>,
    start_ms: i64,
    end_ms: i64,
}

/// Discord Rich Presence, on its own thread because the IPC calls block.
pub struct Discord {
    tx: Sender<Option<Presence>>,
    sent: Option<Option<Presence>>,
}

const DISCORD_CLIENT_ID: &str = "1485248818688688318";

impl Default for Discord {
    fn default() -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || discord_thread(rx));
        Self { tx, sent: None }
    }
}

fn discord_thread(rx: Receiver<Option<Presence>>) {
    use discord_rich_presence::{DiscordIpc, DiscordIpcClient, activity};
    let mut client: Option<DiscordIpcClient> = None;
    let mut want: Option<Presence> = None;
    let mut dirty = false;
    loop {
        // Retries every 15 s while Discord is closed or the last update failed.
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(p) => {
                want = p;
                dirty = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if !dirty {
            continue;
        }
        if client.is_none() {
            if want.is_none() {
                dirty = false;
                continue;
            }
            let mut c = DiscordIpcClient::new(DISCORD_CLIENT_ID);
            if c.connect().is_err() {
                continue;
            }
            client = Some(c);
        }
        let c = client.as_mut().expect("connected above");
        let result = match &want {
            Some(p) => {
                let mut assets = activity::Assets::new()
                    .small_image("sidra_logo")
                    .small_text("Sidra");
                if let Some(art) = &p.art {
                    assets = assets.large_image(art.as_str());
                }
                if !p.album.is_empty() {
                    assets = assets.large_text(p.album.as_str());
                }
                let mut a = activity::Activity::new()
                    .activity_type(activity::ActivityType::Listening)
                    .status_display_type(activity::StatusDisplayType::Details)
                    .details(p.title.as_str())
                    .state(p.artist.as_str())
                    .assets(assets);
                if p.end_ms > p.start_ms {
                    a = a.timestamps(activity::Timestamps::new().start(p.start_ms).end(p.end_ms));
                }
                c.set_activity(a)
            }
            None => c.clear_activity(),
        };
        match result {
            Ok(()) => dirty = false,
            Err(_) => {
                let _ = c.close();
                client = None;
            }
        }
    }
}

impl Discord {
    pub fn tick(&mut self, enabled: bool, pb: &Playback, position: f64) {
        let want = pb
            .now
            .as_ref()
            .filter(|_| enabled && pb.playing)
            .map(|item| {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as i64);
                // Rounded to seconds so the playhead's drift doesn't resend every frame.
                let start_ms = (now_ms - (position * 1000.0) as i64) / 1000 * 1000;
                Presence {
                    title: item.title.clone(),
                    artist: item.artist.clone(),
                    album: item.album.clone(),
                    art: artwork_url(item, 512),
                    start_ms,
                    end_ms: if pb.live {
                        0
                    } else {
                        start_ms + item.duration_ms as i64
                    },
                }
            });
        // A seek moves the start by more than a couple of seconds; drift alone never does.
        let same = match (&self.sent, &want) {
            (Some(Some(a)), Some(b)) => {
                a.title == b.title && a.artist == b.artist && (a.start_ms - b.start_ms).abs() < 3000
            }
            (Some(None), None) => true,
            _ => false,
        };
        if !same {
            let _ = self.tx.send(want.clone());
            self.sent = Some(want);
        }
    }
}

/// The tray icon shown while "Close to tray" is on. Closing the window hides it; the
/// icon's menu brings it back or quits.
#[cfg(windows)]
pub mod tray {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SW_HIDE, SW_SHOW, SetForegroundWindow, ShowWindow,
    };

    static HWND: AtomicIsize = AtomicIsize::new(0);
    /// Set by the tray's Quit item: the next close request really closes.
    pub static QUITTING: AtomicBool = AtomicBool::new(false);

    fn show() {
        let hwnd = HWND.load(Ordering::Relaxed);
        if hwnd != 0 {
            // SAFETY: the handle is Sidra's own top-level window, alive for the whole process.
            unsafe {
                ShowWindow(hwnd as _, SW_SHOW);
                SetForegroundWindow(hwnd as _);
            }
        }
    }

    pub fn hide() {
        let hwnd = HWND.load(Ordering::Relaxed);
        if hwnd != 0 {
            // SAFETY: as in `show`.
            unsafe { ShowWindow(hwnd as _, SW_HIDE) };
        }
    }

    #[derive(Default)]
    pub struct Tray {
        icon: Option<TrayIcon>,
        handlers: bool,
    }

    impl Tray {
        /// Shows or removes the icon to match `enabled`.
        pub fn tick(&mut self, enabled: bool, frame: &eframe::Frame, ctx: &egui::Context) {
            if HWND.load(Ordering::Relaxed) == 0 {
                use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
                if let Ok(h) = frame.window_handle()
                    && let RawWindowHandle::Win32(w) = h.as_raw()
                {
                    HWND.store(w.hwnd.get(), Ordering::Relaxed);
                }
            }
            if !enabled {
                if self.icon.take().is_some() {
                    show();
                }
                return;
            }
            if self.icon.is_some() {
                return;
            }
            let menu = Menu::new();
            let open = MenuItem::with_id("show", "Show Sidra", true, None);
            let quit = MenuItem::with_id("quit", "Quit Sidra", true, None);
            let _ = menu.append_items(&[&open, &quit]);
            let image =
                image::load_from_memory(include_bytes!("../../../assets/icons/sidra-tray@2x.png"))
                    .ok()
                    .map(|i| i.into_rgba8())
                    .and_then(|i| {
                        let (w, h) = i.dimensions();
                        tray_icon::Icon::from_rgba(i.into_raw(), w, h).ok()
                    });
            let mut builder = TrayIconBuilder::new()
                .with_tooltip("Sidra")
                .with_menu(Box::new(menu));
            if let Some(image) = image {
                builder = builder.with_icon(image);
            }
            match builder.build() {
                Ok(icon) => self.icon = Some(icon),
                Err(e) => eprintln!("sidra: tray icon failed: {e}"),
            }
            if !self.handlers {
                self.handlers = true;
                // Handled where they arrive: a hidden window gets no frames to poll from.
                let c = Arc::new(ctx.clone());
                let c2 = c.clone();
                MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                    if e.id == "quit" {
                        QUITTING.store(true, Ordering::Relaxed);
                        show();
                        c.send_viewport_cmd(egui::ViewportCommand::Close);
                    } else {
                        show();
                    }
                    c.request_repaint();
                }));
                TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = e
                    {
                        show();
                        c2.request_repaint();
                    }
                }));
            }
        }
    }
}
