//! Roda a detecção da Leitura Guiada em imagens soltas e desenha o resultado, para conferência visual.
//!
//! Uso: cargo run --release --example guided_eval -- <pasta-de-saída> [--rtl] <imagem>...
//! Os arquivos gerados ficam fora do repositório; nada é gravado no banco.

use std::path::Path;
use std::time::Instant;

use image::imageops::FilterType;
use image::{Rgb, RgbImage};
use prancha_lib::guided::order::reading_sequence;
use prancha_lib::guided::{Rect, analyze};

const OUTPUT_HEIGHT: u32 = 1100;

fn outline(img: &mut RgbImage, r: &Rect, color: Rgb<u8>, thickness: u32) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x0 = (r.x * w) as u32;
    let y0 = (r.y * h) as u32;
    let x1 = ((r.right() * w) as u32).min(img.width() - 1);
    let y1 = ((r.bottom() * h) as u32).min(img.height() - 1);
    for t in 0..thickness {
        for x in x0..=x1 {
            img.put_pixel(x, (y0 + t).min(y1), color);
            img.put_pixel(x, y1.saturating_sub(t).max(y0), color);
        }
        for y in y0..=y1 {
            img.put_pixel((x0 + t).min(x1), y, color);
            img.put_pixel(x1.saturating_sub(t).max(x0), y, color);
        }
    }
}

/// Dígitos em sete segmentos, sobre fundo escuro, para numerar a ordem.
fn number(img: &mut RgbImage, n: usize, x: u32, y: u32) {
    const SEG: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
    let text = n.to_string();
    let (s, t) = (9u32, 2u32);
    let width = text.len() as u32 * (s + t * 2 + 3) + 4;
    let height = s * 2 + t * 3 + 4;
    let mut rect = |x: u32, y: u32, w: u32, h: u32, c: Rgb<u8>| {
        for yy in y..y + h {
            for xx in x..x + w {
                if xx < img.width() && yy < img.height() {
                    img.put_pixel(xx, yy, c);
                }
            }
        }
    };
    rect(x, y, width, height, Rgb([180, 0, 40]));
    for (i, ch) in text.chars().enumerate() {
        let d = ch.to_digit(10).unwrap() as usize;
        let ox = x + 2 + i as u32 * (s + t * 2 + 3);
        let oy = y + 2;
        let segs = [
            (ox + t, oy, s, t),
            (ox + s + t, oy + t, t, s),
            (ox + s + t, oy + s + 2 * t, t, s),
            (ox + t, oy + 2 * s + 2 * t, s, t),
            (ox, oy + s + 2 * t, t, s),
            (ox, oy + t, t, s),
            (ox + t, oy + s + t, s, t),
        ];
        for (k, (sx, sy, sw, sh)) in segs.iter().enumerate() {
            if SEG[d] & (1 << k) != 0 {
                rect(*sx, *sy, *sw, *sh, Rgb([255, 255, 255]));
            }
        }
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = args.remove(0);
    let rtl = args.first().is_some_and(|a| a == "--rtl");
    if rtl {
        args.remove(0);
    }
    std::fs::create_dir_all(&out_dir).unwrap();
    for path in &args {
        let data = std::fs::read(path).unwrap();
        let start = Instant::now();
        let decoded = image::load_from_memory(&data).unwrap();
        let decode_ms = start.elapsed().as_millis();
        let start = Instant::now();
        let analysis = analyze(&decoded);
        let analyze_ms = start.elapsed().as_millis();
        let name = Path::new(path).file_stem().unwrap().to_string_lossy();
        let parent = Path::new(path).parent().and_then(|p| p.file_name()).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        println!(
            "{parent}/{name}: {}x{} decodificar {decode_ms}ms analisar {analyze_ms}ms quadros {} regiões {}",
            analysis.width,
            analysis.height,
            analysis.panels.len(),
            analysis.regions.len()
        );
        let scale = OUTPUT_HEIGHT as f32 / decoded.height() as f32;
        let mut canvas = decoded.resize_exact((decoded.width() as f32 * scale) as u32, OUTPUT_HEIGHT, FilterType::Triangle).to_rgb8();
        for p in &analysis.panels {
            outline(&mut canvas, p, Rgb([40, 200, 255]), 2);
        }
        let rects: Vec<Rect> = analysis.regions.iter().map(|r| r.rect).collect();
        for (n, placed) in reading_sequence(&rects, &analysis.panels, rtl).iter().enumerate() {
            let region = &analysis.regions[placed.region];
            println!(
                "  {:>2}. {:<10} conf {:.2} x {:.3} y {:.3} l {:.3} a {:.3}",
                n + 1,
                region.kind.as_str(),
                region.confidence,
                region.rect.x,
                region.rect.y,
                region.rect.w,
                region.rect.h
            );
            let color = if region.confidence < prancha_lib::guided::LOW_CONFIDENCE { Rgb([255, 170, 0]) } else { Rgb([255, 0, 90]) };
            outline(&mut canvas, &region.rect, color, 3);
            let (cw, ch) = (canvas.width() as f32, canvas.height() as f32);
            number(&mut canvas, n + 1, (region.rect.x * cw) as u32, (region.rect.y * ch) as u32);
        }
        canvas.save(Path::new(&out_dir).join(format!("{parent}-{name}.png"))).unwrap();
    }
}
