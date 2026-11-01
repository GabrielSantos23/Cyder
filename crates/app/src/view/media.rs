//! Teclas de mídia e o "tocando agora" do sistema: SMTC no Windows (o cartão de mídia e a tela de bloqueio),
//! MPRIS no Linux e Now Playing no macOS, pelo souvlaki.
use std::{ffi::c_void, time::Duration};

use gpui::{Context, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};

use super::{Cyder, SEEK_STEP_MS};
use cyder_core::Cmd;

/// Faixa sendo ouvida por aqui, para o Last.fm.
pub(super) struct Listen {
    uri: String,
    /// Quando começou, em segundos Unix: o horário do scrobble.
    started: u64,
    played: Duration,
    last: std::time::Instant,
    now_playing: bool,
    scrobbled: bool,
}

/// Regra do Last.fm: conta a faixa de mais de 30 s ouvida até a metade ou por 4 minutos.
fn counts(duration_ms: u32, played: Duration) -> bool {
    duration_ms > 30_000 && played >= Duration::from_millis(duration_ms as u64 / 2).min(Duration::from_secs(240))
}

/// O que o sistema já mostra, para só mandar de novo quando muda (a posição anda sozinha lá).
#[derive(Clone, Default, PartialEq)]
pub(super) struct Shown {
    uri: String,
    playing: bool,
    anchor_ms: u32,
    /// Tocando, a posição de 5 em 5 s: o Windows não anda a barra do cartão sozinho.
    step: u32,
}

impl Cyder {
    /// Registra o Cyder no controle de mídia do sistema. No Windows precisa da janela (HWND).
    pub fn attach_media(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Com o tema "Sistema", trocar o modo do Windows repinta na hora.
        cx.observe_window_appearance(window, |_, _, cx| cx.notify()).detach();
        let hwnd = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get() as *mut c_void),
            _ => None,
        };
        let mut controls = match MediaControls::new(PlatformConfig { display_name: "Cyder", dbus_name: "cyder", hwnd }) {
            Ok(c) => c,
            Err(e) => return log::warn!("teclas de mídia indisponíveis: {e:?}"),
        };
        // Os eventos chegam numa thread do sistema; o canal os traz para a thread da UI.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        if let Err(e) = controls.attach(move |ev| drop(tx.send(ev))) {
            return log::warn!("teclas de mídia indisponíveis: {e:?}");
        }
        self.media = Some(controls);
        #[cfg(windows)]
        if let Some(hwnd) = hwnd {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            match super::thumbbar::attach(hwnd, tx) {
                Ok(()) => {
                    cx.spawn(async move |this, cx| {
                        while let Some(button) = rx.recv().await {
                            if this.update(cx, |s, cx| { s.thumb_button(button); s.sync_media(); cx.notify(); }).is_err() {
                                break;
                            }
                        }
                    })
                    .detach();
                }
                Err(e) => log::warn!("botões da miniatura indisponíveis: {e}"),
            }
        }
        cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.recv().await {
                if this.update(cx, |s, cx| { s.media_event(ev); s.sync_media(); cx.notify(); }).is_err() {
                    break;
                }
            }
        })
        .detach();
        self.sync_media();
    }

    #[cfg(windows)]
    fn thumb_button(&mut self, button: super::thumbbar::Button) {
        use super::thumbbar::Button;
        match button {
            Button::Like => {
                if let Some(t) = self.tracks.get(self.current).cloned() {
                    let liked = self.is_liked(&t.uri);
                    self.set_liked(&t, !liked);
                }
            }
            Button::Prev => self.prev_track(),
            Button::Toggle => self.toggle_play(),
            Button::Next => self.next_track(),
        }
    }

    fn media_event(&mut self, ev: MediaControlEvent) {
        let step = |dir: SeekDirection, by: i64| if dir == SeekDirection::Forward { by } else { -by };
        match ev {
            MediaControlEvent::Toggle => self.toggle_play(),
            MediaControlEvent::Play if !self.playing => self.toggle_play(),
            MediaControlEvent::Pause | MediaControlEvent::Stop if self.playing => self.toggle_play(),
            MediaControlEvent::Next => self.next_track(),
            MediaControlEvent::Previous => self.prev_track(),
            MediaControlEvent::SetPosition(MediaPosition(at)) => self.seek(at.as_millis() as u32),
            MediaControlEvent::Seek(dir) => self.seek_by(step(dir, SEEK_STEP_MS as i64)),
            MediaControlEvent::SeekBy(dir, by) => self.seek_by(step(dir, by.as_millis() as i64)),
            _ => {}
        }
    }

    /// Conta o tempo ouvido da faixa atual e manda ao Last.fm o "ouvindo agora" e o scrobble.
    /// Só o que toca neste computador: o que toca no celular, o app de lá registra.
    fn track_listen(&mut self) {
        let track = self.tracks.get(self.current).filter(|_| self.lastfm_user.is_some() && self.elsewhere().is_none()).cloned();
        let Some(track) = track else {
            self.listen = None;
            return;
        };
        let position = self.position();
        let now = std::time::Instant::now();
        // Repetindo a mesma faixa, ela volta ao começo: é outra vez ouvida.
        let replay = self.listen.as_ref().is_some_and(|l| l.uri == track.uri && l.scrobbled && position < 3000);
        if replay || self.listen.as_ref().is_none_or(|l| l.uri != track.uri) {
            let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            self.listen = Some(Listen { uri: track.uri.clone(), started: unix.saturating_sub(position as u64 / 1000), played: Duration::ZERO, last: now, now_playing: false, scrobbled: false });
        }
        let Some(l) = self.listen.as_mut() else { return };
        let mut cmds = Vec::new();
        if self.playing {
            l.played += now - l.last;
            if !l.now_playing {
                l.now_playing = true;
                cmds.push(Cmd::NowPlaying(track.clone()));
            }
        }
        l.last = now;
        if !l.scrobbled && counts(track.duration_ms, l.played) {
            l.scrobbled = true;
            cmds.push(Cmd::Scrobble { track, at: l.started });
        }
        cmds.into_iter().for_each(|c| self.send(c));
    }

    /// Manda faixa, capa e estado ao sistema quando algo mudou (e ao Discord e ao Last.fm).
    pub(super) fn sync_media(&mut self) {
        self.track_listen();
        self.sync_discord();
        self.sync_canvas();
        #[cfg(windows)]
        super::thumbbar::update(super::thumbbar::Shown {
            track: self.tracks.get(self.current).is_some(),
            playing: self.playing,
            liked: self.tracks.get(self.current).is_some_and(|t| self.is_liked(&t.uri)),
        });
        let position = self.position();
        let Some(controls) = self.media.as_mut() else { return };
        let track = self.tracks.get(self.current);
        let step = if self.playing { position / 5000 } else { 0 };
        let now = Shown { uri: track.map(|t| t.uri.clone()).unwrap_or_default(), playing: self.playing, anchor_ms: self.anchor_ms, step };
        if now == self.media_shown {
            return;
        }
        if now.uri != self.media_shown.uri {
            let metadata = track.map(|t| MediaMetadata {
                title: Some(&t.name),
                artist: Some(&t.artist),
                album: Some(&t.album),
                cover_url: t.cover_large.as_deref(),
                duration: Some(Duration::from_millis(t.duration_ms as u64)),
            });
            if let Err(e) = controls.set_metadata(metadata.unwrap_or_default()) {
                log::warn!("tocando agora: {e:?}");
            }
        }
        let progress = Some(MediaPosition(Duration::from_millis(position as u64)));
        let playback = match (track.is_some(), self.playing) {
            (false, _) => MediaPlayback::Stopped,
            (true, true) => MediaPlayback::Playing { progress },
            (true, false) => MediaPlayback::Paused { progress },
        };
        if let Err(e) = controls.set_playback(playback) {
            log::warn!("tocando agora: {e:?}");
        }
        self.media_shown = now;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn scrobble_rule() {
        assert!(!super::counts(25_000, Duration::from_secs(25)));
        assert!(!super::counts(200_000, Duration::from_secs(99)));
        assert!(super::counts(200_000, Duration::from_secs(100)));
        // Faixa longa: 4 minutos bastam.
        assert!(super::counts(1_200_000, Duration::from_secs(240)));
    }
}
