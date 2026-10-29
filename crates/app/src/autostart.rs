//! "Abrir com o Windows": um valor em HKCU\…\Run, que o Windows lê ao entrar na conta. Só desta conta,
//! sem pedir permissão de administrador. Abre com `--minimized`: direto na bandeja, sem janela na frente.
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};

/// Argumento que a entrada do Windows passa; o main esconde a janela na bandeja ao ver ele.
pub const MINIMIZED: &str = "--minimized";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "Cyder";

/// Liga ou desliga. Ligar grava o caminho do executável de agora (se o .exe mudar de pasta, ligar de novo corrige).
pub fn set(on: bool) -> Result<(), String> {
    let (key, value) = (wide(RUN_KEY), wide(VALUE));
    let status = if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = wide(&format!("\"{}\" {MINIMIZED}", exe.display()));
        // SAFETY: textos terminados em zero; o tamanho é em bytes e inclui o zero final.
        unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), REG_SZ, command.as_ptr().cast(), (command.len() * 2) as u32) }
    } else {
        // SAFETY: textos terminados em zero.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
        // Já não existia: é o que queríamos.
        if status == windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND { 0 } else { status }
    };
    if status == 0 { Ok(()) } else { Err(format!("o Windows recusou (código {status})")) }
}
