#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod autostart;
#[cfg(windows)]
mod icon;
mod image_lru;
mod theme;
mod view;

use std::borrow::Cow;

use gpui::{
    App, AssetSource, Bounds, KeyBinding, point, SharedString, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px,
    size,
};

use view::{Cyder, ExitImmersive, FocusNext, FocusPrev, FocusSearch, NextTrack, PrevTrack, SeekBack, SeekForward, Quit, ToggleLyrics, TogglePlay, VolumeDown, VolumeUp};

/// "Mostrar animações no Windows" desligado = movimento reduzido.
#[cfg(windows)]
fn os_reduce_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};
    let mut enabled: i32 = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION escreve um BOOL no ponteiro passado.
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&mut enabled as *mut i32).cast(), 0) };
    ok != 0 && enabled == 0
}

#[cfg(not(windows))]
fn os_reduce_motion() -> bool {
    false
}

/// Ícones Lucide (ISC) embutidos no binário.
struct Assets;

const ICONS: &[(&str, &[u8])] = &[
    ("icons/play.svg", include_bytes!("../assets/icons/play.svg")),
    ("icons/pause.svg", include_bytes!("../assets/icons/pause.svg")),
    ("icons/minus.svg", include_bytes!("../assets/icons/minus.svg")),
    ("icons/square.svg", include_bytes!("../assets/icons/square.svg")),
    ("icons/x.svg", include_bytes!("../assets/icons/x.svg")),
    ("icons/music.svg", include_bytes!("../assets/icons/music.svg")),
    ("icons/rewind.svg", include_bytes!("../assets/icons/rewind.svg")),
    ("icons/fast-forward.svg", include_bytes!("../assets/icons/fast-forward.svg")),
    ("icons/shuffle.svg", include_bytes!("../assets/icons/shuffle.svg")),
    ("icons/repeat.svg", include_bytes!("../assets/icons/repeat.svg")),
    ("icons/repeat-1.svg", include_bytes!("../assets/icons/repeat-1.svg")),
    ("icons/volume.svg", include_bytes!("../assets/icons/volume.svg")),
    ("icons/volume-1.svg", include_bytes!("../assets/icons/volume-1.svg")),
    ("icons/volume-2.svg", include_bytes!("../assets/icons/volume-2.svg")),
    ("icons/volume-x.svg", include_bytes!("../assets/icons/volume-x.svg")),
    ("icons/mic-vocal.svg", include_bytes!("../assets/icons/mic-vocal.svg")),
    ("icons/disc-3.svg", include_bytes!("../assets/icons/disc-3.svg")),
    ("icons/user-round.svg", include_bytes!("../assets/icons/user-round.svg")),
    ("icons/list-music.svg", include_bytes!("../assets/icons/list-music.svg")),
    ("icons/chevron-left.svg", include_bytes!("../assets/icons/chevron-left.svg")),
    ("icons/maximize-2.svg", include_bytes!("../assets/icons/maximize-2.svg")),
    ("icons/chevron-right.svg", include_bytes!("../assets/icons/chevron-right.svg")),
    ("icons/panel-left.svg", include_bytes!("../assets/icons/panel-left.svg")),
    ("icons/list-end.svg", include_bytes!("../assets/icons/list-end.svg")),
    ("icons/headphones.svg", include_bytes!("../assets/icons/headphones.svg")),
    ("icons/check.svg", include_bytes!("../assets/icons/check.svg")),
    ("icons/laptop.svg", include_bytes!("../assets/icons/laptop.svg")),
    ("icons/smartphone.svg", include_bytes!("../assets/icons/smartphone.svg")),
    ("icons/speaker.svg", include_bytes!("../assets/icons/speaker.svg")),
    ("icons/heart.svg", include_bytes!("../assets/icons/heart.svg")),
    ("icons/pin.svg", include_bytes!("../assets/icons/pin.svg")),
    ("icons/house.svg", include_bytes!("../assets/icons/house.svg")),
    ("icons/rotate-cw.svg", include_bytes!("../assets/icons/rotate-cw.svg")),
    ("icons/circle-x.svg", include_bytes!("../assets/icons/circle-x.svg")),
    ("icons/copy.svg", include_bytes!("../assets/icons/copy.svg")),
    ("icons/external-link.svg", include_bytes!("../assets/icons/external-link.svg")),
    ("icons/pin-off.svg", include_bytes!("../assets/icons/pin-off.svg")),
    ("icons/ellipsis.svg", include_bytes!("../assets/icons/ellipsis.svg")),
    ("icons/clock.svg", include_bytes!("../assets/icons/clock.svg")),
    ("icons/calendar.svg", include_bytes!("../assets/icons/calendar.svg")),
    ("icons/search.svg", include_bytes!("../assets/icons/search.svg")),
    ("icons/list-filter.svg", include_bytes!("../assets/icons/list-filter.svg")),
    ("icons/settings.svg", include_bytes!("../assets/icons/settings.svg")),
    ("icons/keyboard.svg", include_bytes!("../assets/icons/keyboard.svg")),
    ("icons/log-out.svg", include_bytes!("../assets/icons/log-out.svg")),
    ("icons/audio-lines.svg", include_bytes!("../assets/icons/audio-lines.svg")),
    ("icons/plus.svg", include_bytes!("../assets/icons/plus.svg")),
    ("icons/pencil.svg", include_bytes!("../assets/icons/pencil.svg")),
    ("icons/image.svg", include_bytes!("../assets/icons/image.svg")),
    ("icons/list-minus.svg", include_bytes!("../assets/icons/list-minus.svg")),
    ("icons/list-plus.svg", include_bytes!("../assets/icons/list-plus.svg")),
    ("icons/eye-off.svg", include_bytes!("../assets/icons/eye-off.svg")),
    ("icons/ban.svg", include_bytes!("../assets/icons/ban.svg")),
    ("icons/users.svg", include_bytes!("../assets/icons/users.svg")),
    ("icons/timer.svg", include_bytes!("../assets/icons/timer.svg")),
    ("icons/radio.svg", include_bytes!("../assets/icons/radio.svg")),
    ("icons/info.svg", include_bytes!("../assets/icons/info.svg")),
    ("icons/share-2.svg", include_bytes!("../assets/icons/share-2.svg")),
    ("icons/heart-off.svg", include_bytes!("../assets/icons/heart-off.svg")),
    ("icons/layout-grid.svg", include_bytes!("../assets/icons/layout-grid.svg")),
    ("icons/podcast.svg", include_bytes!("../assets/icons/podcast.svg")),
    ("icons/grip-vertical.svg", include_bytes!("../assets/icons/grip-vertical.svg")),
    ("icons/heart-filled.svg", include_bytes!("../assets/icons/heart-filled.svg")),
    ("icons/picture-in-picture-2.svg", include_bytes!("../assets/icons/picture-in-picture-2.svg")),
    ("icons/sparkles.svg", include_bytes!("../assets/icons/sparkles.svg")),
    ("icons/chart-column.svg", include_bytes!("../assets/icons/chart-column.svg")),
    ("icons/chevron-up.svg", include_bytes!("../assets/icons/chevron-up.svg")),
    ("icons/log-in.svg", include_bytes!("../assets/icons/log-in.svg")),
    ("icons/chevron-down.svg", include_bytes!("../assets/icons/chevron-down.svg")),
    ("icons/arrow-down.svg", include_bytes!("../assets/icons/arrow-down.svg")),
    ("icons/arrow-up.svg", include_bytes!("../assets/icons/arrow-up.svg")),
    ("icons/menu.svg", include_bytes!("../assets/icons/menu.svg")),
    ("icons/list.svg", include_bytes!("../assets/icons/list.svg")),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS.iter().find(|(p, _)| *p == path).map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICONS.iter().map(|(p, _)| SharedString::from(*p)).collect())
    }
}

/// Já tem um Cyder aberto? Então traz a janela dele para a frente (mesmo escondida na bandeja).
/// Dois abertos viravam dois aparelhos "Cyder" no Spotify Connect, brigando pela reprodução.
/// Depois de uma atualização, a versão nova abre enquanto a antiga ainda está fechando: espera ela sair.
#[cfg(windows)]
fn already_running() -> bool {
    use windows_sys::Win32::{Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError}, System::Threading::CreateMutexW};
    let name: Vec<u16> = "Local\\Cyder.SingleInstance\0".encode_utf16().collect();
    let updated = std::env::args().any(|a| a == view::update::RESTARTED);
    // Até 10 s esperando a antiga, só quando veio de uma atualização.
    for _ in 0..if updated { 50 } else { 1 } {
        // SAFETY: nome terminado em zero; o handle fica aberto até o processo sair, e é isso que marca a instância.
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if mutex.is_null() || unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            return false;
        }
        // SAFETY: handle que acabou de ser aberto; fecha para tentar de novo.
        unsafe { CloseHandle(mutex) };
        if updated {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    if let Some(hwnd) = view::running_window() {
        view::show_window(hwnd);
    }
    true
}

/// Abre onde estava da última vez, se esse lugar ainda cabe numa tela (o monitor pode ter sido desligado).
fn window_bounds(cx: &App) -> WindowBounds {
    let default = Bounds::centered(None, size(px(1280.), px(800.)), cx);
    let Some(p) = cyder_core::config::window_place() else { return WindowBounds::Windowed(default) };
    let bounds = Bounds { origin: point(px(p.x), px(p.y)), size: size(px(p.w.max(MIN_W)), px(p.h.max(MIN_H))) };
    // A barra de título (80 px do topo) precisa estar visível numa tela para dar para arrastar a janela.
    let grab = Bounds { origin: bounds.origin, size: size(bounds.size.width.min(px(200.)), px(80.)) };
    if !cx.displays().iter().any(|d| d.bounds().intersects(&grab)) {
        return WindowBounds::Windowed(default);
    }
    if p.maximized { WindowBounds::Maximized(bounds) } else { WindowBounds::Windowed(bounds) }
}

const MIN_W: f32 = 1100.;
const MIN_H: f32 = 680.;

/// Marca deixada por um panic; a próxima abertura avisa e apaga.
fn crash_marker() -> std::path::PathBuf {
    cyder_core::config::cache_dir().join("crash.txt")
}

/// O executável final não tem console: o log vai para `cyder.log` (e para o stderr, quando houver).
/// Cada abertura começa um arquivo novo e guarda o anterior em `cyder.old.log`.
fn init_log() {
    use std::io::Write;
    struct Tee(Option<std::fs::File>);
    impl Write for Tee {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let _ = std::io::stderr().write_all(buf);
            if let Some(f) = &mut self.0 {
                let _ = f.write_all(buf);
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.as_mut().map_or(Ok(()), |f| f.flush())
        }
    }
    let path = cyder_core::config::log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::rename(&path, dir.join("cyder.old.log"));
    }
    // shortcut: um arquivo por abertura, sem limite de tamanho; no nível info cresce pouco, rotacionar por tamanho se precisar.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,cyder=info,cyder_core=info"))
        .target(env_logger::Target::Pipe(Box::new(Tee(std::fs::File::create(&path).ok()))))
        .init();
    // Um panic fecha o app sem janela de erro: fica no log, com o backtrace, e a próxima abertura avisa.
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("o Cyder fechou por um erro: {info}\n{}", std::backtrace::Backtrace::force_capture());
        let _ = std::fs::write(crash_marker(), info.to_string());
        default(info);
    }));
}

/// Aviso da abertura depois de um panic (uma vez só).
pub fn take_crash_notice() -> Option<String> {
    let marker = crash_marker();
    std::fs::metadata(&marker).ok()?;
    let _ = std::fs::remove_file(&marker);
    Some("O Cyder fechou por um erro da última vez. O registro está em Configurações › Sobre › Registro.".into())
}

fn main() {
    #[cfg(windows)]
    if already_running() {
        return;
    }
    init_log();
    // Aberto pelo Windows ao entrar na conta: começa escondido na bandeja.
    #[cfg(windows)]
    let hidden = std::env::args().any(|a| a == autostart::MINIMIZED);
    #[cfg(not(windows))]
    let hidden = false;
    // A entrada no registro guarda o caminho do .exe: se ele mudou de pasta, a próxima abertura com o Windows acha o novo.
    #[cfg(windows)]
    if cyder_core::config::settings().open_at_login {
        if let Err(e) = autostart::set(true) {
            log::warn!("abrir com o Windows: {e}");
        }
    }
    let (cmds, events) = cyder_core::start();
    gpui_platform::application().with_assets(Assets).run(move |cx: &mut App| {
        cx.set_reduce_motion(os_reduce_motion());
        // Teclas do player ficam desligadas enquanto um campo de texto tem o foco.
        const PLAYER_KEYS: Option<&str> = Some("!TextField");
        cx.bind_keys([
            KeyBinding::new("tab", FocusNext, None),
            KeyBinding::new("shift-tab", FocusPrev, None),
            KeyBinding::new("space", TogglePlay, PLAYER_KEYS),
            KeyBinding::new("right", SeekForward, PLAYER_KEYS),
            KeyBinding::new("left", SeekBack, PLAYER_KEYS),
            KeyBinding::new("ctrl-right", NextTrack, PLAYER_KEYS),
            KeyBinding::new("ctrl-left", PrevTrack, PLAYER_KEYS),
            KeyBinding::new("up", VolumeUp, PLAYER_KEYS),
            KeyBinding::new("down", VolumeDown, PLAYER_KEYS),
            KeyBinding::new("l", ToggleLyrics, PLAYER_KEYS),
            KeyBinding::new("escape", ExitImmersive, PLAYER_KEYS),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-k", FocusSearch, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.open_window(
            WindowOptions {
                window_bounds: Some(window_bounds(cx)),
                window_min_size: Some(size(px(MIN_W), px(MIN_H))),
                titlebar: Some(TitlebarOptions { title: Some("Cyder".into()), appears_transparent: true, traffic_light_position: None }),
                show: !hidden,
                focus: !hidden,
                ..Default::default()
            },
            move |window, cx| {
                // Texto e espaçamentos um pouco menores que o padrão de 16 px do GPUI.
                window.set_rem_size(px(15.));
                // Com "Continuar tocando ao fechar a janela", fechar esconde na bandeja (minimiza fora do Windows); Ctrl+Q sai.
                window.on_window_should_close(cx, |window, _| {
                    if view::KEEP_PLAYING.load(std::sync::atomic::Ordering::Relaxed) {
                        #[cfg(windows)]
                        view::hide_window(window);
                        #[cfg(not(windows))]
                        window.minimize_window();
                        false
                    } else {
                        true
                    }
                });
                let view = cx.new(|cx| Cyder::new(cmds, events, cx));
                view.update(cx, |v, cx| {
                    v.attach_media(window, cx);
                    v.watch_window_place(window, cx);
                    v.watch_window_activation(window, cx);
                    #[cfg(windows)]
                    v.attach_tray(window, cx);
                });
                window.focus(&view.read(cx).focus.clone(), cx);
                view
            },
        )
        .expect("janela");
        if !hidden {
            cx.activate(true);
        }
    });
}
