//! Discord Rich Presence: "Ouvindo Cyder" no perfil, com a faixa, a capa e o tempo. Fala com o Discord
//! aberto no computador pelo IPC local (pipe no Windows, socket no Linux e no macOS), numa thread própria.
use std::{io::{Read, Write}, sync::mpsc, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

use serde_json::{Value, json};

use super::Cyder;

/// Sem o Discord aberto, tenta conectar de novo no máximo a cada isto.
const RETRY: Duration = Duration::from_secs(20);

/// O que o Discord mostra agora, para só mandar quando muda.
#[derive(Clone, Default, PartialEq)]
pub(super) struct Shown {
    app_id: String,
    uri: String,
    playing: bool,
    /// Onde a faixa começou, em segundos Unix (muda com seek).
    start: u64,
}

pub(super) struct Presence {
    tx: mpsc::Sender<(String, Option<Value>)>,
    shown: Shown,
}

#[cfg(windows)]
type Pipe = std::fs::File;
#[cfg(unix)]
type Pipe = std::os::unix::net::UnixStream;

#[cfg(windows)]
fn open() -> Option<Pipe> {
    (0..10).find_map(|i| std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}")).ok())
}

#[cfg(unix)]
fn open() -> Option<Pipe> {
    let dirs = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(|v| std::env::var(v).ok()).chain(["/tmp".to_string()]);
    let dirs: Vec<String> = dirs.collect();
    dirs.iter().flat_map(|d| (0..10).map(move |i| format!("{d}/discord-ipc-{i}"))).find_map(|p| Pipe::connect(p).ok())
}

/// Quadro do IPC: código (0 handshake, 1 comando), tamanho e o JSON.
fn send(pipe: &mut Pipe, op: u32, body: &Value) -> std::io::Result<Value> {
    let data = body.to_string();
    let mut frame = Vec::with_capacity(8 + data.len());
    frame.extend(op.to_le_bytes());
    frame.extend((data.len() as u32).to_le_bytes());
    frame.extend(data.as_bytes());
    pipe.write_all(&frame)?;
    let mut head = [0u8; 8];
    pipe.read_exact(&mut head)?;
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    let mut reply = vec![0u8; len.min(1 << 20)];
    pipe.read_exact(&mut reply)?;
    Ok(serde_json::from_slice(&reply).unwrap_or_default())
}

fn connect(app_id: &str) -> Option<Pipe> {
    let mut pipe = open()?;
    let ready = send(&mut pipe, 0, &json!({ "v": 1, "client_id": app_id })).ok()?;
    if ready["evt"] != "READY" {
        log::warn!("Discord recusou o app {app_id}: {ready}");
        return None;
    }
    Some(pipe)
}

/// Recebe (app, atividade) e mantém a conexão; None apaga a atividade.
fn run(rx: mpsc::Receiver<(String, Option<Value>)>) {
    let mut pipe: Option<(String, Pipe)> = None;
    let mut last_try: Option<Instant> = None;
    let pid = std::process::id();
    while let Ok(mut msg) = rx.recv() {
        // Só a mais recente importa.
        while let Ok(newer) = rx.try_recv() {
            msg = newer;
        }
        let (app_id, activity) = msg;
        if pipe.as_ref().is_some_and(|(id, _)| *id != app_id) {
            pipe = None;
        }
        if pipe.is_none() {
            if app_id.is_empty() || last_try.is_some_and(|t| t.elapsed() < RETRY) && activity.is_some() {
                continue;
            }
            last_try = Some(Instant::now());
            pipe = connect(&app_id).map(|p| (app_id.clone(), p));
        }
        let Some((_, p)) = pipe.as_mut() else { continue };
        let body = json!({ "cmd": "SET_ACTIVITY", "args": { "pid": pid, "activity": activity }, "nonce": format!("{:?}", Instant::now()) });
        if send(p, 1, &body).is_err() {
            // O Discord fechou: conecta de novo na próxima mudança.
            pipe = None;
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Cyder {
    /// Manda ao Discord a faixa tocando aqui; pausado, com outro aparelho tocando ou desligado, apaga.
    pub(super) fn sync_discord(&mut self) {
        let on = self.settings.discord && !self.settings.discord_app_id.trim().is_empty();
        if !on && self.presence.is_none() {
            return;
        }
        let start = now_secs().saturating_sub(self.position() as u64 / 1000);
        let here = self.elsewhere().is_none();
        let presence = self.presence.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new().name("discord".into()).spawn(move || run(rx)).ok();
            Presence { tx, shown: Shown::default() }
        });
        let track = self.tracks.get(self.current).filter(|_| on && here);
        let now = Shown {
            app_id: self.settings.discord_app_id.trim().to_string(),
            uri: track.map(|t| t.uri.clone()).unwrap_or_default(),
            playing: track.is_some() && self.playing,
            start: if self.playing { start } else { 0 },
        };
        // O relógio anda sozinho no Discord: um segundo de diferença não é seek.
        let same = now.app_id == presence.shown.app_id && now.uri == presence.shown.uri && now.playing == presence.shown.playing && now.start.abs_diff(presence.shown.start) <= 2;
        if same {
            return;
        }
        let activity = track.filter(|_| now.playing).map(|t| {
            let link = format!("https://open.spotify.com/{}/{}", if t.uri.starts_with("spotify:episode:") { "episode" } else { "track" }, t.id);
            let mut a = json!({
                // 2: "Ouvindo".
                "type": 2,
                "details": t.name.chars().take(128).collect::<String>(),
                "state": t.artist.chars().take(128).collect::<String>(),
                "timestamps": { "start": now.start * 1000, "end": (now.start + t.duration_ms as u64 / 1000) * 1000 },
                "buttons": [{ "label": "Ouvir no Spotify", "url": link }],
            });
            if let Some(cover) = &t.cover_large {
                a["assets"] = json!({ "large_image": cover, "large_text": t.album.chars().take(128).collect::<String>() });
            }
            a
        });
        let _ = presence.tx.send((now.app_id.clone(), activity));
        presence.shown = now;
    }
}
