//! Cor de destaque do Windows (a das Configurações > Personalização > Cores). O tema claro/escuro do sistema
//! já vem do GPUI (`window.appearance()`).
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Cor de destaque do Windows (0xRRGGBB), a mesma das Configurações > Personalização > Cores.
pub fn accent() -> Option<u32> {
    let (key, value) = (wide(r"Software\Microsoft\Windows\DWM"), wide("AccentColor"));
    let mut abgr = 0u32;
    let mut len = 4u32;
    // SAFETY: textos terminados em zero; um DWORD de saída.
    let status = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&raw mut abgr).cast(), &mut len)
    };
    (status == 0).then(|| ((abgr & 0xff) << 16) | (abgr & 0xff00) | ((abgr >> 16) & 0xff))
}
