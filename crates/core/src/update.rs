//! Atualizações pelas Releases do GitHub (o workflow `.github/workflows/release.yml` publica o cyder.exe e
//! o SHA-256 dele). O app confere ao abrir; baixar e trocar o executável só quando a pessoa pede.
//!
//! A troca: o Windows não deixa sobrescrever o .exe que está rodando, mas deixa renomear. O atual vira
//! `cyder.old.exe`, o novo entra no lugar, e a próxima abertura apaga o antigo.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REPO: &str = env!("CARGO_PKG_REPOSITORY");
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const EXE: &str = "cyder.exe";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// Sem o "v": "1.2.3".
    pub version: String,
    pub exe_url: String,
    pub sha_url: String,
    pub page_url: String,
}

/// Em que pé está a atualização, para a UI.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    /// 0.0 a 1.0.
    Downloading(Release, f32),
    /// Já trocado: falta só reiniciar.
    Ready(Release),
    Failed(String),
}

/// "1.2.10" > "1.2.9"; com "v" na frente ou não. Partes que não são número contam como 0.
pub fn newer(latest: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split(['.', '-', '+']).take(3).map(|p| p.parse().unwrap_or(0)).collect() };
    parts(latest) > parts(current)
}

/// "https://github.com/dono/repo" → "dono/repo".
pub(crate) fn repo_path() -> &'static str {
    REPO.trim_end_matches('/').trim_start_matches("https://github.com/")
}

/// A Release mais nova, se for mais nova que esta versão. Sem nenhuma Release publicada ainda, nada.
pub async fn check(http: &reqwest::Client) -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo_path());
    let res = http.get(&url).header("accept", "application/vnd.github+json").send().await.map_err(|e| e.to_string())?;
    if res.status() == 404 {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(format!("o GitHub respondeu {}", res.status()));
    }
    let body: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    let tag = body["tag_name"].as_str().unwrap_or_default();
    let asset = |name: &str| {
        body["assets"].as_array()?.iter().find(|a| a["name"] == name).and_then(|a| a["browser_download_url"].as_str()).map(String::from)
    };
    let (Some(exe_url), Some(sha_url)) = (asset(EXE), asset(&format!("{EXE}.sha256"))) else {
        return Err(format!("a versão {tag} não tem o {EXE} e o SHA-256 dele"));
    };
    if !newer(tag, CURRENT) {
        return Ok(None);
    }
    Ok(Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        exe_url,
        sha_url,
        page_url: body["html_url"].as_str().unwrap_or_default().to_string(),
    }))
}

/// Baixa o executável novo para `dir`, conferindo o SHA-256 publicado junto. `progress` recebe 0.0 a 1.0.
pub async fn download(http: &reqwest::Client, release: &Release, dir: &Path, progress: impl Fn(f32)) -> Result<PathBuf, String> {
    use futures_util::StreamExt;
    let expected = http.get(&release.sha_url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?
        .text().await.map_err(|e| e.to_string())?;
    // O arquivo pode vir como "hash" ou "hash  cyder.exe".
    let expected = expected.split_whitespace().next().unwrap_or_default().to_lowercase();
    let res = http.get(&release.exe_url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let total = res.content_length().unwrap_or(0);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
        if total > 0 {
            progress((bytes.len() as f32 / total as f32).min(1.));
        }
    }
    let got = format!("{:x}", Sha256::digest(&bytes));
    if got != expected {
        return Err("o arquivo baixado não confere com o publicado (SHA-256 diferente)".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(EXE);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

fn old_path(exe: &Path) -> PathBuf {
    exe.with_file_name("cyder.old.exe")
}

/// Põe o executável novo no lugar do que está rodando; se a cópia falhar, devolve o antigo.
pub fn install(new: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("não deu para trocar o executável ({e}); a pasta do Cyder pode estar protegida"))?;
    if let Err(e) = std::fs::copy(new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("não deu para instalar a versão nova: {e}"));
    }
    let _ = std::fs::remove_file(new);
    Ok(())
}

/// Apaga o executável da versão anterior, que fica para trás depois de uma atualização.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn compares_versions() {
        assert!(super::newer("v0.2.0", "0.1.9"));
        assert!(super::newer("1.2.10", "1.2.9"));
        assert!(!super::newer("v0.1.0", "0.1.0"));
        assert!(!super::newer("0.0.9", "0.1.0"));
        assert!(super::newer("v1.0.0-beta", "0.9.0"));
    }

    #[test]
    fn repo_from_cargo() {
        assert!(!super::repo_path().starts_with("https://"));
        assert_eq!(super::repo_path().split('/').count(), 2);
    }
}
