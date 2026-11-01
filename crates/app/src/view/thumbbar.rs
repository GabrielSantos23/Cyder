//! Botões na miniatura da barra de tarefas (a prévia que aparece com o mouse em cima do ícone), como no
//! Spotify: curtir, anterior, tocar/pausar e próxima. Os cliques chegam pela janela como WM_COMMAND.
use std::{cell::RefCell, ffi::c_void};

use tokio::sync::mpsc::UnboundedSender;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject},
        System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx},
        UI::{
            Shell::{DefSubclassProc, ITaskbarList3, SetWindowSubclass, TaskbarList, THB_FLAGS, THB_ICON, THB_TOOLTIP, THBF_DISABLED, THBF_ENABLED, THBN_CLICKED, THUMBBUTTON, THUMBBUTTONMASK},
            WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, GetSystemMetrics, HICON, ICONINFO, RegisterWindowMessageW, SM_CXSMICON, WM_COMMAND},
        },
    },
    core::w,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Like,
    Prev,
    Toggle,
    Next,
}

const BUTTONS: [Button; 4] = [Button::Like, Button::Prev, Button::Toggle, Button::Next];

/// O que os botões mostram.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Shown {
    pub track: bool,
    pub playing: bool,
    pub liked: bool,
}

struct Bar {
    hwnd: HWND,
    list: ITaskbarList3,
    /// Mensagem que o Explorer manda quando o botão da barra existe (inclusive depois de reiniciar o Explorer).
    created: u32,
    added: bool,
    shown: Shown,
    icons: Icons,
    tx: UnboundedSender<Button>,
}

struct Icons {
    heart: HICON,
    heart_filled: HICON,
    prev: HICON,
    play: HICON,
    pause: HICON,
    next: HICON,
}

impl Drop for Icons {
    fn drop(&mut self) {
        for icon in [self.heart, self.heart_filled, self.prev, self.play, self.pause, self.next] {
            // SAFETY: ícones criados por `rasterize`, de ninguém mais.
            unsafe { drop(DestroyIcon(icon)) };
        }
    }
}

thread_local! {
    // A janela e o subclass rodam na thread da UI; nada aqui cruza threads.
    static BAR: RefCell<Option<Bar>> = const { RefCell::new(None) };
}

/// Liga os botões na janela `hwnd`. Os cliques vão para `tx`.
pub fn attach(hwnd: *mut c_void, tx: UnboundedSender<Button>) -> Result<(), String> {
    let hwnd = HWND(hwnd);
    // SAFETY: chamadas COM/Win32 na thread da UI com o HWND vivo da janela principal.
    unsafe {
        // O GPUI já iniciou o COM nesta thread; repetir só devolve S_FALSE.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let list: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        list.HrInit().map_err(|e| e.to_string())?;
        let created = RegisterWindowMessageW(w!("TaskbarButtonCreated"));
        let icons = Icons {
            heart: rasterize(include_str!("../../assets/icons/heart.svg"), "white")?,
            heart_filled: rasterize(include_str!("../../assets/icons/heart-filled.svg"), "#1ed760")?,
            prev: rasterize(include_str!("../../assets/icons/rewind.svg"), "white")?,
            play: rasterize(include_str!("../../assets/icons/play.svg"), "white")?,
            pause: rasterize(include_str!("../../assets/icons/pause.svg"), "white")?,
            next: rasterize(include_str!("../../assets/icons/fast-forward.svg"), "white")?,
        };
        BAR.with_borrow_mut(|b| *b = Some(Bar { hwnd, list, created, added: false, shown: Shown::default(), icons, tx }));
        if !SetWindowSubclass(hwnd, Some(subclass), 1, 0).as_bool() {
            return Err("SetWindowSubclass falhou".into());
        }
    }
    // A janela já pode estar na barra; se não estiver, o TaskbarButtonCreated chega depois.
    add();
    Ok(())
}

/// Atualiza ícones e dicas; só fala com o Windows quando algo mudou.
pub fn update(shown: Shown) {
    let call = BAR.with_borrow_mut(|b| {
        let bar = b.as_mut().filter(|bar| bar.shown != shown)?;
        bar.shown = shown;
        bar.added.then(|| (bar.list.clone(), bar.hwnd, buttons(bar)))
    });
    if let Some((list, hwnd, buttons)) = call {
        // SAFETY: a lista e o HWND são os do `attach`.
        if let Err(e) = unsafe { list.ThumbBarUpdateButtons(hwnd, &buttons) } {
            log::warn!("botões da miniatura: {e}");
        }
    }
}

// As chamadas ao Windows ficam fora do `BAR`: elas mandam mensagens para a janela na hora, o subclass roda
// dentro delas e pegaria o `BAR` de novo (um panic ali, dentro do callback do Windows, fecha o app).
fn add() {
    let Some((list, hwnd, buttons)) = BAR.with_borrow(|b| b.as_ref().map(|bar| (bar.list.clone(), bar.hwnd, buttons(bar)))) else { return };
    // SAFETY: a lista e o HWND são os do `attach`.
    match unsafe { list.ThumbBarAddButtons(hwnd, &buttons) } {
        Ok(()) => BAR.with_borrow_mut(|b| b.as_mut().map(|bar| bar.added = true)).unwrap_or_default(),
        Err(e) => log::debug!("botões da miniatura ainda não: {e}"),
    }
}

fn buttons(bar: &Bar) -> [THUMBBUTTON; 4] {
    let s = bar.shown;
    BUTTONS.map(|button| {
        let (icon, tip) = match button {
            Button::Like if s.liked => (bar.icons.heart_filled, "Remover das Músicas curtidas"),
            Button::Like => (bar.icons.heart, "Adicionar às Músicas curtidas"),
            Button::Prev => (bar.icons.prev, "Anterior"),
            Button::Toggle if s.playing => (bar.icons.pause, "Pausar"),
            Button::Toggle => (bar.icons.play, "Tocar"),
            Button::Next => (bar.icons.next, "Próxima"),
        };
        let mut sz_tip = [0u16; 260];
        for (dst, src) in sz_tip.iter_mut().zip(tip.encode_utf16()) {
            *dst = src;
        }
        THUMBBUTTON {
            dwMask: THUMBBUTTONMASK(THB_ICON.0 | THB_TOOLTIP.0 | THB_FLAGS.0),
            iId: button as u32,
            hIcon: icon,
            szTip: sz_tip,
            dwFlags: if s.track { THBF_ENABLED } else { THBF_DISABLED },
            ..Default::default()
        }
    })
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    let hi = (wparam.0 >> 16) as u32 & 0xffff;
    // `try_borrow`: nada de panic aqui dentro; ocupado, a mensagem segue para a janela.
    let created = BAR.with(|b| match b.try_borrow().as_deref() {
        Ok(Some(bar)) if msg == bar.created => true,
        Ok(Some(bar)) if msg == WM_COMMAND && hi == THBN_CLICKED => {
            if let Some(button) = BUTTONS.get(wparam.0 & 0xffff) {
                let _ = bar.tx.send(*button);
            }
            false
        }
        _ => false,
    });
    if created {
        add();
    }
    // SAFETY: repassa a mensagem para a janela do GPUI, como pede o subclass.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Ícone do tamanho pequeno do sistema (16 px a 100%, maior com escala), com `currentColor` em `color`.
fn rasterize(svg: &str, color: &str) -> Result<HICON, String> {
    // SAFETY: só lê uma métrica do sistema.
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    let tree = resvg::usvg::Tree::from_str(&svg.replace("currentColor", color), &resvg::usvg::Options::default()).map_err(|e| e.to_string())?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or("tamanho de ícone inválido")?;
    let scale = size as f32 / tree.size().width();
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    // O tiny-skia guarda RGBA pré-multiplicado; o ícone quer BGRA pré-multiplicado também (DIB de 32 bits).
    let bgra: Vec<u8> = pixmap.data().chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    // SAFETY: DIB e máscara criados e soltos aqui; o HICON copia os dois.
    unsafe {
        let header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            // Negativa: linhas de cima para baixo, como o pixmap.
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color_bmp = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).map_err(|e| e.to_string())?;
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO { fIcon: true.into(), hbmMask: mask, hbmColor: color_bmp, ..Default::default() });
        let _ = DeleteObject(color_bmp.into());
        let _ = DeleteObject(mask.into());
        icon.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn icons_render() {
        for svg in [include_str!("../../assets/icons/heart.svg"), include_str!("../../assets/icons/rewind.svg")] {
            assert!(super::rasterize(svg, "white").is_ok());
        }
    }
}
