//! Spotify Soloist: o player oficial do Spotify, sem tela, rodando como aparelho Connect neste computador.
//! Serve às contas em que o Spotify recusa a chave de áudio ao librespot (librespot #1649): o Cyder manda o play
//! para ele pela Web API e o segue como segue qualquer outro aparelho.
//! Cada pessoa usa a própria chave (painel do Spotify, com Premium). O executável não pode ser redistribuído:
//! vem sempre do CDN do Spotify, e cada versão vence 90 dias depois de feita.
use std::{path::PathBuf, process::Stdio, time::Duration};

use futures_util::StreamExt;
use tokio::{io::AsyncBufReadExt, process::Command, sync::mpsc::UnboundedSender};
use tokio_tungstenite::tungstenite::Message;

use crate::config;

/// Como ele aparece no Spotify Connect (e como o Cyder o acha na lista de aparelhos).
pub const DEVICE_NAME: &str = "Cyder (Soloist)";
/// Onde cada pessoa gera a própria chave.
pub const KEY_PAGE: &str = "https://developer.spotify.com/dashboard/soloist";
const DOWNLOAD: &str = "https://soloist-builds.spotifycdn.com/soloist_release_windows_x86_64.zip";
/// Baixa a versão nova bem antes de a atual vencer.
const REFRESH_AFTER: Duration = Duration::from_secs(30 * 24 * 3600);
/// Código de saída de uma versão vencida.
const EXPIRED: i32 = 10;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Sem chave: o Cyder toca pelo próprio player.
    Off,
    /// Baixando ou abrindo; o texto diz qual.
    Starting(String),
    /// Rodando, esperando ser escolhido uma vez no app do Spotify.
    Pairing,
    Ready,
    Failed(String),
}

fn dir() -> PathBuf {
    config::cache_dir().join("soloist")
}

fn exe() -> PathBuf {
    dir().join("soloist.exe")
}

fn key_path() -> PathBuf {
    config::config_dir().join("soloist-key")
}

pub fn key() -> Option<String> {
    std::fs::read_to_string(key_path()).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

/// Grava a chave (só neste computador, na pasta de configuração) ou a apaga com None.
pub fn save_key(key: Option<&str>) -> Result<(), String> {
    match key {
        Some(k) => std::fs::create_dir_all(config::config_dir()).and_then(|_| std::fs::write(key_path(), k.trim())).map_err(|e| e.to_string()),
        None => match std::fs::remove_file(key_path()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        },
    }
}

/// O formato da chave não é documentado; só pega cópia com espaço ou quebra de linha no meio.
pub fn check_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        Err("Cole a chave do Soloist.".into())
    } else if key.chars().any(char::is_whitespace) {
        Err("A chave não tem espaços. Confira se copiou só ela.".into())
    } else {
        Ok(())
    }
}

/// O Soloist aberto; soltar fecha ele.
pub struct Running(tokio::task::JoinHandle<()>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Abre o Soloist com esta chave e o mantém aberto (baixando a versão nova quando precisa) até soltar o `Running`.
pub fn start(http: reqwest::Client, key: String, tx: UnboundedSender<Status>) -> Running {
    Running(tokio::spawn(run(http, key, tx)))
}

async fn run(http: reqwest::Client, key: String, tx: UnboundedSender<Status>) {
    if !cfg!(windows) {
        let _ = tx.send(Status::Failed("Por enquanto o Cyder só usa o Soloist no Windows.".into()));
        return;
    }
    loop {
        let fresh = match download(&http, &tx).await {
            Ok(fresh) => fresh,
            Err(e) => {
                log::error!("soloist: download: {e}");
                let _ = tx.send(Status::Failed(format!("Não deu para baixar o Soloist: {e}")));
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };
        let _ = tx.send(Status::Starting("Abrindo o Soloist…".into()));
        let wait = match session(&key, &tx).await {
            Ok(EXPIRED) if !fresh => {
                // Versão vencida: a próxima volta baixa a atual.
                log::info!("soloist: versão vencida; baixando a nova");
                let _ = std::fs::remove_file(exe());
                continue;
            }
            Ok(EXPIRED) => {
                let _ = std::fs::remove_file(exe());
                let _ = tx.send(Status::Failed("A versão do Soloist no site do Spotify está vencida. O Cyder tenta de novo em uma hora.".into()));
                3600
            }
            Ok(code) => {
                log::error!("soloist: fechou com código {code}");
                let _ = tx.send(Status::Failed(format!("O Soloist fechou (código {code}). Confira a chave; o Cyder tenta de novo em instantes.")));
                15
            }
            Err(e) => {
                log::error!("soloist: {e}");
                let _ = tx.send(Status::Failed(format!("O Soloist não abriu: {e}")));
                15
            }
        };
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
}

/// Baixa o Soloist se faltar ou estiver velho. `true`: acabou de baixar.
async fn download(http: &reqwest::Client, tx: &UnboundedSender<Status>) -> Result<bool, String> {
    let age = std::fs::metadata(exe()).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if age.is_some_and(|a| a < REFRESH_AFTER) {
        return Ok(false);
    }
    let _ = tx.send(Status::Starting("Baixando o Soloist do Spotify…".into()));
    let bytes = http.get(DOWNLOAD).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?.bytes().await.map_err(|e| e.to_string())?;
    let dir = dir();
    let zip = dir.join("soloist.zip");
    std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&zip, &bytes)).map_err(|e| e.to_string())?;
    // O Windows 10 e 11 trazem o tar (bsdtar), que abre .zip.
    let tar = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32").join("tar.exe");
    let mut cmd = Command::new(tar);
    cmd.arg("-xf").arg(&zip).arg("-C").arg(&dir).arg("soloist.exe");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output().await.map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&zip);
    if !out.status.success() {
        return Err(format!("não deu para abrir o arquivo baixado: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    // O zip traz a data da versão; a idade para baixar de novo conta a partir de agora.
    std::fs::File::options().write(true).open(exe()).and_then(|f| f.set_modified(std::time::SystemTime::now())).map_err(|e| e.to_string())?;
    log::info!("soloist: baixado ({} KB)", bytes.len() / 1024);
    Ok(true)
}

/// Roda o Soloist até ele fechar, contando o pareamento pela API local. Devolve o código de saída.
async fn session(key: &str, tx: &UnboundedSender<Status>) -> Result<i32, String> {
    let data = dir().join("data");
    let port_file = data.join("ws.port");
    let _ = std::fs::remove_file(&port_file);
    let mut cmd = Command::new(exe());
    cmd.arg("--device-name").arg(DEVICE_NAME).arg("--api-key").arg(key).arg("--data-dir").arg(&data).arg("--cache-dir").arg(dir().join("cache"))
        .arg("--ws").arg("127.0.0.1:0")
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    close_with_cyder(&child);
    if let Some(err) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log::info!("soloist: {line}");
            }
        });
    }
    let code = |s: std::process::ExitStatus| s.code().unwrap_or(-1);
    // A porta escolhida pelo sistema aparece num arquivo logo depois de abrir.
    let mut port = None;
    for _ in 0..80 {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(code(s));
        }
        port = std::fs::read_to_string(&port_file).ok().and_then(|p| p.trim().parse::<u16>().ok());
        if port.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let mut port = port.ok_or("a API local não respondeu")?;
    // O arquivo sai um pouco antes de a porta aceitar conexão, e a porta pode mudar logo depois: tenta de novo
    // relendo o arquivo. Sem isto, o Soloist fechava e reabria 15 s depois (ou insistia na porta velha).
    let mut tries = 0;
    let mut ws = loop {
        port = std::fs::read_to_string(&port_file).ok().and_then(|p| p.trim().parse::<u16>().ok()).unwrap_or(port);
        match tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}")).await {
            Ok((ws, _)) => break ws,
            Err(e) if tries < 20 => {
                tries += 1;
                log::debug!("soloist: API local ainda fechada ({e})");
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    loop {
        tokio::select! {
            s = child.wait() => return Ok(code(s.map_err(|e| e.to_string())?)),
            msg = ws.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    note_position(&text);
                    if let Some(logged_in) = auth_state(&text) {
                        log::info!("soloist: {}", if logged_in { "pareado" } else { "esperando ser escolhido no app do Spotify" });
                        let _ = tx.send(if logged_in { Status::Ready } else { Status::Pairing });
                    }
                }
                Some(Ok(_)) => {}
                // A API caiu e o processo segue: espera ele fechar.
                Some(Err(_)) | None => return Ok(code(child.wait().await.map_err(|e| e.to_string())?)),
            },
        }
    }
}

/// A última posição que o Soloist mediu: a faixa, a posição, quando (ms desde 1970) e a velocidade (0 pausado).
#[derive(Clone, Debug, Default, PartialEq)]
struct Synced {
    uri: String,
    position_ms: u32,
    at_ms: u64,
    speed: f64,
}

static SYNCED: std::sync::Mutex<Option<Synced>> = std::sync::Mutex::new(None);
/// Faixa do último `playback_state` (o `position_sync` não diz de qual faixa é).
static PLAYING_URI: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Avisa quando o Soloist mede uma posição nova (play, pausa, seek, troca de faixa).
pub fn synced() -> &'static tokio::sync::Notify {
    static N: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
    N.get_or_init(tokio::sync::Notify::new)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Guarda `playback_state` (qual faixa) e `position_sync` (`{"position_ms", "timestamp_ms", "speed"}`).
fn note_position(text: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return };
    match v["type"].as_str() {
        Some("playback_state") => {
            if let Some(uri) = v["item"]["uri"].as_str() {
                *PLAYING_URI.lock().unwrap() = uri.to_string();
            }
        }
        Some("position_sync") => {
            let p = &v["position"];
            let (Some(position_ms), Some(at_ms)) = (p["position_ms"].as_u64(), p["timestamp_ms"].as_u64()) else { return };
            let uri = PLAYING_URI.lock().unwrap().clone();
            *SYNCED.lock().unwrap() = Some(Synced { uri, position_ms: position_ms as u32, at_ms, speed: p["speed"].as_f64().unwrap_or(0.) });
            synced().notify_one();
        }
        _ => {}
    }
}

/// Onde o Soloist está agora em `uri` e se está tocando, contado da última posição que ele mediu. Bem mais exato
/// que o `progress_ms` da Web API, que chega atrasado pela ida e volta do pedido (a letra ficava para trás).
/// None quando a última medida é de outra faixa.
pub fn position(uri: &str) -> Option<(u32, bool)> {
    let s = SYNCED.lock().unwrap().clone().filter(|s| s.uri == uri)?;
    Some((extrapolate(&s, now_ms()), s.speed > 0.))
}

fn extrapolate(s: &Synced, now_ms: u64) -> u32 {
    (s.position_ms as f64 + now_ms.saturating_sub(s.at_ms) as f64 * s.speed).round() as u32
}

/// `logged_in` de um evento `auth_state`; None para os outros eventos.
fn auth_state(text: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    (v["type"] == "auth_state").then(|| v["logged_in"].as_bool().unwrap_or(false))
}

/// Põe o Soloist num job do Windows que morre com o Cyder: fechar ou derrubar o app não deixa música tocando.
#[cfg(windows)]
fn close_with_cyder(child: &tokio::process::Child) {
    use std::sync::OnceLock;

    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    // O job nunca é fechado aqui: o Windows fecha quando o Cyder sai, e aí fecha o Soloist.
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| {
        // SAFETY: cria e configura um job novo, sem nome; a estrutura vai zerada com só o limite ligado.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const std::ffi::c_void, size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32);
            job as usize
        }
    });
    if let Some(process) = child.raw_handle() {
        // SAFETY: o handle é do processo filho vivo; o job é o de cima.
        if unsafe { AssignProcessToJobObject(job as _, process as _) } == 0 {
            log::warn!("soloist: não deu para prender ao Cyder; ele pode ficar aberto se o Cyder cair");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn follows_the_position_the_soloist_measured() {
        super::note_position(r#"{"type":"playback_state","status":"playing","item":{"uri":"spotify:track:a"}}"#);
        super::note_position(r#"{"type":"position_sync","position":{"position_ms":1000,"timestamp_ms":5000,"speed":1}}"#);
        let s = super::SYNCED.lock().unwrap().clone().unwrap();
        assert_eq!(s.uri, "spotify:track:a");
        // Tocando, anda junto com o relógio; pausado (speed 0), fica parado.
        assert_eq!(super::extrapolate(&s, 5250), 1250);
        assert_eq!(super::extrapolate(&super::Synced { speed: 0., ..s.clone() }, 9000), 1000);
        assert!(super::position("spotify:track:outra").is_none());
        assert!(super::position("spotify:track:a").is_some_and(|(_, on)| on));
    }

    #[test]
    fn reads_auth_state() {
        assert_eq!(super::auth_state(r#"{"type":"auth_state","logged_in":true,"is_active":false,"device_name":"x"}"#), Some(true));
        assert_eq!(super::auth_state(r#"{"type":"auth_state","logged_in":false}"#), Some(false));
        assert_eq!(super::auth_state(r#"{"type":"playback_changed","status":"paused"}"#), None);
        assert!(super::check_key("abc123").is_ok());
        assert!(super::check_key("abc 123").is_err());
        assert!(super::check_key("  ").is_err());
    }
}
