//! Atalhos globais (Windows): valem com o Cyder em segundo plano ou escondido na bandeja, para quem não tem
//! teclas de mídia. Uma thread própria registra os atalhos com RegisterHotKey e espera as mensagens deles.
use std::{ptr::null_mut, sync::Mutex};

use gpui::{Context, FocusHandle, KeyDownEvent, Keystroke, Window};
use windows_sys::Win32::{
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey},
        WindowsAndMessaging::{GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_APP, WM_HOTKEY},
    },
};

use super::{Cyder, VOLUME_STEP};

/// Na ordem de `Settings::hotkeys`.
pub(super) const NAMES: [&str; 6] = ["Tocar ou pausar", "Próxima", "Anterior", "Aumentar o volume", "Diminuir o volume", "Curtir a faixa atual"];

/// O que a thread deve registrar (modificadores, tecla virtual); ela lê ao receber WM_APP.
static WANTED: Mutex<Vec<Option<(u32, u32)>>> = Mutex::new(Vec::new());

enum Event {
    Pressed(usize),
    /// Os que o Windows recusou: outro programa já usa a combinação.
    Taken([bool; 6]),
}

pub(super) struct State {
    thread: u32,
    pub(super) taken: [bool; 6],
    /// O atalho esperando a combinação nova.
    pub(super) recording: Option<usize>,
    pub(super) error: Option<String>,
    pub(super) focus: FocusHandle,
}

impl State {
    pub(super) fn new(cx: &mut Context<Cyder>) -> Self {
        State { thread: 0, taken: [false; 6], recording: None, error: None, focus: cx.focus_handle() }
    }
}

/// "ctrl-alt-space" → (MOD_CONTROL | MOD_ALT, VK_SPACE). Sem Ctrl, Alt ou Win o atalho tomaria a tecla de todo
/// programa, então não vale.
fn parse(s: &str) -> Option<(u32, u32)> {
    let k = Keystroke::parse(s).ok()?;
    let m = k.modifiers;
    if !(m.control || m.alt || m.platform) {
        return None;
    }
    let vk = match k.key.as_str() {
        "space" => 0x20,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "end" => 0x23,
        "home" => 0x24,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "insert" => 0x2d,
        "delete" => 0x2e,
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() => key.as_bytes()[0].to_ascii_uppercase() as u32,
        key => match key.strip_prefix('f')?.parse::<u32>().ok()? {
            n @ 1..=24 => 0x6f + n,
            _ => return None,
        },
    };
    let mods = [(m.control, MOD_CONTROL), (m.alt, MOD_ALT), (m.shift, MOD_SHIFT), (m.platform, MOD_WIN)];
    Some((mods.iter().filter(|(on, _)| *on).fold(0, |acc, (_, f)| acc | f), vk))
}

/// "ctrl-alt-space" → "Ctrl + Alt + Espaço", como na janela de atalhos.
pub(super) fn label(s: &str) -> String {
    let Ok(k) = Keystroke::parse(s) else { return s.into() };
    let m = k.modifiers;
    let mut parts: Vec<String> = [(m.control, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift"), (m.platform, "Win")]
        .iter().filter(|(on, _)| *on).map(|(_, name)| name.to_string()).collect();
    parts.push(match k.key.as_str() {
        "space" => "Espaço".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "pageup" => "Page Up".into(),
        "pagedown" => "Page Down".into(),
        key if key.len() == 1 => key.to_uppercase(),
        key => key[..1].to_uppercase() + &key[1..],
    });
    parts.join(" + ")
}

/// A thread dos atalhos: os atalhos registrados sem janela chegam na fila de mensagens dela.
fn run(ready: std::sync::mpsc::Sender<u32>, tx: tokio::sync::mpsc::UnboundedSender<Event>) {
    // SAFETY: MSG é dado puro; as chamadas usam a fila desta thread.
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    unsafe {
        // Cria a fila antes de passar o id: antes disso o PostThreadMessageW se perderia.
        PeekMessageW(&mut msg, null_mut(), WM_APP, WM_APP, PM_NOREMOVE);
        let _ = ready.send(GetCurrentThreadId());
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            match msg.message {
                WM_HOTKEY => drop(tx.send(Event::Pressed(msg.wParam.wrapping_sub(1)))),
                WM_APP => {
                    let wanted = WANTED.lock().map(|w| w.clone()).unwrap_or_default();
                    // Solta todos antes: trocar os atalhos de dois comandos entre si não esbarra neles mesmos.
                    (1..=6).for_each(|id| { UnregisterHotKey(null_mut(), id); });
                    let mut taken = [false; 6];
                    for (i, keys) in wanted.iter().enumerate().take(6) {
                        if let Some((mods, vk)) = keys {
                            taken[i] = RegisterHotKey(null_mut(), i as i32 + 1, *mods, *vk) == 0;
                        }
                    }
                    let _ = tx.send(Event::Taken(taken));
                }
                _ => {}
            }
        }
    }
}

impl Cyder {
    pub fn attach_hotkeys(&mut self, cx: &mut Context<Self>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        if let Err(e) = std::thread::Builder::new().name("hotkeys".into()).spawn(move || run(ready_tx, tx)) {
            return log::warn!("atalhos globais indisponíveis: {e}");
        }
        self.hotkeys.thread = ready_rx.recv().unwrap_or(0);
        cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.recv().await {
                if this.update(cx, |s, cx| { s.hotkey_event(ev); cx.notify(); }).is_err() {
                    break;
                }
            }
        })
        .detach();
        self.apply_hotkeys();
    }

    /// Registra os atalhos das configurações, ou solta todos (desligados ou gravando um novo).
    pub(super) fn apply_hotkeys(&mut self) {
        let on = self.settings.global_hotkeys && self.hotkeys.recording.is_none();
        let wanted = self.settings.hotkeys.iter().enumerate().map(|(i, k)| {
            // Segurar o volume continua mudando; os outros valem uma vez por toque.
            parse(k).map(|(mods, vk)| (if matches!(i, 3 | 4) { mods } else { mods | MOD_NOREPEAT }, vk))
        });
        if let Ok(mut w) = WANTED.lock() {
            *w = if on { wanted.collect() } else { Vec::new() };
        }
        // SAFETY: id de thread; se ela não existe, a chamada só falha.
        unsafe { PostThreadMessageW(self.hotkeys.thread, WM_APP, 0, 0) };
    }

    fn hotkey_event(&mut self, ev: Event) {
        match ev {
            Event::Taken(taken) => return self.hotkeys.taken = taken,
            Event::Pressed(0) => self.toggle_play(),
            Event::Pressed(1) => self.next_track(),
            Event::Pressed(2) => self.prev_track(),
            Event::Pressed(3) => self.set_volume(self.volume + VOLUME_STEP),
            Event::Pressed(4) => self.set_volume(self.volume - VOLUME_STEP),
            Event::Pressed(_) => self.toggle_like_current(),
        }
        self.sync_media();
    }

    /// Começa a esperar a combinação nova do atalho `i`, com os atalhos soltos para ela chegar à janela.
    pub(super) fn record_hotkey(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.hotkeys.recording = Some(i);
        self.hotkeys.error = None;
        window.focus(&self.hotkeys.focus, cx);
        self.apply_hotkeys();
    }

    pub(super) fn stop_recording_hotkey(&mut self) {
        self.hotkeys.recording = None;
        self.apply_hotkeys();
    }

    /// A combinação apertada enquanto grava. Esc sozinho cancela.
    pub(super) fn hotkey_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.hotkeys.recording else { return };
        cx.stop_propagation();
        let k = &e.keystroke;
        // Só o modificador, por enquanto: espera a tecla.
        if matches!(k.key.as_str(), "control" | "alt" | "shift" | "platform" | "function") {
            return;
        }
        let keys = Keystroke { modifiers: k.modifiers, key: k.key.clone(), key_char: None }.unparse();
        if k.key == "escape" && keys == "escape" {
            self.hotkeys.error = None;
        } else if parse(&keys).is_none() {
            self.hotkeys.error = Some("Use Ctrl, Alt ou Win com uma letra, número, seta, Espaço ou F1 a F24.".into());
            return cx.notify();
        } else if let Some(j) = (0..6).find(|&j| j != i && self.settings.hotkeys[j] == keys) {
            self.hotkeys.error = Some(format!("{} já é o atalho de “{}”.", label(&keys), NAMES[j]));
            return cx.notify();
        } else {
            self.hotkeys.error = None;
            self.hotkeys.recording = None;
            self.change(|st| st.hotkeys[i] = keys, false);
        }
        self.stop_recording_hotkey();
        window.focus(&self.focus, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(parse("ctrl-alt-space"), Some((MOD_CONTROL | MOD_ALT, 0x20)));
        assert_eq!(parse("ctrl-alt-l"), Some((MOD_CONTROL | MOD_ALT, 0x4c)));
        assert_eq!(parse("ctrl-shift-f12"), Some((MOD_CONTROL | MOD_SHIFT, 0x7b)));
        assert_eq!(parse("shift-l"), None);
        assert_eq!(parse("ctrl-alt-tab"), None);
        assert_eq!(label("ctrl-alt-right"), "Ctrl + Alt + →");
        assert_eq!(label("ctrl-alt-space"), "Ctrl + Alt + Espaço");
    }
}
