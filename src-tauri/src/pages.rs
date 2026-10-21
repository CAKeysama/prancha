//! Serviço de páginas do leitor.
//!
//! - ZIP, TAR e pastas: cada página é lida diretamente do arquivo original.
//! - RAR e 7z: o arquivo é percorrido uma única vez e as páginas são gravadas no cache
//!   interno do app (`cache/pages/<id>-<impressão>/<índice>`). Os nomes das entradas
//!   nunca viram caminhos no disco, o que elimina path traversal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::formats::{self, ComicFormat, FormatError, FormatResult, PageEntry};
use crate::library::ComicFileInfo;

/// Tamanho máximo do cache de páginas extraídas antes de descartar as HQs menos recentes.
const PAGE_CACHE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const LISTING_CACHE_ENTRIES: usize = 32;
const COMPLETE_MARKER: &str = "complete";

pub struct CachedListing {
    pub path: String,
    pub fingerprint: String,
    pub format: ComicFormat,
    pub pages: Vec<PageEntry>,
    pub page_count: usize,
}

struct ExtractState {
    ready: Vec<bool>,
    finished: bool,
    error: Option<String>,
}

struct Extraction {
    dir: PathBuf,
    state: Mutex<ExtractState>,
    cv: Condvar,
}

pub struct PageService {
    cache_dir: PathBuf,
    listings: Mutex<HashMap<i64, Arc<CachedListing>>>,
    extractions: Mutex<HashMap<i64, Arc<Extraction>>>,
}

pub fn mime_for(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    match lower.rsplit_once('.').map(|(_, e)| e) {
        Some("jpg" | "jpeg" | "jpe" | "jfif") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    }
}

pub fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

impl PageService {
    pub fn new(cache_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&cache_dir);
        PageService { cache_dir, listings: Mutex::new(HashMap::new()), extractions: Mutex::new(HashMap::new()) }
    }

    /// Lista as páginas (com cache em memória invalidado por caminho e impressão digital).
    pub fn listing(&self, info: &ComicFileInfo) -> FormatResult<Arc<CachedListing>> {
        if let Some(l) = self.listings.lock().unwrap().get(&info.id) {
            if l.path == info.path && l.fingerprint == info.fingerprint {
                return Ok(l.clone());
            }
        }
        let format = ComicFormat::parse(&info.format).ok_or_else(|| FormatError::Unsupported(info.format.clone()))?;
        let path = Path::new(&info.path);
        if !path.exists() {
            return Err(FormatError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "missing")));
        }
        let listing = formats::list(format, path)?;
        let cached = Arc::new(CachedListing {
            path: info.path.clone(),
            fingerprint: info.fingerprint.clone(),
            format,
            page_count: formats::page_count(format, &listing),
            pages: listing.pages,
        });
        let mut map = self.listings.lock().unwrap();
        if map.len() >= LISTING_CACHE_ENTRIES {
            map.clear();
        }
        map.insert(info.id, cached.clone());
        Ok(cached)
    }

    pub fn invalidate(&self, id: i64) {
        self.listings.lock().unwrap().remove(&id);
    }

    /// Devolve os bytes e o tipo MIME de uma página (índice começando em 0).
    pub fn read_page(&self, info: &ComicFileInfo, index: usize) -> FormatResult<(Vec<u8>, &'static str)> {
        let listing = self.listing(info)?;
        let entry = listing.pages.get(index).ok_or(FormatError::PageNotFound)?;
        let mime = mime_for(&entry.name);
        if !listing.format.is_sequential() {
            let data = formats::read_entry(listing.format, Path::new(&listing.path), entry)?;
            return Ok((data, mime));
        }
        let extraction = self.extraction(info, &listing)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(180);
        let mut state = extraction.state.lock().unwrap();
        loop {
            if state.ready.get(index).copied().unwrap_or(false) {
                break;
            }
            if state.finished {
                return Err(match &state.error {
                    Some(e) => FormatError::Corrupt(e.clone()),
                    None => FormatError::PageNotFound,
                });
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(FormatError::Corrupt("tempo esgotado ao preparar as páginas".into()));
            }
            state = extraction.cv.wait_timeout(state, deadline - now).unwrap().0;
        }
        drop(state);
        let data = std::fs::read(extraction.dir.join(index.to_string()))?;
        Ok((data, mime))
    }

    fn extraction_dir(&self, info: &ComicFileInfo) -> PathBuf {
        let fp: String = info.fingerprint.chars().take(16).collect();
        self.cache_dir.join(format!("{}-{}", info.id, fp))
    }

    fn extraction(&self, info: &ComicFileInfo, listing: &Arc<CachedListing>) -> FormatResult<Arc<Extraction>> {
        let mut map = self.extractions.lock().unwrap();
        let dir = self.extraction_dir(info);
        if let Some(e) = map.get(&info.id) {
            let state = e.state.lock().unwrap();
            let failed = state.finished && state.error.is_some();
            if e.dir == dir && !failed && e.dir.exists() {
                drop(state);
                return Ok(e.clone());
            }
        }
        let count = listing.pages.len();
        if dir.join(COMPLETE_MARKER).exists() {
            let _ = filetime_touch(&dir.join(COMPLETE_MARKER));
            let e = Arc::new(Extraction {
                dir,
                state: Mutex::new(ExtractState { ready: vec![true; count], finished: true, error: None }),
                cv: Condvar::new(),
            });
            map.insert(info.id, e.clone());
            return Ok(e);
        }
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let active: Vec<PathBuf> =
            map.values().filter(|e| !e.state.lock().unwrap().finished).map(|e| e.dir.clone()).collect();
        self.prune(info.id, &active);
        let e = Arc::new(Extraction {
            dir: dir.clone(),
            state: Mutex::new(ExtractState { ready: vec![false; count], finished: false, error: None }),
            cv: Condvar::new(),
        });
        map.insert(info.id, e.clone());
        drop(map);

        let wanted: HashMap<String, usize> =
            listing.pages.iter().enumerate().map(|(i, p)| (p.name.clone(), i)).collect();
        let worker = e.clone();
        let format = listing.format;
        let path = PathBuf::from(&listing.path);
        std::thread::Builder::new()
            .name(format!("prancha-extract-{}", info.id))
            .spawn(move || {
                let result = formats::extract_pages(format, &path, &wanted, &mut |idx, data| {
                    let tmp = worker.dir.join(format!("{idx}.tmp"));
                    std::fs::write(&tmp, &data)?;
                    std::fs::rename(&tmp, worker.dir.join(idx.to_string()))?;
                    let mut st = worker.state.lock().unwrap();
                    if let Some(r) = st.ready.get_mut(idx) {
                        *r = true;
                    }
                    worker.cv.notify_all();
                    Ok(true)
                });
                let mut st = worker.state.lock().unwrap();
                st.finished = true;
                match result {
                    Ok(()) => {
                        if st.ready.iter().all(|r| *r) {
                            let _ = std::fs::write(worker.dir.join(COMPLETE_MARKER), b"ok");
                        }
                    }
                    Err(err) => st.error = Some(err.to_string()),
                }
                worker.cv.notify_all();
            })
            .map_err(FormatError::Io)?;
        Ok(e)
    }

    /// Remove extrações antigas quando o cache passa do limite.
    /// Recebe as extrações ativas porque é chamada com o mapa já travado.
    fn prune(&self, keep_id: i64, active: &[PathBuf]) {
        let Ok(entries) = std::fs::read_dir(&self.cache_dir) else { return };
        let mut dirs: Vec<(PathBuf, std::time::SystemTime, u64)> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter(|e| !e.file_name().to_string_lossy().starts_with(&format!("{keep_id}-")))
            .map(|e| {
                let p = e.path();
                let t = std::fs::metadata(p.join(COMPLETE_MARKER))
                    .or_else(|_| e.metadata())
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                let size = dir_size(&p);
                (p, t, size)
            })
            .collect();
        let mut total: u64 = dirs.iter().map(|d| d.2).sum();
        dirs.sort_by_key(|d| d.1);
        for (p, _, size) in dirs {
            if total <= PAGE_CACHE_LIMIT {
                break;
            }
            if active.contains(&p) {
                continue;
            }
            if std::fs::remove_dir_all(&p).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }

    pub fn cache_size(&self) -> u64 {
        dir_size(&self.cache_dir)
    }

    /// Limpa o cache de páginas extraídas (as HQs originais não são tocadas).
    pub fn clear_cache(&self) -> u64 {
        let mut map = self.extractions.lock().unwrap();
        let active: Vec<PathBuf> =
            map.values().filter(|e| !e.state.lock().unwrap().finished).map(|e| e.dir.clone()).collect();
        map.retain(|_, e| active.contains(&e.dir));
        drop(map);
        let mut freed = 0;
        if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
            for e in entries.filter_map(Result::ok) {
                let p = e.path();
                if active.contains(&p) {
                    continue;
                }
                let size = dir_size(&p);
                if std::fs::remove_dir_all(&p).is_ok() {
                    freed += size;
                }
            }
        }
        freed
    }

    pub fn forget_comic(&self, id: i64) {
        self.invalidate(id);
        self.extractions.lock().unwrap().remove(&id);
        if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
            for e in entries.filter_map(Result::ok) {
                if e.file_name().to_string_lossy().starts_with(&format!("{id}-")) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}

/// Atualiza a data de modificação (usada como "último uso" na limpeza do cache).
fn filetime_touch(path: &Path) -> std::io::Result<()> {
    let f = std::fs::OpenOptions::new().write(true).open(path)?;
    f.set_modified(std::time::SystemTime::now())
}
