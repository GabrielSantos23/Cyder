//! Ícone na bandeja do sistema (Windows): com "Continuar tocando ao fechar", fechar a janela a esconde ali.
//! Clicar no ícone traz a janela de volta; o menu tem os controles, o mini player e Sair.
use std::ffi::c_void;

use gpui::{Context, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{IsIconic, SW_HIDE, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow};

use super::Cyder;
use crate::icon::icon_rgba;

fn hwnd(window: &Window) -> Option<*mut c_void> {
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get() as *mut c_void),
        _ => None,
    }
}

/// Esconde a janela (some da barra de tarefas; o ícone da bandeja fica).
pub fn hide(window: &Window) {
    if let Some(h) = hwnd(window) {
        // SAFETY: HWND da janela viva do GPUI.
        unsafe { ShowWindow(h, SW_HIDE) };
    }
}

fn window_file() -> std::path::PathBuf {
    cyder_core::config::cache_dir().join("window.hwnd")
}

/// A janela principal do Cyder que já está aberto, pelo número que ele gravou ao abrir. Confere que ainda é
/// uma janela do GPUI ("Zed::Window"): o número pode ter sido reaproveitado por outra janela depois.
pub fn running_window() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, IsWindow};
    let hwnd: isize = std::fs::read_to_string(window_file()).ok()?.trim().parse().ok()?;
    let h = hwnd as *mut c_void;
    let mut class = [0u16; 32];
    // SAFETY: IsWindow aceita qualquer valor; o buffer tem o tamanho passado.
    let len = unsafe {
        if IsWindow(h) == 0 {
            return None;
        }
        GetClassNameW(h, class.as_mut_ptr(), class.len() as i32)
    };
    (String::from_utf16_lossy(&class[..len.max(0) as usize]) == "Zed::Window").then_some(hwnd)
}

/// Mostra a janela de novo, desminimiza e traz para a frente.
pub fn show(hwnd: isize) {
    let h = hwnd as *mut c_void;
    // SAFETY: HWND da janela principal, que vive até o app sair.
    unsafe {
        ShowWindow(h, if IsIconic(h) != 0 { SW_RESTORE } else { SW_SHOW });
        SetForegroundWindow(h);
    }
}

pub(super) struct Tray {
    _icon: TrayIcon,
}

enum Do {
    Show,
    Toggle,
    Next,
    Prev,
    Mini,
    Quit,
}

impl Cyder {
    pub fn attach_tray(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(h) = hwnd(window) else { return };
        self.hwnd = h as isize;
        // Para a segunda abertura achar esta janela: o título muda com a música, então não dá para procurar por ele.
        let _ = std::fs::write(window_file(), self.hwnd.to_string());
        let items = [
            ("show", "Mostrar o Cyder"),
            ("toggle", "Tocar ou pausar"),
            ("next", "Próxima"),
            ("prev", "Anterior"),
            ("mini", "Mini player"),
            ("quit", "Sair"),
        ];
        let menu = Menu::new();
        for (i, (id, label)) in items.iter().enumerate() {
            if matches!(i, 1 | 4 | 5) {
                let _ = menu.append(&PredefinedMenuItem::separator());
            }
            let _ = menu.append(&MenuItem::with_id(*id, *label, true, None));
        }
        let icon = match Icon::from_rgba(icon_rgba(32), 32, 32) {
            Ok(i) => i,
            Err(e) => return log::warn!("ícone da bandeja: {e}"),
        };
        let tray = match TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Cyder").with_icon(icon).build() {
            Ok(t) => t,
            Err(e) => return log::warn!("bandeja indisponível: {e}"),
        };
        self.tray = Some(Tray { _icon: tray });
        // Os eventos chegam fora da thread da UI; o canal os traz para cá.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let menu_tx = tx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let what = match e.id.as_ref() {
                "show" => Do::Show,
                "toggle" => Do::Toggle,
                "next" => Do::Next,
                "prev" => Do::Prev,
                "mini" => Do::Mini,
                _ => Do::Quit,
            };
            let _ = menu_tx.send(what);
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                let _ = tx.send(Do::Show);
            }
        }));
        cx.spawn(async move |this, cx| {
            while let Some(what) = rx.recv().await {
                let alive = this.update(cx, |s, cx| {
                    match what {
                        Do::Show => show(s.hwnd),
                        Do::Toggle => s.toggle_play(),
                        Do::Next => s.next_track(),
                        Do::Prev => s.prev_track(),
                        Do::Mini => s.toggle_mini(cx),
                        Do::Quit => cx.quit(),
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_shape() {
        let px = super::icon_rgba(32);
        let at = |x: usize, y: usize| &px[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        // Canto transparente, borda branca, play preto no meio.
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(4, 16), [255, 255, 255, 255]);
        assert_eq!(at(16, 16)[..3], [0, 0, 0]);
    }
}
