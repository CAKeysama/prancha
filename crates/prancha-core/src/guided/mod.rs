//! Leitura Guiada: encontra as regiões de texto de uma página e define a ordem de leitura.
//!
//! Tudo roda localmente, sobre a imagem da página, sem OCR nem rede:
//! - `raster`: prepara a imagem e agrupa pixels de cor contínua.
//! - `balloons`: encontra áreas lisas com letras dentro (balões, caixas, texto solto).
//! - `classify`: define o tipo de cada região pela forma e pela cor.
//! - `panels`: encontra os quadros pelas calhas entre eles.
//! - `order`: ordena quadros e regiões conforme a direção de leitura.
//! - `store`: guarda os resultados no banco, com as correções do usuário à parte.

pub mod balloons;
pub mod classify;
pub mod order;
pub mod panels;
pub mod raster;
pub mod store;

use image::DynamicImage;
use serde::{Deserialize, Serialize};

/// Aumente ao mudar a detecção: resultados automáticos antigos são refeitos na próxima leitura.
/// Correções do usuário nunca dependem deste número.
pub const DETECTOR_VERSION: i64 = 1;

/// Abaixo disso a região aparece como duvidosa e a interface oferece revisão.
pub const LOW_CONFIDENCE: f32 = 0.55;

/// Retângulo normalizado (0–1) em relação à página.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn area(&self) -> f32 {
        self.w.max(0.0) * self.h.max(0.0)
    }
    pub fn intersection(&self, o: &Rect) -> f32 {
        let w = self.right().min(o.right()) - self.x.max(o.x);
        let h = self.bottom().min(o.bottom()) - self.y.max(o.y);
        if w <= 0.0 || h <= 0.0 { 0.0 } else { w * h }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    Speech,
    Thought,
    Shout,
    Whisper,
    Offscreen,
    Narration,
    Caption,
    Note,
    Other,
}

impl RegionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RegionKind::Speech => "speech",
            RegionKind::Thought => "thought",
            RegionKind::Shout => "shout",
            RegionKind::Whisper => "whisper",
            RegionKind::Offscreen => "offscreen",
            RegionKind::Narration => "narration",
            RegionKind::Caption => "caption",
            RegionKind::Note => "note",
            RegionKind::Other => "other",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "speech" => RegionKind::Speech,
            "thought" => RegionKind::Thought,
            "shout" => RegionKind::Shout,
            "whisper" => RegionKind::Whisper,
            "offscreen" => RegionKind::Offscreen,
            "narration" => RegionKind::Narration,
            "caption" => RegionKind::Caption,
            "note" => RegionKind::Note,
            "other" => RegionKind::Other,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedRegion {
    pub rect: Rect,
    pub kind: RegionKind,
    pub confidence: f32,
    /// Contorno do balão em frações de `rect`; vazio quando a forma não é conhecida.
    pub outline: Vec<[f32; 2]>,
}

/// Resultado da análise de uma página, antes de qualquer correção.
/// A direção de leitura só entra ao ler o resultado, então ele vale para as duas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageAnalysis {
    pub width: u32,
    pub height: u32,
    pub panels: Vec<Rect>,
    pub regions: Vec<DetectedRegion>,
}

/// Analisa uma página já decodificada.
pub fn analyze(image: &DynamicImage) -> PageAnalysis {
    let work = raster::Work::from_image(image);
    let panels = panels::detect(&work);
    let regions = balloons::detect(&work, &panels);
    PageAnalysis { width: image.width(), height: image.height(), panels, regions }
}
