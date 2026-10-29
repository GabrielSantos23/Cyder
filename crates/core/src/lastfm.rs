//! Scrobbling no Last.fm, com a chave de API do próprio usuário (como o Client ID do Spotify).
//! O que toca pelo Cyder não entra no histórico do Spotify; o Last.fm guarda o histórico por fora.
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Track;

const API: &str = "https://ws.audioscrobbler.com/2.0/";
const KEYRING_SERVICE: &str = "cyder";
const KEYRING_USER: &str = "lastfm";
/// O Last.fm aceita até 50 scrobbles por pedido.
pub const BATCH_MAX: usize = 50;

/// Chave e segredo do app do usuário no Last.fm, e a sessão depois que ele autoriza.
#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub key: String,
    pub secret: String,
    pub session: String,
    pub user: String,
}

pub fn saved() -> Option<Account> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
    serde_json::from_str(&entry.get_password().ok()?).ok()
}

pub fn save(account: &Account) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        if let Err(e) = entry.set_password(&serde_json::to_string(account).unwrap_or_default()) {
            log::warn!("guardar o login do Last.fm: {e}");
        }
    }
}

pub fn forget() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }
}

/// Assinatura do Last.fm: os parâmetros em ordem alfabética, nome e valor colados, mais o segredo, em MD5.
fn sign(params: &[(String, String)], secret: &str) -> String {
    let mut sorted: Vec<_> = params.iter().filter(|(k, _)| k != "format").collect();
    sorted.sort();
    let mut hasher = Md5::new();
    for (k, v) in sorted {
        hasher.update(k.as_bytes());
        hasher.update(v.as_bytes());
    }
    hasher.update(secret.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Erro do Last.fm: código e mensagem.
pub struct Error {
    pub code: i64,
    pub message: String,
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error { code: 0, message: e.to_string() }
    }
}

async fn call(http: &reqwest::Client, secret: &str, mut params: Vec<(String, String)>) -> Result<Value, Error> {
    let sig = sign(&params, secret);
    params.push(("api_sig".into(), sig));
    params.push(("format".into(), "json".into()));
    let v: Value = http.post(API).form(&params).send().await?.json().await?;
    match v["error"].as_i64() {
        Some(code) => Err(Error { code, message: v["message"].as_str().unwrap_or("erro do Last.fm").to_string() }),
        None => Ok(v),
    }
}

fn p(k: &str, v: impl Into<String>) -> (String, String) {
    (k.to_string(), v.into())
}

/// Primeiro passo do login: um token que o usuário autoriza no navegador, em `auth_url`.
pub async fn token(http: &reqwest::Client, key: &str, secret: &str) -> Result<String, Error> {
    let v = call(http, secret, vec![p("method", "auth.getToken"), p("api_key", key)]).await?;
    v["token"].as_str().map(String::from).ok_or(Error { code: 0, message: "o Last.fm não devolveu o token".into() })
}

pub fn auth_url(key: &str, token: &str) -> String {
    format!("https://www.last.fm/api/auth/?api_key={key}&token={token}")
}

/// Sessão depois da autorização; None enquanto o usuário ainda não autorizou (erro 14).
pub async fn session(http: &reqwest::Client, key: &str, secret: &str, token: &str) -> Result<Option<(String, String)>, Error> {
    match call(http, secret, vec![p("method", "auth.getSession"), p("api_key", key), p("token", token)]).await {
        Ok(v) => Ok(Some((v["session"]["key"].as_str().unwrap_or_default().to_string(), v["session"]["name"].as_str().unwrap_or_default().to_string()))),
        Err(e) if e.code == 14 => Ok(None),
        Err(e) => Err(e),
    }
}

/// Só o primeiro artista: o Last.fm junta os outros sozinho pelo nome da faixa ("feat.").
fn main_artist(track: &Track) -> String {
    track.artists.first().map_or_else(|| track.artist.split(", ").next().unwrap_or_default().to_string(), |a| a.1.clone())
}

pub async fn now_playing(http: &reqwest::Client, account: &Account, track: &Track) -> Result<(), Error> {
    call(http, &account.secret, vec![
        p("method", "track.updateNowPlaying"),
        p("api_key", &account.key),
        p("sk", &account.session),
        p("artist", main_artist(track)),
        p("track", &track.name),
        p("album", &track.album),
        p("duration", (track.duration_ms / 1000).to_string()),
    ])
    .await
    .map(drop)
}

/// Faixa ouvida e quando começou (segundos Unix). Até `BATCH_MAX` de uma vez.
pub async fn scrobble(http: &reqwest::Client, account: &Account, listens: &[(Track, u64)]) -> Result<(), Error> {
    let mut params = vec![p("method", "track.scrobble"), p("api_key", &account.key), p("sk", &account.session)];
    for (i, (track, at)) in listens.iter().take(BATCH_MAX).enumerate() {
        params.push(p(&format!("artist[{i}]"), main_artist(track)));
        params.push(p(&format!("track[{i}]"), &track.name));
        params.push(p(&format!("album[{i}]"), &track.album));
        params.push(p(&format!("timestamp[{i}]"), at.to_string()));
        params.push(p(&format!("duration[{i}]"), (track.duration_ms / 1000).to_string()));
    }
    call(http, &account.secret, params).await.map(drop)
}

/// Erros em que tentar de novo não adianta: chave, sessão ou assinatura inválidas.
pub fn is_auth_error(code: i64) -> bool {
    matches!(code, 4 | 9 | 10 | 13 | 26)
}

#[cfg(test)]
mod tests {
    use md5::Digest;

    #[test]
    fn signature() {
        // Exemplo da documentação do Last.fm: md5("api_keyxxxxxxxxmethodauth.getSessiontokenyyyyyyilovecher").
        let params = vec![super::p("method", "auth.getSession"), super::p("token", "yyyyyy"), super::p("api_key", "xxxxxxxx"), super::p("format", "json")];
        assert_eq!(super::sign(&params, "ilovecher"), format!("{:x}", md5::Md5::digest(b"api_keyxxxxxxxxmethodauth.getSessiontokenyyyyyyilovecher")));
    }
}
