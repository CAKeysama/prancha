//! Imagem de trabalho e agrupamento de pixels por continuidade de cor.

use std::collections::HashMap;

use image::DynamicImage;
use image::imageops::FilterType;

/// Maior lado da imagem analisada: preserva as letras e mantém a análise rápida.
const WORK_SIDE: u32 = 1600;

/// Diferença máxima por canal entre vizinhos da mesma área lisa.
const STEP_TOLERANCE: i16 = 12;

pub struct Work {
    pub w: usize,
    pub h: usize,
    /// RGB intercalado.
    pub rgb: Vec<u8>,
}

impl Work {
    pub fn from_image(image: &DynamicImage) -> Self {
        let longest = image.width().max(image.height());
        let rgb = if longest > WORK_SIDE {
            let scale = WORK_SIDE as f32 / longest as f32;
            let w = ((image.width() as f32 * scale).round() as u32).max(1);
            let h = ((image.height() as f32 * scale).round() as u32).max(1);
            image.resize_exact(w, h, FilterType::Triangle).to_rgb8()
        } else {
            image.to_rgb8()
        };
        Work { w: rgb.width() as usize, h: rgb.height() as usize, rgb: rgb.into_raw() }
    }

    #[inline]
    pub fn pixel(&self, i: usize) -> [u8; 3] {
        [self.rgb[i * 3], self.rgb[i * 3 + 1], self.rgb[i * 3 + 2]]
    }

    #[inline]
    pub fn luma(&self, i: usize) -> f32 {
        let [r, g, b] = self.pixel(i);
        0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PixelBox {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

impl PixelBox {
    pub fn at(x: usize, y: usize) -> Self {
        PixelBox { x0: x, y0: y, x1: x, y1: y }
    }
    pub fn add(&mut self, x: usize, y: usize) {
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x);
        self.y1 = self.y1.max(y);
    }
    pub fn merge(&mut self, o: &PixelBox) {
        self.x0 = self.x0.min(o.x0);
        self.y0 = self.y0.min(o.y0);
        self.x1 = self.x1.max(o.x1);
        self.y1 = self.y1.max(o.y1);
    }
    pub fn width(&self) -> usize {
        self.x1 - self.x0 + 1
    }
    pub fn height(&self) -> usize {
        self.y1 - self.y0 + 1
    }
    pub fn area(&self) -> usize {
        self.width() * self.height()
    }
}

/// Uma área de cor contínua.
#[derive(Debug, Clone)]
pub struct Area {
    pub id: u32,
    pub bbox: PixelBox,
    pub mean: [f32; 3],
    pub luma_std: f32,
}

pub struct Labels {
    pub ids: Vec<u32>,
    pub areas: Vec<Area>,
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        let up = parent[parent[x as usize] as usize];
        parent[x as usize] = up;
        x = up;
    }
    x
}

#[inline]
fn close(rgb: &[u8], a: usize, b: usize) -> bool {
    (0..3).all(|c| (rgb[a * 3 + c] as i16 - rgb[b * 3 + c] as i16).abs() <= STEP_TOLERANCE)
}

#[derive(Clone, Copy)]
struct Tally {
    pixels: usize,
    bbox: PixelBox,
    rgb: [f64; 3],
    luma: f64,
    luma2: f64,
}

/// Agrupa pixels vizinhos de cor parecida, em tempo linear, sem atravessar de uma zona para outra.
/// Só as áreas com pelo menos `min_pixels` entram na lista.
pub fn label_smooth_areas(work: &Work, zones: &[u16], min_pixels: usize) -> Labels {
    let (w, h) = (work.w, work.h);
    let mut parent: Vec<u32> = (0..(w * h) as u32).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if x > 0 && zones[i] == zones[i - 1] && close(&work.rgb, i, i - 1) {
                let (a, b) = (find(&mut parent, i as u32), find(&mut parent, (i - 1) as u32));
                if a != b {
                    parent[a.max(b) as usize] = a.min(b);
                }
            }
            if y > 0 && zones[i] == zones[i - w] && close(&work.rgb, i, i - w) {
                let (a, b) = (find(&mut parent, i as u32), find(&mut parent, (i - w) as u32));
                if a != b {
                    parent[a.max(b) as usize] = a.min(b);
                }
            }
        }
    }
    let mut ids = vec![0u32; w * h];
    let mut tallies: HashMap<u32, Tally> = HashMap::new();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let root = find(&mut parent, i as u32);
            ids[i] = root;
            let [r, g, b] = work.pixel(i);
            let l = work.luma(i) as f64;
            let t = tallies.entry(root).or_insert(Tally { pixels: 0, bbox: PixelBox::at(x, y), rgb: [0.0; 3], luma: 0.0, luma2: 0.0 });
            t.pixels += 1;
            t.bbox.add(x, y);
            t.rgb[0] += r as f64;
            t.rgb[1] += g as f64;
            t.rgb[2] += b as f64;
            t.luma += l;
            t.luma2 += l * l;
        }
    }
    let areas = tallies
        .into_iter()
        .filter(|(_, t)| t.pixels >= min_pixels)
        .map(|(id, t)| {
            let n = t.pixels as f64;
            let mean = t.luma / n;
            let variance = (t.luma2 / n - mean * mean).max(0.0);
            Area {
                id,
                bbox: t.bbox,
                mean: [(t.rgb[0] / n) as f32, (t.rgb[1] / n) as f32, (t.rgb[2] / n) as f32],
                luma_std: variance.sqrt() as f32,
            }
        })
        .collect();
    Labels { ids, areas }
}
