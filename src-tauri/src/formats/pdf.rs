//! PDF: o backend valida a estrutura e conta páginas (lopdf, sem renderizar);
//! a renderização das páginas e da miniatura é feita pelo PDF.js na interface.

use std::path::Path;

use super::{FormatError, FormatResult, Listing};
use crate::text::clean_opt;

/// Acima disso não carregamos a estrutura na memória durante a importação;
/// o PDF.js informa o número de páginas ao abrir.
const MAX_METADATA_BYTES: u64 = 1024 * 1024 * 1024;

/// Títulos genéricos gerados por ferramentas de exportação não servem como título.
fn useful_title(t: &str) -> bool {
    let lower = t.to_lowercase();
    !(lower.starts_with("microsoft word")
        || lower.starts_with("untitled")
        || lower.starts_with("sem título")
        || lower.ends_with(".pdf")
        || lower.ends_with(".doc")
        || lower.ends_with(".docx")
        || lower.len() < 2)
}

pub fn list(path: &Path) -> FormatResult<Listing> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_METADATA_BYTES {
        return Ok(Listing::default());
    }
    let meta = lopdf::Document::load_metadata(path).map_err(|e| match e {
        lopdf::Error::IO(io) => FormatError::Io(io),
        other => FormatError::Corrupt(format!("PDF inválido ({other})")),
    })?;
    if meta.page_count == 0 {
        return Err(FormatError::NoPages);
    }
    Ok(Listing {
        page_count: meta.page_count as usize,
        pdf_title: clean_opt(meta.title).filter(|t| useful_title(t)),
        pdf_author: clean_opt(meta.author),
        ..Default::default()
    })
}
