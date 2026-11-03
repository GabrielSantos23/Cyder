//! Notificação do Windows (toast) na troca de faixa: capa, nome, artista e "Pular". Só com a opção ligada,
//! a janela do Cyder fora de foco e a música tocando neste computador.
use std::time::{Duration, Instant};

use tauri_winrt_notification::{IconCrop, Toast};
use windows_sys::Win32::{
    System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW},
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

use super::Cyder;

/// AppUserModelID: o Windows só mostra toasts de um app registrado (aqui, em HKCU, sem atalho no Menu Iniciar).
const AUMID: &str = "Cyder.Player";
/// Sem a capa até lá, mostra sem ela.
const ART_WAIT: Duration = Duration::from_secs(3);

/// A faixa já tratada: notificada ou descartada (janela em foco, opção desligada, outro aparelho).
#[derive(Default)]
pub(super) struct Toasted {
    uri: String,
    since: Option<Instant>,
    done: bool,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Nome e ícone que aparecem no toast e nas Configurações de notificações do Windows.
fn register() {
    let icon = cyder_core::config::cache_dir().join("cyder-toast.png");
    if !icon.exists() {
        let _ = image::save_buffer(&icon, &crate::icon::icon_rgba(64), 64, 64, image::ColorType::Rgba8);
    }
    let key = wide(&format!(r"Software\Classes\AppUserModelId\{AUMID}"));
    for (name, value) in [("DisplayName", "Cyder".to_string()), ("IconUri", icon.display().to_string())] {
        let (name, value) = (wide(name), wide(&value));
        // SAFETY: textos terminados em zero; o tamanho é em bytes e inclui o zero final.
        unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32) };
    }
}

/// O Windows não mostra as notificações do Cyder: desligadas no sistema todo ou só para ele.
pub(crate) fn blocked() -> bool {
    use windows::UI::Notifications::{NotificationSetting, ToastNotificationManager};
    ToastNotificationManager::CreateToastNotifierWithId(&windows::core::HSTRING::from(AUMID))
        .and_then(|n| n.Setting())
        .is_ok_and(|s| s != NotificationSetting::Enabled)
}

/// A janela em primeiro plano é do Cyder (a principal ou o mini player).
fn focused() -> bool {
    let mut pid = 0;
    // SAFETY: só lê o processo da janela em primeiro plano.
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    pid == std::process::id()
}

impl Cyder {
    pub(super) fn sync_toast(&mut self) {
        let Some(track) = self.tracks.get(self.current) else { return };
        if track.uri != self.toasted.uri {
            self.toasted = Toasted { uri: track.uri.clone(), since: Some(Instant::now()), done: false };
        }
        // Pausado, espera: a troca de faixa conta quando começa a tocar.
        if self.toasted.done || !self.playing {
            return;
        }
        if !self.settings.toasts || self.elsewhere().is_some() || focused() {
            self.toasted.done = true;
            return;
        }
        let art = self.art.get(&self.current).map(|a| a.cover.clone());
        if art.is_none() && self.toasted.since.is_some_and(|t| t.elapsed() < ART_WAIT) {
            return;
        }
        self.toasted.done = true;
        let Some(tx) = self.toast_tx.clone() else { return };
        if !self.toast_registered {
            self.toast_registered = true;
            register();
        }
        // Uma só na Central de Notificações: a faixa anterior sai.
        if let Ok(history) = windows::UI::Notifications::ToastNotificationManager::History() {
            let _ = history.ClearWithId(&windows::core::HSTRING::from(AUMID));
        }
        let mut toast = Toast::new(AUMID).title(&track.name).text1(&track.artist).sound(None)
            .add_button("Pular", "skip")
            .on_activated(move |action| {
                if action.as_deref() == Some("skip") {
                    let _ = tx.send(());
                }
                Ok(())
            });
        if let Some(cover) = &art {
            toast = toast.icon(cover, IconCrop::Square, &track.album);
        }
        if let Err(e) = toast.show() {
            log::warn!("notificação da faixa: {e}");
        }
    }
}
