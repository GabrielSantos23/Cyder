//! Histórico do Spotify (doc 09): reporta cada reprodução ao gabo-receiver, igual ao client oficial.
//! É o que põe a música em "Tocadas recentemente", no Wrapped e nas recomendações. O librespot não faz
//! isso (doc 09). Montado à mão: o librespot traz os .proto mas não os compila.
//!
//! O layout abaixo é a cópia exata dos lotes capturados do client de desktop 1.3.6.1921 (bytes reais,
//! `scratchpad/capture/*.bin` do mitmproxy — doc 09, §6.6): mesmos campos, mesmos vazios explícitos
//! (o client escreve strings vazias e zeros no fio — presença importa) e mesmos valores-sentinela
//! (u64 máx = "desconhecido"). Cada reprodução tem dois ids de 16 bytes: o de sessão (que o Connect
//! anuncia) e o do arquivo (`clip_playback_id`, que aparece no Download).

use librespot::core::Session;

/// Uma reprodução que terminou, para o histórico do Spotify.
pub struct EndSong {
    pub track_uri: String,
    pub context: Option<String>,
    /// Quando começou, em milissegundos Unix.
    pub started_ms: u64,
    pub ms_played: u32,
    pub duration_ms: u32,
    pub shuffle: bool,
}

/// Qualidade configurada como (bits/s, código do formato Ogg Vorbis, nome no formato do client).
fn audio_format() -> (u64, i32, &'static str) {
    match crate::config::settings().quality {
        crate::config::Quality::Normal => (96_000, 1, "Vorbis 96 kbps"),
        crate::config::Quality::High => (160_000, 2, "Vorbis 160 kbps"),
        crate::config::Quality::VeryHigh => (320_000, 3, "Vorbis 320 kbps"),
    }
}

const DESKTOP_VERSION: &str = "1.3.6.1921";
/// version_code que o desktop 1.3.6.1921 manda.
const DESKTOP_VERSION_CODE: u64 = 130_601_921;
/// core_version que o desktop 1.3.6.1921 manda (constante no lote capturado).
const DESKTOP_CORE_VERSION: u64 = 6_006_400_000_000_000;
/// O client id do desktop oficial (o mesmo em todos os lotes capturados).
const DESKTOP_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
/// version_name do SDK de eventos do desktop; `cpp` é o type.
const DESKTOP_SDK: &str = "0.9.4-rl-essopt-loginsend-onlinesend-bcdsend-heartbeat300.0s/30.0s-modern-payload125kB-batch100";
/// u64 máximo: o sentinela de "desconhecido" do client oficial.
const UNKNOWN: u64 = u64::MAX;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()).collect()
}

fn put_field(out: &mut Vec<u8>, field: u64, v: u64) {
    put_varint(out, field << 3);
    put_varint(out, v);
}

fn put_bytes(out: &mut Vec<u8>, field: u64, data: &[u8]) {
    put_varint(out, field << 3 | 2);
    put_varint(out, data.len() as u64);
    out.extend(data);
}

/// string vazia explícita: o desktop escreve o campo com zero bytes (presença importa).
fn put_empty(out: &mut Vec<u8>, field: u64) {
    put_bytes(out, field, b"");
}

/// double: wiretype 1, 8 bytes little-endian.
fn put_double(out: &mut Vec<u8>, field: u64, v: f64) {
    put_varint(out, field << 3 | 1);
    out.extend(v.to_le_bytes());
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn fragment(name: &str, data: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    put_bytes(&mut f, 1, name.as_bytes());
    put_bytes(&mut f, 2, data);
    f
}

fn one_bytes(v: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    put_bytes(&mut f, 1, v);
    f
}

/// Id em base62 (22 caracteres), o formato do player_session_id do desktop.
fn base62_22(bytes16: &[u8]) -> String {
    const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut n = u128::from_le_bytes(bytes16.try_into().unwrap_or([0u8; 16]));
    let mut out = Vec::with_capacity(22);
    while out.len() < 22 {
        out.push(ALPHABET[(n % 62) as usize]);
        n /= 62;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Id em formato UUID com traços (o formato do predict_id no AudioFileSelection do desktop).
fn uuid_str(bytes16: &[u8]) -> String {
    let b = bytes16.try_into().unwrap_or([0u8; 16]);
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15])
}

/// O envelope completo do desktop: o evento e os 9 fragmentos de contexto, na ordem capturada.
#[allow(clippy::too_many_arguments)]
fn envelope(event_name: &str, msg: &[u8], device_id: &str, now_ms: u64, mono_ms: u64, seq_id: &[u8], seq_num: u64, app_session: &[u8]) -> Vec<u8> {
    let mut e = Vec::new();
    put_bytes(&mut e, 2, event_name.as_bytes());
    put_bytes(&mut e, 3, &fragment("message", msg));
    put_bytes(&mut e, 3, &fragment("context_client_id", &one_bytes(&unhex(DESKTOP_CLIENT_ID))));
    put_bytes(&mut e, 3, &fragment("context_installation_id", &one_bytes(&<sha2::Sha256 as sha2::Digest>::digest(device_id.as_bytes())[..16])));
    let mut app = Vec::new();
    put_bytes(&mut app, 1, DESKTOP_VERSION.as_bytes());
    put_field(&mut app, 2, DESKTOP_VERSION_CODE);
    put_bytes(&mut app, 3, app_session);
    put_bytes(&mut e, 3, &fragment("context_application_desktop", &app));
    let mut device = Vec::new();
    put_bytes(&mut device, 1, std::env::consts::OS.as_bytes());
    put_empty(&mut device, 2);
    put_empty(&mut device, 3);
    put_bytes(&mut device, 4, device_id.as_bytes());
    put_empty(&mut device, 5);
    put_bytes(&mut e, 3, &fragment("context_device_desktop", &device));
    let mut mono = Vec::new();
    put_field(&mut mono, 1, 677);
    put_field(&mut mono, 2, mono_ms);
    put_bytes(&mut e, 3, &fragment("context_monotonic_clock", &mono));
    let mut sdk = Vec::new();
    put_bytes(&mut sdk, 1, DESKTOP_SDK.as_bytes());
    put_bytes(&mut sdk, 2, b"cpp");
    put_bytes(&mut e, 3, &fragment("context_sdk", &sdk));
    put_bytes(&mut e, 3, &fragment("context_client_context_id", &one_bytes(b"")));
    let mut time = Vec::new();
    put_field(&mut time, 1, now_ms);
    put_bytes(&mut e, 3, &fragment("context_time", &time));
    put_bytes(&mut e, 4, seq_id);
    put_field(&mut e, 5, seq_num);
    e
}

/// Conta a reprodução para o Spotify: a família completa de eventos num lote (doc 09, §6.6).
/// shortcut: sem fila em disco, um lote que falha (sem internet) se perde; guardar e reenviar se fizer falta.
pub async fn report_end_song(session: &Session, play: &EndSong) -> Result<(), String> {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::{OnceLock, atomic::{AtomicI64, Ordering}};
    // Ids aleatórios sem dependência nova: o RandomState já vem com sementes aleatórias.
    let random16 = || {
        let state = std::collections::hash_map::RandomState::new();
        let mut out = Vec::with_capacity(16);
        for i in 0..2u8 {
            let mut h = state.build_hasher();
            h.write_u8(i);
            out.extend(h.finish().to_le_bytes());
        }
        out
    };
    // Sequência com 20 bytes (formato do desktop) e contador global que sobe a cada evento.
    static SEQUENCE_ID: OnceLock<Vec<u8>> = OnceLock::new();
    static APP_SESSION: OnceLock<Vec<u8>> = OnceLock::new();
    static PLAYER_SESSION_ID: OnceLock<String> = OnceLock::new();
    static DECISION_ID: OnceLock<String> = OnceLock::new();
    static APP_STARTED: OnceLock<std::time::Instant> = OnceLock::new();
    static SEQUENCE_NUMBER: AtomicI64 = AtomicI64::new(1);
    let sequence_id = SEQUENCE_ID.get_or_init(|| {
        let mut v = random16();
        v.extend(random16().into_iter().take(4));
        v
    });
    let app_session = APP_SESSION.get_or_init(random16);
    let player_session_id = PLAYER_SESSION_ID.get_or_init(|| base62_22(&random16()));
    let decision_id = DECISION_ID.get_or_init(|| format!("ssp~{}", hex(&<sha2::Sha256 as sha2::Digest>::digest(random16()))));
    let app_started = APP_STARTED.get_or_init(std::time::Instant::now);
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let mono_ms = app_started.elapsed().as_millis() as u64;
    let next_seq = || (SEQUENCE_NUMBER.fetch_add(1, Ordering::Relaxed) + 1) as u64;

    let device_id = session.device_id().to_string();
    let (bps, format_code, format_name) = audio_format();

    // O id de sessão (o que o Connect anunciou) e o arquivo que tocou.
    let playback_id = {
        let mut ids = librespot::connect::PLAYBACK_IDS.lock().unwrap_or_else(|e| e.into_inner());
        ids.remove(&play.track_uri).map_or_else(random16, |id| unhex(&id))
    };
    let file_id = {
        let mut files = librespot::connect::FILE_IDS.lock().unwrap_or_else(|e| e.into_inner());
        files.remove(&play.track_uri).and_then(|list| {
            list.iter().find(|(code, _)| *code == format_code).or_else(|| list.first()).map(|(_, id)| unhex(id)).filter(|b| !b.is_empty())
        })
    };
    if file_id.is_none() {
        log::debug!("histórico: {} sem arquivo conhecido (Download fica de fora)", play.track_uri);
    }
    // O segundo id: o da transferência do arquivo (aparece no segmento, no RawCoreStream e no Download).
    let file_playback_id = random16();
    let stream_id = random16();

    let done = play.ms_played as u64 * 10 >= play.duration_ms as u64 * 9;
    // Todo play do desktop tem contexto (até Curtidas é uma playlist); sem ele o servidor descarta o
    // evento. Quando o play veio sem contexto (busca, fila, curtidas), o contexto é a própria faixa.
    let context = play.context.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| play.track_uri.clone());
    let reason = if done { "trackdone" } else { "fwdbtn" };
    let source = context.split(':').nth(1).filter(|s| !s.is_empty()).unwrap_or("track").to_string();
    let list_uri = (context.starts_with("spotify:playlist:") || context.starts_with("spotify:album:")).then_some(context.clone());
    // O id do aparelho no formato do desktop: 40 hex estáveis (o desktop manda um sha1 do aparelho).
    let connect_id = hex(&<sha1::Sha1 as sha1::Digest>::digest(device_id.as_bytes()));

    let mut envelopes = Vec::new();

    // `AudioFileSelection` (capturado): strategy, bitrate em bits/s, predict_id (uuid), file_origin.
    let mut selection = Vec::new();
    put_bytes(&mut selection, 1, &playback_id);
    put_bytes(&mut selection, 2, b"best matching bitrate");
    put_field(&mut selection, 3, bps);
    put_bytes(&mut selection, 4, uuid_str(&random16()).as_bytes());
    put_bytes(&mut selection, 5, b"audiophile");
    put_field(&mut selection, 6, bps);
    envelopes.push(envelope("AudioFileSelection", &selection, &device_id, now_ms, mono_ms, sequence_id, next_seq(), app_session));

    // `RawCoreStreamSegment` (capturado): o áudio tocado, em trechos de até 30 s.
    let chunk = 30_000u64;
    let total = play.ms_played as u64;
    let mut start = 0u64;
    while start < total.max(1) {
        let end = (start + chunk).min(total.max(start + 1));
        let mut seg = Vec::new();
        put_bytes(&mut seg, 1, &playback_id);
        put_field(&mut seg, 2, start);
        put_field(&mut seg, 3, end);
        put_field(&mut seg, 4, end - start);
        put_bytes(&mut seg, 5, reason.as_bytes());
        put_bytes(&mut seg, 6, reason.as_bytes());
        put_double(&mut seg, 7, 1.0);
        put_field(&mut seg, 8, play.started_ms + start);
        put_field(&mut seg, 9, play.started_ms + end);
        put_field(&mut seg, 10, 0);
        put_field(&mut seg, 11, 0);
        put_field(&mut seg, 12, 0);
        put_bytes(&mut seg, 13, b"audio");
        put_field(&mut seg, 14, DESKTOP_CORE_VERSION);
        put_bytes(&mut seg, 15, play.track_uri.as_bytes());
        put_field(&mut seg, 16, mono_ms);
        put_field(&mut seg, 17, mono_ms + (end - start));
        put_field(&mut seg, 18, (end >= total) as u64);
        put_bytes(&mut seg, 19, b"context");
        put_bytes(&mut seg, 20, b"boombox");
        put_empty(&mut seg, 21);
        put_bytes(&mut seg, 22, &stream_id);
        put_empty(&mut seg, 23);
        put_empty(&mut seg, 24);
        put_bytes(&mut seg, 25, context.as_bytes());
        put_field(&mut seg, 26, play.started_ms + start);
        put_field(&mut seg, 27, play.started_ms + end);
        put_field(&mut seg, 28, 0);
        put_field(&mut seg, 30, 0);
        put_bytes(&mut seg, 31, b"unknown");
        put_field(&mut seg, 33, end);
        put_field(&mut seg, 34, end);
        put_field(&mut seg, 36, 0);
        put_bytes(&mut seg, 38, &file_playback_id);
        put_bytes(&mut seg, 39, b"list-player");
        if let Some(uri) = &list_uri {
            put_bytes(&mut seg, 40, uri.as_bytes());
        }
        put_bytes(&mut seg, 43, b"speaker");
        put_bytes(&mut seg, 44, b"wired");
        for f in [45u64, 46, 47, 48, 50, 51, 54] {
            put_field(&mut seg, f, 0);
        }
        put_bytes(&mut seg, 56, format_name.as_bytes());
        put_bytes(&mut seg, 58, b"none");
        put_bytes(&mut seg, 62, source.as_bytes());
        put_bytes(&mut seg, 63, source.as_bytes());
        put_bytes(&mut seg, 64, player_session_id.as_bytes());
        put_bytes(&mut seg, 65, decision_id.as_bytes());
        put_empty(&mut seg, 66);
        put_empty(&mut seg, 67);
        put_field(&mut seg, 70, play.shuffle as u64);
        put_field(&mut seg, 71, 0);
        put_field(&mut seg, 72, 0);
        put_field(&mut seg, 73, play.started_ms);
        put_field(&mut seg, 74, 0);
        put_field(&mut seg, 76, 0);
        put_field(&mut seg, 77, 0);
        put_field(&mut seg, 78, 0);
        put_bytes(&mut seg, 79, b"spotify");
        put_bytes(&mut seg, 80, b"PC desktop");
        put_bytes(&mut seg, 81, b"computer");
        put_empty(&mut seg, 82);
        put_empty(&mut seg, 84);
        put_bytes(&mut seg, 87, reason.as_bytes());
        put_bytes(&mut seg, 88, connect_id.as_bytes());
        envelopes.push(envelope("RawCoreStreamSegment", &seg, &device_id, now_ms, mono_ms, sequence_id, next_seq(), app_session));
        start = end;
    }

    // `RawCoreStream` (capturado, 567B): o resumo da reprodução, no fim.
    let mut msg = Vec::new();
    put_bytes(&mut msg, 1, &playback_id);
    put_empty(&mut msg, 2);
    put_empty(&mut msg, 3);
    put_empty(&mut msg, 4);
    put_bytes(&mut msg, 5, b"audio");
    put_empty(&mut msg, 9);
    put_bytes(&mut msg, 10, reason.as_bytes());
    put_empty(&mut msg, 11);
    put_bytes(&mut msg, 12, reason.as_bytes());
    put_field(&mut msg, 13, play.started_ms);
    put_field(&mut msg, 14, total);
    put_field(&mut msg, 15, total);
    put_field(&mut msg, 16, 0);
    put_field(&mut msg, 17, 0);
    put_field(&mut msg, 19, 0);
    put_field(&mut msg, 20, 0);
    put_field(&mut msg, 21, play.shuffle as u64);
    put_bytes(&mut msg, 22, format_name.as_bytes());
    if !context.is_empty() {
        put_bytes(&mut msg, 23, context.as_bytes());
    }
    put_bytes(&mut msg, 24, play.track_uri.as_bytes());
    put_empty(&mut msg, 25);
    put_field(&mut msg, 26, 0);
    put_field(&mut msg, 27, 0);
    put_bytes(&mut msg, 28, b"context");
    put_empty(&mut msg, 29);
    put_bytes(&mut msg, 32, b"none");
    put_empty(&mut msg, 34);
    put_empty(&mut msg, 35);
    put_field(&mut msg, 37, DESKTOP_CORE_VERSION);
    put_bytes(&mut msg, 38, b"full");
    put_field(&mut msg, 39, 1);
    put_field(&mut msg, 40, 0);
    put_field(&mut msg, 42, 0);
    put_field(&mut msg, 43, 0);
    put_bytes(&mut msg, 44, b"local");
    put_bytes(&mut msg, 45, b"boombox");
    put_field(&mut msg, 46, 0);
    put_empty(&mut msg, 47);
    put_bytes(&mut msg, 48, decision_id.as_bytes());
    put_empty(&mut msg, 49);
    put_empty(&mut msg, 52);
    put_bytes(&mut msg, 53, &stream_id);
    put_field(&mut msg, 54, 0);
    put_field(&mut msg, 55, 0);
    put_field(&mut msg, 56, now_ms);
    put_field(&mut msg, 57, 0);
    put_bytes(&mut msg, 59, player_session_id.as_bytes());
    put_empty(&mut msg, 61);
    put_empty(&mut msg, 64);
    put_field(&mut msg, 67, 0);
    put_bytes(&mut msg, 69, b"boombox");
    put_bytes(&mut msg, 70, &file_playback_id);
    put_bytes(&mut msg, 71, b"list-player");
    put_bytes(&mut msg, 75, b"speaker");
    put_bytes(&mut msg, 76, b"wired");
    if let Some(uri) = &list_uri {
        put_bytes(&mut msg, 77, uri.as_bytes());
    }
    put_bytes(&mut msg, 78, b"spotify");
    put_bytes(&mut msg, 79, b"PC desktop");
    put_bytes(&mut msg, 80, b"computer");
    put_bytes(&mut msg, 82, connect_id.as_bytes());
    envelopes.push(envelope("RawCoreStream", &msg, &device_id, now_ms, mono_ms, sequence_id, next_seq(), app_session));

    // `BoomboxPlaybackSession` (capturado): latências desconhecidas vão como u64 máx.
    let mut boombox = Vec::new();
    put_bytes(&mut boombox, 1, &playback_id);
    for f in [2u64, 3, 4, 5] {
        put_field(&mut boombox, f, UNKNOWN);
    }
    for f in [6u64, 7, 8, 9, 10, 11, 12, 16] {
        put_field(&mut boombox, f, 0);
    }
    put_bytes(&mut boombox, 13, b"default");
    put_bytes(&mut boombox, 14, b"list-player");
    put_bytes(&mut boombox, 15, b"none");
    put_empty(&mut boombox, 23);
    envelopes.push(envelope("BoomboxPlaybackSession", &boombox, &device_id, now_ms, mono_ms, sequence_id, next_seq(), app_session));

    // `Download` (capturado): amarra o download do áudio ao playback_id do arquivo. Sem o file_id não
    // há o que amarrar: o evento fica de fora. Os bytes são estimados pelo bitrate, porque o librespot
    // não expõe o total baixado.
    if let Some(file_id) = &file_id {
        let bytes = bps / 8 * total.max(1) / 1000;
        let mut download = Vec::new();
        put_bytes(&mut download, 1, file_id);
        put_bytes(&mut download, 2, &file_playback_id);
        put_field(&mut download, 7, bytes);
        put_field(&mut download, 9, 0);
        put_field(&mut download, 10, bytes);
        put_bytes(&mut download, 11, b"music");
        for f in [12u64, 13, 14, 16] {
            put_field(&mut download, f, UNKNOWN);
        }
        put_double(&mut download, 15, -1.0);
        put_double(&mut download, 17, -1.0);
        put_double(&mut download, 29, -1.0);
        put_bytes(&mut download, 30, b"https");
        put_bytes(&mut download, 31, b"audio-cf.spotifycdn.com");
        put_bytes(&mut download, 32, b"no");
        put_field(&mut download, 33, 0);
        put_field(&mut download, 34, 0);
        envelopes.push(envelope("Download", &download, &device_id, now_ms, mono_ms, sequence_id, next_seq(), app_session));
    }

    // `ContentIntegrity` (capturado, 22B): só o playback_id (anti-ripping em repouso).
    let mut integrity = Vec::new();
    put_bytes(&mut integrity, 1, &playback_id);
    envelopes.push(envelope("ContentIntegrity", &integrity, &device_id, now_ms, mono_ms, sequence_id, next_seq(), app_session));

    let mut body = Vec::new();
    for e in &envelopes {
        put_bytes(&mut body, 1, e);
    }
    log::trace!("lote do histórico: {} eventos, {} bytes", envelopes.len(), body.len());

    // Mesma assinatura HTTP do núcleo nativo. O desktop manda o corpo gzip; o ingest aceita sem gzip.
    let mut headers = http::HeaderMap::new();
    headers.insert(http::header::CONTENT_TYPE, http::HeaderValue::from_static("application/x-protobuf"));
    headers.insert(http::header::HeaderName::from_static("app-platform"), http::HeaderValue::from_static("Win32_x86_64"));
    headers.insert(http::header::HeaderName::from_static("spotify-app-version"), http::HeaderValue::from_static(DESKTOP_VERSION));
    match session.connection_id() {
        id if !id.is_empty() => {
            if let Ok(v) = http::HeaderValue::from_str(&id) {
                headers.insert(http::header::HeaderName::from_static("x-spotify-connection-id"), v);
            }
        }
        _ => log::debug!("histórico: sem id de conexão do dealer"),
    }
    let bytes = session.spclient().request(&http::Method::POST, "/gabo-receiver-service/v3/events/", Some(headers), Some(&body)).await.map_err(|e| e.to_string())?;
    // PublishEventsResponse: só traz `repeated EventError error = 1` (index=1, transient=2, reason=3)
    // quando algum evento foi recusado.
    let mut refused = 0usize;
    let mut input = protobuf::CodedInputStream::from_bytes(&bytes);
    while let Some(tag) = input.read_raw_tag_or_eof().map_err(|e| e.to_string())? {
        if tag >> 3 == 1 && tag & 7 == 2 {
            let item = input.read_bytes().map_err(|e| e.to_string())?;
            let mut err = protobuf::CodedInputStream::from_bytes(&item);
            let (mut index, mut reason) = (0u64, 0u64);
            while let Ok(Some(t)) = err.read_raw_tag_or_eof() {
                match (t >> 3, t & 7) {
                    (1, 0) => index = err.read_raw_varint32().unwrap_or(0) as u64,
                    (3, 0) => reason = err.read_raw_varint32().unwrap_or(0) as u64,
                    _ => break,
                }
            }
            log::debug!("histórico: evento recusado (índice {index}, razão {reason})");
            refused += 1;
        } else {
            break;
        }
    }
    if refused == 0 {
        return Ok(());
    }
    Err(format!("o Spotify recusou {refused} de {} eventos", envelopes.len()))
}
