//! Capas: escolha da imagem de origem e geração de miniaturas em cache.
//!
//! Ordem de preferência:
//! 1. página marcada como `FrontCover` no ComicInfo.xml (`metadata`);
//! 2. imagem nomeada como capa no arquivo (`cover.jpg`, `capa.png`...) (`format`);
//! 3. primeira página que decodifica corretamente (`first_page`);
//! 4. capa substituta desenhada pela interface (`placeholder`), sempre identificada como tal.
//!
//! PDFs são renderizados pelo PDF.js na interface, que devolve a miniatura (`first_page`).

use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, ImageFormat, ImageReader, Limits};

use crate::formats::{self, ComicFormat, Listing};

/// Mudar estes parâmetros invalida as miniaturas existentes (são regeneradas).
pub const COVER_PARAMS_VERSION: i64 = 1;
pub const COVER_WIDTH: u32 = 400;
/// Proporção máxima altura/largura (tiras verticais muito longas são recortadas no topo).
const MAX_ASPECT: f32 = 2.0;
const MAX_DIMENSION: u32 = 30_000;
/// Quantas páginas tentamos antes de desistir da primeira página válida.
const FIRST_PAGE_ATTEMPTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverKind {
    Pending,
    Metadata,
    Format,
    FirstPage,
    Placeholder,
}

impl CoverKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CoverKind::Pending => "pending",
            CoverKind::Metadata => "metadata",
            CoverKind::Format => "format",
            CoverKind::FirstPage => "first_page",
            CoverKind::Placeholder => "placeholder",
        }
    }
}

#[derive(Debug)]
pub struct CoverOutcome {
    pub kind: CoverKind,
    pub page: Option<usize>,
    pub jpeg: Option<Vec<u8>>,
    pub error: Option<String>,
}

/// Decodifica uma imagem com limites e gera a miniatura JPEG.
pub fn make_thumbnail(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("não foi possível identificar a imagem ({e})"))?;
    if reader.format().is_none() {
        return Err("formato de imagem não reconhecido para miniatura".into());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(1024 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| format!("imagem inválida ({e})"))?;
    thumbnail_from_image(img)
}

pub fn thumbnail_from_image(img: DynamicImage) -> Result<Vec<u8>, String> {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err("imagem sem dimensões".into());
    }
    let img = if (h as f32 / w as f32) > MAX_ASPECT {
        img.crop_imm(0, 0, w, (w as f32 * MAX_ASPECT) as u32)
    } else {
        img
    };
    let target_w = COVER_WIDTH.min(img.width());
    let target_h = ((img.height() as f32) * (target_w as f32 / img.width() as f32)).round().max(1.0) as u32;
    let thumb = img.resize_exact(target_w, target_h, image::imageops::FilterType::Triangle);
    // JPEG não tem transparência: compõe sobre branco.
    let rgba = thumb.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as f32 / 255.0;
        let blend = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)).round() as u8;
        rgb.put_pixel(x, y, image::Rgb([blend(p[0]), blend(p[1]), blend(p[2])]));
    }
    let mut out = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
    DynamicImage::ImageRgb8(rgb)
        .write_with_encoder(encoder)
        .map_err(|e| format!("falha ao gravar miniatura ({e})"))?;
    Ok(out.into_inner())
}

/// Valida que bytes recebidos (ex.: miniatura de PDF renderizada pela interface) são JPEG/PNG legítimos
/// e os normaliza com os mesmos parâmetros das demais capas.
pub fn normalize_rendered(bytes: &[u8]) -> Result<Vec<u8>, String> {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Jpeg) | Ok(ImageFormat::Png) | Ok(ImageFormat::WebP) => make_thumbnail(bytes),
        _ => Err("a miniatura recebida não é uma imagem válida".into()),
    }
}

/// Escolhe e gera a capa de uma HQ de imagens (não PDF).
pub fn generate(format: ComicFormat, path: &Path, listing: &Listing) -> CoverOutcome {
    if format == ComicFormat::Pdf {
        return CoverOutcome { kind: CoverKind::Pending, page: None, jpeg: None, error: None };
    }
    let mut last_error: Option<String> = None;
    let mut try_page = |idx: usize| -> Option<Vec<u8>> {
        let entry = listing.pages.get(idx)?;
        match formats::read_entry(format, path, entry).map_err(|e| e.to_string()).and_then(|b| make_thumbnail(&b)) {
            Ok(jpeg) => Some(jpeg),
            Err(e) => {
                last_error = Some(format!("página {}: {e}", idx + 1));
                None
            }
        }
    };

    if let Some(idx) = listing.comic_info.as_ref().and_then(|i| i.front_cover) {
        if let Some(jpeg) = try_page(idx) {
            return CoverOutcome { kind: CoverKind::Metadata, page: Some(idx), jpeg: Some(jpeg), error: None };
        }
    }
    if let Some(idx) = listing.cover_named_index() {
        if let Some(jpeg) = try_page(idx) {
            return CoverOutcome { kind: CoverKind::Format, page: Some(idx), jpeg: Some(jpeg), error: None };
        }
    }
    for idx in 0..listing.pages.len().min(FIRST_PAGE_ATTEMPTS) {
        if let Some(jpeg) = try_page(idx) {
            return CoverOutcome { kind: CoverKind::FirstPage, page: Some(idx), jpeg: Some(jpeg), error: None };
        }
    }
    CoverOutcome {
        kind: CoverKind::Placeholder,
        page: None,
        jpeg: None,
        error: Some(last_error.unwrap_or_else(|| "nenhuma página disponível para gerar a capa".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 255) as u8, (y % 255) as u8, 90, 255]));
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img).write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn thumbnail_keeps_aspect_ratio() {
        let jpeg = make_thumbnail(&png(800, 1200)).unwrap();
        let img = image::load_from_memory(&jpeg).unwrap();
        assert_eq!(img.width(), COVER_WIDTH);
        assert_eq!(img.height(), 600);
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let jpeg = make_thumbnail(&png(100, 150)).unwrap();
        let img = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((img.width(), img.height()), (100, 150));
    }

    #[test]
    fn very_tall_strips_are_cropped() {
        let jpeg = make_thumbnail(&png(400, 4000)).unwrap();
        let img = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((img.width(), img.height()), (400, 800));
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(make_thumbnail(b"definitely not an image").is_err());
        assert!(normalize_rendered(b"GIF89a....").is_err());
    }
}
