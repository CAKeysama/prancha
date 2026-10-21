//! Gera quadrinhos de teste próprios (desenhos geométricos, sem conteúdo comercial)
//! para os testes E2E e as capturas de tela.
//!
//! Uso: cargo run --example make_fixtures -- <pasta-de-saída>

#[path = "../tests/common/mod.rs"]
mod common;

use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

const W: u32 = 720;
const H: u32 = 1080;

fn palette(seed: u32) -> [Rgb<u8>; 4] {
    let sets = [
        [[238, 108, 77], [41, 50, 65], [152, 193, 217], [224, 251, 252]],
        [[42, 157, 143], [233, 196, 106], [244, 162, 97], [231, 111, 81]],
        [[106, 76, 147], [255, 202, 58], [138, 201, 38], [25, 130, 196]],
        [[220, 47, 2], [255, 186, 8], [3, 7, 30], [232, 93, 4]],
        [[0, 95, 115], [10, 147, 150], [148, 210, 189], [233, 216, 166]],
    ];
    let s = sets[(seed as usize) % sets.len()];
    s.map(|c| Rgb([c[0], c[1], c[2]]))
}

fn rect(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, c: Rgb<u8>) {
    for yy in y..(y + h).min(img.height()) {
        for xx in x..(x + w).min(img.width()) {
            img.put_pixel(xx, yy, c);
        }
    }
}

fn circle(img: &mut RgbImage, cx: i32, cy: i32, r: i32, c: Rgb<u8>) {
    for y in (cy - r).max(0)..(cy + r).min(img.height() as i32) {
        for x in (cx - r).max(0)..(cx + r).min(img.width() as i32) {
            if (x - cx).pow(2) + (y - cy).pow(2) <= r * r {
                img.put_pixel(x as u32, y as u32, c);
            }
        }
    }
}

/// Dígito em sete segmentos (para numerar páginas sem depender de fontes).
fn digit(img: &mut RgbImage, d: u32, x: u32, y: u32, s: u32, c: Rgb<u8>) {
    const SEG: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
    let m = SEG[d as usize % 10];
    let t = s / 5;
    let segs = [
        (x + t, y, s, t),
        (x + s + t, y + t, t, s),
        (x + s + t, y + s + 2 * t, t, s),
        (x + t, y + 2 * s + 2 * t, s, t),
        (x, y + s + 2 * t, t, s),
        (x, y + t, t, s),
        (x + t, y + s + t, s, t),
    ];
    for (i, (sx, sy, sw, sh)) in segs.iter().enumerate() {
        if m & (1 << i) != 0 {
            rect(img, *sx, *sy, *sw, *sh, c);
        }
    }
}

fn number(img: &mut RgbImage, n: u32, x: u32, y: u32, s: u32, c: Rgb<u8>) {
    let text = n.to_string();
    for (i, ch) in text.chars().enumerate() {
        digit(img, ch.to_digit(10).unwrap(), x + i as u32 * (s + s / 2 + 4), y, s, c);
    }
}

fn page(seed: u32, n: u32) -> RgbImage {
    let p = palette(seed);
    let mut img = RgbImage::from_pixel(W, H, Rgb([250, 248, 242]));
    let m = 36;
    let gutter = 18;
    let layout = (seed + n) % 3;
    let panels: Vec<(u32, u32, u32, u32)> = match layout {
        0 => vec![(m, m, W - 2 * m, 400), (m, m + 400 + gutter, (W - 2 * m - gutter) / 2, 500), (m + (W - 2 * m + gutter) / 2, m + 400 + gutter, (W - 2 * m - gutter) / 2, 500)],
        1 => vec![(m, m, (W - 2 * m - gutter) / 2, 920), (m + (W - 2 * m + gutter) / 2, m, (W - 2 * m - gutter) / 2, 450), (m + (W - 2 * m + gutter) / 2, m + 470, (W - 2 * m - gutter) / 2, 450)],
        _ => vec![(m, m, W - 2 * m, 290), (m, m + 308, W - 2 * m, 290), (m, m + 616, W - 2 * m, 304)],
    };
    for (i, (x, y, w, h)) in panels.iter().enumerate() {
        rect(&mut img, *x, *y, *w, *h, Rgb([20, 20, 24]));
        let fill = p[(i + n as usize) % 4];
        rect(&mut img, x + 6, y + 6, w - 12, h - 12, fill);
        let r = ((*w).min(*h) / 4) as i32;
        circle(&mut img, (x + w / 2) as i32, (y + h / 2) as i32, r, p[(i + 1 + n as usize) % 4]);
        rect(&mut img, x + 24, y + h - 60, w / 3, 26, Rgb([250, 248, 242]));
    }
    number(&mut img, n, W / 2 - 30, H - 100, 30, Rgb([20, 20, 24]));
    img
}

fn cover(seed: u32, issue: u32) -> RgbImage {
    let p = palette(seed);
    let mut img = RgbImage::from_pixel(W, H, p[0]);
    rect(&mut img, 0, 0, W, 210, p[2]);
    rect(&mut img, 0, 200, W, 14, Rgb([20, 20, 24]));
    circle(&mut img, (W / 2) as i32, 620, 260, p[1]);
    circle(&mut img, (W / 2) as i32, 620, 170, p[3]);
    rect(&mut img, 40, 40, 150, 130, Rgb([250, 248, 242]));
    number(&mut img, issue, 70, 55, 28, Rgb([20, 20, 24]));
    img
}

fn encode(img: RgbImage, fmt: ImageFormat) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img).write_to(&mut out, fmt).unwrap();
    out.into_inner()
}

fn comic_pages(seed: u32, issue: u32, count: u32, fmt: ImageFormat, ext: &str) -> Vec<(&'static str, Vec<u8>)> {
    let mut v: Vec<(&'static str, Vec<u8>)> = Vec::new();
    for i in 0..count {
        let name: &'static str = Box::leak(format!("{:03}.{ext}", i + 1).into_boxed_str());
        let img = if i == 0 { cover(seed, issue) } else { page(seed, i + 1) };
        v.push((name, encode(img, fmt)));
    }
    v
}

fn main() {
    let out = std::env::args().nth(1).expect("informe a pasta de saída");
    let out = Path::new(&out);
    let lib = out.join("biblioteca");
    let errors = out.join("erros");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::create_dir_all(&errors).unwrap();

    let info = r#"<?xml version="1.0"?><ComicInfo><Title>O Farol de Papel</Title><Series>Farol</Series><Number>1</Number>
<Year>2024</Year><Writer>Equipe Prancha</Writer><Penciller>Ilustração Geométrica</Penciller><Tags>teste, aventura</Tags>
<Summary>HQ de teste gerada pelo Prancha para validar a leitura.</Summary><Pages><Page Image="0" Type="FrontCover"/></Pages></ComicInfo>"#;
    let mut farol = comic_pages(0, 1, 14, ImageFormat::Jpeg, "jpg");
    farol.push(("ComicInfo.xml", info.as_bytes().to_vec()));
    common::make_cbz(&lib.join("O Farol de Papel 01.cbz"), &farol);
    common::make_rar5(&lib.join("Rio Vermelho 02.cbr"), &comic_pages(1, 2, 10, ImageFormat::Jpeg, "jpg"));
    common::make_cb7(&lib.join("Cidade Nublada.cb7"), &comic_pages(2, 3, 8, ImageFormat::Png, "png"));
    common::make_cbt(&lib.join("Tiras de Domingo.cbt"), &comic_pages(3, 4, 6, ImageFormat::Jpeg, "jpg"));

    // Pasta com nomes sem zeros à esquerda para validar a ordenação natural (1, 2, ..., 10).
    let folder = lib.join("Caderno de Esboços");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 1..=10u32 {
        let img = if i == 1 { cover(4, 5) } else { page(4, i) };
        std::fs::write(folder.join(format!("{i}.jpg")), encode(img, ImageFormat::Jpeg)).unwrap();
    }
    write_pdf(&lib.join("Manual Ilustrado.pdf"), 5);

    // Item cujas imagens não decodificam: entra com capa substituta.
    common::make_cbz(&lib.join("Sem Capa Legível.cbz"), &[("001.jpg", b"isto nao e jpeg".to_vec()), ("002.jpg", b"nem isto".to_vec())]);

    // Casos de erro.
    std::fs::write(errors.join("arquivo-corrompido.cbz"), b"PK\x03\x04 conteudo truncado").unwrap();
    std::fs::write(errors.join("anotacoes.txt"), b"nao e quadrinho").unwrap();
    std::fs::write(errors.join("vazio.cbr"), b"").unwrap();
    std::fs::write(errors.join("livro-digital.epub"), b"PK epub de teste").unwrap();
    println!("fixtures em {}", out.display());
}

/// PDF com desenhos vetoriais (para o PDF.js ter o que renderizar).
fn write_pdf(path: &Path, pages: usize) {
    let mut objects: Vec<String> = vec!["<< /Type /Catalog /Pages 2 0 R >>".into()];
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i * 2)).collect();
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {pages} >>", kids.join(" ")));
    for i in 0..pages {
        let content_id = 4 + i * 2;
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 420 630] /Contents {content_id} 0 R >>"));
        let r = [0.93, 0.16, 0.55, 0.2, 0.85][i % 5];
        let stream = format!(
            "q 0.98 0.97 0.95 rg 0 0 420 630 re f Q q 0.08 0.08 0.1 rg 20 330 380 280 re f {r} 0.45 0.6 rg 26 336 368 268 re f \
             0.08 0.08 0.1 rg 20 20 185 290 re f 0.2 0.6 {r} rg 26 26 173 278 re f \
             0.08 0.08 0.1 rg 215 20 185 290 re f 0.95 0.75 0.3 rg 221 26 173 278 re f Q"
        );
        objects.push(format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()));
    }
    let info_id = objects.len() + 1;
    objects.push("<< /Title (Manual Ilustrado) /Author (Equipe Prancha) >>".into());
    let mut body = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(body.len());
        body.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, obj));
    }
    let xref = body.len();
    body.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for off in offsets {
        body.push_str(&format!("{off:010} 00000 n \n"));
    }
    body.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R /Info {info_id} 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
    std::fs::write(path, body).unwrap();
}
