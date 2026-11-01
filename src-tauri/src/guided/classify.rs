//! Tipo da região, medido pela forma do contorno e pela cor do fundo.

use super::RegionKind;

/// Medidas de uma região já preenchida (área lisa + letras).
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    /// Área preenchida sobre a área do retângulo envolvente.
    pub rectangularity: f32,
    /// Área preenchida sobre a área do casco convexo.
    pub solidity: f32,
    /// Picos no contorno, medidos com o formato normalizado para um círculo.
    pub bumps: u32,
    /// Altura média dos picos em relação ao raio.
    pub bump_depth: f32,
    /// Fração do entorno ocupada por traço escuro.
    pub outline: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Tone {
    pub luma: f32,
    pub chroma: f32,
}

const ANGLE_BINS: usize = 96;

/// Mede o contorno a partir da máscara preenchida do recorte: (solidez, picos, altura dos picos).
pub fn measure(mask: &[bool], w: usize, h: usize) -> (f32, u32, f32) {
    let filled = mask.iter().filter(|m| **m).count();
    if filled == 0 {
        return (0.0, 0, 0.0);
    }
    let mut boundary: Vec<(f32, f32)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] && is_edge(mask, w, h, x, y) {
                boundary.push((x as f32, y as f32));
            }
        }
    }
    let hull = convex_hull(&boundary);
    let solidity = (filled as f32 / polygon_area(&hull).max(1.0)).min(1.0);
    let (bumps, depth) = contour_bumps(&boundary, w as f32, h as f32);
    (solidity, bumps, depth)
}

/// O pixel preenchido toca a borda do recorte ou um vizinho vazio.
pub fn is_edge(mask: &[bool], w: usize, h: usize, x: usize, y: usize) -> bool {
    x == 0 || y == 0 || x + 1 == w || y + 1 == h || !mask[y * w + x - 1] || !mask[y * w + x + 1] || !mask[(y - 1) * w + x] || !mask[(y + 1) * w + x]
}

fn convex_hull(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: (f32, f32), a: (f32, f32), b: (f32, f32)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut hull: Vec<(f32, f32)> = Vec::with_capacity(pts.len() * 2);
    for &p in &pts {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    let lower = hull.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    hull
}

fn polygon_area(p: &[(f32, f32)]) -> f32 {
    if p.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        sum += a.0 * b.1 - b.0 * a.1;
    }
    sum.abs() / 2.0
}

/// Conta os picos do raio ao longo do contorno, com o retângulo envolvente esticado até um círculo.
fn contour_bumps(boundary: &[(f32, f32)], w: f32, h: f32) -> (u32, f32) {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let mut radii = [0f32; ANGLE_BINS];
    for &(x, y) in boundary {
        let (dx, dy) = ((x - cx) / cx.max(1.0), (y - cy) / cy.max(1.0));
        let angle = dy.atan2(dx) + std::f32::consts::PI;
        let bin = ((angle / std::f32::consts::TAU) * ANGLE_BINS as f32) as usize % ANGLE_BINS;
        radii[bin] = radii[bin].max((dx * dx + dy * dy).sqrt());
    }
    let mut smooth = [0f32; ANGLE_BINS];
    for i in 0..ANGLE_BINS {
        smooth[i] = (radii[(i + ANGLE_BINS - 1) % ANGLE_BINS] + radii[i] * 2.0 + radii[(i + 1) % ANGLE_BINS]) / 4.0;
    }
    let mean = smooth.iter().sum::<f32>() / ANGLE_BINS as f32;
    let mut bumps = 0;
    let mut depth = 0.0;
    for i in 0..ANGLE_BINS {
        let (a, b, c) = (smooth[(i + ANGLE_BINS - 1) % ANGLE_BINS], smooth[i], smooth[(i + 1) % ANGLE_BINS]);
        if b > a && b >= c {
            // Vale mais próximo de cada lado, dentro de uma janela curta.
            let reach = 4;
            let low_l = (1..=reach).map(|k| smooth[(i + ANGLE_BINS - k) % ANGLE_BINS]).fold(f32::MAX, f32::min);
            let low_r = (1..=reach).map(|k| smooth[(i + k) % ANGLE_BINS]).fold(f32::MAX, f32::min);
            let prominence = b - low_l.max(low_r);
            if prominence > 0.03 * mean {
                bumps += 1;
                depth += prominence / mean.max(f32::EPSILON);
            }
        }
    }
    (bumps, if bumps > 0 { depth / bumps as f32 } else { 0.0 })
}

/// Classifica a região. Em dúvida, fica como fala, o tipo mais comum.
pub fn classify(shape: &Shape, tone: &Tone) -> RegionKind {
    let colored = tone.chroma > 28.0;
    let dark = tone.luma < 90.0;
    if shape.rectangularity >= 0.93 || ((colored || dark) && shape.rectangularity >= 0.8) {
        return RegionKind::Narration;
    }
    if shape.bumps >= 9 && shape.bump_depth >= 0.14 && shape.solidity < 0.86 {
        return RegionKind::Shout;
    }
    if shape.bumps >= 7 && shape.bump_depth >= 0.035 && shape.solidity >= 0.8 {
        return RegionKind::Thought;
    }
    if shape.outline > 0.25 && shape.outline < 0.62 {
        return RegionKind::Whisper;
    }
    RegionKind::Speech
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(rectangularity: f32, solidity: f32, bumps: u32, bump_depth: f32, outline: f32) -> Shape {
        Shape { rectangularity, solidity, bumps, bump_depth, outline }
    }
    const WHITE: Tone = Tone { luma: 250.0, chroma: 2.0 };

    #[test]
    fn rectangles_are_narration_and_color_relaxes_the_rule() {
        assert_eq!(classify(&shape(0.97, 0.99, 0, 0.0, 0.9), &WHITE), RegionKind::Narration);
        assert_eq!(classify(&shape(0.85, 0.95, 0, 0.0, 0.9), &Tone { luma: 200.0, chroma: 60.0 }), RegionKind::Narration);
        assert_eq!(classify(&shape(0.85, 0.95, 0, 0.0, 0.9), &WHITE), RegionKind::Speech);
    }

    #[test]
    fn contour_separates_shout_thought_and_whisper() {
        assert_eq!(classify(&shape(0.5, 0.7, 12, 0.3, 0.9), &WHITE), RegionKind::Shout);
        assert_eq!(classify(&shape(0.7, 0.88, 10, 0.06, 0.9), &WHITE), RegionKind::Thought);
        assert_eq!(classify(&shape(0.78, 0.97, 1, 0.1, 0.45), &WHITE), RegionKind::Whisper);
        assert_eq!(classify(&shape(0.78, 0.95, 2, 0.1, 0.95), &WHITE), RegionKind::Speech);
    }

    #[test]
    fn hull_measures_a_filled_square() {
        let (w, h) = (20, 20);
        let mask = vec![true; w * h];
        let (solidity, _, _) = measure(&mask, w, h);
        assert!(solidity > 0.95, "{solidity}");
    }
}
