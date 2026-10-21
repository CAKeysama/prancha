//! Adaptadores de formato. Cada formato sabe listar suas páginas, ler uma página
//! e (para formatos sequenciais) extrair várias páginas em uma única passada.

pub mod comicinfo;
mod folder;
pub mod pdf;
mod rarfmt;
mod sevenz;
mod tarfmt;
mod zipfmt;

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::natural::natural_path_cmp;
pub use comicinfo::ComicInfo;

/// Limites de recursos para proteger contra arquivos malformados ou desproporcionais.
pub const MAX_ENTRIES: usize = 20_000;
pub const MAX_PAGE_BYTES: u64 = 150 * 1024 * 1024;
pub const MAX_COMICINFO_BYTES: u64 = 2 * 1024 * 1024;

pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "jfif", "png", "webp", "gif", "bmp", "avif"];
pub const COMIC_EXTENSIONS: &[&str] = &["cbz", "zip", "cbr", "rar", "cb7", "7z", "cbt", "tar", "pdf"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComicFormat {
    Cbz,
    Cbr,
    Cb7,
    Cbt,
    Pdf,
    Folder,
}

impl ComicFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ComicFormat::Cbz => "cbz",
            ComicFormat::Cbr => "cbr",
            ComicFormat::Cb7 => "cb7",
            ComicFormat::Cbt => "cbt",
            ComicFormat::Pdf => "pdf",
            ComicFormat::Folder => "folder",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "cbz" => ComicFormat::Cbz,
            "cbr" => ComicFormat::Cbr,
            "cb7" => ComicFormat::Cb7,
            "cbt" => ComicFormat::Cbt,
            "pdf" => ComicFormat::Pdf,
            "folder" => ComicFormat::Folder,
            _ => return None,
        })
    }

    /// Formatos sem acesso aleatório barato (RAR e 7z, possivelmente sólidos):
    /// as páginas são extraídas uma vez para o cache interno.
    pub fn is_sequential(self) -> bool {
        matches!(self, ComicFormat::Cbr | ComicFormat::Cb7)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("Formato não suportado ({0}). Formatos aceitos: CBZ, CBR, CB7, CBT, PDF e pastas com imagens.")]
    Unsupported(String),
    #[error("Arquivo corrompido ou ilegível: {0}")]
    Corrupt(String),
    #[error("Nenhuma página de imagem válida foi encontrada.")]
    NoPages,
    #[error("O arquivo ultrapassa os limites de segurança do Prancha: {0}")]
    TooLarge(String),
    #[error("Arquivos protegidos por senha não são suportados.")]
    Encrypted,
    #[error("A página solicitada não existe neste arquivo.")]
    PageNotFound,
    #[error("{}", crate::error::io_message(.0))]
    Io(#[from] std::io::Error),
}

impl FormatError {
    pub fn code(&self) -> &'static str {
        match self {
            FormatError::Unsupported(_) => "unsupported",
            FormatError::Corrupt(_) => "corrupt",
            FormatError::NoPages => "no_pages",
            FormatError::TooLarge(_) => "too_large",
            FormatError::Encrypted => "encrypted",
            FormatError::PageNotFound => "page_not_found",
            FormatError::Io(e) => match e.kind() {
                std::io::ErrorKind::NotFound => "file_missing",
                std::io::ErrorKind::PermissionDenied => "permission_denied",
                _ => "io",
            },
        }
    }
}

impl From<FormatError> for crate::error::AppError {
    fn from(e: FormatError) -> Self {
        crate::error::AppError::user(e.code(), e.to_string())
    }
}

pub type FormatResult<T> = Result<T, FormatError>;

/// Uma página dentro do contêiner. `name` é a chave de leitura no formato original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageEntry {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Default)]
pub struct Listing {
    /// Páginas em ordem de leitura (vazio para PDF).
    pub pages: Vec<PageEntry>,
    pub comic_info: Option<ComicInfo>,
    /// Número de páginas quando não há lista de entradas (PDF).
    pub page_count: usize,
    pub pdf_title: Option<String>,
    pub pdf_author: Option<String>,
}

impl Listing {
    /// Índice de uma imagem cujo nome a identifica como capa (`cover.jpg`, `capa.png`...).
    pub fn cover_named_index(&self) -> Option<usize> {
        self.pages.iter().position(|p| is_cover_name(&p.name))
    }
}

pub fn extension_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_comic_file(path: &Path) -> bool {
    COMIC_EXTENSIONS.contains(&extension_of(path).as_str())
}

pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    match lower.rsplit_once('.') {
        Some((_, ext)) => IMAGE_EXTENSIONS.contains(&ext),
        None => false,
    }
}

/// Entradas que não são páginas: metadados do macOS, arquivos ocultos, miniaturas do Windows.
fn is_ignored_entry(name: &str) -> bool {
    let norm = name.replace('\\', "/");
    norm.split('/').any(|c| c.starts_with('.') || c.eq_ignore_ascii_case("__MACOSX"))
        || norm.rsplit('/').next().is_some_and(|b| b.eq_ignore_ascii_case("thumbs.db"))
}

pub(crate) fn is_page_entry(name: &str) -> bool {
    !is_ignored_entry(name) && is_image_name(name)
}

pub(crate) fn is_comicinfo_entry(name: &str) -> bool {
    let norm = name.replace('\\', "/");
    norm.rsplit('/').next().is_some_and(|b| b.eq_ignore_ascii_case("ComicInfo.xml"))
}

fn is_cover_name(name: &str) -> bool {
    let norm = name.replace('\\', "/");
    let base = norm.rsplit('/').next().unwrap_or(&norm).to_lowercase();
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(&base);
    const NAMES: &[&str] = &["cover", "capa", "folder", "front", "frontcover", "poster"];
    if NAMES.contains(&stem) {
        return true;
    }
    // "0_cover", "00-capa", "000 cover"
    let trimmed = stem.trim_start_matches('0').trim_start_matches(['-', '_', ' ']);
    trimmed != stem && (trimmed == "cover" || trimmed == "capa")
}

/// Ordena as páginas em ordem natural e valida limites.
pub(crate) fn finalize_pages(mut pages: Vec<PageEntry>) -> FormatResult<Vec<PageEntry>> {
    if pages.len() > MAX_ENTRIES {
        return Err(FormatError::TooLarge(format!("mais de {MAX_ENTRIES} páginas")));
    }
    pages.sort_by(|a, b| natural_path_cmp(&a.name, &b.name));
    Ok(pages)
}

pub(crate) fn read_bounded(reader: &mut dyn Read, declared: u64, limit: u64) -> FormatResult<Vec<u8>> {
    if declared > limit {
        return Err(FormatError::TooLarge(format!(
            "uma entrada tem {} MB (limite {} MB)",
            declared / (1024 * 1024),
            limit / (1024 * 1024)
        )));
    }
    let mut buf = Vec::with_capacity(declared.min(limit) as usize);
    reader.take(limit + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(FormatError::TooLarge("conteúdo maior que o declarado".into()));
    }
    Ok(buf)
}

/// Identifica o formato pelo conteúdo (assinatura), não apenas pela extensão.
/// Um `.cbr` que na verdade é ZIP é tratado como CBZ, situação comum na prática.
pub fn detect(path: &Path) -> FormatResult<ComicFormat> {
    let meta = std::fs::metadata(path)?;
    if meta.is_dir() {
        return Ok(ComicFormat::Folder);
    }
    let ext = extension_of(path);
    if !COMIC_EXTENSIONS.contains(&ext.as_str()) {
        return Err(FormatError::Unsupported(if ext.is_empty() { "sem extensão".into() } else { format!(".{ext}") }));
    }
    let mut head = [0u8; 1024];
    let mut file = File::open(path)?;
    let mut read = 0;
    while read < head.len() {
        let n = file.read(&mut head[read..])?;
        if n == 0 {
            break;
        }
        read += n;
    }
    let head = &head[..read];
    if head.is_empty() {
        return Err(FormatError::Corrupt("o arquivo está vazio".into()));
    }
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return Ok(ComicFormat::Cbz);
    }
    if head.starts_with(b"Rar!\x1a\x07") {
        return Ok(ComicFormat::Cbr);
    }
    if head.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Ok(ComicFormat::Cb7);
    }
    if head.windows(5).any(|w| w == b"%PDF-") {
        return Ok(ComicFormat::Pdf);
    }
    if head.len() >= 262 && &head[257..262] == b"ustar" {
        return Ok(ComicFormat::Cbt);
    }
    Err(FormatError::Corrupt(
        "o conteúdo não corresponde a nenhum formato de quadrinhos conhecido (cabeçalho inválido)".into(),
    ))
}

pub fn list(format: ComicFormat, path: &Path) -> FormatResult<Listing> {
    let listing = match format {
        ComicFormat::Cbz => zipfmt::list(path)?,
        ComicFormat::Cbr => rarfmt::list(path)?,
        ComicFormat::Cb7 => sevenz::list(path)?,
        ComicFormat::Cbt => tarfmt::list(path)?,
        ComicFormat::Folder => folder::list(path)?,
        ComicFormat::Pdf => pdf::list(path)?,
    };
    if format != ComicFormat::Pdf && listing.pages.is_empty() {
        return Err(FormatError::NoPages);
    }
    Ok(listing)
}

pub fn page_count(format: ComicFormat, listing: &Listing) -> usize {
    if format == ComicFormat::Pdf { listing.page_count } else { listing.pages.len() }
}

/// Lê uma única entrada. Para RAR/7z isso percorre o arquivo até a entrada.
pub fn read_entry(format: ComicFormat, path: &Path, entry: &PageEntry) -> FormatResult<Vec<u8>> {
    match format {
        ComicFormat::Cbz => zipfmt::read_entry(path, &entry.name),
        ComicFormat::Cbr => rarfmt::read_entry(path, &entry.name),
        ComicFormat::Cb7 => sevenz::read_entry(path, &entry.name),
        ComicFormat::Cbt => tarfmt::read_entry(path, &entry.name),
        ComicFormat::Folder => folder::read_entry(path, &entry.name),
        ComicFormat::Pdf => Err(FormatError::PageNotFound),
    }
}

/// Extrai, em uma passada, as entradas de `wanted` (nome → índice da página).
/// `sink` recebe cada página; devolver `false` interrompe a extração.
pub fn extract_pages(
    format: ComicFormat,
    path: &Path,
    wanted: &HashMap<String, usize>,
    sink: &mut dyn FnMut(usize, Vec<u8>) -> FormatResult<bool>,
) -> FormatResult<()> {
    match format {
        ComicFormat::Cbr => rarfmt::extract(path, wanted, sink),
        ComicFormat::Cb7 => sevenz::extract(path, wanted, sink),
        ComicFormat::Pdf => Err(FormatError::PageNotFound),
        _ => {
            let mut items: Vec<(&String, &usize)> = wanted.iter().collect();
            items.sort_by_key(|(_, i)| **i);
            for (name, idx) in items {
                let data = read_entry(format, path, &PageEntry { name: name.clone(), size: 0 })?;
                if !sink(*idx, data)? {
                    break;
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_entry_filter() {
        assert!(is_page_entry("01.jpg"));
        assert!(is_page_entry("cap1/Pagina 02.PNG"));
        assert!(!is_page_entry("__MACOSX/01.jpg"));
        assert!(!is_page_entry(".hidden.jpg"));
        assert!(!is_page_entry("ComicInfo.xml"));
        assert!(!is_page_entry("notes.txt"));
        assert!(is_comicinfo_entry("sub/comicinfo.XML"));
    }

    #[test]
    fn cover_names() {
        assert!(is_cover_name("cover.jpg"));
        assert!(is_cover_name("x/Capa.png"));
        assert!(is_cover_name("000_cover.webp"));
        assert!(!is_cover_name("covers-gallery-2.jpg"));
        assert!(!is_cover_name("001.jpg"));
    }
}
