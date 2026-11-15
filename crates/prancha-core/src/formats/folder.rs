//! Pastas com imagens: cada pasta que contém imagens diretamente é uma HQ.

use std::fs::File;
use std::path::Path;

use super::{
    FormatError, FormatResult, Listing, MAX_COMICINFO_BYTES, MAX_ENTRIES, MAX_PAGE_BYTES, PageEntry, comicinfo,
    finalize_pages, is_comicinfo_entry, is_page_entry, read_bounded,
};

pub fn list(path: &Path) -> FormatResult<Listing> {
    let mut pages = Vec::new();
    let mut info = None;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if !ft.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if is_comicinfo_entry(&name) {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if let Ok(mut f) = File::open(entry.path()) {
                if let Ok(bytes) = read_bounded(&mut f, size, MAX_COMICINFO_BYTES) {
                    info = comicinfo::parse(&bytes);
                }
            }
        } else if is_page_entry(&name) {
            pages.push(PageEntry { name, size: entry.metadata().map(|m| m.len()).unwrap_or(0) });
            if pages.len() > MAX_ENTRIES {
                return Err(FormatError::TooLarge(format!("mais de {MAX_ENTRIES} imagens")));
            }
        }
    }
    Ok(Listing { pages: finalize_pages(pages)?, comic_info: info, ..Default::default() })
}

/// Lê uma imagem da pasta. O nome nunca pode sair da pasta da HQ.
pub fn read_entry(dir: &Path, name: &str) -> FormatResult<Vec<u8>> {
    if name.is_empty() || name.contains(['/', '\\']) || name == ".." || name == "." {
        return Err(FormatError::PageNotFound);
    }
    let path = dir.join(name);
    let mut f = File::open(&path)?;
    let size = f.metadata()?.len();
    read_bounded(&mut f, size, MAX_PAGE_BYTES)
}
