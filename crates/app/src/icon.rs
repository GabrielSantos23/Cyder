// Desenho do ícone do Cyder, sem dependências: a bandeja usa em tempo de execução e o build.rs inclui
// este arquivo (`include!`) para gravar o .ico do executável (barra de tarefas, Alt+Tab, Explorer).

/// Quadrado branco arredondado com o triângulo de play, como o ícone do Sobre. RGBA, linha a linha.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let r = s * 0.22;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Distância até o retângulo arredondado (negativa dentro).
            let qx = (fx - s / 2.).abs() - (s / 2. - r);
            let qy = (fy - s / 2.).abs() - (s / 2. - r);
            let d = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - r;
            let alpha = (0.5 - d).clamp(0., 1.);
            // Triângulo apontando para a direita, um pouco para a direita do centro.
            let (ax, top, bottom, tip) = (s * 0.38, s * 0.28, s * 0.72, s * 0.74);
            let half = (bottom - top) / 2. * (1. - (fx - ax) / (tip - ax));
            let in_play = fx >= ax && fx <= tip && (fy - s / 2.).abs() <= half;
            let v = if in_play { 0 } else { 255 };
            out.extend([v, v, v, (alpha * 255.) as u8]);
        }
    }
    out
}
