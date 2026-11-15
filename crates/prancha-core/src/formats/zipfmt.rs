//! CBZ: arquivos ZIP com imagens. Acesso aleatório por nome de entrada.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use super::{
    FormatError, FormatResult, Listing, MAX_COMICINFO_BYTES, MAX_ENTRIES, MAX_PAGE_BYTES, PageEntry, comicinfo,
    finalize_pages, is_comicinfo_entry, is_page_entry, read_bounded,
};

fn open(path: &Path) -> FormatResult<zip::ZipArchive<BufReader<File>>> {
    let file = File::open(path)?;
    zip::ZipArchive::new(BufReader::new(file)).map_err(map_err)
}

fn map_err(e: zip::result::ZipError) -> FormatError {
    match e {
        zip::result::ZipError::Io(io) => FormatError::Io(io),
        zip::result::ZipError::UnsupportedArchive(msg) if msg.to_lowercase().contains("password") => {
            FormatError::Encrypted
        }
        zip::result::ZipError::FileNotFound => FormatError::PageNotFound,
        other => FormatError::Corrupt(format!("ZIP inválido ({other})")),
    }
}

pub fn list(path: &Path) -> FormatResult<Listing> {
    let mut zip = open(path)?;
    if zip.len() > MAX_ENTRIES {
        return Err(FormatError::TooLarge(format!("mais de {MAX_ENTRIES} entradas")));
    }
    let mut pages = Vec::new();
    let mut info_name = None;
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(map_err)?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().map_err(map_err)?.to_string();
        if is_comicinfo_entry(&name) {
            info_name.get_or_insert(name);
        } else if is_page_entry(&name) {
            if f.encrypted() {
                return Err(FormatError::Encrypted);
            }
            pages.push(PageEntry { name, size: f.size() });
        }
    }
    let comic_info = info_name.and_then(|n| {
        let mut f = zip.by_name(&n).ok()?;
        let size = f.size();
        read_bounded(&mut f, size, MAX_COMICINFO_BYTES).ok().and_then(|b| comicinfo::parse(&b))
    });
    Ok(Listing { pages: finalize_pages(pages)?, comic_info, ..Default::default() })
}

pub fn read_entry(path: &Path, name: &str) -> FormatResult<Vec<u8>> {
    let mut zip = open(path)?;
    let mut f = zip.by_name(name).map_err(map_err)?;
    let size = f.size();
    read_bounded(&mut f, size, MAX_PAGE_BYTES)
}
