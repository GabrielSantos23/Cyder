//! Links do Spotify: `spotify:album:ID` (do protocolo, pela linha de comando ou de outra abertura do Cyder) e
//! `https://open.spotify.com/album/ID` (colado com Ctrl+V ou solto na janela) abrem a tela, e a faixa toca.
use cyder_core::{Card, Cmd, Kind};
use gpui::Context;

use super::{Cyder, Phase, library::{Screen, TrackSource}};

/// Tipo e id de um link do Spotify: `spotify:track:ID`, `spotify://track/ID` ou `https://open.spotify.com/intl-pt/track/ID?si=…`.
pub fn parse(link: &str) -> Option<(&str, &str)> {
    let link = link.trim();
    let rest = link.strip_prefix("spotify:").map(|r| r.trim_start_matches('/'))
        .or_else(|| link.strip_prefix("https://open.spotify.com/"))
        .or_else(|| link.strip_prefix("http://open.spotify.com/"))?;
    let rest = rest.split(['?', '#']).next()?;
    let mut parts = rest.split([':', '/']).filter(|p| !p.is_empty() && !p.starts_with("intl-"));
    let (kind, id) = (parts.next()?, parts.next()?);
    let known = ["track", "album", "playlist", "artist", "show", "episode"].contains(&kind);
    // Ids do Spotify: 22 letras e números (base62).
    (known && id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric())).then_some((kind, id))
}

impl Cyder {
    /// Abre o link (ou guarda até o login terminar). Falso se não é um link do Spotify.
    pub(super) fn open_link(&mut self, link: &str) -> bool {
        let Some((kind, id)) = parse(link) else { return false };
        if !matches!(self.phase, Phase::Ready) {
            self.pending_link = Some(link.to_string());
            return true;
        }
        let uri = format!("spotify:{kind}:{id}");
        let kind = match kind {
            "album" => Kind::Album,
            "artist" => Kind::Artist,
            "playlist" => Kind::Playlist,
            "show" => Kind::Show,
            // Faixa e episódio: o core acha o álbum ou o podcast (`AppEvent::Link`).
            _ => {
                self.send(Cmd::OpenLink(uri));
                return true;
            }
        };
        self.set_immersive(false);
        self.go(Screen::Page(Card { kind, id: id.to_string(), uri, title: String::new(), subtitle: String::new(), image: None }));
        true
    }

    /// Resposta do core para um link de faixa: abre a página dela e toca quando a página chegar.
    pub(super) fn open_link_page(&mut self, card: Card, uri: String) {
        self.link_play = Some((card.id.clone(), uri));
        self.set_immersive(false);
        self.go(Screen::Page(card));
        self.play_link();
    }

    /// Toca a faixa do link se a página dela já chegou.
    pub(super) fn play_link(&mut self) {
        let Some((id, uri)) = self.link_play.clone() else { return };
        let Some(page) = self.pages.get(&id) else { return };
        let found = page.tracks.iter().position(|t| t.uri == uri);
        self.link_play = None;
        match found {
            Some(i) => self.play_from(&TrackSource::Page(id), i),
            None => self.notify("A faixa do link não está nesta página."),
        }
    }

    /// Ctrl+V fora de campo de texto.
    pub(super) fn paste_link(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else { return };
        if !text.trim().is_empty() && !self.open_link(&text) {
            self.notify("Para abrir aqui, cole um link do Spotify (open.spotify.com).");
        }
    }
}

#[cfg(windows)]
pub use win::{attach, send};

#[cfg(windows)]
mod win {
    use std::{cell::RefCell, ffi::c_void};

    use tokio::sync::mpsc::UnboundedSender;
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, POINTL, WPARAM},
            System::{
                Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL},
                DataExchange::{COPYDATASTRUCT, RegisterClipboardFormatW},
                Memory::{GlobalLock, GlobalSize, GlobalUnlock},
                Ole::{CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop},
                SystemServices::MODIFIERKEYS_FLAGS,
            },
            UI::{Shell::{DefSubclassProc, SetWindowSubclass}, WindowsAndMessaging::{SendMessageW, WM_COPYDATA}},
        },
        core::{Ref, implement, w},
    };

    /// Marca do WM_COPYDATA dos links ("CYDL"; a Jump List usa "CYDR"), para não confundir com mensagens de outros programas.
    const MARK: usize = 0x4359_444C;

    thread_local! {
        // A janela e o subclass rodam na thread da UI.
        static TX: RefCell<Option<UnboundedSender<String>>> = const { RefCell::new(None) };
    }

    /// Manda o link para o Cyder já aberto (de outra abertura, com o link na linha de comando).
    pub fn send(hwnd: isize, link: &str) {
        let text: Vec<u16> = link.encode_utf16().collect();
        let data = COPYDATASTRUCT { dwData: MARK, cbData: (text.len() * 2) as u32, lpData: text.as_ptr() as *mut c_void };
        // SAFETY: SendMessage espera a janela tratar; `text` e `data` vivem até lá.
        unsafe { SendMessageW(HWND(hwnd as *mut c_void), WM_COPYDATA, None, Some(LPARAM(&data as *const _ as isize))) };
    }

    /// Recebe links de outras aberturas (WM_COPYDATA) e soltos na janela; vão para `tx`.
    /// O alvo de soltar do GPUI só aceita arquivos, que o Cyder não usa: o nosso fica no lugar dele.
    pub fn attach(hwnd: *mut c_void, tx: UnboundedSender<String>) -> Result<(), String> {
        let hwnd = HWND(hwnd);
        TX.with_borrow_mut(|t| *t = Some(tx.clone()));
        let target: IDropTarget = LinkDrop { tx, link: RefCell::new(None) }.into();
        // SAFETY: HWND vivo da janela principal, na thread da UI (onde o GPUI já iniciou o OLE).
        unsafe {
            if !SetWindowSubclass(hwnd, Some(subclass), 2, 0).as_bool() {
                return Err("SetWindowSubclass falhou".into());
            }
            let _ = RevokeDragDrop(hwnd);
            RegisterDragDrop(hwnd, &target).map_err(|e| e.to_string())
        }
    }

    unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
        if msg == WM_COPYDATA {
            // SAFETY: no WM_COPYDATA, o lparam aponta para um COPYDATASTRUCT válido enquanto a mensagem é tratada.
            let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
            if data.dwData == MARK && !data.lpData.is_null() {
                // SAFETY: `cbData` bytes em `lpData`, texto UTF-16 mandado por `send`.
                let text = unsafe { std::slice::from_raw_parts(data.lpData as *const u16, data.cbData as usize / 2) };
                let link = String::from_utf16_lossy(text);
                TX.with(|t| t.try_borrow().ok().and_then(|t| t.as_ref().map(|tx| tx.send(link))));
                return LRESULT(1);
            }
        }
        // SAFETY: repassa a mensagem para a janela do GPUI, como pede o subclass.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    #[implement(IDropTarget)]
    struct LinkDrop {
        tx: UnboundedSender<String>,
        /// O link sendo arrastado por cima da janela; None se não é um link do Spotify.
        link: RefCell<Option<String>>,
    }

    /// Texto ou endereço arrastado (o navegador manda os dois para um link).
    fn dragged_text(data: &IDataObject) -> Option<String> {
        // SAFETY: nome terminado em zero.
        let url = unsafe { RegisterClipboardFormatW(w!("UniformResourceLocatorW")) } as u16;
        [CF_UNICODETEXT.0, url].into_iter().find_map(|format| {
            let config = FORMATETC { cfFormat: format, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
            // SAFETY: GetData devolve um HGLOBAL nosso, travado só durante a leitura e solto no fim.
            unsafe {
                let mut medium = data.GetData(&config).ok()?;
                let global = medium.u.hGlobal;
                let ptr = GlobalLock(global) as *const u16;
                let text = (!ptr.is_null()).then(|| {
                    let text = std::slice::from_raw_parts(ptr, GlobalSize(global) / 2);
                    String::from_utf16_lossy(&text[..text.iter().position(|&c| c == 0).unwrap_or(text.len())])
                });
                let _ = GlobalUnlock(global);
                ReleaseStgMedium(&mut medium);
                text
            }
        })
    }

    #[allow(non_snake_case)]
    impl IDropTarget_Impl for LinkDrop_Impl {
        fn DragEnter(&self, data: Ref<IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
            let link = data.ok().ok().and_then(dragged_text).filter(|t| super::parse(t).is_some());
            *self.link.borrow_mut() = link;
            self.DragOver(MODIFIERKEYS_FLAGS(0), _pt, effect)
        }

        fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
            // SAFETY: o Windows passa um ponteiro válido para o efeito.
            unsafe { *effect = if self.link.borrow().is_some() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
            Ok(())
        }

        fn DragLeave(&self) -> windows::core::Result<()> {
            *self.link.borrow_mut() = None;
            Ok(())
        }

        fn Drop(&self, _data: Ref<IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
            let link = self.link.borrow_mut().take();
            // SAFETY: como no DragOver.
            unsafe { *effect = if link.is_some() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
            if let Some(link) = link {
                let _ = self.tx.send(link);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_spotify_links() {
        let id = "4uLU6hMCjMI75M1A2tKUQC";
        assert_eq!(parse(&format!("spotify:track:{id}")), Some(("track", id)));
        assert_eq!(parse(&format!("spotify://album/{id}")), Some(("album", id)));
        assert_eq!(parse(&format!(" https://open.spotify.com/intl-pt/playlist/{id}?si=abc ")), Some(("playlist", id)));
        assert_eq!(parse(&format!("https://open.spotify.com/episode/{id}#x")), Some(("episode", id)));
        assert_eq!(parse(&format!("https://open.spotify.com/user/{id}")), None);
        assert_eq!(parse("spotify:track:curto"), None);
        assert_eq!(parse("olá"), None);
    }
}
