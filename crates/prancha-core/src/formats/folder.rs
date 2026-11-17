//! Pastas com imagens: cada pasta que contém imagens diretamente é uma HQ.

use std::path::Path;

use super::{
    FormatError, FormatResult, Listing, MAX_COMICINFO_BYTES, MAX_ENTRIES, MAX_PAGE_BYTES, PageEntry, comicinfo,
    finalize_pages, is_comicinfo_entry, is_page_entry, read_bounded,
};
use crate::location;

pub fn list(path: &Path) -> FormatResult<Listing> {
    let mut pages = Vec::new();
    let mut info = None;
    for entry in location::read_dir(path)? {
        if !entry.is_file {
            continue;
        }
        let name = entry.name;
        if is_comicinfo_entry(&name) {
            let size = entry.len;
            if let Ok(mut f) = location::open(&entry.path) {
                if let Ok(bytes) = read_bounded(&mut f, size, MAX_COMICINFO_BYTES) {
                    info = comicinfo::parse(&bytes);
                }
            }
        } else if is_page_entry(&name) {
            pages.push(PageEntry { name, size: entry.len });
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
    let path = location::child(dir, name)?;
    let mut f = location::open(&path)?;
    let size = f.metadata()?.len();
    read_bounded(&mut f, size, MAX_PAGE_BYTES)
}
