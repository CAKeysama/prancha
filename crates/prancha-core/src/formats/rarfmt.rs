//! CBR: arquivos RAR (RAR4 e RAR5) lidos pela biblioteca UnRAR oficial.
//! O UnRAR só permite leitura sequencial; páginas avulsas exigem percorrer o arquivo.

use std::collections::HashMap;
use std::path::Path;

use super::{
    FormatError, FormatResult, Listing, MAX_COMICINFO_BYTES, MAX_ENTRIES, MAX_PAGE_BYTES, PageEntry, comicinfo,
    finalize_pages, is_comicinfo_entry, is_page_entry,
};

fn map_err(e: unrar::error::UnrarError) -> FormatError {
    use unrar::error::Code;
    match e.code {
        Code::MissingPassword | Code::BadPassword => FormatError::Encrypted,
        Code::EOpen => FormatError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string())),
        _ => FormatError::Corrupt(format!("RAR inválido ou incompleto ({e})")),
    }
}

fn entry_name(h: &unrar::FileHeader) -> String {
    h.filename.to_string_lossy().replace('\\', "/")
}

pub fn list(path: &Path) -> FormatResult<Listing> {
    let archive = unrar::Archive::new(path).open_for_listing().map_err(map_err)?;
    let mut pages = Vec::new();
    let mut info_name = None;
    let mut count = 0usize;
    for entry in archive {
        let h = entry.map_err(map_err)?;
        count += 1;
        if count > MAX_ENTRIES {
            return Err(FormatError::TooLarge(format!("mais de {MAX_ENTRIES} entradas")));
        }
        if h.is_directory() {
            continue;
        }
        let name = entry_name(&h);
        if is_comicinfo_entry(&name) {
            info_name.get_or_insert(name);
        } else if is_page_entry(&name) {
            if h.is_encrypted() {
                return Err(FormatError::Encrypted);
            }
            pages.push(PageEntry { name, size: h.unpacked_size });
        }
    }
    let comic_info = info_name
        .and_then(|n| read_limited(path, &n, MAX_COMICINFO_BYTES).ok())
        .and_then(|b| comicinfo::parse(&b));
    Ok(Listing { pages: finalize_pages(pages)?, comic_info, ..Default::default() })
}

fn read_limited(path: &Path, name: &str, limit: u64) -> FormatResult<Vec<u8>> {
    let mut archive = unrar::Archive::new(path).open_for_processing().map_err(map_err)?;
    while let Some(header) = archive.read_header().map_err(map_err)? {
        if entry_name(header.entry()) == name {
            let size = header.entry().unpacked_size;
            if size > limit {
                return Err(FormatError::TooLarge(format!("entrada com {} MB", size / (1024 * 1024))));
            }
            let (data, _rest) = header.read().map_err(map_err)?;
            return Ok(data);
        }
        archive = header.skip().map_err(map_err)?;
    }
    Err(FormatError::PageNotFound)
}

pub fn read_entry(path: &Path, name: &str) -> FormatResult<Vec<u8>> {
    read_limited(path, name, MAX_PAGE_BYTES)
}

pub fn extract(
    path: &Path,
    wanted: &HashMap<String, usize>,
    sink: &mut dyn FnMut(usize, Vec<u8>) -> FormatResult<bool>,
) -> FormatResult<()> {
    let mut remaining = wanted.len();
    let mut archive = unrar::Archive::new(path).open_for_processing().map_err(map_err)?;
    while remaining > 0 {
        let Some(header) = archive.read_header().map_err(map_err)? else { break };
        let name = entry_name(header.entry());
        match wanted.get(&name) {
            Some(&idx) => {
                let size = header.entry().unpacked_size;
                if size > MAX_PAGE_BYTES {
                    return Err(FormatError::TooLarge(format!("página com {} MB", size / (1024 * 1024))));
                }
                let (data, rest) = header.read().map_err(map_err)?;
                remaining -= 1;
                if !sink(idx, data)? {
                    return Ok(());
                }
                archive = rest;
            }
            None => archive = header.skip().map_err(map_err)?,
        }
    }
    Ok(())
}
