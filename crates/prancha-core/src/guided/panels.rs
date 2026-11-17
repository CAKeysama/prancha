//! Quadros da página: cortes pelas calhas e, onde não há corte reto, ilhas separadas.

use super::Rect;
use super::raster::{PixelBox, Work};

/// Diferença máxima por canal para um pixel contar como calha.
const GUTTER_TOLERANCE: i16 = 28;
/// Fração de calha para uma linha inteira contar como separação (balões podem cruzá-la).
const LINE_FILL: f32 = 0.92;
/// Espessura mínima da calha em fração do lado da página.
const MIN_THICKNESS: f32 = 0.0035;
/// Menor quadro aceito, em fração da área da página.
const MIN_PANEL_AREA: f32 = 0.012;
const MAX_DEPTH: u32 = 8;

struct Mask {
    w: usize,
    h: usize,
    gutter: Vec<bool>,
}

impl Mask {
    fn of(work: &Work, color: [u8; 3]) -> Mask {
        let near = |i: usize| {
            let p = work.pixel(i);
            (0..3).all(|c| (p[c] as i16 - color[c] as i16).abs() <= GUTTER_TOLERANCE)
        };
        Mask { w: work.w, h: work.h, gutter: (0..work.w * work.h).map(near).collect() }
    }

    fn at(&self, x: usize, y: usize) -> bool {
        self.gutter[y * self.w + x]
    }

    fn row_fill(&self, y: usize, b: &PixelBox) -> f32 {
        (b.x0..=b.x1).filter(|x| self.at(*x, y)).count() as f32 / b.width() as f32
    }

    fn col_fill(&self, x: usize, b: &PixelBox) -> f32 {
        (b.y0..=b.y1).filter(|y| self.at(x, *y)).count() as f32 / b.height() as f32
    }

    /// Remove as bordas de calha ao redor do retângulo. `None` se ele for só calha.
    fn trim(&self, b: &PixelBox) -> Option<PixelBox> {
        let mut t = *b;
        while t.y0 <= t.y1 && self.row_fill(t.y0, &t) >= LINE_FILL {
            t.y0 += 1;
        }
        if t.y0 > t.y1 {
            return None;
        }
        while t.y1 > t.y0 && self.row_fill(t.y1, &t) >= LINE_FILL {
            t.y1 -= 1;
        }
        while t.x0 <= t.x1 && self.col_fill(t.x0, &t) >= LINE_FILL {
            t.x0 += 1;
        }
        if t.x0 > t.x1 {
            return None;
        }
        while t.x1 > t.x0 && self.col_fill(t.x1, &t) >= LINE_FILL {
            t.x1 -= 1;
        }
        Some(t)
    }
}

/// Faixas internas de calha: (início, fim) de cada linha ou coluna separadora.
fn gutter_runs(start: usize, end: usize, min: usize, fill: impl Fn(usize) -> f32) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut open: Option<usize> = None;
    for i in start..=end {
        if fill(i) >= LINE_FILL {
            open.get_or_insert(i);
        } else if let Some(s) = open.take() {
            if i - s >= min {
                runs.push((s, i - 1));
            }
        }
    }
    runs
}

fn cut(mask: &Mask, region: PixelBox, depth: u32, leaves: &mut Vec<PixelBox>) {
    let Some(b) = mask.trim(&region) else { return };
    if depth < MAX_DEPTH {
        let min_h = ((mask.h as f32 * MIN_THICKNESS) as usize).max(3);
        let rows = gutter_runs(b.y0, b.y1, min_h, |y| mask.row_fill(y, &b));
        if !rows.is_empty() {
            let mut from = b.y0;
            for (s, e) in rows {
                cut(mask, PixelBox { y0: from, y1: s - 1, ..b }, depth + 1, leaves);
                from = e + 1;
            }
            cut(mask, PixelBox { y0: from, ..b }, depth + 1, leaves);
            return;
        }
        let min_w = ((mask.w as f32 * MIN_THICKNESS) as usize).max(3);
        let cols = gutter_runs(b.x0, b.x1, min_w, |x| mask.col_fill(x, &b));
        if !cols.is_empty() {
            let mut from = b.x0;
            for (s, e) in cols {
                cut(mask, PixelBox { x0: from, x1: s - 1, ..b }, depth + 1, leaves);
                from = e + 1;
            }
            cut(mask, PixelBox { x0: from, ..b }, depth + 1, leaves);
            return;
        }
    }
    leaves.push(b);
}

/// Separa ilhas dentro de um retângulo sem corte reto (calhas inclinadas).
fn islands(mask: &Mask, b: &PixelBox) -> Vec<PixelBox> {
    let (bw, bh) = (b.width(), b.height());
    let mut seen = vec![false; bw * bh];
    let min_pixels = (bw * bh) / 16;
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for sy in 0..bh {
        for sx in 0..bw {
            if seen[sy * bw + sx] || mask.at(b.x0 + sx, b.y0 + sy) {
                continue;
            }
            let mut boxed = PixelBox::at(b.x0 + sx, b.y0 + sy);
            let mut count = 0usize;
            seen[sy * bw + sx] = true;
            stack.push((sx, sy));
            while let Some((x, y)) = stack.pop() {
                count += 1;
                boxed.add(b.x0 + x, b.y0 + y);
                let mut visit = |nx: usize, ny: usize| {
                    let k = ny * bw + nx;
                    if !seen[k] && !mask.at(b.x0 + nx, b.y0 + ny) {
                        seen[k] = true;
                        stack.push((nx, ny));
                    }
                };
                if x > 0 {
                    visit(x - 1, y);
                }
                if x + 1 < bw {
                    visit(x + 1, y);
                }
                if y > 0 {
                    visit(x, y - 1);
                }
                if y + 1 < bh {
                    visit(x, y + 1);
                }
            }
            if count >= min_pixels {
                out.push(boxed);
            }
        }
    }
    out
}

/// Cor predominante na margem externa da página (mediana por canal).
fn border_color(work: &Work) -> [u8; 3] {
    let m = (work.w.min(work.h) / 100).max(2);
    let mut channels: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for y in 0..work.h {
        for x in 0..work.w {
            if x < m || y < m || x + m >= work.w || y + m >= work.h {
                let p = work.pixel(y * work.w + x);
                for (c, channel) in channels.iter_mut().enumerate() {
                    channel.push(p[c]);
                }
            }
        }
    }
    let mut out = [255u8; 3];
    for (c, channel) in channels.iter_mut().enumerate() {
        channel.sort_unstable();
        out[c] = channel[channel.len() / 2];
    }
    out
}

fn panels_for(work: &Work, color: [u8; 3]) -> Vec<PixelBox> {
    let mask = Mask::of(work, color);
    let page = PixelBox { x0: 0, y0: 0, x1: work.w - 1, y1: work.h - 1 };
    let mut leaves = Vec::new();
    cut(&mask, page, 0, &mut leaves);
    let min_area = (MIN_PANEL_AREA * (work.w * work.h) as f32) as usize;
    let mut out = Vec::new();
    for leaf in leaves {
        if leaf.area() < min_area {
            continue;
        }
        let parts = islands(&mask, &leaf);
        if parts.len() >= 2 {
            out.extend(parts.into_iter().filter(|p| p.area() >= min_area));
        } else {
            out.push(leaf);
        }
    }
    out
}

/// Encontra os quadros. Testa a cor da margem, branco e preto, e fica com a primeira que separa:
/// sem essa prioridade, o contorno preto dos quadros de uma página de margem branca vira "calha".
/// Página sem calhas devolve lista vazia: a ordem passa a considerar só as regiões.
pub fn detect(work: &Work) -> Vec<Rect> {
    if work.w < 16 || work.h < 16 {
        return Vec::new();
    }
    let page_area = (work.w * work.h) as f32;
    let mut best: Vec<PixelBox> = Vec::new();
    for color in [border_color(work), [255, 255, 255], [0, 0, 0]] {
        let found = panels_for(work, color);
        let covered: f32 = found.iter().map(|b| b.area() as f32).sum::<f32>() / page_area;
        if best.len() < 2 && found.len() > best.len() && covered >= 0.35 {
            best = found;
        }
    }
    if best.len() < 2 {
        return Vec::new();
    }
    best.into_iter()
        .map(|b| Rect {
            x: b.x0 as f32 / work.w as f32,
            y: b.y0 as f32 / work.h as f32,
            w: b.width() as f32 / work.w as f32,
            h: b.height() as f32 / work.h as f32,
        })
        .collect()
}
