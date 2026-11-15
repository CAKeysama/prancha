//! CB7: arquivos 7z, lidos por `sevenz-rust2` (Rust puro).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use sevenz_rust2::{ArchiveReader, Password};

use super::{
    FormatError, FormatResult, Listing, MAX_COMICINFO_BYTES, MAX_ENTRIES, MAX_PAGE_BYTES, PageEntry, comicinfo,
    finalize_pages, is_comicinfo_entry, is_page_entry, read_bounded,
};

fn map_err(e: sevenz_rust2::Error) -> FormatError {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("password") {
        FormatError::Encrypted
    } else if let sevenz_rust2::Error::Io(io, _) = e {
        FormatError::Io(io)
    } else {
        FormatError::Corrupt(format!("7z inválido ({msg})"))
    }
}

fn open(path: &Path) -> FormatResult<ArchiveReader<std::fs::File>> {
    let mut reader = ArchiveReader::open(path, Password::empty()).map_err(map_err)?;
    reader.set_thread_count(1);
    Ok(reader)
}

pub fn list(path: &Path) -> FormatResult<Listing> {
    let mut reader = open(path)?;
    let files = &reader.archive().files;
    if files.len() > MAX_ENTRIES {
        return Err(FormatError::TooLarge(format!("mais de {MAX_ENTRIES} entradas")));
    }
    let mut pages = Vec::new();
    let mut info_name = None;
    for f in files {
        if f.is_directory || !f.has_stream {
            continue;
        }
        let name = f.name.replace('\\', "/");
        if is_comicinfo_entry(&name) {
            info_name.get_or_insert((f.name.clone(), f.size));
        } else if is_page_entry(&name) {
            pages.push(PageEntry { name: f.name.clone(), size: f.size });
        }
    }
    let comic_info = info_name.and_then(|(n, size)| {
        if size > MAX_COMICINFO_BYTES {
            return None;
        }
        reader.read_file(&n).ok().and_then(|b| comicinfo::parse(&b))
    });
    Ok(Listing { pages: finalize_pages(pages)?, comic_info, ..Default::default() })
}

pub fn read_entry(path: &Path, name: &str) -> FormatResult<Vec<u8>> {
    let mut reader = open(path)?;
    let size = reader
        .archive()
        .files
        .iter()
        .find(|f| f.name == name)
        .map(|f| f.size)
        .ok_or(FormatError::PageNotFound)?;
    if size > MAX_PAGE_BYTES {
        return Err(FormatError::TooLarge(format!("página com {} MB", size / (1024 * 1024))));
    }
    reader.read_file(name).map_err(map_err)
}

pub fn extract(
    path: &Path,
    wanted: &HashMap<String, usize>,
    sink: &mut dyn FnMut(usize, Vec<u8>) -> FormatResult<bool>,
) -> FormatResult<()> {
    let mut reader = open(path)?;
    let mut remaining = wanted.len();
    let mut sink_error: Option<FormatError> = None;
    let result = reader.for_each_entries(|entry, rd| {
        match wanted.get(&entry.name) {
            Some(&idx) => {
                let data = match read_bounded(rd, entry.size, MAX_PAGE_BYTES) {
                    Ok(d) => d,
                    Err(e) => {
                        sink_error = Some(e);
                        return Ok(false);
                    }
                };
                remaining -= 1;
                match sink(idx, data) {
                    Ok(true) => Ok(remaining > 0),
                    Ok(false) => Ok(false),
                    Err(e) => {
                        sink_error = Some(e);
                        Ok(false)
                    }
                }
            }
            None => {
                // Em blocos sólidos os dados seguintes dependem dos anteriores: é preciso consumir.
                std::io::copy(&mut rd.take(u64::MAX), &mut std::io::sink())?;
                Ok(true)
            }
        }
    });
    if let Some(e) = sink_error {
        return Err(e);
    }
    result.map_err(map_err)
}
