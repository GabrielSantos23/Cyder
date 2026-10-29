use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct Config {
    /// Client ID do app que o próprio usuário cria no dashboard do Spotify (BYO, doc 02).
    pub client_id: String,
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("cyder")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("cyder")
}

/// Registro desta execução; o da anterior fica em `cyder.old.log`.
pub fn log_path() -> PathBuf {
    cache_dir().join("cyder.log")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Playlists fixadas no topo da sidebar. Ficam na pasta de configuração, não no cache: são escolha do usuário.
fn load_list(file: &str) -> Vec<String> {
    std::fs::read_to_string(config_dir().join(file)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_list(file: &str, list: &[String]) {
    let text = serde_json::to_string_pretty(list).unwrap_or_default();
    if let Err(e) = std::fs::create_dir_all(config_dir()).and_then(|_| std::fs::write(config_dir().join(file), text)) {
        log::warn!("{file}: {e}");
    }
}

pub fn pins() -> Vec<String> {
    load_list("pins.json")
}

pub fn save_pins(pins: &[String]) {
    save_list("pins.json", pins);
}

/// Faixas ocultas num álbum ou playlist ("contexto|faixa"): pulam quando tocam ali. Só neste computador.
pub fn hidden() -> Vec<String> {
    load_list("hidden.json")
}

pub fn save_hidden(list: &[String]) {
    save_list("hidden.json", list);
}

/// Ordem da lista da sidebar arrastada à mão (URIs). Só neste computador: a Web API não reordena a biblioteca.
pub fn library_order() -> Vec<String> {
    load_list("library-order.json")
}

pub fn save_library_order(order: &[String]) {
    save_list("library-order.json", order);
}

/// Onde a janela estava: posição e tamanho em pixels lógicos (de quando não está maximizada) e se estava maximizada.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlace {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub maximized: bool,
}

pub fn window_place() -> Option<WindowPlace> {
    std::fs::read_to_string(config_dir().join("window.json")).ok().and_then(|t| serde_json::from_str(&t).ok())
}

pub fn save_window_place(p: &WindowPlace) {
    let text = serde_json::to_string_pretty(p).unwrap_or_default();
    if let Err(e) = std::fs::create_dir_all(config_dir()).and_then(|_| std::fs::write(config_dir().join("window.json"), text)) {
        log::warn!("window.json: {e}");
    }
}

/// Últimas buscas, a mais recente primeiro.
pub fn recent_searches() -> Vec<String> {
    load_list("recent-searches.json")
}

pub fn save_recent_searches(list: &[String]) {
    save_list("recent-searches.json", list);
}

/// Qualidade do streaming (kbps do Ogg Vorbis): 96, 160 e 320.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Normal,
    High,
    VeryHigh,
}

/// Tema das cores: escuro, claro ou o do sistema.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

/// Configurações da tela de configurações. Campos que faltam no arquivo ficam no padrão.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Nome do Cyder no Spotify Connect.
    pub device_name: String,
    pub quality: Quality,
    pub normalize: bool,
    pub autoplay: bool,
    pub gapless: bool,
    /// Fechar a janela só minimiza: a música continua.
    pub keep_playing_on_close: bool,
    /// Abrir com o Windows (escondido na bandeja). O app grava no registro do Windows quando muda.
    pub open_at_login: bool,
    pub audio_cache: bool,
    pub audio_cache_mb: u32,
    /// Cores da capa nas páginas e no destaque.
    pub cover_colors: bool,
    pub theme: Theme,
    /// Canvas (o vídeo curto de algumas músicas) no fundo do modo imersivo.
    pub canvas: bool,
    /// Equalizador: ligado e o ganho de cada faixa em dB (`fx::BANDS`).
    pub eq_on: bool,
    pub eq: [f32; 6],
    /// Transição suave entre músicas, em segundos; 0 desliga.
    pub fade_secs: u32,
    /// Mostra no Discord o que está tocando, pelo app que o usuário criou no Discord.
    pub discord: bool,
    pub discord_app_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            device_name: "Cyder".into(),
            quality: Quality::VeryHigh,
            normalize: false,
            autoplay: true,
            gapless: true,
            keep_playing_on_close: false,
            open_at_login: false,
            audio_cache: true,
            audio_cache_mb: 1024,
            cover_colors: true,
            theme: Theme::Dark,
            canvas: true,
            eq_on: false,
            eq: [0.; 6],
            fade_secs: 0,
            discord: false,
            discord_app_id: String::new(),
        }
    }
}

impl Settings {
    /// Os efeitos de áudio que valem com estas configurações.
    pub fn fx(&self) -> crate::fx::Settings {
        crate::fx::Settings { eq: self.eq_on.then_some(self.eq), fade_ms: self.fade_secs * 1000 }
    }

    /// Nome no Connect; vazio vira "Cyder".
    pub fn device_name(&self) -> String {
        let name = self.device_name.trim();
        if name.is_empty() { "Cyder".into() } else { name.chars().take(40).collect() }
    }
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn settings() -> Settings {
    std::fs::read_to_string(settings_path()).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or_default()
}

pub fn save_settings(s: &Settings) {
    let text = serde_json::to_string_pretty(s).unwrap_or_default();
    if let Err(e) = std::fs::create_dir_all(config_dir()).and_then(|_| std::fs::write(settings_path(), text)) {
        log::warn!("configurações: {e}");
    }
}

/// Pastas do cache que a tela de configurações mostra.
pub fn audio_cache_dir() -> PathBuf {
    cache_dir().join("audio")
}

pub fn credentials_dir() -> PathBuf {
    cache_dir().join("credentials")
}

pub enum ConfigError {
    /// Arquivo ausente ou campo vazio: primeira execução.
    Missing,
    /// Preenchido, mas errado; a mensagem diz o que corrigir.
    Invalid(String),
}

/// O Client ID do Spotify tem 32 caracteres hexadecimais.
pub fn check_client_id(id: &str) -> Result<(), String> {
    match id.len() {
        0 => Err("Cole o Client ID do seu app.".into()),
        32 if id.chars().all(|c| c.is_ascii_hexdigit()) => Ok(()),
        32 => Err("O Client ID só tem letras de a a f e números. Confira se copiou o campo certo.".into()),
        n => Err(format!("Esse texto tem {n} caracteres; o Client ID tem 32.")),
    }
}

/// Grava o Client ID digitado na tela de configuração.
pub fn save(client_id: &str) -> Result<(), String> {
    let id = client_id.trim();
    check_client_id(id)?;
    let text = serde_json::to_string_pretty(&Config { client_id: id.to_string() }).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir())
        .and_then(|_| std::fs::write(config_path(), text))
        .map_err(|e| format!("Não deu para salvar a configuração: {e}"))
}

pub fn load() -> Result<Config, ConfigError> {
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        let _ = std::fs::create_dir_all(config_dir());
        let template = serde_json::to_string_pretty(&Config { client_id: String::new() }).unwrap_or_default();
        let _ = std::fs::write(&path, template);
        return Err(ConfigError::Missing);
    };
    // Editores no Windows (Bloco de Notas, PowerShell) costumam salvar com BOM, que o serde_json recusa.
    let cfg: Config = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| {
        ConfigError::Invalid("A configuração salva está corrompida. Cole o Client ID de novo.".into())
    })?;
    if cfg.client_id.trim().is_empty() {
        return Err(ConfigError::Missing);
    }
    check_client_id(cfg.client_id.trim()).map_err(ConfigError::Invalid)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    #[test]
    fn accepts_bom() {
        let text = "\u{feff}{ \"client_id\": \"x\" }";
        let cfg: super::Config = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        assert_eq!(cfg.client_id, "x");
    }

    #[test]
    fn settings_fill_missing_fields() {
        let s: super::Settings = serde_json::from_str("{ \"normalize\": true }").unwrap();
        assert!(s.normalize && s.autoplay && s.audio_cache_mb == 1024);
        assert_eq!(super::Settings { device_name: "  ".into(), ..Default::default() }.device_name(), "Cyder");
    }

    #[test]
    fn checks_client_id() {
        assert!(super::check_client_id("2c38327a19a54c4f88c2b89af901246d").is_ok());
        assert!(super::check_client_id("2c38327a19a54c4f88c2b89af901246").is_err());
        assert!(super::check_client_id("zc38327a19a54c4f88c2b89af901246d").is_err());
        assert!(super::check_client_id("").is_err());
    }
}
