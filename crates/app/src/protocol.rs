//! Protocolo `spotify:` (links `spotify:track:…`): HKCU\Software\Classes\spotify, só desta conta, sem administrador.
//! O app oficial do Spotify grava a mesma chave. Com ele instalado, o Cyder só toma os links se a pessoa ligar a opção,
//! e guarda o comando dele para devolver quando ela desligar.
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};

const KEY: &str = r"Software\Classes\spotify";
const COMMAND: &str = r"Software\Classes\spotify\shell\open\command";
/// Comando do outro app, guardado quando o Cyder tomou o protocolo.
const PREVIOUS: &str = "Cyder.previous";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn read(key: &str, value: &str) -> Option<String> {
    let (key, value) = (wide(key), wide(value));
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: textos terminados em zero; o buffer tem o tamanho em bytes passado.
    let status = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len)
    };
    (status == 0).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
}

fn write(key: &str, value: &str, data: &str) -> Result<(), String> {
    let (key, value, data) = (wide(key), wide(value), wide(data));
    // SAFETY: textos terminados em zero; o tamanho é em bytes e inclui o zero final. Cria a chave se falta.
    let status = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) };
    if status == 0 { Ok(()) } else { Err(format!("o Windows recusou (código {status})")) }
}

fn is_ours(command: &str) -> bool {
    command.to_lowercase().contains("cyder.exe")
}

/// Outro app (o Spotify oficial) abre os links, ou abria antes do Cyder tomar.
pub fn other_app() -> bool {
    read(KEY, PREVIOUS).is_some() || read(COMMAND, "").is_some_and(|c| !is_ours(&c))
}

/// Grava o Cyder como quem abre os links (com o caminho do .exe de agora). Se outro app tem o protocolo,
/// só toma com `take`, guardando o comando dele.
pub fn sync(take: bool) -> Result<(), String> {
    match read(COMMAND, "") {
        Some(c) if !is_ours(&c) && !take => return Ok(()),
        Some(c) if !is_ours(&c) => write(KEY, PREVIOUS, &c)?,
        _ => {}
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    write(KEY, "", "URL:Spotify link")?;
    write(KEY, "URL Protocol", "")?;
    write(COMMAND, "", &format!("\"{}\" \"%1\"", exe.display()))
}

/// Devolve os links ao app que tinha antes. Sem outro app, o Cyder continua com eles.
pub fn release() -> Result<(), String> {
    let Some(previous) = read(KEY, PREVIOUS) else { return Ok(()) };
    if read(COMMAND, "").is_some_and(|c| is_ours(&c)) {
        write(COMMAND, "", &previous)?;
    }
    let (key, value) = (wide(KEY), wide(PREVIOUS));
    // SAFETY: textos terminados em zero.
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
    Ok(())
}
