//! Canvas: o vídeo curto em loop que o Spotify mostra em algumas músicas, como fundo do modo imersivo.
//! O GPUI não toca vídeo: uma thread lê o MP4 (`mp4`), decodifica o H.264 (`openh264`) no ritmo do vídeo
//! e entrega cada quadro como imagem; a tela troca de imagem a cada quadro novo.
use std::{
    io::BufReader,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}},
    time::{Duration, Instant},
};

use gpui::{Context, RenderImage, Window};

use super::Cyder;

/// Os quadros saem com metade da largura e da altura: o fundo é borrado e escurecido, e assim cada um
/// ocupa ~1 MB em vez de ~4 MB.
const SCALE: usize = 2;
/// Quadro novo por aqui: a tela confere a esta frequência.
const POLL: Duration = Duration::from_millis(33);

pub(super) struct Canvas {
    pub uri: String,
    frame: Arc<Mutex<Option<Arc<RenderImage>>>>,
    count: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    /// Último quadro mostrado, para tirar da memória de vídeo quando o próximo entra.
    shown: Option<Arc<RenderImage>>,
    seen: u64,
}

impl Drop for Canvas {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// AVCC (cada NAL com 4 bytes de tamanho, como no MP4) para Annex B (cada NAL com 00 00 00 01), que o openh264 lê.
fn annex_b(sample: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i + 4 <= sample.len() {
        let len = u32::from_be_bytes([sample[i], sample[i + 1], sample[i + 2], sample[i + 3]]) as usize;
        i += 4;
        let end = (i + len).min(sample.len());
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&sample[i..end]);
        i = end;
    }
}

/// RGBA → BGRA (a ordem do GPUI), pegando um pixel a cada `SCALE`.
fn downscale_bgra(rgba: &[u8], w: usize, h: usize) -> image::RgbaImage {
    let (ow, oh) = ((w / SCALE).max(1), (h / SCALE).max(1));
    let mut out = image::RgbaImage::new(ow as u32, oh as u32);
    for (y, row) in out.chunks_exact_mut(ow * 4).enumerate() {
        for (x, px) in row.chunks_exact_mut(4).enumerate() {
            let s = ((y * SCALE) * w + x * SCALE) * 4;
            px.copy_from_slice(&[rgba[s + 2], rgba[s + 1], rgba[s], 255]);
        }
    }
    out
}

/// Toca o vídeo em loop até `stop`. Erros só vão para o log: sem Canvas, fica a capa borrada.
fn play(path: PathBuf, frame: Arc<Mutex<Option<Arc<RenderImage>>>>, count: Arc<AtomicU64>, stop: Arc<AtomicBool>) -> Result<(), String> {
    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let mut mp4 = mp4::Mp4Reader::read_header(BufReader::new(file), size).map_err(|e| e.to_string())?;
    let (track_id, timescale, header) = {
        let track = mp4.tracks().values().find(|t| matches!(t.track_type(), Ok(mp4::TrackType::Video))).ok_or("sem vídeo")?;
        let mut header = Vec::new();
        for ps in [track.sequence_parameter_set().map_err(|e| e.to_string())?, track.picture_parameter_set().map_err(|e| e.to_string())?] {
            header.extend_from_slice(&[0, 0, 0, 1]);
            header.extend_from_slice(ps);
        }
        (track.track_id(), track.timescale().max(1), header)
    };
    let samples = mp4.sample_count(track_id).map_err(|e| e.to_string())?;
    let mut rgba = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        // Cada volta recomeça o decodificador: o loop do Canvas volta a um quadro-chave.
        let mut decoder = openh264::decoder::Decoder::new().map_err(|e| e.to_string())?;
        let mut first = true;
        for id in 1..=samples {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let started = Instant::now();
            let Some(sample) = mp4.read_sample(track_id, id).map_err(|e| e.to_string())? else { continue };
            let mut nal = if first { header.clone() } else { Vec::new() };
            first = false;
            annex_b(&sample.bytes, &mut nal);
            if let Some(yuv) = decoder.decode(&nal).map_err(|e| e.to_string())? {
                use openh264::formats::YUVSource;
                let (w, h) = yuv.dimensions();
                rgba.resize(w * h * 4, 0);
                yuv.write_rgba8(&mut rgba);
                let image = Arc::new(RenderImage::new(vec![image::Frame::new(downscale_bgra(&rgba, w, h))]));
                *frame.lock().unwrap_or_else(|e| e.into_inner()) = Some(image);
                count.fetch_add(1, Ordering::Relaxed);
            }
            let wait = Duration::from_secs_f64(sample.duration as f64 / timescale as f64);
            std::thread::sleep(wait.saturating_sub(started.elapsed()));
        }
    }
    Ok(())
}

impl Cyder {
    /// No imersivo, pede o Canvas da faixa atual (uma vez por faixa) e para o vídeo ao sair.
    pub(super) fn sync_canvas(&mut self) {
        let want = self.settings.canvas && self.immersive;
        let uri = self.tracks.get(self.current).map(|t| t.uri.clone()).filter(|u| want && u.starts_with("spotify:track:"));
        if self.canvas.as_ref().is_some_and(|c| Some(&c.uri) != uri.as_ref()) {
            self.canvas = None;
        }
        let Some(uri) = uri else { return };
        match self.canvas_files.get(&uri).cloned() {
            None => {
                // Pedido uma vez: a resposta (com ou sem vídeo) entra em `canvas_files`.
                self.canvas_files.insert(uri.clone(), None);
                self.send(cyder_core::Cmd::Canvas(uri));
            }
            Some(Some(path)) if self.canvas.is_none() => self.start_canvas(uri, path),
            Some(_) => {}
        }
    }

    fn start_canvas(&mut self, uri: String, path: PathBuf) {
        let (frame, count, stop) = (Arc::new(Mutex::new(None)), Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        let (f, c, s) = (frame.clone(), count.clone(), stop.clone());
        let name = uri.clone();
        let spawned = std::thread::Builder::new().name("canvas".into()).spawn(move || {
            if let Err(e) = play(path, f, c, s) {
                log::warn!("canvas de {name}: {e}");
            }
        });
        if spawned.is_ok() {
            self.canvas = Some(Canvas { uri, frame, count, stop, shown: None, seen: 0 });
        }
    }

    /// Quadro atual do vídeo; tira da memória de vídeo o anterior.
    pub(super) fn canvas_frame(&mut self, window: &mut Window) -> Option<Arc<RenderImage>> {
        let c = self.canvas.as_mut()?;
        let current = c.frame.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
        if c.shown.as_ref().is_some_and(|s| !Arc::ptr_eq(s, &current)) {
            if let Some(old) = c.shown.take() {
                let _ = window.drop_image(old);
            }
        }
        c.shown = Some(current.clone());
        Some(current)
    }

    /// Enquanto o vídeo toca, repinta quando chega um quadro novo.
    pub(super) fn watch_canvas(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let alive = this.update(cx, |s, cx| {
                    if let Some(c) = s.canvas.as_mut() {
                        let n = c.count.load(Ordering::Relaxed);
                        if n != c.seen {
                            c.seen = n;
                            cx.notify();
                        }
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn avcc_to_annex_b() {
        let mut out = Vec::new();
        super::annex_b(&[0, 0, 0, 2, 0x65, 0xaa, 0, 0, 0, 1, 0x41], &mut out);
        assert_eq!(out, [0, 0, 0, 1, 0x65, 0xaa, 0, 0, 0, 1, 0x41]);
    }

    #[test]
    fn halves_and_swaps_to_bgra() {
        // 2×2 vermelho vira 1×1 com os canais na ordem BGRA.
        let rgba = [255, 0, 0, 255].repeat(4);
        let out = super::downscale_bgra(&rgba, 2, 2);
        assert_eq!(out.as_raw().as_slice(), [0, 0, 255, 255]);
    }
}
