//! Regiões de texto: áreas lisas com letras alinhadas dentro.
//!
//! Um balão é uma área de cor contínua cujas letras aparecem como "buracos". Isso vale para
//! qualquer idioma e para fundos brancos, coloridos ou escuros, e preserva o contorno inteiro do
//! balão, com a ponta. Texto solto sobre um fundo grande vira uma região em torno das próprias letras.

use std::collections::{HashMap, VecDeque};

use super::classify::{self, Shape, Tone};
use super::raster::{Area, PixelBox, Work, label_smooth_areas};
use super::{DetectedRegion, Rect, RegionKind};

/// Menor área lisa considerada, em fração da página.
const MIN_AREA: f32 = 0.0005;
/// Oscilação máxima de brilho dentro de um fundo liso.
const MAX_LUMA_STD: f32 = 10.0;
/// Fundos de cena (céu, parede) ainda podem ter texto solto em cima.
const MAX_BACKDROP_STD: f32 = 30.0;
/// Altura das letras em fração da altura da página.
const MIN_LETTER: f32 = 0.0036;
const MAX_LETTER: f32 = 0.035;
/// Em papel branco cabem letras bem maiores: é o grito dentro do balão ("RAHHH!").
const MAX_LETTER_ON_PAPER: f32 = 0.09;
/// Brilho a partir do qual o fundo conta como papel branco.
const WHITE_PAPER: f32 = 235.0;
/// Confiança de fala curta com letras coladas: aparece, mas como duvidosa.
const LONE_WORD_CONFIDENCE: f32 = 0.45;
/// Contraste mínimo entre letra e fundo.
const MIN_CONTRAST: f32 = 55.0;
/// Abaixo disso, o texto ocupa pouco da área: ela é fundo de cena, não balão.
const MIN_TEXT_FILL: f32 = 0.2;
/// Fração mínima das letras em linhas regulares: desenhos também têm buracos, mas sem esse alinhamento.
const MIN_STRONG: f32 = 0.6;
const MAX_LETTERS: usize = 900;
/// Margem que inclui o traço do balão, em fração do lado da página.
const OUTLINE_PAD: f32 = 0.004;
/// Faixas horizontais usadas para descrever o contorno de um balão.
const OUTLINE_BANDS: usize = 40;
/// Folga do contorno em volta do papel: só o traço do balão, sem trazer a arte vizinha junto.
const STROKE_PAD: f32 = 0.0025;
/// Margem em volta de texto solto.
const TEXT_PAD: f32 = 0.008;
/// Sem evidência suficiente, a região nem aparece.
const MIN_CONFIDENCE: f32 = 0.34;
/// Um pescoço mais estreito que esta fração da parte menor separa dois balões colados.
const NECK_RATIO: f32 = 0.62;
/// A área lisa pode vazar para o desenho; a região só vai até esta distância do texto (cabe a ponta), em linhas.
const MAX_REACH: f32 = 4.0;
/// Letras muito maiores que as da página são onomatopeias.
const MAX_LETTER_RATIO: f32 = 2.2;
/// Faixa ignorada na borda do quadro, em fração do lado da página, por causa do traço suavizado.
const PANEL_INSET: f32 = 0.0025;

struct Found {
    region: DetectedRegion,
    letter_size: f32,
    /// Letras grandes num balão de verdade: é grito, não onomatopeia solta.
    loud: bool,
}

#[derive(Debug, Clone, Copy)]
struct Letter {
    b: PixelBox,
}

impl Letter {
    fn word_like(&self) -> bool {
        self.b.width() as f32 >= self.b.height() as f32 * 2.2
    }
    /// Letras coladas formam um único buraco largo: contam como várias.
    fn weight(&self) -> usize {
        if self.word_like() { 3 } else { 1 }
    }
}

/// Letras organizadas em linhas ou colunas (texto vertical).
struct TextLayout {
    lines: Vec<Vec<usize>>,
    /// Fração das letras em linhas com vizinhas.
    aligned: f32,
    /// Regularidade das alturas (0–1).
    regularity: f32,
    vertical: bool,
}

fn group(n: usize, joined: impl Fn(usize, usize) -> bool) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for a in 0..n {
        for b in a + 1..n {
            if joined(a, b) {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|g| g[0]);
    out
}

fn overlap(a0: usize, a1: usize, b0: usize, b1: usize) -> f32 {
    let (lo, hi) = (a0.max(b0), a1.min(b1));
    if hi < lo { 0.0 } else { (hi - lo + 1) as f32 }
}

fn gap(a0: usize, a1: usize, b0: usize, b1: usize) -> f32 {
    if b0 > a1 {
        (b0 - a1) as f32
    } else if a0 > b1 {
        (a0 - b1) as f32
    } else {
        0.0
    }
}

fn same_line(a: &Letter, b: &Letter) -> bool {
    let (ha, hb) = (a.b.height() as f32, b.b.height() as f32);
    overlap(a.b.y0, a.b.y1, b.b.y0, b.b.y1) >= 0.5 * ha.min(hb) && gap(a.b.x0, a.b.x1, b.b.x0, b.b.x1) <= 1.6 * ha.max(hb)
}

fn same_column(a: &Letter, b: &Letter) -> bool {
    let (wa, wb) = (a.b.width() as f32, b.b.width() as f32);
    let size = a.b.height().max(a.b.width()).max(b.b.height()).max(b.b.width()) as f32;
    overlap(a.b.x0, a.b.x1, b.b.x0, b.b.x1) >= 0.5 * wa.min(wb) && gap(a.b.y0, a.b.y1, b.b.y0, b.b.y1) <= 1.1 * size
}

fn layout_of(letters: &[Letter]) -> TextLayout {
    let score = |lines: &[Vec<usize>]| {
        let aligned: usize = lines.iter().filter(|l| l.iter().map(|i| letters[*i].weight()).sum::<usize>() >= 2).map(|l| l.len()).sum();
        aligned as f32 / letters.len().max(1) as f32
    };
    let longest = |g: &[Vec<usize>]| g.iter().map(|l| l.len()).max().unwrap_or(0);
    let rows = group(letters.len(), |a, b| same_line(&letters[a], &letters[b]));
    let cols = group(letters.len(), |a, b| same_column(&letters[a], &letters[b]));
    let (row_score, col_score) = (score(&rows), score(&cols));
    // Texto vertical só quando as colunas explicam claramente mais do que as linhas.
    let vertical = col_score > row_score + 0.1 && longest(&cols) > longest(&rows);
    let (lines, aligned) = if vertical { (cols, col_score) } else { (rows, row_score) };
    let mut sizes: Vec<f32> = lines
        .iter()
        .filter(|l| l.len() >= 2)
        .flatten()
        .map(|i| if vertical { letters[*i].b.width() as f32 } else { letters[*i].b.height() as f32 })
        .collect();
    sizes.sort_by(f32::total_cmp);
    let regularity = if sizes.len() >= 2 {
        let median = sizes[sizes.len() / 2];
        let spread = sizes.iter().map(|h| (h - median).abs()).sum::<f32>() / sizes.len() as f32;
        (1.0 - spread / median.max(1.0)).clamp(0.0, 1.0)
    } else {
        0.5
    };
    TextLayout { lines, aligned, regularity, vertical }
}

/// Peso das letras em linhas de verdade: `min_line` ou mais letras com o mesmo tamanho.
fn strong_weight(letters: &[Letter], layout: &TextLayout, min_line: usize) -> usize {
    let size = |l: &Letter| if layout.vertical { l.b.width() } else { l.b.height() } as f32;
    let mut total = 0;
    for line in &layout.lines {
        let mut sizes: Vec<f32> = line.iter().map(|i| size(&letters[*i])).collect();
        sizes.sort_by(f32::total_cmp);
        let median = sizes[sizes.len() / 2];
        let weight: usize = line.iter().filter(|i| {
            // Acentos deixam a letra mais alta, nunca mais baixa.
            let s = size(&letters[**i]);
            s >= 0.55 * median && s <= 1.8 * median
        }).map(|i| letters[*i].weight()).sum();
        if weight >= min_line {
            total += weight;
        }
    }
    total
}

/// Junta linhas próximas em blocos de texto (um por balão, em geral): (retângulo, peso das letras).
fn blocks_of(letters: &[Letter], layout: &TextLayout) -> Vec<(PixelBox, usize)> {
    let boxes: Vec<PixelBox> = layout
        .lines
        .iter()
        .map(|l| {
            let mut b = letters[l[0]].b;
            for i in l {
                b.merge(&letters[*i].b);
            }
            b
        })
        .collect();
    let vertical = layout.vertical;
    let groups = group(boxes.len(), |a, b| {
        let (x, y) = (&boxes[a], &boxes[b]);
        if vertical {
            let size = x.width().max(y.width()) as f32;
            overlap(x.y0, x.y1, y.y0, y.y1) > 0.0 && gap(x.x0, x.x1, y.x0, y.x1) <= 1.3 * size
        } else {
            let size = x.height().max(y.height()) as f32;
            overlap(x.x0, x.x1, y.x0, y.x1) > 0.0 && gap(x.y0, x.y1, y.y0, y.y1) <= 1.3 * size
        }
    });
    groups
        .into_iter()
        .map(|g| {
            let mut b = boxes[g[0]];
            let mut count = 0;
            for i in &g {
                b.merge(&boxes[*i]);
                count += layout.lines[*i].iter().map(|k| letters[*k].weight()).sum::<usize>();
            }
            (b, count)
        })
        .collect()
}

fn median_size(letters: &[Letter]) -> f32 {
    let mut sizes: Vec<usize> = letters.iter().map(|l| l.b.height()).collect();
    sizes.sort_unstable();
    sizes[sizes.len() / 2] as f32
}

/// Limita o retângulo da área à vizinhança do texto. Devolve também se cortou muito (houve vazamento).
fn clip_to_text(area: &PixelBox, text: &PixelBox, line: f32) -> (PixelBox, bool) {
    let reach = (line * MAX_REACH) as usize;
    let clipped = PixelBox {
        x0: area.x0.max(text.x0.saturating_sub(reach)),
        y0: area.y0.max(text.y0.saturating_sub(reach)),
        x1: area.x1.min(text.x1 + reach),
        y1: area.y1.min(text.y1 + reach),
    };
    (clipped, area.area() > clipped.area() * 2)
}

fn to_rect(work: &Work, b: &PixelBox, pad: f32) -> Rect {
    let (w, h) = (work.w as f32, work.h as f32);
    let px = pad * w.max(h);
    let x0 = (b.x0 as f32 - px).max(0.0);
    let y0 = (b.y0 as f32 - px).max(0.0);
    let x1 = (b.x1 as f32 + 1.0 + px).min(w);
    let y1 = (b.y1 as f32 + 1.0 + px).min(h);
    Rect { x: x0 / w, y: y0 / h, w: (x1 - x0) / w, h: (y1 - y0) / h }
}

/// Contorno do balão dentro de `within`, em frações de `rect`, para destacar só o balão, com o traço e a ponta.
/// shortcut: guarda só o extremo esquerdo e o direito de cada faixa (o vão entre dois lóbulos sobrepostos vem junto), traçar a borda de verdade se isso incomodar.
fn outline_of(work: &Work, crop: &Crop, within: &PixelBox, rect: &Rect) -> Vec<[f32; 2]> {
    let row_size = crop.b.width();
    let band = (within.height() / OUTLINE_BANDS).max(1);
    // (esquerda, direita, topo, base) de cada faixa que tem balão.
    let mut spans: Vec<(usize, usize, usize, usize)> = Vec::new();
    for top in (within.y0..=within.y1).step_by(band) {
        let bottom = (top + band - 1).min(within.y1);
        let mut span: Option<(usize, usize)> = None;
        for row in top..=bottom {
            let cells = &crop.filled[(row - crop.b.y0) * row_size..(row - crop.b.y0 + 1) * row_size];
            for x in within.x0..=within.x1 {
                if cells[x - crop.b.x0] {
                    span = Some(span.map_or((x, x), |(a, b)| (a.min(x), b.max(x))));
                }
            }
        }
        if let Some((a, b)) = span {
            spans.push((a, b, top, bottom));
        }
    }
    let px = STROKE_PAD * work.w.max(work.h) as f32;
    let point = |x: f32, y: f32| {
        let fx = (x / work.w as f32 - rect.x) / rect.w;
        let fy = (y / work.h as f32 - rect.y) / rect.h;
        [(fx.clamp(0.0, 1.0) * 1000.0).round() / 1000.0, (fy.clamp(0.0, 1.0) * 1000.0).round() / 1000.0]
    };
    let last = spans.len().saturating_sub(1);
    let y_of = |n: usize, s: &(usize, usize, usize, usize)| {
        if n == 0 {
            s.2 as f32 - px
        } else if n == last {
            s.3 as f32 + 1.0 + px
        } else {
            (s.2 + s.3) as f32 / 2.0
        }
    };
    let right = spans.iter().enumerate().map(|(n, s)| point(s.1 as f32 + 1.0 + px, y_of(n, s)));
    let left = spans.iter().enumerate().rev().map(|(n, s)| point(s.0 as f32 - px, y_of(n, s)));
    right.chain(left).collect()
}

/// Recorte de uma área com o que foi encontrado dentro dela.
struct Crop {
    b: PixelBox,
    /// Área + buracos (o balão "preenchido").
    filled: Vec<bool>,
    filled_count: usize,
    letters: Vec<Letter>,
    /// Pixels em buracos grandes demais para serem letras (personagens, outros balões).
    big_holes: usize,
}

const STATE_FREE: u8 = 0;
const STATE_AREA: u8 = 1;
const STATE_OUTSIDE: u8 = 2;
const STATE_HOLE: u8 = 3;

fn inspect(work: &Work, ids: &[u32], area: &Area) -> Crop {
    let b = area.bbox;
    let (bw, bh) = (b.width(), b.height());
    let mut state = vec![STATE_FREE; bw * bh];
    for y in 0..bh {
        for x in 0..bw {
            if ids[(b.y0 + y) * work.w + b.x0 + x] == area.id {
                state[y * bw + x] = STATE_AREA;
            }
        }
    }
    // Tudo que alcança a borda do recorte sem cruzar a área é "fora"; o resto são buracos.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut seed = |x: usize, y: usize, state: &mut Vec<u8>| {
        if state[y * bw + x] == STATE_FREE {
            state[y * bw + x] = STATE_OUTSIDE;
            stack.push((x, y));
        }
    };
    for y in 0..bh {
        seed(0, y, &mut state);
        seed(bw - 1, y, &mut state);
    }
    for x in 0..bw {
        seed(x, 0, &mut state);
        seed(x, bh - 1, &mut state);
    }
    while let Some((x, y)) = stack.pop() {
        for (nx, ny) in neighbors4(x, y, bw, bh) {
            if state[ny * bw + nx] == STATE_FREE {
                state[ny * bw + nx] = STATE_OUTSIDE;
                stack.push((nx, ny));
            }
        }
    }
    let area_luma = 0.299 * area.mean[0] + 0.587 * area.mean[1] + 0.114 * area.mean[2];
    let min_h = ((work.h as f32 * MIN_LETTER) as usize).max(4);
    let max_letter = if area_luma >= WHITE_PAPER { MAX_LETTER_ON_PAPER } else { MAX_LETTER };
    let max_h = ((work.h as f32 * max_letter) as usize).max(12);
    let max_w = work.w * 35 / 100;
    let mut letters = Vec::new();
    let mut big_holes = 0usize;
    for sy in 0..bh {
        for sx in 0..bw {
            if state[sy * bw + sx] != STATE_FREE {
                continue;
            }
            // Um buraco: letra, ou qualquer outra coisa cercada pela área.
            let mut hole = PixelBox::at(sx, sy);
            let mut pixels = 0usize;
            let mut contrast = 0f32;
            state[sy * bw + sx] = STATE_HOLE;
            stack.push((sx, sy));
            while let Some((x, y)) = stack.pop() {
                pixels += 1;
                hole.add(x, y);
                contrast = contrast.max((work.luma((b.y0 + y) * work.w + b.x0 + x) - area_luma).abs());
                for (nx, ny) in neighbors8(x, y, bw, bh) {
                    if state[ny * bw + nx] == STATE_FREE {
                        state[ny * bw + nx] = STATE_HOLE;
                        stack.push((nx, ny));
                    }
                }
            }
            let (hw, hh) = (hole.width(), hole.height());
            // Traços muito compridos são desenho (listras, fios, código de barras), não letras.
            let fits = hh >= min_h && hh <= max_h && hw <= max_w && hw <= hh * 8 && hh <= hw * 8 && pixels * 100 >= hole.area() * 8;
            if fits && contrast >= MIN_CONTRAST {
                letters.push(Letter {
                    b: PixelBox { x0: b.x0 + hole.x0, y0: b.y0 + hole.y0, x1: b.x0 + hole.x1, y1: b.y0 + hole.y1 },
                });
            } else if hh > max_h || hw > max_w {
                big_holes += pixels;
            }
        }
    }
    let filled: Vec<bool> = state.iter().map(|s| *s == STATE_AREA || *s == STATE_HOLE).collect();
    let filled_count = filled.iter().filter(|f| **f).count();
    Crop { b, filled, filled_count, letters, big_holes }
}

fn neighbors4(x: usize, y: usize, w: usize, h: usize) -> impl Iterator<Item = (usize, usize)> {
    [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)]
        .into_iter()
        .filter_map(move |(dx, dy)| step(x, y, dx, dy, w, h))
}

fn neighbors8(x: usize, y: usize, w: usize, h: usize) -> impl Iterator<Item = (usize, usize)> {
    [(-1i32, 0i32), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)]
        .into_iter()
        .filter_map(move |(dx, dy)| step(x, y, dx, dy, w, h))
}

fn step(x: usize, y: usize, dx: i32, dy: i32, w: usize, h: usize) -> Option<(usize, usize)> {
    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
    (nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32).then_some((nx as usize, ny as usize))
}

fn tone_of(area: &Area) -> Tone {
    let [r, g, b] = area.mean;
    Tone { luma: 0.299 * r + 0.587 * g + 0.114 * b, chroma: r.max(g).max(b) - r.min(g).min(b) }
}

/// Fração do contorno com traço escuro logo ao lado.
fn outline_fraction(work: &Work, crop: &Crop) -> f32 {
    let (bw, bh) = (crop.b.width(), crop.b.height());
    let (mut ring, mut dark) = (0usize, 0usize);
    let reach = 3i32;
    for y in 0..bh {
        for x in 0..bw {
            if !crop.filled[y * bw + x] || !classify::is_edge(&crop.filled, bw, bh, x, y) {
                continue;
            }
            ring += 1;
            'probe: for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let (px, py) = (crop.b.x0 as i32 + x as i32 + dx, crop.b.y0 as i32 + y as i32 + dy);
                    if px < 0 || py < 0 || px >= work.w as i32 || py >= work.h as i32 {
                        continue;
                    }
                    if work.luma(py as usize * work.w + px as usize) < 90.0 {
                        dark += 1;
                        break 'probe;
                    }
                }
            }
        }
    }
    if ring == 0 { 1.0 } else { dark as f32 / ring as f32 }
}

fn confidence(letters: &[Letter], layout: &TextLayout, solidity: f32) -> f32 {
    let count: usize = letters.iter().map(Letter::weight).sum();
    let amount = (count as f32 / 10.0).min(1.0);
    let aligned = ((layout.aligned - 0.3) / 0.6).clamp(0.0, 1.0);
    let shape = ((solidity - 0.45) / 0.45).clamp(0.0, 1.0);
    0.4 * aligned + 0.25 * amount + 0.15 * layout.regularity + 0.2 * shape
}

/// Reparte uma área com vários blocos de texto: cada pixel vai para o bloco mais próximo, andando só
/// por dentro da área. Devolve os retângulos das partes se elas estiverem ligadas por pescoços estreitos.
fn split_by_blocks(crop: &Crop, blocks: &[(PixelBox, usize)]) -> Option<Vec<PixelBox>> {
    let (bw, bh) = (crop.b.width(), crop.b.height());
    let mut owner = vec![u16::MAX; bw * bh];
    let mut queue = VecDeque::new();
    for (n, (block, _)) in blocks.iter().enumerate() {
        for y in block.y0..=block.y1 {
            for x in block.x0..=block.x1 {
                let k = (y - crop.b.y0) * bw + (x - crop.b.x0);
                if crop.filled[k] && owner[k] == u16::MAX {
                    owner[k] = n as u16;
                    queue.push_back(k);
                }
            }
        }
    }
    let mut border = vec![0usize; blocks.len()];
    while let Some(k) = queue.pop_front() {
        let (x, y) = (k % bw, k / bw);
        let o = owner[k];
        for (nx, ny) in neighbors4(x, y, bw, bh) {
            let n = ny * bw + nx;
            if !crop.filled[n] {
                continue;
            }
            if owner[n] == u16::MAX {
                owner[n] = o;
                queue.push_back(n);
            } else if owner[n] != o {
                border[o as usize] += 1;
            }
        }
    }
    let mut boxes: Vec<Option<PixelBox>> = vec![None; blocks.len()];
    for y in 0..bh {
        for x in 0..bw {
            let o = owner[y * bw + x];
            if o == u16::MAX {
                continue;
            }
            let (px, py) = (crop.b.x0 + x, crop.b.y0 + y);
            match &mut boxes[o as usize] {
                Some(b) => b.add(px, py),
                None => boxes[o as usize] = Some(PixelBox::at(px, py)),
            }
        }
    }
    let boxes: Vec<PixelBox> = boxes.into_iter().flatten().collect();
    if boxes.len() != blocks.len() {
        return None;
    }
    for (i, b) in boxes.iter().enumerate() {
        let side = b.width().min(b.height()).max(1) as f32;
        if border[i] as f32 / 2.0 > side * NECK_RATIO {
            return None;
        }
    }
    Some(boxes)
}

fn shape_of(work: &Work, crop: &Crop) -> Shape {
    let (solidity, bumps, bump_depth) = classify::measure(&crop.filled, crop.b.width(), crop.b.height());
    Shape {
        rectangularity: crop.filled_count as f32 / crop.b.area().max(1) as f32,
        solidity,
        bumps,
        bump_depth,
        outline: outline_fraction(work, crop),
    }
}

/// Texto solto: blocos de letras sobre um fundo grande, sem contorno próprio.
fn loose_text(work: &Work, crop: &Crop, out: &mut Vec<Found>) {
    let layout = layout_of(&crop.letters);
    for (block, count) in blocks_of(&crop.letters, &layout) {
        if count < 6 {
            continue;
        }
        let members: Vec<Letter> = crop
            .letters
            .iter()
            .copied()
            .filter(|l| l.b.x0 >= block.x0 && l.b.x1 <= block.x1 && l.b.y0 >= block.y0 && l.b.y1 <= block.y1)
            .collect();
        let local = layout_of(&members);
        // Sem contorno, a evidência vem só do alinhamento: confiança limitada.
        let c = (0.55 * local.aligned + 0.2 * (count as f32 / 20.0).min(1.0) + 0.15 * local.regularity).min(0.72);
        // Listras e hachuras também formam fileiras: sem contorno, só vale texto regular, e bem regular se for curto.
        let min_regularity = if count < 10 { 0.9 } else { 0.7 };
        if c >= MIN_CONFIDENCE && local.aligned >= 0.8 && local.regularity >= min_regularity {
            let region = DetectedRegion { rect: to_rect(work, &block, TEXT_PAD), kind: RegionKind::Other, confidence: c, outline: Vec::new() };
            out.push(Found { region, letter_size: median_size(&members), loud: false });
        }
    }
}

fn from_area(work: &Work, area: &Area, ids: &[u32], out: &mut Vec<Found>) {
    let crop = inspect(work, ids, area);
    if crop.letters.is_empty() || crop.letters.len() > MAX_LETTERS {
        return;
    }
    let count: usize = crop.letters.iter().map(Letter::weight).sum();
    let layout = layout_of(&crop.letters);
    let mut text_box = crop.letters[0].b;
    for l in &crop.letters {
        text_box.merge(&l.b);
    }
    let text_fill = text_box.area() as f32 / crop.filled_count.max(1) as f32;
    let page = (work.w * work.h) as f32;
    let tone = tone_of(area);
    // Balão branco pequeno com fala curta ("OLÁ"): sobra papel em volta do texto, mas não é fundo.
    let short_speech = tone.luma >= WHITE_PAPER && (crop.filled_count as f32) < page * 0.006;
    let backdrop = crop.filled_count as f32 > page * 0.28 || (text_fill < MIN_TEXT_FILL && !short_speech) || crop.big_holes as f32 > crop.filled_count as f32 * 0.25;
    if backdrop {
        loose_text(work, &crop, out);
        return;
    }
    let paper = tone.luma >= 150.0 || (tone.chroma > 40.0 && tone.luma >= 100.0);
    // Fundo escuro também é sombra de desenho: exige bem mais texto, e uma malha de hachuras ocupa mais que o próprio fundo.
    let dark_caption = tone.luma < 60.0 && count >= 10 && layout.aligned >= 0.8 && layout.regularity >= 0.7 && text_fill <= 1.0;
    // Em papel branco, duas letras bastam ("OI", "AH"); a baixa contagem já reduz a confiança.
    let min_line = if tone.luma >= WHITE_PAPER { 2 } else { 3 };
    let strong = strong_weight(&crop.letters, &layout, min_line) as f32;
    // Letras coladas numa mancha só ("OLÁ"): vale se a mancha é larga e está no meio do balão.
    let lone_word = short_speech && crop.letters.len() == 1 && {
        let l = crop.letters[0].b;
        let dx = (l.x0 + l.x1).abs_diff(crop.b.x0 + crop.b.x1) as f32 / 2.0;
        let dy = (l.y0 + l.y1).abs_diff(crop.b.y0 + crop.b.y1) as f32 / 2.0;
        l.width() as f32 >= l.height() as f32 * 1.3 && dx <= crop.b.width() as f32 * 0.25 && dy <= crop.b.height() as f32 * 0.25
    };
    // Grito à mão tem letras irregulares ("RAHHH!"): em papel branco, o alinhamento compensa.
    let shouted = tone.luma >= WHITE_PAPER && layout.aligned >= 0.9;
    let regular = (count as f32 * MIN_STRONG).max(min_line as f32);
    let required = if shouted { regular.min(3.0) } else { regular };
    if !(paper || dark_caption) || area.luma_std > MAX_LUMA_STD || (strong < required && !lone_word) {
        return;
    }
    let shape = shape_of(work, &crop);
    // Papel colorido se confunde com pintura: só vale quando a forma é cheia, como a de uma caixa.
    if tone.luma < WHITE_PAPER && shape.solidity < 0.85 {
        return;
    }
    // Muitas "letras" sem regularidade nenhuma, numa forma cheia de reentrâncias, são respingos do desenho.
    if count >= 6 && layout.regularity < 0.2 && shape.solidity < 0.8 {
        return;
    }
    // Retângulo perfeito com "letras" irregulares é código de barras, não legenda.
    if shape.rectangularity >= 0.98 && layout.regularity < 0.65 {
        return;
    }
    // Pouca letra e forma cheia de reentrâncias: é respingo do desenho, não balão.
    if (lone_word || count < 4) && shape.solidity < 0.8 {
        return;
    }
    let kind = classify::classify(&shape, &tone);
    let conf = if lone_word { LONE_WORD_CONFIDENCE } else { confidence(&crop.letters, &layout, shape.solidity) };
    if conf < MIN_CONFIDENCE {
        return;
    }
    let letter_size = median_size(&crop.letters);
    // Onomatopeia em fundo branco: ou o fundo preenche o quadro (retangular), ou só contorna a palavra.
    // Grito de verdade tem papel em volta das letras ou é uma frase inteira.
    let loud = tone.luma >= WHITE_PAPER && shape.rectangularity < 0.9 && ((shape.solidity >= 0.9 && text_fill <= 0.8) || (shape.solidity >= 0.8 && count >= 8));
    let blocks = blocks_of(&crop.letters, &layout);
    if blocks.len() >= 2 {
        if let Some(parts) = split_by_blocks(&crop, &blocks) {
            for (part, (block, _)) in parts.iter().zip(&blocks) {
                let (b, _) = clip_to_text(part, block, letter_size);
                let rect = to_rect(work, &b, OUTLINE_PAD);
                let region = DetectedRegion { rect, kind, confidence: conf * 0.9, outline: outline_of(work, &crop, &b, &rect) };
                out.push(Found { region, letter_size, loud });
            }
            return;
        }
    }
    let (b, leaked) = clip_to_text(&crop.b, &text_box, letter_size);
    let rect = to_rect(work, &b, OUTLINE_PAD);
    let region = if leaked {
        DetectedRegion { rect, kind: RegionKind::Other, confidence: conf * 0.8, outline: Vec::new() }
    } else {
        DetectedRegion { rect, kind, confidence: conf, outline: outline_of(work, &crop, &b, &rect) }
    };
    out.push(Found { region, letter_size, loud });
}

/// Quadro de cada pixel (0 = calha). Balões cortados pela borda do quadro não vazam para a calha branca.
/// shortcut: balão que cruza a calha vira duas regiões, juntar as partes se isso aparecer em páginas reais.
fn zones(work: &Work, panels: &[Rect]) -> Vec<u16> {
    let mut out = vec![0u16; work.w * work.h];
    for (n, p) in panels.iter().enumerate() {
        let inset = PANEL_INSET * work.w.max(work.h) as f32;
        let y0 = (p.y * work.h as f32 + inset) as usize;
        let y1 = ((p.bottom() * work.h as f32 - inset).max(0.0) as usize).min(work.h);
        let x0 = (p.x * work.w as f32 + inset) as usize;
        let x1 = ((p.right() * work.w as f32 - inset).max(0.0) as usize).min(work.w);
        if x1 <= x0 {
            continue;
        }
        for y in y0..y1 {
            out[y * work.w + x0..y * work.w + x1].fill(n as u16 + 1);
        }
    }
    out
}

/// Encontra as regiões de texto da página, sem ordem definida.
pub fn detect(work: &Work, panels: &[Rect]) -> Vec<DetectedRegion> {
    let page = (work.w * work.h) as f32;
    let labels = label_smooth_areas(work, &zones(work, panels), (page * MIN_AREA) as usize);
    let mut found: Vec<Found> = Vec::new();
    for area in &labels.areas {
        if area.luma_std <= MAX_BACKDROP_STD {
            from_area(work, area, &labels.ids, &mut found);
        }
    }
    if found.len() >= 3 {
        let mut sizes: Vec<f32> = found.iter().map(|f| f.letter_size).collect();
        sizes.sort_by(f32::total_cmp);
        let typical = sizes[sizes.len() / 2];
        found.retain(|f| f.loud || f.letter_size <= typical * MAX_LETTER_RATIO);
    }
    // Regiões quase contidas umas nas outras: fica a de maior confiança.
    found.sort_by(|a, b| b.region.confidence.total_cmp(&a.region.confidence));
    let mut kept: Vec<DetectedRegion> = Vec::new();
    for Found { region: r, .. } in found {
        let duplicated = kept.iter().any(|k| r.rect.intersection(&k.rect) >= 0.6 * r.rect.area().min(k.rect.area()));
        if !duplicated {
            kept.push(r);
        }
    }
    kept
}
