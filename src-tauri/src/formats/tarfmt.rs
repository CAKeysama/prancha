//! CBT: arquivos TAR com imagens. TAR não é compactado, então pular entradas é barato.

use std::fs::File;
use std::path::Path;

use super::{
    FormatError, FormatResult, Listing, MAX_COMICINFO_BYTES, MAX_ENTRIES, MAX_PAGE_BYTES, PageEntry, comicinfo,
    finalize_pages, is_comicinfo_entry, is_page_entry, read_bounded,
};

fn corrupt(e: std::io::Error) -> FormatError {
    match e.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => FormatError::Io(e),
        _ => FormatError::Corrupt(format!("TAR inválido ({e})")),
    }
}

fn for_each(path: &Path, mut f: impl FnMut(&str, &mut tar::Entry<'_, File>) -> FormatResult<bool>) -> FormatResult<()> {
    let mut archive = tar::Archive::new(File::open(path)?);
    let mut count = 0usize;
    for entry in archive.entries_with_seek().map_err(corrupt)? {
        let mut entry = entry.map_err(corrupt)?;
        count += 1;
        if count > MAX_ENTRIES {
            return Err(FormatError::TooLarge(format!("mais de {MAX_ENTRIES} entradas")));
        }
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry.path().map_err(corrupt)?.to_string_lossy().replace('\\', "/");
        if !f(&name, &mut entry)? {
            break;
        }
    }
    Ok(())
}

pub fn list(path: &Path) -> FormatResult<Listing> {
    let mut pages = Vec::new();
    let mut info = None;
    for_each(path, |name, entry| {
        if is_comicinfo_entry(name) && info.is_none() {
            let size = entry.size();
            if let Ok(bytes) = read_bounded(entry, size, MAX_COMICINFO_BYTES) {
                info = comicinfo::parse(&bytes);
            }
        } else if is_page_entry(name) {
            pages.push(PageEntry { name: name.to_string(), size: entry.size() });
        }
        Ok(true)
    })?;
    Ok(Listing { pages: finalize_pages(pages)?, comic_info: info, ..Default::default() })
}

pub fn read_entry(path: &Path, name: &str) -> FormatResult<Vec<u8>> {
    let mut out = None;
    for_each(path, |entry_name, entry| {
        if entry_name == name {
            let size = entry.size();
            out = Some(read_bounded(entry, size, MAX_PAGE_BYTES)?);
            return Ok(false);
        }
        Ok(true)
    })?;
    out.ok_or(FormatError::PageNotFound)
}
