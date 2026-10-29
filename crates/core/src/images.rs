//! Capas em disco, fundo pré-borrado e cor de acento (doc 03, regras 1 e 3).
use std::path::{Path, PathBuf};

use image::{GenericImageView, RgbImage};

use crate::Track;

#[derive(Clone, Debug)]
pub struct Art {
    pub cover: PathBuf,
    pub background: PathBuf,
    /// Cor dominante saturada da capa, para destaques.
    pub accent: [u8; 3],
}

async fn download(http: &reqwest::Client, url: &str, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Ok(());
    }
    let bytes = http.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?
        .bytes().await.map_err(|e| e.to_string())?;
    // Grava em arquivo temporário e renomeia, para nunca deixar uma capa pela metade no cache.
    let tmp = to.with_extension("part");
    tokio::fs::write(&tmp, &bytes).await.map_err(|e| e.to_string())?;
    tokio::fs::rename(&tmp, to).await.map_err(|e| e.to_string())
}

/// Média das cores ponderada pela saturação: ignora cinzas, preto e branco.
pub fn accent_color(img: &RgbImage) -> [u8; 3] {
    let (mut r, mut g, mut b, mut w) = (0f64, 0f64, 0f64, 0f64);
    for p in img.pixels() {
        let [pr, pg, pb] = p.0.map(f64::from);
        let max = pr.max(pg).max(pb);
        let min = pr.min(pg).min(pb);
        let sat = if max == 0.0 { 0.0 } else { (max - min) / max };
        let weight = sat * sat * (max / 255.0);
        r += pr * weight;
        g += pg * weight;
        b += pb * weight;
        w += weight;
    }
    if w < 1e-6 {
        return [200, 200, 210];
    }
    [(r / w).round() as u8, (g / w).round() as u8, (b / w).round() as u8]
}

/// Afasta cada cor do cinza de mesma luminância; o blur deixa a capa lavada, o Cider usa a cor viva.
pub fn saturate(img: &mut RgbImage, amount: f32) {
    for p in img.pixels_mut() {
        let [r, g, b] = p.0.map(f32::from);
        let lum = 0.299 * r + 0.587 * g + 0.114 * b;
        p.0 = [r, g, b].map(|c| (lum + (c - lum) * amount).clamp(0.0, 255.0).round() as u8);
    }
}

/// Nome de arquivo estável para uma URL. Pela URL inteira: capas de playlists do Spotify
/// podem terminar igual ("…/artist/ID/en"), e o último trecho sozinho misturava capas.
fn url_key(url: &str) -> String {
    // FNV-1a de 64 bits: estável entre versões do Rust, ao contrário do DefaultHasher.
    let hash = url.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

/// Lado máximo das miniaturas de grade: os cartões têm ~180–250 px, e cada pixel a mais custa RAM ao decodificar.
const THUMB_MAX: u32 = 240;
pub const THUMB_DIR: &str = "thumbs-240";

/// Pastas das capas baixadas (miniaturas, capas grandes e fundos).
pub fn art_dirs() -> [PathBuf; 2] {
    let cache = crate::config::cache_dir();
    [cache.join(THUMB_DIR), cache.join("images")]
}

/// Arquivo que pede para apagar as capas na próxima abertura: apagar com o app aberto deixaria imagens quebradas na tela.
fn clear_mark() -> PathBuf {
    crate::config::cache_dir().join("clear-art")
}

pub fn request_clear() {
    let _ = std::fs::create_dir_all(crate::config::cache_dir()).and_then(|_| std::fs::write(clear_mark(), ""));
}

pub fn clear_requested() -> bool {
    clear_mark().exists()
}

pub(crate) fn clear_if_requested() {
    if clear_requested() {
        for dir in art_dirs() {
            let _ = std::fs::remove_dir_all(dir);
        }
        let _ = std::fs::remove_file(clear_mark());
    }
}

/// Capa de playlist para o Spotify: quadrada (cortada no centro), JPEG em base64 abaixo de 256 KB.
pub fn playlist_cover(path: &Path) -> Result<String, String> {
    use base64::Engine;
    let img = open(path).map_err(|e| format!("Não deu para abrir a imagem: {e}"))?;
    let square = img.resize_to_fill(640, 640, image::imageops::FilterType::Lanczos3).to_rgb8();
    for quality in [90, 80, 65, 50, 35] {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality).encode_image(&square).map_err(|e| e.to_string())?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&jpeg);
        if encoded.len() <= 256 * 1024 {
            return Ok(encoded);
        }
    }
    Err("A imagem ficou grande demais mesmo comprimida.".into())
}

/// Tamanho de uma pasta e do que tem dentro, em bytes.
pub fn dir_size(path: &Path) -> u64 {
    std::fs::read_dir(path).map_or(0, |entries| {
        entries.flatten().map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map_or(0, |m| m.len()),
        }).sum()
    })
}
/// Abre pelo conteúdo, não pela extensão: o Spotify serve algumas capas (mosaicos de playlist) em WebP
/// num endereço sem extensão, e elas ficam em disco como `.jpg`.
fn open(path: &Path) -> Result<image::DynamicImage, String> {
    image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())
}

/// Versão para listas (sidebar), onde a capa tem 28 px.
const THUMB_SMALL: u32 = 64;

/// Miniatura de grade e versão pequena para listas, em disco.
pub async fn thumb(http: &reqwest::Client, cache_dir: &Path, url: &str) -> Result<(PathBuf, PathBuf), String> {
    // "thumbs-240": miniaturas de tamanhos antigos ficam em outra pasta, que o engine apaga.
    let dir = cache_dir.join(THUMB_DIR);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let key = url_key(url);
    let (path, small) = (dir.join(format!("{key}.jpg")), dir.join(format!("{key}-s.jpg")));
    if small.exists() {
        return Ok((path, small));
    }
    download(http, url, &path).await?;
    let (p, s) = (path.clone(), small.clone());
    let shrunk = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let img = open(&p)?;
        if img.width() > THUMB_MAX + 40 {
            img.resize(THUMB_MAX, THUMB_MAX, image::imageops::FilterType::Triangle).to_rgb8().save(&p).map_err(|e| e.to_string())?;
        }
        img.resize(THUMB_SMALL, THUMB_SMALL, image::imageops::FilterType::Triangle).to_rgb8().save(&s).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;
    // Formato que o `image` não abre (o GPUI abre mais formatos): usa a original nas listas também.
    if let Err(e) = shrunk {
        log::warn!("miniatura pequena de {url}: {e}");
        return Ok((path.clone(), path));
    }
    Ok((path, small))
}

/// Vídeo do Canvas em disco (fica junto das capas e sai com elas no "Limpar capas").
pub async fn canvas(http: &reqwest::Client, cache_dir: &Path, url: &str) -> Result<PathBuf, String> {
    let dir = cache_dir.join(THUMB_DIR);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.mp4", url_key(url)));
    if !path.exists() {
        download(http, url, &path).await?;
    }
    Ok(path)
}

/// Imagem no tamanho original (a foto grande do artista), sem reduzir.
pub async fn original(http: &reqwest::Client, cache_dir: &Path, url: &str) -> Result<PathBuf, String> {
    let dir = cache_dir.join(THUMB_DIR);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}-o.jpg", url_key(url)));
    download(http, url, &path).await?;
    Ok(path)
}

/// Fundo borrado das páginas de álbum e playlist, a partir da miniatura pequena (64 px): a GPU estica.
pub async fn backdrop(http: &reqwest::Client, cache_dir: &Path, url: &str) -> Result<PathBuf, String> {
    let (_, small) = thumb(http, cache_dir, url).await?;
    let bg = small.with_file_name(format!("{}-bg.png", url_key(url)));
    if bg.exists() {
        return Ok(bg);
    }
    let out = bg.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let img = open(&small)?;
        let mut blurred = img.resize_exact(128, 128, image::imageops::FilterType::Triangle).blur(8.0).to_rgb8();
        saturate(&mut blurred, 1.4);
        blurred.save(&out).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(bg)
}

pub async fn prepare(http: &reqwest::Client, cache_dir: &Path, t: &Track) -> Result<Art, String> {
    let dir = cache_dir.join("images");
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let small = dir.join(format!("{}-64.jpg", t.album_id));
    let cover = dir.join(format!("{}-640.jpg", t.album_id));
    let background = dir.join(format!("{}-bg2.png", t.album_id));
    let (small_url, large_url) = match (&t.cover_small, &t.cover_large) {
        (Some(s), Some(l)) => (s, l),
        _ => return Err("faixa sem capa".into()),
    };
    download(http, small_url, &small).await?;
    download(http, large_url, &cover).await?;

    let (bg, small) = (background.clone(), small.clone());
    let accent = tokio::task::spawn_blocking(move || -> Result<[u8; 3], String> {
        let img = open(&small)?;
        if !bg.exists() {
            // Borra a capa pequena; a GPU estica para a janela inteira.
            let (w, h) = img.dimensions();
            let mut blurred = img.resize_exact(w * 2, h * 2, image::imageops::FilterType::Triangle).blur(8.0).to_rgb8();
            saturate(&mut blurred, 1.5);
            blurred.save(&bg).map_err(|e| e.to_string())?;
        }
        Ok(accent_color(&img.to_rgb8()))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Art { cover, background, accent })
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    #[test]
    fn accent_prefers_saturated_pixels() {
        let mut img = RgbImage::from_pixel(10, 10, Rgb([128, 128, 128]));
        for x in 0..3 {
            img.put_pixel(x, 0, Rgb([220, 30, 40]));
        }
        assert_eq!(super::accent_color(&img), [220, 30, 40]);
        assert_eq!(super::accent_color(&RgbImage::from_pixel(4, 4, Rgb([0, 0, 0]))), [200, 200, 210]);
    }

    #[test]
    fn urls_with_same_tail_get_different_files() {
        let a = super::url_key("https://seeded-session-images.scdn.co/v2/img/122/secondary/artist/AAA/en");
        let b = super::url_key("https://seeded-session-images.scdn.co/v2/img/122/secondary/artist/BBB/en");
        assert_ne!(a, b);
        assert_eq!(a, super::url_key("https://seeded-session-images.scdn.co/v2/img/122/secondary/artist/AAA/en"));
    }

    #[test]
    fn saturate_keeps_grays_and_pushes_colors() {
        let mut img = RgbImage::from_vec(2, 1, vec![100, 100, 100, 150, 100, 100]).unwrap();
        super::saturate(&mut img, 1.5);
        assert_eq!(img.get_pixel(0, 0).0, [100, 100, 100]);
        let [r, g, _] = img.get_pixel(1, 0).0;
        assert!(r > 150 && g < 100);
    }
}
