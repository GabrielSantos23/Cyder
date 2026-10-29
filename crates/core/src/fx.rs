//! Efeitos na saída de áudio: equalizador de 6 faixas e a transição suave entre músicas.
//! O app muda os ajustes (`set`); a saída de áudio (`sink.rs`) aplica em cada pedaço que toca.
use std::sync::{Mutex, atomic::{AtomicU64, Ordering}};
use std::time::Instant;

use librespot::playback::SAMPLE_RATE;

/// Centro de cada faixa do equalizador, em Hz. A primeira e a última são prateleiras (graves e agudos).
pub const BANDS: [f32; 6] = [60., 230., 910., 3600., 8000., 14000.];
pub const GAIN_MAX: f32 = 12.;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settings {
    /// Ganho de cada faixa em dB; None com o equalizador desligado.
    pub eq: Option<[f32; 6]>,
    /// Duração do fade no fim e no começo de cada música, em ms; 0 desliga.
    pub fade_ms: u32,
}

static SETTINGS: Mutex<Settings> = Mutex::new(Settings { eq: None, fade_ms: 0 });
/// Muda a cada `set`: a saída refaz os filtros só quando mudou.
static VERSION: AtomicU64 = AtomicU64::new(1);

pub fn set(s: Settings) {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = s;
    VERSION.fetch_add(1, Ordering::Relaxed);
}

pub fn get() -> Settings {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Volume do fade: vai de `from` a `to` em `ms`, a partir de `start`.
#[derive(Clone, Copy)]
struct Fade {
    start: Instant,
    ms: u32,
    from: f32,
    to: f32,
}

static FADE: Mutex<Option<Fade>> = Mutex::new(None);

/// O player pede o fade: abaixar até o fim da música ou subir no começo da próxima.
pub fn fade(from: f32, to: f32, ms: u32) {
    *FADE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Fade { start: Instant::now(), ms: ms.max(1), from, to });
}

/// Volta ao volume normal na hora (pausa, seek, outra música escolhida).
pub fn cancel_fade() {
    *FADE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn fade_gain() -> f32 {
    let mut slot = FADE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(f) = *slot else { return 1. };
    let t = (f.start.elapsed().as_millis() as f32 / f.ms as f32).min(1.);
    let gain = f.from + (f.to - f.from) * t;
    // Terminou de subir: não precisa mais calcular.
    if t >= 1. && f.to >= 1. {
        *slot = None;
    }
    gain
}

/// Biquad (fórmulas do "Audio EQ Cookbook" de Robert Bristow-Johnson).
#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    /// Estado de cada canal: x1, x2, y1, y2.
    z: [[f64; 4]; 2],
}

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    LowShelf,
    Peak,
    HighShelf,
}

impl Biquad {
    fn new(shape: Shape, freq: f32, gain_db: f32) -> Self {
        let a = 10f64.powf(gain_db as f64 / 40.);
        let w = 2. * std::f64::consts::PI * freq as f64 / SAMPLE_RATE as f64;
        let (sin, cos) = w.sin_cos();
        let q = 1.0;
        let alpha = sin / (2. * q);
        let (b0, b1, b2, a0, a1, a2) = match shape {
            Shape::Peak => (1. + alpha * a, -2. * cos, 1. - alpha * a, 1. + alpha / a, -2. * cos, 1. - alpha / a),
            Shape::LowShelf => {
                let s = 2. * a.sqrt() * alpha;
                (a * ((a + 1.) - (a - 1.) * cos + s), 2. * a * ((a - 1.) - (a + 1.) * cos), a * ((a + 1.) - (a - 1.) * cos - s),
                 (a + 1.) + (a - 1.) * cos + s, -2. * ((a - 1.) + (a + 1.) * cos), (a + 1.) + (a - 1.) * cos - s)
            }
            Shape::HighShelf => {
                let s = 2. * a.sqrt() * alpha;
                (a * ((a + 1.) + (a - 1.) * cos + s), -2. * a * ((a - 1.) + (a + 1.) * cos), a * ((a + 1.) + (a - 1.) * cos - s),
                 (a + 1.) - (a - 1.) * cos + s, 2. * ((a - 1.) - (a + 1.) * cos), (a + 1.) - (a - 1.) * cos - s)
            }
        };
        Biquad { b: [b0 / a0, b1 / a0, b2 / a0], a: [a1 / a0, a2 / a0], z: [[0.; 4]; 2] }
    }

    fn run(&mut self, ch: usize, x: f64) -> f64 {
        let z = &mut self.z[ch];
        let y = self.b[0] * x + self.b[1] * z[0] + self.b[2] * z[1] - self.a[0] * z[2] - self.a[1] * z[3];
        *z = [x, z[0], y, z[2]];
        y
    }
}

/// O que a saída de áudio guarda entre um pedaço e outro.
#[derive(Default)]
pub struct Processor {
    version: u64,
    filters: Vec<Biquad>,
    /// Abaixa tudo pelo maior reforço, para o equalizador não estourar o som.
    preamp: f64,
}

impl Processor {
    /// Aplica o equalizador e o fade, no lugar. `samples` vem intercalado (esquerdo, direito).
    pub fn process(&mut self, samples: &mut [f64], channels: usize) {
        let version = VERSION.load(Ordering::Relaxed);
        if version != self.version {
            self.version = version;
            let eq = get().eq.filter(|g| g.iter().any(|v| v.abs() > 0.05));
            self.filters = eq.map_or_else(Vec::new, |gains| {
                BANDS.iter().zip(gains).enumerate().map(|(i, (&f, g))| {
                    let shape = if i == 0 { Shape::LowShelf } else if i == BANDS.len() - 1 { Shape::HighShelf } else { Shape::Peak };
                    Biquad::new(shape, f, g.clamp(-GAIN_MAX, GAIN_MAX))
                }).collect()
            });
            let boost = eq.map_or(0., |g| g.iter().copied().fold(0f32, f32::max));
            self.preamp = 10f64.powf(-boost as f64 / 20.);
        }
        let gain = fade_gain() as f64;
        if self.filters.is_empty() && gain >= 1. {
            return;
        }
        for (i, s) in samples.iter_mut().enumerate() {
            let ch = (i % channels).min(1);
            let mut x = *s;
            if !self.filters.is_empty() {
                x *= self.preamp;
                for f in &mut self.filters {
                    x = f.run(ch, x);
                }
            }
            *s = x * gain;
        }
    }
}

/// Ajustes prontos do equalizador: nome e ganhos.
pub const PRESETS: [(&str, [f32; 6]); 6] = [
    ("Plano", [0., 0., 0., 0., 0., 0.]),
    ("Graves", [6., 4., 0., 0., 0., 0.]),
    ("Vocal", [-2., -1., 3., 4., 2., 0.]),
    ("Agudos", [0., 0., 0., 2., 4., 6.]),
    ("Rock", [4., 2., -2., 1., 3., 4.]),
    ("Eletrônica", [5., 3., 0., -1., 2., 4.]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_peak_passes_through() {
        let mut f = Biquad::new(Shape::Peak, 1000., 0.);
        for x in [0.5, -0.25, 0.75] {
            assert!((f.run(0, x) - x).abs() < 1e-9);
        }
    }

    #[test]
    fn low_shelf_boosts_dc() {
        // Sinal constante (frequência 0) sai com o ganho da prateleira: +6 dB ≈ ×2.
        let mut f = Biquad::new(Shape::LowShelf, 60., 6.);
        let mut y = 0.;
        for _ in 0..20_000 {
            y = f.run(0, 1.);
        }
        assert!((y - 10f64.powf(6. / 20.)).abs() < 0.01, "{y}");
    }
}
