// Spike descartável: librespot (spike 1), token do librespot na Web API (spike 2), LRCLIB (spike 4).
mod lrclib;

use std::sync::Arc;

use librespot::core::{
    SpotifyUri, authentication::Credentials, cache::Cache, config::SessionConfig,
    session::Session,
};
use librespot::playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::NoOpVolume,
    player::{Player, PlayerEvent},
};
use librespot_oauth::OAuthClientBuilder;
use serde_json::{Value, json};
use tauri::{Emitter, Manager, State};
use tokio::sync::Mutex;

// Mesmos escopos que o binário do librespot pede.
const OAUTH_SCOPES: &[&str] = &[
    "streaming", "playlist-read-private", "playlist-read-collaborative", "playlist-modify-private",
    "playlist-modify-public", "user-follow-read", "user-follow-modify", "user-library-read",
    "user-library-modify", "user-read-private", "user-top-read", "user-read-recently-played",
    "user-read-playback-state", "user-modify-playback-state", "user-read-currently-playing",
    "ugc-image-upload",
];

#[derive(Default)]
struct AppState {
    session: Mutex<Option<(Session, Arc<Player>)>>,
    // Token da Web API obtido com o Client ID do próprio usuário (BYO).
    web_token: Mutex<Option<String>>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn results_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join("cyder-spike");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn save(name: &str, v: &Value) {
    let _ = std::fs::write(results_dir().join(format!("{name}.json")), serde_json::to_string_pretty(v).unwrap_or_default());
}

/// Marca o início de uma fase (para cruzar com as amostras de RAM).
#[tauri::command]
fn mark(phase: String) {
    use std::io::Write;
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(results_dir().join("marks.txt")) {
        let _ = writeln!(f, "{ms} {phase}");
    }
}

#[tauri::command]
async fn login(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let mut guard = state.session.lock().await;
    if guard.is_some() {
        return Ok("já conectado".into());
    }
    let dir = app.path().app_cache_dir().map_err(err)?;
    let cache = Cache::new(Some(dir.join("creds")), None, Some(dir.join("audio")), Some(1 << 30))
        .map_err(err)?;
    let config = SessionConfig::default();

    let credentials = match cache.credentials() {
        Some(c) => c,
        None => {
            let client_id = config.client_id.clone();
            let token = tokio::task::spawn_blocking(move || {
                OAuthClientBuilder::new(&client_id, "http://127.0.0.1:5588/login", OAUTH_SCOPES.to_vec())
                    .open_in_browser()
                    .with_custom_message("Login feito. Pode voltar para o app.")
                    .build()?
                    .get_access_token()
            })
            .await
            .map_err(err)?
            .map_err(err)?;
            Credentials::with_access_token(token.access_token)
        }
    };

    let session = Session::new(config, Some(cache));
    session.connect(credentials, true).await.map_err(err)?;

    let backend = audio_backend::find(None).ok_or("sem backend de áudio")?;
    let player = Player::new(PlayerConfig::default(), session.clone(), Box::new(NoOpVolume), move || {
        backend(None, AudioFormat::default())
    });

    let mut events = player.get_player_event_channel();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = events.recv().await {
            let payload = match ev {
                PlayerEvent::Playing { position_ms, track_id, .. } => json!({"kind":"playing","position_ms":position_ms,"uri":track_id.to_uri().ok()}),
                PlayerEvent::Paused { position_ms, .. } => json!({"kind":"paused","position_ms":position_ms}),
                PlayerEvent::Loading { .. } => json!({"kind":"loading"}),
                PlayerEvent::EndOfTrack { .. } => json!({"kind":"end"}),
                PlayerEvent::Unavailable { .. } => json!({"kind":"unavailable"}),
                PlayerEvent::PositionCorrection { position_ms, .. } => json!({"kind":"position","position_ms":position_ms}),
                _ => continue,
            };
            mark(format!("player:{}", payload["kind"].as_str().unwrap_or("?")));
            let _ = app.emit("player", payload);
        }
    });

    let user = session.username();
    *guard = Some((session, player));
    Ok(user)
}

async fn player(state: &State<'_, AppState>) -> Result<Arc<Player>, String> {
    state.session.lock().await.as_ref().map(|(_, p)| p.clone()).ok_or("faça login".into())
}

#[tauri::command]
async fn play(uri: String, state: State<'_, AppState>) -> Result<(), String> {
    let uri = SpotifyUri::from_uri(&uri).map_err(err)?;
    player(&state).await?.load(uri, true, 0);
    Ok(())
}

#[tauri::command]
async fn pause(state: State<'_, AppState>) -> Result<(), String> {
    player(&state).await?.pause();
    Ok(())
}

#[tauri::command]
async fn resume(state: State<'_, AppState>) -> Result<(), String> {
    player(&state).await?.play();
    Ok(())
}

#[tauri::command]
async fn seek(ms: u32, state: State<'_, AppState>) -> Result<(), String> {
    player(&state).await?.seek(ms);
    Ok(())
}

/// OAuth PKCE com o Client ID do usuário (redirect cadastrado no dashboard: http://127.0.0.1:5589/callback).
#[tauri::command]
async fn web_login(client_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let token = tokio::task::spawn_blocking(move || {
        OAuthClientBuilder::new(client_id.trim(), "http://127.0.0.1:5589/callback", OAUTH_SCOPES.to_vec())
            .open_in_browser()
            .with_custom_message("Web API autorizada. Pode voltar para o app.")
            .build()?
            .get_access_token()
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    *state.web_token.lock().await = Some(token.access_token);
    Ok(())
}

async fn web_token(state: &State<'_, AppState>) -> Result<String, String> {
    if let Some(t) = state.web_token.lock().await.clone() {
        return Ok(t);
    }
    // Fallback: token do librespot — deu 429 em todas as chamadas (out/2026), ver doc 02.
    let guard = state.session.lock().await;
    let (session, _) = guard.as_ref().ok_or("faça login")?;
    // keymaster responde 403 "Invalid request" (out/2026); login5 é o caminho atual do librespot.
    let t = session.login5().auth_token().await.map_err(err)?;
    Ok(t.access_token)
}

/// Spike 2: chama endpoints da Web API com o token do librespot e devolve o status de cada um.
#[tauri::command]
async fn probe_web_api(state: State<'_, AppState>) -> Result<Value, String> {
    let token = web_token(&state).await?;
    let http = reqwest::Client::new();
    // Artista/faixa conhecidos (Daft Punk / One More Time) só para testar endpoints.
    let artist = "4tZwfgrHOc3mvqYlEYSvVi";
    let track = "0DiWol3AO6WpXZgp0goxAV";
    let probes = [
        ("me", "/me".to_string()),
        ("me/tracks", "/me/tracks?limit=50".into()),
        ("me/playlists", "/me/playlists?limit=50".into()),
        ("me/top/tracks", "/me/top/tracks?limit=50".into()),
        ("me/following", "/me/following?type=artist&limit=50".into()),
        ("me/player/devices", "/me/player/devices".into()),
        ("me/player/recently-played", "/me/player/recently-played?limit=50".into()),
        ("me/library/contains (novo)", format!("/me/library/contains?uris=spotify:track:{track}")),
        ("search limit=10", "/search?q=daft%20punk&type=track&limit=10".into()),
        ("search limit=50 (removido p/ dev)", "/search?q=daft%20punk&type=track&limit=50".into()),
        ("artists/{id}", format!("/artists/{artist}")),
        ("artists/{id}/albums", format!("/artists/{artist}/albums?limit=10")),
        ("artists/{id}/top-tracks (removido p/ dev)", format!("/artists/{artist}/top-tracks")),
        ("artists/{id}/related-artists (removido 2024)", format!("/artists/{artist}/related-artists")),
        ("tracks?ids= lote (removido p/ dev)", format!("/tracks?ids={track}")),
        ("recommendations (removido 2024)", format!("/recommendations?seed_tracks={track}")),
        ("audio-features (removido 2024)", format!("/audio-features/{track}")),
        ("browse/new-releases (removido p/ dev)", "/browse/new-releases".into()),
        ("Discover Weekly-like editorial (Today's Top Hits)", "/playlists/37i9dQZF1DXcBWIGoYBM5M".into()),
        ("items de playlist editorial", "/playlists/37i9dQZF1DXcBWIGoYBM5M/items?limit=5".into()),
    ];
    let mut out = serde_json::Map::new();
    for (name, path) in probes {
        let res = http
            .get(format!("https://api.spotify.com/v1{path}"))
            .bearer_auth(&token)
            .send()
            .await;
        let v = match res {
            Ok(r) => {
                let status = r.status().as_u16();
                let retry_after = r.headers().get("retry-after").and_then(|h| h.to_str().ok()).map(String::from);
                let body: Value = r.json().await.unwrap_or(Value::Null);
                let extra = body.get("items").and_then(|i| i.as_array()).map(|a| a.len())
                    .or_else(|| body.pointer("/tracks/items").and_then(|i| i.as_array()).map(|a| a.len()));
                json!({"status": status, "items": extra, "error": body.pointer("/error/message"), "retry_after": retry_after})
            }
            Err(e) => json!({"error": e.to_string()}),
        };
        out.insert(name.into(), v);
    }
    let out = Value::Object(out);
    save("web_api", &out);
    Ok(out)
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("cyder-spike/0.1 (https://github.com/local/cyder-spotify)")
        .build()
        .map_err(err)
}

async fn liked(http: &reqwest::Client, token: &str, limit: usize) -> Result<Vec<Value>, String> {
    let mut tracks = Vec::new();
    let mut offset = 0;
    while tracks.len() < limit {
        let page: Value = http
            .get(format!("https://api.spotify.com/v1/me/tracks?limit=50&offset={offset}"))
            .bearer_auth(token)
            .send().await.map_err(err)?
            .json().await.map_err(err)?;
        let items = page["items"].as_array().cloned().unwrap_or_default();
        if items.is_empty() { break; }
        offset += items.len();
        tracks.extend(items.into_iter().map(|i| i["track"].clone()));
    }
    tracks.truncate(limit);
    Ok(tracks)
}

/// Spike 4: pega até `limit` músicas curtidas e mede o acerto no LRCLIB.
#[tauri::command]
async fn probe_lrclib(limit: usize, app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Value, String> {
    let token = web_token(&state).await?;
    let http = http()?;
    let tracks = liked(&http, &token, limit).await?;
    save("liked_tracks", &Value::Array(tracks.clone()));
    let out = lrclib::measure(&http, &tracks, |i, n| {
        let _ = app.emit("progress", format!("Spike 4: LRCLIB {i}/{n}…"));
    })
    .await;
    save("lrclib", &out);
    Ok(out)
}

/// Spike 3: faixas reais (só os campos que a UI usa, capa de 64px) para popular a lista fake.
#[tauri::command]
async fn liked_tracks(limit: usize, state: State<'_, AppState>) -> Result<Value, String> {
    let token = web_token(&state).await?;
    let tracks = liked(&http()?, &token, limit).await?;
    Ok(tracks.iter().map(|t| json!({
        "uri": t["uri"], "name": t["name"], "artist": t["artists"][0]["name"],
        "album": t["album"]["name"], "duration_ms": t["duration_ms"],
        "cover64": t["album"]["images"].as_array().and_then(|i| i.last()).map(|i| i["url"].clone()),
        "cover640": t["album"]["images"][0]["url"],
    })).collect())
}

/// Letra sincronizada da faixa (mesmo matching do spike 4).
#[tauri::command]
async fn lyrics(name: String, artist: String, album: String, duration_ms: u64) -> Result<Value, String> {
    Ok(lrclib::lookup(&http()?, &name, &artist, &album, (duration_ms + 500) / 1000).await)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![login, play, pause, resume, seek, probe_web_api, probe_lrclib, liked_tracks, lyrics, mark, web_login])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
