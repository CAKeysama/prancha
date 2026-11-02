//! Fila de importação e manutenção de capas, executadas em uma thread dedicada.
//!
//! Fluxo por item: expandir pastas → identificar formato → validar/listar páginas →
//! impressão digital → duplicatas → registrar → gerar capa → informar o resultado.

use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Value, json};

use crate::covers::{self, COVER_PARAMS_VERSION, CoverKind};
use crate::error::AppResult;
use crate::formats::{self, ComicFormat, FormatError, Listing};
use crate::library::{self, NewComic};
use crate::natural::natural_path_cmp;
use crate::text::title_from_file_name;

pub type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub const EVENT_IMPORT: &str = "import:progress";
pub const EVENT_LIBRARY: &str = "library:changed";
pub const EVENT_COVERS: &str = "covers:progress";

/// Profundidade máxima ao procurar quadrinhos dentro de uma pasta escolhida.
const MAX_SCAN_DEPTH: usize = 8;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemResult {
    pub path: String,
    pub name: String,
    /// added | relinked | duplicate | skipped | failed
    pub status: String,
    pub message: Option<String>,
    pub comic_id: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobCounts {
    pub total: i64,
    pub processed: i64,
    pub added: i64,
    pub skipped: i64,
    pub failed: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicatePolicy {
    Skip,
    Allow,
}

pub fn duplicate_policy(conn: &Connection) -> DuplicatePolicy {
    match library::get_setting(conn, "library.duplicates").ok().flatten() {
        Some(Value::String(s)) if s == "allow" => DuplicatePolicy::Allow,
        _ => DuplicatePolicy::Skip,
    }
}

/// Caminho absoluto sem o prefixo `\\?\` do Windows, para comparação estável.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    let c = std::fs::canonicalize(path)?;
    let s = c.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        if !rest.starts_with("UNC\\") {
            return Ok(PathBuf::from(rest));
        }
        return Ok(PathBuf::from(format!(r"\\{}", &rest[4..])));
    }
    Ok(c)
}

fn display_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn failed(path: &Path, message: impl Into<String>) -> ItemResult {
    ItemResult {
        path: path.to_string_lossy().to_string(),
        name: display_name(path),
        status: "failed".into(),
        message: Some(message.into()),
        comic_id: None,
    }
}

/// Expande os caminhos escolhidos em candidatos (arquivos de HQ e pastas com imagens).
pub fn expand(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<ItemResult>) {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                errors.push(failed(path, crate::error::io_message(&e)));
                continue;
            }
        };
        if meta.is_file() {
            if formats::is_comic_file(path) {
                candidates.push(path.clone());
            } else {
                let ext = formats::extension_of(path);
                errors.push(failed(path, FormatError::Unsupported(if ext.is_empty() { "sem extensão".into() } else { format!(".{ext}") }).to_string()));
            }
            continue;
        }
        let mut found: Vec<PathBuf> = Vec::new();
        let mut image_dirs: BTreeSet<PathBuf> = BTreeSet::new();
        for entry in walkdir::WalkDir::new(path).max_depth(MAX_SCAN_DEPTH).follow_links(false) {
            match entry {
                Ok(e) => {
                    if !e.file_type().is_file() {
                        continue;
                    }
                    let p = e.path();
                    let name = e.file_name().to_string_lossy();
                    if name.starts_with('.') {
                        continue;
                    }
                    if formats::is_comic_file(p) {
                        found.push(p.to_path_buf());
                    } else if formats::KNOWN_UNSUPPORTED_EXTENSIONS.contains(&formats::extension_of(p).as_str()) {
                        errors.push(failed(p, FormatError::Unsupported(format!(".{}", formats::extension_of(p))).to_string()));
                    } else if formats::is_image_name(&name) {
                        if let Some(parent) = p.parent() {
                            image_dirs.insert(parent.to_path_buf());
                        }
                    }
                }
                Err(e) => {
                    let p = e.path().map(Path::to_path_buf).unwrap_or_else(|| path.clone());
                    let msg = e.io_error().map(crate::error::io_message).unwrap_or_else(|| e.to_string());
                    errors.push(failed(&p, format!("Não foi possível ler a pasta: {msg}")));
                }
            }
        }
        found.extend(image_dirs);
        if found.is_empty() {
            errors.push(failed(path, "A pasta não contém imagens nem arquivos de quadrinhos compatíveis."));
        }
        found.sort_by(|a, b| natural_path_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
        candidates.extend(found);
    }
    candidates.dedup();
    (candidates, errors)
}

fn file_mtime(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

/// Impressão digital do conteúdo (ver ARCHITECTURE.md, "Duplicatas").
/// - Arquivos: BLAKE3 de todos os bytes.
/// - Pastas: BLAKE3 da lista ordenada (nome, tamanho) de páginas + bytes da primeira página.
pub fn fingerprint(format: ComicFormat, path: &Path, listing: &Listing) -> Result<String, FormatError> {
    let mut hasher = blake3::Hasher::new();
    if format == ComicFormat::Folder {
        hasher.update(b"prancha-folder-v1\0");
        for p in &listing.pages {
            hasher.update(p.name.as_bytes());
            hasher.update(b"\0");
            hasher.update(&p.size.to_le_bytes());
        }
        if let Some(first) = listing.pages.first() {
            let mut f = File::open(path.join(&first.name))?;
            hasher.update_reader(&mut f)?;
        }
    } else {
        let mut f = File::open(path)?;
        hasher.update_reader(&mut f)?;
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn write_cover_file(covers_dir: &Path, id: i64, jpeg: &[u8]) -> std::io::Result<String> {
    std::fs::create_dir_all(covers_dir)?;
    let name = format!("{id}.jpg");
    let tmp = covers_dir.join(format!("{id}.jpg.tmp"));
    std::fs::write(&tmp, jpeg)?;
    std::fs::rename(&tmp, covers_dir.join(&name))?;
    Ok(name)
}

/// Gera e grava a capa de uma HQ já registrada. Nunca falha a importação:
/// em caso de erro, registra a capa substituta e o motivo.
pub fn apply_cover(conn: &Connection, covers_dir: &Path, id: i64, format: ComicFormat, path: &Path, listing: &Listing) -> AppResult<Option<String>> {
    let outcome = covers::generate(format, path, listing);
    match (outcome.kind, outcome.jpeg) {
        (CoverKind::Pending, _) => {
            library::set_cover(conn, id, "pending", None, None, None, 0)?;
            Ok(None)
        }
        (kind, Some(jpeg)) => match write_cover_file(covers_dir, id, &jpeg) {
            Ok(file) => {
                library::set_cover(conn, id, kind.as_str(), Some(&file), outcome.page.map(|p| p as i64), None, COVER_PARAMS_VERSION)?;
                Ok(None)
            }
            Err(e) => {
                let msg = format!("não foi possível gravar a miniatura ({e})");
                library::set_cover(conn, id, "placeholder", None, None, Some(&msg), COVER_PARAMS_VERSION)?;
                Ok(Some(msg))
            }
        },
        (_, None) => {
            let msg = outcome.error.unwrap_or_else(|| "capa indisponível".into());
            library::set_cover(conn, id, "placeholder", None, None, Some(&msg), COVER_PARAMS_VERSION)?;
            Ok(Some(msg))
        }
    }
}

fn build_new_comic(format: ComicFormat, path: &Path, listing: &Listing, fp: String, size: i64, mtime: Option<i64>) -> NewComic {
    let file_name = display_name(path);
    let mut sources: HashMap<String, String> = HashMap::new();
    let info = listing.comic_info.clone().unwrap_or_default();
    let title = if let Some(t) = info.title.clone() {
        sources.insert("title".into(), "file".into());
        t
    } else if let Some(series) = info.series.clone() {
        sources.insert("title".into(), "file".into());
        match &info.number {
            Some(n) => format!("{series} #{n}"),
            None => series,
        }
    } else if let Some(t) = listing.pdf_title.clone() {
        sources.insert("title".into(), "file".into());
        t
    } else {
        sources.insert("title".into(), "filename".into());
        title_from_file_name(&file_name, format == ComicFormat::Folder)
    };
    let writer = info.writer.clone().or_else(|| listing.pdf_author.clone());
    for (field, present) in [
        ("series", info.series.is_some()),
        ("issueNumber", info.number.is_some()),
        ("volume", info.volume.is_some()),
        ("year", info.year.is_some()),
        ("writer", writer.is_some()),
        ("artist", info.artist.is_some()),
        ("publisher", info.publisher.is_some()),
        ("summary", info.summary.is_some()),
        ("tags", !info.tags.is_empty()),
    ] {
        if present {
            sources.insert(field.into(), "file".into());
        }
    }
    let count = formats::page_count(format, listing);
    NewComic {
        path: path.to_string_lossy().to_string(),
        file_name,
        format: format.as_str().into(),
        file_size: size,
        file_mtime: mtime,
        fingerprint: fp,
        page_count: if count > 0 { Some(count as i64) } else { None },
        title,
        series: info.series,
        issue_number: info.number,
        volume: info.volume,
        year: info.year,
        writer,
        artist: info.artist,
        publisher: info.publisher,
        summary: info.summary,
        tags: info.tags,
        field_sources: sources,
    }
}

/// Processa um candidato. Erros viram `ItemResult` com explicação; nada é descartado em silêncio.
pub fn process_one(conn: &Connection, covers_dir: &Path, raw: &Path, policy: DuplicatePolicy) -> ItemResult {
    let path = match canonical(raw) {
        Ok(p) => p,
        Err(e) => return failed(raw, crate::error::io_message(&e)),
    };
    let path_str = path.to_string_lossy().to_string();
    let name = display_name(&path);
    let result = |status: &str, message: Option<String>, comic_id: Option<i64>| ItemResult {
        path: path_str.clone(),
        name: name.clone(),
        status: status.into(),
        message,
        comic_id,
    };

    match library::find_by_path(conn, &path_str) {
        Ok(Some(id)) => {
            let _ = library::set_missing(conn, id, false);
            return result("skipped", Some("Já está na biblioteca.".into()), Some(id));
        }
        Ok(None) => {}
        Err(e) => return result("failed", Some(e.to_string()), None),
    }

    let format = match formats::detect(&path) {
        Ok(f) => f,
        Err(e) => return result("failed", Some(e.to_string()), None),
    };
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => return result("failed", Some(crate::error::io_message(&e)), None),
    };
    // Confirma permissão de leitura antes de processar.
    if format != ComicFormat::Folder {
        if let Err(e) = File::open(&path) {
            return result("failed", Some(crate::error::io_message(&e)), None);
        }
    }
    let listing = match formats::list(format, &path) {
        Ok(l) => l,
        Err(e) => return result("failed", Some(e.to_string()), None),
    };
    let fp = match fingerprint(format, &path, &listing) {
        Ok(f) => f,
        Err(e) => return result("failed", Some(e.to_string()), None),
    };
    let size = if format == ComicFormat::Folder {
        listing.pages.iter().map(|p| p.size as i64).sum()
    } else {
        meta.len() as i64
    };
    let mtime = file_mtime(&meta);

    let dups = match library::find_by_fingerprint(conn, &fp) {
        Ok(d) => d,
        Err(e) => return result("failed", Some(e.to_string()), None),
    };
    if let Some(moved) = dups.iter().find(|d| d.missing || !Path::new(&d.path).exists()) {
        return match library::relink(conn, moved.id, &path_str, &name, size, mtime) {
            Ok(()) => result(
                "relinked",
                Some(format!("“{}” foi encontrado em um novo local. A localização foi atualizada e o progresso mantido.", moved.title)),
                Some(moved.id),
            ),
            Err(e) => result("failed", Some(e.to_string()), None),
        };
    }
    if policy == DuplicatePolicy::Skip {
        if let Some(d) = dups.first() {
            return result(
                "duplicate",
                Some(format!("Conteúdo idêntico a “{}”, que já está na biblioteca.", d.title)),
                Some(d.id),
            );
        }
    }

    let new = build_new_comic(format, &path, &listing, fp, size, mtime);
    let id = match library::insert_comic(conn, &new) {
        Ok(id) => id,
        Err(e) => return result("failed", Some(e.to_string()), None),
    };
    match apply_cover(conn, covers_dir, id, format, &path, &listing) {
        Ok(Some(cover_err)) => result("added", Some(format!("Adicionado com capa substituta: {cover_err}.")), Some(id)),
        Ok(None) => result("added", None, Some(id)),
        Err(e) => result("added", Some(format!("Adicionado, mas a capa não foi registrada: {e}")), Some(id)),
    }
}

/// Executa uma importação completa de forma síncrona (usada pela thread e pelos testes).
pub fn run_import(
    conn: &Connection,
    covers_dir: &Path,
    job_id: i64,
    paths: &[PathBuf],
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(&str, &JobCounts, Option<&str>, Option<&ItemResult>),
) -> (JobCounts, Vec<ItemResult>, bool) {
    on_event("scanning", &JobCounts::default(), None, None);
    let (candidates, scan_errors) = expand(paths);
    let policy = duplicate_policy(conn);
    let mut counts = JobCounts { total: (candidates.len() + scan_errors.len()) as i64, ..Default::default() };
    let mut items = Vec::new();
    let record = |counts: &mut JobCounts, item: ItemResult, items: &mut Vec<ItemResult>| {
        counts.processed += 1;
        match item.status.as_str() {
            "added" | "relinked" => counts.added += 1,
            "failed" => counts.failed += 1,
            _ => counts.skipped += 1,
        }
        let _ = library::add_import_item(conn, job_id, &item.path, &item.status, item.message.as_deref(), item.comic_id);
        items.push(item);
    };
    for err in scan_errors {
        record(&mut counts, err, &mut items);
        on_event("processing", &counts, None, items.last());
    }
    let mut cancelled = false;
    for cand in candidates {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let name = display_name(&cand);
        on_event("processing", &counts, Some(&name), None);
        let item = process_one(conn, covers_dir, &cand, policy);
        record(&mut counts, item, &mut items);
        on_event("processing", &counts, None, items.last());
    }
    let status = if cancelled { "cancelled" } else { "finished" };
    let _ = library::finish_import_job(conn, job_id, status, (counts.total, counts.processed, counts.added, counts.skipped, counts.failed));
    on_event(if cancelled { "cancelled" } else { "done" }, &counts, None, None);
    (counts, items, cancelled)
}

/// Regenera capas pendentes ou geradas com parâmetros antigos (exceto PDF, feito pela interface).
pub fn regenerate_covers(conn: &Connection, covers_dir: &Path, emit: &Emit) -> AppResult<usize> {
    let ids = library::comics_needing_cover(conn, false)?;
    let total = ids.len();
    for (i, id) in ids.into_iter().enumerate() {
        let info = library::file_info(conn, id)?;
        let path = PathBuf::from(&info.path);
        let Some(format) = ComicFormat::parse(&info.format) else { continue };
        if !path.exists() {
            library::set_missing(conn, id, true)?;
            continue;
        }
        match formats::list(format, &path) {
            Ok(listing) => {
                apply_cover(conn, covers_dir, id, format, &path, &listing)?;
            }
            Err(e) => {
                library::set_cover(conn, id, "placeholder", None, None, Some(&e.to_string()), COVER_PARAMS_VERSION)?;
            }
        }
        emit(EVENT_COVERS, json!({ "total": total, "done": i + 1 }));
        if (i + 1) % 8 == 0 || i + 1 == total {
            emit(EVENT_LIBRARY, json!({}));
        }
    }
    Ok(total)
}

// ---------------------------------------------------------------------------
// Thread de trabalho
// ---------------------------------------------------------------------------

enum Task {
    Import { job_id: i64, paths: Vec<PathBuf>, cancel: Arc<AtomicBool> },
    Covers,
}

pub struct Importer {
    tx: Mutex<mpsc::Sender<Task>>,
    cancels: Arc<Mutex<HashMap<i64, Arc<AtomicBool>>>>,
}

impl Importer {
    /// `open_conn` abre uma conexão própria para a thread (o WAL permite leituras concorrentes).
    pub fn spawn(open_conn: impl Fn() -> AppResult<Connection> + Send + 'static, covers_dir: PathBuf, emit: Emit) -> AppResult<Self> {
        let (tx, rx) = mpsc::channel::<Task>();
        let cancels: Arc<Mutex<HashMap<i64, Arc<AtomicBool>>>> = Arc::new(Mutex::new(HashMap::new()));
        let cancels_worker = cancels.clone();
        let conn = open_conn()?;
        std::thread::Builder::new()
            .name("prancha-import".into())
            .spawn(move || {
                for task in rx {
                    match task {
                        Task::Import { job_id, paths, cancel } => {
                            let emit2 = emit.clone();
                            let mut on_event = |phase: &str, counts: &JobCounts, current: Option<&str>, item: Option<&ItemResult>| {
                                emit2(EVENT_IMPORT, json!({
                                    "jobId": job_id, "phase": phase, "counts": counts, "current": current, "item": item
                                }));
                                if item.is_some_and(|i| i.status == "added" || i.status == "relinked") {
                                    emit2(EVENT_LIBRARY, json!({}));
                                }
                            };
                            run_import(&conn, &covers_dir, job_id, &paths, &cancel, &mut on_event);
                            cancels_worker.lock().unwrap().remove(&job_id);
                            emit(EVENT_LIBRARY, json!({}));
                        }
                        Task::Covers => {
                            if let Err(e) = regenerate_covers(&conn, &covers_dir, &emit) {
                                log::error!("falha ao regenerar capas: {e}");
                            }
                            emit(EVENT_LIBRARY, json!({}));
                        }
                    }
                }
            })
            .map_err(crate::error::AppError::Io)?;
        Ok(Importer { tx: Mutex::new(tx), cancels })
    }

    pub fn enqueue_import(&self, job_id: i64, paths: Vec<PathBuf>) {
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancels.lock().unwrap().insert(job_id, cancel.clone());
        let _ = self.tx.lock().unwrap().send(Task::Import { job_id, paths, cancel });
    }

    pub fn cancel(&self, job_id: i64) -> bool {
        match self.cancels.lock().unwrap().get(&job_id) {
            Some(c) => {
                c.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Há importação na fila ou em andamento.
    pub fn is_busy(&self) -> bool {
        !self.cancels.lock().unwrap().is_empty()
    }

    pub fn enqueue_covers(&self) {
        let _ = self.tx.lock().unwrap().send(Task::Covers);
    }
}
