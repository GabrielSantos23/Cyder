// Grava o ícone do Cyder num .ico e o embute no executável do Windows (recurso 1, que o GPUI também usa
// como ícone da janela). O desenho é o mesmo da bandeja.
include!("src/icon.rs");

/// .ico com imagens BMP de 32 bits (com alfa), uma por tamanho.
fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| {
        let rgba = icon_rgba(s);
        let mask_row = (s as usize).div_ceil(32) * 4;
        let mut bmp = Vec::new();
        // BITMAPINFOHEADER: a altura conta duas vezes (cor + máscara).
        for v in [40u32, s, s * 2] {
            bmp.extend(v.to_le_bytes());
        }
        bmp.extend(1u16.to_le_bytes());
        bmp.extend(32u16.to_le_bytes());
        bmp.extend([0u8; 24]);
        // De baixo para cima, em BGRA.
        for y in (0..s as usize).rev() {
            for px in rgba[y * s as usize * 4..(y + 1) * s as usize * 4].chunks_exact(4) {
                bmp.extend([px[2], px[1], px[0], px[3]]);
            }
        }
        // Máscara AND vazia: a transparência vem do alfa.
        bmp.extend(vec![0u8; mask_row * s as usize]);
        bmp
    }).collect();
    let mut out = Vec::new();
    out.extend([0u16, 1, sizes.len() as u16].iter().flat_map(|v| v.to_le_bytes()));
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&s, img) in sizes.iter().zip(&images) {
        // 256 é gravado como 0.
        let b = if s >= 256 { 0 } else { s as u8 };
        out.extend([b, b, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((img.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend(img);
    }
    out
}

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("cyder.ico");
    std::fs::write(&path, ico(&[16, 20, 24, 32, 40, 48, 64, 256])).unwrap();
    let mut res = winresource::WindowsResource::new();
    res.set_icon(path.to_str().unwrap()).set("FileDescription", "Cyder").set("ProductName", "Cyder");
    // Sem o compilador de recursos (rc.exe do Windows SDK), o app compila do mesmo jeito, só sem ícone.
    if let Err(e) = res.compile() {
        println!("cargo:warning=ícone do executável não embutido: {e}");
    }
}
