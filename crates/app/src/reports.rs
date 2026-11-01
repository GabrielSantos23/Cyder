//! Relatórios de erro pelo Sentry, só com o consentimento da pessoa (pergunta na primeira vez; muda em
//! Configurações › Sobre). Vão os erros e travamentos com a versão do Cyder e a do sistema. Antes de sair,
//! cada relatório perde o nome do computador, o usuário do Windows, a chave do Soloist e os ids do Spotify.
use std::{collections::HashSet, sync::{Arc, Mutex}};

use sentry::protocol::Event;

/// Endereço do projeto no Sentry (não é segredo: vai em todo app que manda relatórios). `CYDER_SENTRY_DSN` na
/// compilação troca por outro; vazio, o Cyder nem oferece o envio.
const DSN: &str = match option_env!("CYDER_SENTRY_DSN") {
    Some(dsn) => dsn,
    None => "https://eb9d61752f75d89da11cf5e00522b076@o4512358244876288.ingest.us.sentry.io/4512358294290432",
};
/// Por abertura: um erro que se repete a cada faixa não gasta a cota do plano gratuito.
const MAX_EVENTS: usize = 20;

static CLIENT: Mutex<Option<sentry::ClientInitGuard>> = Mutex::new(None);
static SENT: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub fn available() -> bool {
    !DSN.is_empty()
}

/// Liga ou desliga o envio na hora.
pub fn set(on: bool) {
    let mut client = CLIENT.lock().unwrap_or_else(|e| e.into_inner());
    if !on || !available() {
        // Soltar o guard fecha o cliente: nada mais sai.
        *client = None;
        return;
    }
    if client.is_none() {
        let mut options = sentry::ClientOptions::default();
        options.release = Some(concat!("cyder@", env!("CARGO_PKG_VERSION")).into());
        options.send_default_pii = false;
        options.max_breadcrumbs = 0;
        options.before_send = Some(Arc::new(clean));
        *client = Some(sentry::init((DSN, options)));
    }
}

/// Espera o envio do que estiver na fila (o app está fechando por um panic).
pub fn flush() {
    if let Some(client) = sentry::Hub::current().client() {
        client.flush(Some(std::time::Duration::from_secs(2)));
    }
}

fn clean(mut event: Event<'static>) -> Option<Event<'static>> {
    let secrets = Secrets::now();
    event.server_name = None;
    event.user = None;
    event.message = event.message.map(|m| secrets.scrub(&m));
    if let Some(entry) = event.logentry.as_mut() {
        entry.message = secrets.scrub(&entry.message);
    }
    for e in event.exception.values.iter_mut() {
        e.value = e.value.as_deref().map(|v| secrets.scrub(v));
    }
    // O mesmo erro uma vez por abertura, e no máximo MAX_EVENTS.
    let key: String = event.message.as_deref().or(event.logentry.as_ref().map(|l| l.message.as_str())).unwrap_or_default().chars().take(80).collect();
    let mut sent = SENT.lock().unwrap_or_else(|e| e.into_inner());
    let sent = sent.get_or_insert_with(HashSet::new);
    (sent.len() < MAX_EVENTS && sent.insert(key)).then_some(event)
}

/// O que não pode sair do computador.
struct Secrets {
    home: Option<String>,
    user: Option<String>,
    soloist_key: Option<String>,
}

impl Secrets {
    fn now() -> Self {
        let env = |k| std::env::var(k).ok().filter(|v: &String| v.len() > 2);
        Secrets { home: env("USERPROFILE"), user: env("USERNAME"), soloist_key: cyder_core::soloist::key() }
    }

    fn scrub(&self, text: &str) -> String {
        let mut out = text.to_string();
        if let Some(key) = &self.soloist_key {
            out = out.replace(key.as_str(), "<chave>");
        }
        if let Some(home) = &self.home {
            out = out.replace(home.as_str(), "~").replace(&home.replace('\\', "/"), "~");
        }
        if let Some(user) = &self.user {
            out = out.replace(user.as_str(), "<usuário>");
        }
        scrub_spotify_ids(&out)
    }
}

/// "spotify:track:7E8rNzgr0EmLgH0Vx6jcGH" → "spotify:track:…": o que a pessoa ouve não vai junto.
fn scrub_spotify_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("spotify:") {
        let (before, after) = rest.split_at(at + "spotify:".len());
        out.push_str(before);
        let kind_len = after.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(after.len());
        let (kind, tail) = after.split_at(kind_len);
        out.push_str(kind);
        match tail.strip_prefix(':') {
            Some(id) => {
                let id_len = id.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(id.len());
                out.push_str(":…");
                rest = &id[id_len..];
            }
            None => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn scrubs() {
        assert_eq!(super::scrub_spotify_ids("unable to load <SpotifyUri(\"spotify:track:7E8rNzgr0EmLgH0Vx6jcGH\")>: ()"), "unable to load <SpotifyUri(\"spotify:track:…\")>: ()");
        assert_eq!(super::scrub_spotify_ids("spotify:user:fulano:collection e spotify:"), "spotify:user:…:collection e spotify:");
        let s = super::Secrets { home: Some(r"C:\Users\gabs".into()), user: Some("gabs".into()), soloist_key: Some("abc123xyz".into()) };
        assert_eq!(s.scrub(r"C:\Users\gabs\AppData --api-key abc123xyz C:/Users/gabs/x"), r"~\AppData --api-key <chave> ~/x");
    }
}
