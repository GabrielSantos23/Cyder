use std::path::Path;

use librespot::core::{authentication::Credentials, cache::Cache, config::SessionConfig, session::Session};
use librespot_oauth::{OAuthClient, OAuthClientBuilder, OAuthToken};

/// Escopos da Web API pedidos para o Client ID do usuário.
const WEB_SCOPES: &[&str] = &[
    "user-library-read", "user-library-modify", "user-read-private", "user-top-read",
    "user-read-recently-played", "user-read-playback-state", "user-modify-playback-state",
    "playlist-read-private", "playlist-read-collaborative", "playlist-modify-private",
    "playlist-modify-public", "user-follow-read", "user-follow-modify",
    // Trocar a capa de playlist. Logins anteriores não têm: o Cyder pede para entrar de novo quando faz falta.
    "ugc-image-upload",
];
const WEB_REDIRECT: &str = "http://127.0.0.1:5589/callback";
const KEYRING_SERVICE: &str = "cyder";
const KEYRING_USER: &str = "web-refresh-token";
const DONE_PAGE: &str = "<html><body style=\"font-family:sans-serif;background:#0b0b0e;color:#fff;padding:40px\">Pronto. Pode voltar para o Cyder.</body></html>";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Sessão do librespot (áudio) e as credenciais para ela. Reusa as credenciais salvas; na primeira vez abre o navegador.
/// A sessão volta **sem conectar**: quem conecta é o `Spirc::new`, e conectar duas vezes trava (doc 07).
pub async fn audio_session(cache_dir: &Path) -> Result<(Session, Credentials), String> {
    let settings = crate::config::settings();
    let audio = settings.audio_cache.then(|| cache_dir.join("audio"));
    let limit = settings.audio_cache.then_some(settings.audio_cache_mb as u64 * 1024 * 1024);
    let cache = Cache::new(Some(cache_dir.join("credentials")), None, audio, limit).map_err(err)?;
    // Como no Spotify: quando o álbum ou a playlist acaba, segue com músicas parecidas.
    let defaults = SessionConfig::default();
    let device_id = crate::config::device_id(|| defaults.device_id.clone());
    let config = SessionConfig { autoplay: Some(settings.autoplay), device_id, ..defaults };
    let credentials = match cache.credentials() {
        Some(c) => c,
        None => {
            let client_id = config.client_id.clone();
            let token = tokio::task::spawn_blocking(move || {
                // Mesmo redirect do binário do librespot, registrado para o client ID interno.
                // Escopos que funcionaram no spike (doc 07); a sessão de áudio exige mais que só "streaming".
                let scopes = [&["streaming"], WEB_SCOPES].concat();
                OAuthClientBuilder::new(&client_id, "http://127.0.0.1:5588/login", scopes)
                    .open_in_browser()
                    .with_custom_message(DONE_PAGE)
                    .build()?
                    .get_access_token()
            })
            .await
            .map_err(err)?
            .map_err(err)?;
            Credentials::with_access_token(token.access_token)
        }
    };
    Ok((Session::new(config, Some(cache)), credentials))
}

/// Esquece só o login da Web API (para trocar o Client ID); o do áudio continua.
pub fn forget_web_login() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }
}

/// Sai da conta: apaga o login salvo (áudio e Web API) e os dados da conta em cache.
/// Capas e áudio em cache ficam: não são da conta.
pub fn logout(cache_dir: &Path) {
    for dir in [cache_dir.join("credentials"), cache_dir.join("data")] {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            log::info!("sair: {}: {e}", dir.display());
        }
    }
    let _ = std::fs::remove_file(crate::config::config_dir().join("pins.json"));
    let _ = std::fs::remove_file(crate::config::config_dir().join("library-order.json"));
    let _ = std::fs::remove_file(crate::config::config_dir().join("recent-searches.json"));
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }
}

pub struct WebAuth {
    client: OAuthClient,
    token: OAuthToken,
}

impl WebAuth {
    /// Token da Web API com o Client ID do usuário: tenta o refresh token salvo no cofre do SO, senão PKCE no navegador.
    pub async fn login(client_id: &str) -> Result<Self, String> {
        let client = OAuthClientBuilder::new(client_id.trim(), WEB_REDIRECT, WEB_SCOPES.to_vec())
            .open_in_browser()
            .with_custom_message(DONE_PAGE)
            .build()
            .map_err(err)?;
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(err)?;
        let saved = entry.get_password().ok().filter(|r| !r.is_empty());
        let token = match saved {
            Some(refresh) => match client.refresh_token_async(&refresh).await {
                Ok(t) => keep_refresh(t, &refresh),
                Err(e) => {
                    log::warn!("refresh token rejeitado, refazendo login: {e}");
                    client.get_access_token_async().await.map_err(err)?
                }
            },
            None => client.get_access_token_async().await.map_err(err)?,
        };
        save_refresh(&token.refresh_token);
        Ok(Self { client, token })
    }

    /// Access token válido, renovando um minuto antes de expirar.
    pub async fn access_token(&mut self) -> Result<String, String> {
        if self.token.expires_at <= std::time::Instant::now() + std::time::Duration::from_secs(60) {
            let old = self.token.refresh_token.clone();
            let fresh = self.client.refresh_token_async(&old).await.map_err(err)?;
            self.token = keep_refresh(fresh, &old);
            save_refresh(&self.token.refresh_token);
        }
        Ok(self.token.access_token.clone())
    }
}

/// Nem toda renovação traz um refresh token novo; o librespot devolve "" nesse caso, e o antigo continua válido.
fn keep_refresh(mut token: OAuthToken, previous: &str) -> OAuthToken {
    if token.refresh_token.is_empty() {
        token.refresh_token = previous.to_string();
    }
    token
}

fn save_refresh(refresh: &str) {
    // Gravar vazio apagaria o login salvo.
    if refresh.is_empty() {
        return;
    }
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).and_then(|e| e.set_password(refresh)) {
        Ok(()) => {}
        Err(e) => log::warn!("não deu para salvar o login no cofre do sistema: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use librespot_oauth::OAuthToken;

    #[test]
    fn empty_refresh_keeps_previous() {
        let token = |r: &str| OAuthToken { access_token: "a".into(), refresh_token: r.into(), expires_at: Instant::now(), token_type: String::new(), scopes: vec![] };
        assert_eq!(super::keep_refresh(token(""), "old").refresh_token, "old");
        assert_eq!(super::keep_refresh(token("new"), "old").refresh_token, "new");
    }
}
