// Desenho do ícone do Cyder, sem dependências: a bandeja usa em tempo de execução e o build.rs inclui
// este arquivo (`include!`) para gravar o .ico do executável (barra de tarefas, Alt+Tab, Explorer).

/// A marca do Cyder, igual à do Sobre (`view::logo`): quadrado arredondado num gradiente âmbar → coral, com o
/// play e duas bolhas (de sidra) em branco. RGBA, linha a linha, com antisserrilhado 4×4.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let r = s * 0.26;
    // A marca ocupa 62% do quadrado, centralizada; as medidas abaixo são as do SVG (24 × 24).
    let (scale, off) = (s * 0.62 / 24., s * 0.19);
    let at = |x: f32, y: f32| ((x - off) / scale, (y - off) / scale);
    // Play: triângulo com a ponta à direita.
    let (ax, ay, bx, by, cx, cy) = (7.4f32, 7.0f32, 19.0f32, 12.3f32, 7.4f32, 17.6f32);
    let edge = |px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32| (x1 - x0) * (py - y0) - (y1 - y0) * (px - x0);
    let in_play = |px: f32, py: f32| {
        let (e0, e1, e2) = (edge(px, py, ax, ay, bx, by), edge(px, py, bx, by, cx, cy), edge(px, py, cx, cy, ax, ay));
        (e0 >= 0. && e1 >= 0. && e2 >= 0.) || (e0 <= 0. && e1 <= 0. && e2 <= 0.)
    };
    // Bolhas de sidra subindo da ponta do play: duas bolinhas.
    let in_bubbles = |px: f32, py: f32| (px - 16.6).hypot(py - 6.4) <= 1.55 || (px - 19.6).hypot(py - 3.6) <= 1.0;
    let lerp = |a: u8, b: u8, t: f32| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut inside, mut white) = (0u32, 0u32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let (fx, fy) = (x as f32 + (sx as f32 + 0.5) / 4., y as f32 + (sy as f32 + 0.5) / 4.);
                    // Distância até o retângulo arredondado (negativa dentro).
                    let qx = (fx - s / 2.).abs() - (s / 2. - r);
                    let qy = (fy - s / 2.).abs() - (s / 2. - r);
                    let d = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - r;
                    if d <= 0. {
                        inside += 1;
                        let (u, v) = at(fx, fy);
                        if in_play(u, v) || in_bubbles(u, v) {
                            white += 1;
                        }
                    }
                }
            }
            let t = ((x + y) as f32 / (2. * s)).clamp(0., 1.);
            let (bg_r, bg_g, bg_b) = (lerp(0xf2, 0xd9, t), lerp(0x64, 0x2d, t), lerp(0x3d, 0x6b, t));
            let w = if inside == 0 { 0. } else { white as f32 / inside as f32 };
            out.extend([lerp(bg_r, 255, w), lerp(bg_g, 255, w), lerp(bg_b, 255, w), (inside as f32 / 16. * 255.).round() as u8]);
        }
    }
    out
}
