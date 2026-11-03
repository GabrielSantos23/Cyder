//! Jump List (o menu do botão direito no ícone da barra de tarefas): tocar/pausar, próxima, anterior e as
//! playlists e álbuns tocados por último. Cada item abre o cyder.exe com argumentos; se o Cyder já está aberto,
//! a segunda abertura repassa os argumentos para ele (`forward`) e sai.
//!
//! Formato do repasse: WM_COPYDATA para a janela principal, `dwData` = `ARGS`, e os argumentos (sem o exe)
//! em UTF-8, um por linha. Quem mais precisar mandar algo ao Cyder aberto usa o mesmo formato.
use std::{cell::RefCell, ffi::c_void};

use gpui::Context;
use tokio::sync::mpsc::UnboundedSender;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Storage::EnhancedStorage::PKEY_Title,
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, StructuredStorage::PROPVARIANT},
            DataExchange::COPYDATASTRUCT,
            Variant::VT_LPWSTR,
        },
        UI::{
            Shell::{
                Common::{IObjectArray, IObjectCollection},
                DefSubclassProc, DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW,
                PropertiesSystem::IPropertyStore, SHStrDupW, SetWindowSubclass, ShellLink,
            },
            WindowsAndMessaging::{SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COPYDATA},
        },
    },
    core::{HSTRING, Interface, w},
};

use super::Cyder;
use cyder_core::{Card, Kind};

/// "CYDR": marca as mensagens WM_COPYDATA com argumentos.
const ARGS: usize = 0x4359_4452;

const TASKS: [(&str, &str); 3] = [("--toggle", "Tocar/Pausar"), ("--next", "Próxima"), ("--prev", "Anterior")];

thread_local! {
    // A janela e o subclass rodam na thread da UI.
    static TX: RefCell<Option<UnboundedSender<Vec<String>>>> = const { RefCell::new(None) };
    /// Recentes já na Jump List (URI e nome), para só refazer quando mudam.
    static SHOWN: RefCell<Option<Vec<(String, String)>>> = const { RefCell::new(None) };
}

/// Manda `args` ao Cyder já aberto na janela `hwnd`. `false` se ele não respondeu.
pub fn forward(hwnd: isize, args: &[String]) -> bool {
    let text = args.join("\n");
    let data = COPYDATASTRUCT { dwData: ARGS, cbData: text.len() as u32, lpData: text.as_ptr() as *mut c_void };
    let mut result = 0usize;
    // SAFETY: `data` e `text` vivem até o fim da chamada, que espera a outra janela tratar a mensagem.
    let sent = unsafe {
        SendMessageTimeoutW(HWND(hwnd as *mut c_void), WM_COPYDATA, WPARAM(0), LPARAM(&data as *const _ as isize), SMTO_ABORTIFHUNG, 3000, Some(&mut result as *mut usize))
    };
    sent.0 != 0 && result != 0
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if msg == WM_COPYDATA {
        // SAFETY: no WM_COPYDATA o lparam aponta para um COPYDATASTRUCT válido durante a mensagem.
        let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
        if data.dwData == ARGS {
            let bytes = match data.lpData.is_null() {
                true => &[][..],
                // SAFETY: `cbData` bytes em `lpData`, copiados antes de responder.
                false => unsafe { std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize) },
            };
            let args = String::from_utf8_lossy(bytes).lines().map(String::from).collect();
            // `try_borrow`: nada de panic dentro do callback do Windows.
            TX.with(|tx| tx.try_borrow().ok().and_then(|tx| tx.as_ref().map(|tx| tx.send(args))));
            return LRESULT(1);
        }
    }
    // SAFETY: repassa a mensagem para a janela do GPUI, como pede o subclass.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

impl Cyder {
    /// Recebe os argumentos da Jump List: os desta abertura e os repassados por aberturas seguintes.
    /// Precisa do `self.hwnd` (do `attach_tray`).
    pub fn attach_jumplist(&mut self, cx: &mut Context<Self>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _ = tx.send(std::env::args().skip(1).collect());
        TX.with_borrow_mut(|t| *t = Some(tx));
        // SAFETY: HWND da janela principal, viva até o app sair; id 2 (o 1 é o dos botões da miniatura).
        if !unsafe { SetWindowSubclass(HWND(self.hwnd as *mut c_void), Some(subclass), 2, 0) }.as_bool() {
            log::warn!("Jump List: SetWindowSubclass falhou");
        }
        cx.spawn(async move |this, cx| {
            while let Some(args) = rx.recv().await {
                if this.update(cx, |s, cx| { s.run_args(&args); s.sync_media(); cx.notify(); }).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn run_args(&mut self, args: &[String]) {
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--toggle" => self.toggle_play(),
                "--next" => self.next_track(),
                "--prev" => self.prev_track(),
                "--play" => {
                    let Some(uri) = args.next() else { continue };
                    let card = self.recent_contexts(usize::MAX).into_iter().map(|(c, _)| c).find(|c| &c.uri == uri);
                    let card = card.unwrap_or_else(|| Card {
                        kind: if uri.starts_with("spotify:album:") { Kind::Album } else { Kind::Playlist },
                        id: uri.rsplit(':').next().unwrap_or_default().into(),
                        uri: uri.clone(),
                        title: String::new(),
                        subtitle: String::new(),
                        image: None,
                    });
                    self.play_context(&card);
                }
                _ => {}
            }
        }
    }

    /// Refaz a Jump List quando as playlists e álbuns recentes mudam.
    pub(super) fn sync_jumplist(&self) {
        let recent: Vec<(String, String)> = self.recent_contexts(usize::MAX).into_iter()
            .filter(|(c, _)| matches!(c.kind, Kind::Playlist | Kind::Album))
            .map(|(c, _)| (c.uri, c.title))
            .take(8)
            .collect();
        if SHOWN.with_borrow(|s| s.as_ref() == Some(&recent)) {
            return;
        }
        match commit(&recent) {
            Ok(()) => SHOWN.with_borrow_mut(|s| *s = Some(recent)),
            Err(e) => log::warn!("Jump List: {e}"),
        }
    }
}

fn commit(recent: &[(String, String)]) -> Result<(), String> {
    let exe = HSTRING::from(std::env::current_exe().map_err(|e| e.to_string())?.as_os_str());
    // SAFETY: COM na thread da UI (o GPUI já o iniciou); tudo criado e solto aqui.
    unsafe {
        let list: ICustomDestinationList = CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let mut slots = 0;
        let removed: IObjectArray = list.BeginList(&mut slots).map_err(|e| e.to_string())?;
        // Item que a pessoa tirou da lista não pode voltar: o Windows recusa a categoria inteira.
        let mut removed_args = Vec::new();
        for i in 0..removed.GetCount().unwrap_or(0) {
            let mut buf = [0u16; 1024];
            if let Ok(link) = removed.GetAt::<IShellLinkW>(i) && link.GetArguments(&mut buf).is_ok() {
                removed_args.push(String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]));
            }
        }
        let tasks: IObjectCollection = CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        for (args, title) in TASKS {
            tasks.AddObject(&link(&exe, args, title).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }
        list.AddUserTasks(&tasks.cast::<IObjectArray>().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let items: IObjectCollection = CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let room = (slots as usize).saturating_sub(TASKS.len()).max(1);
        let mut any = false;
        for (uri, title) in recent.iter().filter(|(uri, _)| !removed_args.contains(&format!("--play {uri}"))).take(room) {
            items.AddObject(&link(&exe, &format!("--play {uri}"), title).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            any = true;
        }
        if any {
            list.AppendCategory(w!("Recentes"), &items.cast::<IObjectArray>().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }
        list.CommitList().map_err(|e| e.to_string())
    }
}

/// Atalho para o próprio exe com `args`, com o ícone do Cyder e `title` como nome.
unsafe fn link(exe: &HSTRING, args: &str, title: &str) -> windows::core::Result<IShellLinkW> {
    // SAFETY: objetos COM criados aqui; o PROPVARIANT solta a cópia do texto no Drop (PropVariantClear).
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(exe)?;
        link.SetArguments(&HSTRING::from(args))?;
        link.SetIconLocation(exe, 0)?;
        // VT_LPWSTR, como o InitPropVariantFromString do SDK (que é inline, sem export).
        let mut value = PROPVARIANT::default();
        let inner = &mut *value.Anonymous.Anonymous;
        inner.Anonymous.pwszVal = SHStrDupW(&HSTRING::from(if title.is_empty() { "Sem nome" } else { title }))?;
        inner.vt = VT_LPWSTR;
        let store: IPropertyStore = link.cast()?;
        store.SetValue(&PKEY_Title, &value)?;
        store.Commit()?;
        Ok(link)
    }
}
