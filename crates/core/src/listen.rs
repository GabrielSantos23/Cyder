//! Ouvir junto: quem convida publica o que toca (faixa, posição, tocando ou não) num canal do ntfy.sh,
//! um serviço público e gratuito de mensagens; quem entra com o código assina o canal e segue.
//! Só passa a URI da faixa e a posição; o áudio cada um toca da própria conta do Spotify.
use serde::{Deserialize, Serialize};

const RELAY: &str = "https://ntfy.sh";
/// Sem I, O, 0 e 1, que confundem ao ditar o código.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const CODE_LEN: usize = 8;

/// O que quem convida está tocando.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct State {
    pub uri: String,
    pub position_ms: u32,
    pub playing: bool,
    /// Nome de quem convida, para quem entra saber com quem está.
    #[serde(default)]
    pub host: String,
}

/// Código novo, do relógio e do id do processo (não precisa ser criptográfico: só difícil de adivinhar).
pub fn new_code() -> String {
    let mut x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64) ^ ((std::process::id() as u64) << 32);
    (0..CODE_LEN).map(|_| {
        // xorshift64*
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        ALPHABET[(x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 59) as usize % ALPHABET.len()] as char
    }).collect()
}

/// Código como o usuário digitou ("abcd-efgh", com espaços): só as letras e números válidos, em maiúsculas.
pub fn normalize(code: &str) -> Option<String> {
    let c: String = code.to_uppercase().chars().filter(|ch| ALPHABET.contains(&(*ch as u8)) && ch.is_ascii()).collect();
    (c.len() == CODE_LEN).then_some(c)
}

fn topic(code: &str) -> String {
    format!("{RELAY}/cyder-listen-{}", code.to_lowercase())
}

pub async fn publish(http: &reqwest::Client, code: &str, state: &State) -> Result<(), String> {
    let body = serde_json::to_string(state).map_err(|e| e.to_string())?;
    let res = http.post(topic(code)).body(body).send().await.map_err(|e| e.to_string())?;
    if res.status().is_success() { Ok(()) } else { Err(format!("ntfy respondeu {}", res.status())) }
}

/// Assina o canal e manda cada estado novo para `tx`, até o canal fechar (quem entrou saiu).
/// Começa pela última mensagem guardada, para quem entra já pegar a música.
pub async fn follow(code: String, tx: tokio::sync::mpsc::UnboundedSender<Result<State, String>>) {
    use futures_util::StreamExt;
    let url = format!("{}/json?since=latest", topic(&code));
    loop {
        // O stream do ntfy não tem fim; o timeout de 30 s do cliente não serve aqui.
        let res = match reqwest::Client::new().get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                let _ = tx.send(Err(format!("ntfy respondeu {}", r.status())));
                return;
            }
            Err(e) => {
                if tx.send(Err(e.to_string())).is_err() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };
        let mut stream = res.bytes_stream();
        let mut line = Vec::new();
        while let Some(chunk) = stream.next().await {
            let Ok(chunk) = chunk else { break };
            for &b in chunk.iter() {
                if b != b'\n' {
                    line.push(b);
                    continue;
                }
                let msg: serde_json::Value = serde_json::from_slice(&line).unwrap_or_default();
                line.clear();
                if msg["event"] != "message" {
                    continue;
                }
                if let Some(state) = msg["message"].as_str().and_then(|m| serde_json::from_str::<State>(m).ok()) {
                    if tx.send(Ok(state)).is_err() {
                        return;
                    }
                }
            }
        }
        if tx.is_closed() {
            return;
        }
        // Caiu a conexão: assina de novo.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn codes() {
        let c = super::new_code();
        assert_eq!(c.len(), super::CODE_LEN);
        assert_eq!(super::normalize(&c.to_lowercase()), Some(c.clone()));
        assert_eq!(super::normalize("abcd-efgh"), Some("ABCDEFGH".into()));
        assert_eq!(super::normalize("abc"), None);
    }
}
