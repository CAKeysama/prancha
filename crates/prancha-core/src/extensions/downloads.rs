//! Fila de downloads: o estado fica no banco e sobrevive ao fechamento do app.
//! Cada capítulo vira um CBZ gerado pelo Prancha e entra na biblioteca pelo fluxo de importação de sempre.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::json;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use super::install;
use super::net::{Access, Cancel, Limits, NetError, Request};
use super::runtime::{Installed, Page};
use super::signing::sha256_hex;
use super::{sources, suwayomi};
use crate::core::Core;
use crate::covers;
use crate::db;
use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::formats::{self, ComicFormat};
use crate::import::{self, DuplicatePolicy, EVENT_LIBRARY};
use crate::notifications::{self, Event, Kind, NewNotification};

pub const EVENT_DOWNLOADS: &str = "downloads:changed";

const WORKERS: usize = 3;
const PER_SOURCE: usize = 1;
const MAX_PAGES: usize = 2000;
const MAX_PAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CHAPTER_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const PAGE_TIMEOUT: Duration = Duration::from_secs(60);
const WAIT_STEP: Duration = Duration::from_millis(50);
/// Uma fonte não pode prender um dos downloads por dias pedindo um intervalo enorme.
const MAX_SOURCE_INTERVAL: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Consulta
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadView {
    pub id: i64,
    pub chapter_id: i64,
    pub series_id: i64,
    pub series_title: String,
    pub chapter_title: String,
    pub source_name: Option<String>,
    pub status: String,
    pub pages_done: i64,
    pub pages_total: i64,
    pub bytes: i64,
    pub error: Option<String>,
    pub comic_id: Option<i64>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

const VIEW_SELECT: &str = "SELECT d.id, d.chapter_id, o.id, o.title, c.title, s.name, d.status, d.pages_done, d.pages_total, d.bytes, d.error, c.comic_id, d.created_at, d.finished_at
    FROM chapter_downloads d
    JOIN online_chapters c ON c.id = d.chapter_id
    JOIN online_series o ON o.id = c.series_id
    LEFT JOIN extension_sources s ON s.extension_id = o.extension_id AND s.source_id = o.source_id";

fn view_from_row(r: &rusqlite::Row) -> rusqlite::Result<DownloadView> {
    Ok(DownloadView {
        id: r.get(0)?,
        chapter_id: r.get(1)?,
        series_id: r.get(2)?,
        series_title: r.get(3)?,
        chapter_title: r.get(4)?,
        source_name: r.get(5)?,
        status: r.get(6)?,
        pages_done: r.get(7)?,
        pages_total: r.get(8)?,
        bytes: r.get(9)?,
        error: r.get(10)?,
        comic_id: r.get(11)?,
        created_at: r.get(12)?,
        finished_at: r.get(13)?,
    })
}

pub fn list(conn: &Connection, limit: i64) -> AppResult<Vec<DownloadView>> {
    let mut statement = conn.prepare(&format!(
        "{VIEW_SELECT} ORDER BY CASE WHEN d.status IN ('running', 'queued', 'paused') THEN 0 ELSE 1 END, CASE WHEN d.status IN ('running', 'queued', 'paused') THEN d.id ELSE -d.id END LIMIT ?1"
    ))?;
    let rows = statement.query_map([limit.clamp(1, 1000)], view_from_row)?.collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<DownloadView> {
    conn.query_row(&format!("{VIEW_SELECT} WHERE d.id = ?1"), [id], view_from_row)
        .optional()?
        .ok_or_else(|| AppError::user("download_not_found", "Este download não está mais na lista."))
}

// ---------------------------------------------------------------------------
// Fila
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Intent {
    Run,
    Pause,
    Cancel,
}

struct Running {
    cancel: Cancel,
    source: String,
    intent: Intent,
}

#[derive(Default)]
pub struct Downloads {
    running: Mutex<HashMap<i64, Running>>,
    wake: Condvar,
    workers: OnceLock<()>,
    /// Quando cada fonte pode receber o próximo pedido, mesmo vindo de outro capítulo.
    next_request: Mutex<HashMap<String, Instant>>,
}

impl Downloads {
    fn lock(&self) -> MutexGuard<'_, HashMap<i64, Running>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Passa pelo lock de propósito: quem está decidindo dormir termina de decidir antes de receber o aviso.
    fn wake_workers(&self) {
        let _running = self.lock();
        self.wake.notify_all();
    }

    /// Reserva a próxima vez desta fonte. Quem baixa e quem lê online dividem o mesmo intervalo.
    fn reserve_request(&self, source: &str, interval: Duration) -> Instant {
        let mut next = self.next_request.lock().unwrap_or_else(|e| e.into_inner());
        let slot = next.get(source).copied().unwrap_or_else(Instant::now).max(Instant::now());
        next.insert(source.to_string(), slot + interval);
        slot
    }
}

fn notify(core: &Core, id: i64) {
    if let Ok(view) = get(&core.db(), id) {
        (core.emit)(EVENT_DOWNLOADS, serde_json::to_value(view).unwrap_or_else(|_| json!({})));
    }
}

/// As threads só nascem quando existe algo a baixar e dormem enquanto a fila está vazia.
fn ensure_workers(core: &Arc<Core>) {
    core.downloads.workers.get_or_init(|| {
        for _ in 0..WORKERS {
            let core = core.clone();
            std::thread::spawn(move || worker(core));
        }
    });
    core.downloads.wake_workers();
}

struct Job {
    id: i64,
    source: String,
    cancel: Cancel,
}

fn next_job(core: &Core) -> Job {
    let mut running = core.downloads.lock();
    loop {
        let mut busy: HashMap<&str, usize> = HashMap::new();
        for item in running.values() {
            *busy.entry(item.source.as_str()).or_insert(0) += 1;
        }
        let picked = pick(&core.db(), &busy);
        match picked {
            Some((id, source)) => {
                let cancel = Cancel::default();
                running.insert(id, Running { cancel: cancel.clone(), source: source.clone(), intent: Intent::Run });
                return Job { id, source, cancel };
            }
            None => running = core.downloads.wake.wait(running).unwrap_or_else(|e| e.into_inner()),
        }
    }
}

/// O próximo da fila, respeitando o limite por fonte.
fn pick(conn: &Connection, busy: &HashMap<&str, usize>) -> Option<(i64, String)> {
    let mut statement = conn
        .prepare(
            "SELECT d.id, o.extension_id || '/' || o.source_id FROM chapter_downloads d
             JOIN online_chapters c ON c.id = d.chapter_id JOIN online_series o ON o.id = c.series_id
             WHERE d.status = 'queued' ORDER BY d.id",
        )
        .ok()?;
    let mut rows = statement.query([]).ok()?;
    while let Ok(Some(row)) = rows.next() {
        let (id, source): (i64, String) = (row.get(0).ok()?, row.get(1).ok()?);
        if busy.get(source.as_str()).copied().unwrap_or(0) < PER_SOURCE {
            drop(rows);
            conn.execute("UPDATE chapter_downloads SET status = 'running', error = NULL, updated_at = ?2 WHERE id = ?1", params![id, now_ms()]).ok()?;
            return Some((id, source));
        }
    }
    None
}

fn worker(core: Arc<Core>) {
    loop {
        let job = next_job(&core);
        notify(&core, job.id);
        let outcome = download(&core, &job);
        let intent = core.downloads.lock().remove(&job.id).map_or(Intent::Run, |r| r.intent);
        finish(&core, &job, outcome, intent);
        core.downloads.wake_workers();
    }
}

fn set_status(core: &Core, id: i64, status: &str, error: Option<&str>) {
    let finished = matches!(status, "completed" | "failed" | "cancelled").then(now_ms);
    if let Err(e) = core.db().execute("UPDATE chapter_downloads SET status = ?2, error = ?3, updated_at = ?4, finished_at = ?5 WHERE id = ?1", params![id, status, error, now_ms(), finished]) {
        log::warn!("download {id}: {e}");
    }
}

fn temp_dir(core: &Core, id: i64) -> PathBuf {
    core.paths.data_dir.join("downloads").join("tmp").join(id.to_string())
}

fn finish(core: &Core, job: &Job, outcome: AppResult<()>, intent: Intent) {
    let temp = temp_dir(core, job.id);
    // A limpeza vem antes do estado final: com ele à vista, um "tentar de novo" já pode usar a mesma pasta.
    match (outcome, intent) {
        (Ok(()), _) => {
            let _ = std::fs::remove_dir_all(&temp);
            set_status(core, job.id, "completed", None);
            (core.emit)(EVENT_LIBRARY, json!({}));
        }
        (Err(_), Intent::Pause) => set_status(core, job.id, "paused", None),
        (Err(_), Intent::Cancel) => {
            let _ = std::fs::remove_dir_all(&temp);
            set_status(core, job.id, "cancelled", None);
        }
        (Err(error), Intent::Run) => {
            let _ = std::fs::remove_dir_all(&temp);
            set_status(core, job.id, "failed", Some(&error.to_string()));
        }
    }
    notify(core, job.id);
    announce(core, job.id);
}

/// Concluídos e falhas da mesma obra num intervalo curto somam numa notificação só: um lote de
/// capítulos não vira dezenas de avisos. A chave usa o capítulo e o momento do pedido, que não se
/// repetem, e a falha também o fim da tentativa: tentar de novo e falhar outra vez é outro evento.
fn announce(core: &Core, id: i64) {
    let conn = core.db();
    let Ok(view) = get(&conn, id) else { return };
    let (kind, key) = match view.status.as_str() {
        "completed" => (Kind::DownloadCompleted, format!("download:completed:{}:{}", view.chapter_id, view.created_at)),
        "failed" => (Kind::DownloadFailed, format!("download:failed:{}:{}:{}", view.chapter_id, view.created_at, view.finished_at.unwrap_or_default())),
        _ => return,
    };
    let event = Event::new(key).item(view.chapter_title.clone()).chapter(view.chapter_id);
    let mut n = NewNotification::new(kind).subject(view.series_title.clone()).series(view.series_id).event(event).grouped(notifications::DOWNLOADS_WINDOW_MS);
    if let Some(comic) = view.comic_id.filter(|_| kind == Kind::DownloadCompleted) {
        n = n.comic(comic);
    }
    notifications::publish_with(core, &conn, &n);
}

// ---------------------------------------------------------------------------
// Ações
// ---------------------------------------------------------------------------

/// Coloca capítulos na fila. Quem já está na fila ou na biblioteca fica como está.
pub fn enqueue(core: &Arc<Core>, chapter_ids: &[i64]) -> AppResult<Vec<i64>> {
    let mut created = Vec::new();
    {
        let conn = core.db();
        for chapter_id in chapter_ids {
            let chapter: Option<Option<i64>> = conn.query_row("SELECT comic_id FROM online_chapters WHERE id = ?1", [chapter_id], |r| r.get(0)).optional()?;
            let Some(comic_id) = chapter else {
                return Err(AppError::user("chapter_not_found", "Este capítulo não está mais registrado."));
            };
            if comic_id.is_some_and(|id| comic_file_exists(&conn, id)) {
                continue;
            }
            let open: Option<i64> = conn.query_row("SELECT id FROM chapter_downloads WHERE chapter_id = ?1 AND status IN ('queued', 'running', 'paused')", [chapter_id], |r| r.get(0)).optional()?;
            if open.is_some() {
                continue;
            }
            let now = now_ms();
            conn.execute("INSERT INTO chapter_downloads (chapter_id, status, created_at, updated_at) VALUES (?1, 'queued', ?2, ?2)", params![chapter_id, now])?;
            created.push(conn.last_insert_rowid());
        }
    }
    for id in &created {
        notify(core, *id);
    }
    if !created.is_empty() {
        ensure_workers(core);
    }
    Ok(created)
}

fn comic_file_exists(conn: &Connection, comic_id: i64) -> bool {
    crate::library::file_info(conn, comic_id).is_ok_and(|info| Path::new(&info.path).exists())
}

fn status_of(core: &Core, id: i64) -> AppResult<String> {
    core.db()
        .query_row("SELECT status FROM chapter_downloads WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::user("download_not_found", "Este download não está mais na lista."))
}

fn interrupt(core: &Core, id: i64, intent: Intent) -> bool {
    match core.downloads.lock().get_mut(&id) {
        Some(running) => {
            running.intent = intent;
            running.cancel.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

pub fn pause(core: &Core, id: i64) -> AppResult<()> {
    if !interrupt(core, id, Intent::Pause) && status_of(core, id)? == "queued" {
        set_status(core, id, "paused", None);
        notify(core, id);
    }
    Ok(())
}

pub fn resume(core: &Arc<Core>, id: i64) -> AppResult<()> {
    if status_of(core, id)? == "paused" {
        set_status(core, id, "queued", None);
        notify(core, id);
        ensure_workers(core);
    }
    Ok(())
}

pub fn cancel(core: &Core, id: i64) -> AppResult<()> {
    if !interrupt(core, id, Intent::Cancel) && matches!(status_of(core, id)?.as_str(), "queued" | "paused") {
        let _ = std::fs::remove_dir_all(temp_dir(core, id));
        set_status(core, id, "cancelled", None);
        notify(core, id);
    }
    Ok(())
}

/// Apaga as páginas soltas dos downloads de uma obra que vai sair do banco.
pub fn remove_partial_pages(core: &Core, conn: &Connection, series_id: i64) -> AppResult<()> {
    let mut statement = conn.prepare("SELECT d.id FROM chapter_downloads d JOIN online_chapters c ON c.id = d.chapter_id WHERE c.series_id = ?1")?;
    let ids: Vec<i64> = statement.query_map([series_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for id in ids {
        let _ = std::fs::remove_dir_all(temp_dir(core, id));
    }
    Ok(())
}

pub fn retry(core: &Arc<Core>, id: i64) -> AppResult<()> {
    if matches!(status_of(core, id)?.as_str(), "failed" | "cancelled") {
        core.db().execute("UPDATE chapter_downloads SET status = 'queued', error = NULL, pages_done = 0, pages_total = 0, bytes = 0, finished_at = NULL, updated_at = ?2 WHERE id = ?1", params![id, now_ms()])?;
        notify(core, id);
        ensure_workers(core);
    }
    Ok(())
}

/// Tira do histórico. O arquivo baixado continua na biblioteca.
pub fn clear_finished(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute("DELETE FROM chapter_downloads WHERE status IN ('completed', 'failed', 'cancelled')", [])?)
}

/// O que estava em andamento quando o app fechou volta para a fila e continua de onde parou.
pub fn on_startup(core: &Arc<Core>) -> AppResult<()> {
    let (queued, open): (i64, Vec<i64>) = {
        let conn = core.db();
        conn.execute("UPDATE chapter_downloads SET status = 'queued', updated_at = ?1 WHERE status = 'running'", [now_ms()])?;
        let queued = conn.query_row("SELECT count(*) FROM chapter_downloads WHERE status = 'queued'", [], |r| r.get(0))?;
        let mut statement = conn.prepare("SELECT id FROM chapter_downloads WHERE status IN ('queued', 'paused')")?;
        let open = statement.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        (queued, open)
    };
    // Temporários sem download em aberto são restos de uma operação encerrada.
    let root = core.paths.data_dir.join("downloads").join("tmp");
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().parse::<i64>().ok();
            if id.is_some_and(|id| !open.contains(&id)) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    if queued > 0 {
        ensure_workers(core);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

struct ChapterInfo {
    chapter_id: i64,
    extension_id: String,
    source_id: String,
    chapter_remote: String,
    chapter_title: String,
    chapter_number: Option<String>,
    series_remote: String,
    series_title: String,
    author: Option<String>,
    summary: Option<String>,
    language: Option<String>,
}

fn chapter_info(conn: &Connection, id: i64) -> AppResult<ChapterInfo> {
    Ok(conn.query_row(
        "SELECT c.id, o.extension_id, o.source_id, c.remote_id, c.title, c.number, o.remote_id, o.title, o.author, o.summary, o.language
         FROM chapter_downloads d JOIN online_chapters c ON c.id = d.chapter_id JOIN online_series o ON o.id = c.series_id WHERE d.id = ?1",
        [id],
        |r| {
            Ok(ChapterInfo {
                chapter_id: r.get(0)?,
                extension_id: r.get(1)?,
                source_id: r.get(2)?,
                chapter_remote: r.get(3)?,
                chapter_title: r.get(4)?,
                chapter_number: r.get(5)?,
                series_remote: r.get(6)?,
                series_title: r.get(7)?,
                author: r.get(8)?,
                summary: r.get(9)?,
                language: r.get(10)?,
            })
        },
    )?)
}

/// Ordem pela posição declarada. Repetição ou lacuna significa que faltaria página, então o capítulo falha.
pub fn ordered_pages(mut pages: Vec<Page>) -> AppResult<Vec<Page>> {
    if pages.is_empty() {
        return Err(AppError::user("download_order", "A fonte não informou nenhuma página para este capítulo."));
    }
    if pages.len() > MAX_PAGES {
        return Err(AppError::user("download_order", "O capítulo tem páginas demais."));
    }
    pages.sort_by_key(|page| page.index);
    if pages.iter().enumerate().any(|(position, page)| page.index as usize != position) {
        return Err(AppError::user("download_order", "A fonte informou as páginas fora de ordem, com repetições ou lacunas."));
    }
    Ok(pages)
}

/// O formato vem do conteúdo. O nome e o tipo informados pelo servidor não contam.
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some("png"),
        image::ImageFormat::Jpeg => Some("jpg"),
        image::ImageFormat::WebP => Some("webp"),
        image::ImageFormat::Gif => Some("gif"),
        image::ImageFormat::Bmp => Some("bmp"),
        _ => None,
    }
}

fn page_error(position: usize, error: NetError) -> AppError {
    match error {
        NetError::Cancelled => AppError::user("cancelled", "Operação cancelada."),
        NetError::Blocked(why) => AppError::user("download_blocked", format!("Página {}: endereço não permitido ({why}).", position + 1)),
        NetError::TooLarge => AppError::user("download_page", format!("Página {}: a imagem passa do limite de tamanho.", position + 1)),
        NetError::Timeout => AppError::user("download_network", format!("Página {}: o servidor demorou demais.", position + 1)),
        NetError::Network(why) => {
            log::warn!("download: {why}");
            AppError::user("download_network", format!("Página {}: não foi possível baixar.", position + 1))
        }
    }
}

/// Espera o intervalo da fonte, sem deixar de atender a um cancelamento.
/// Espera a vez desta fonte, respeitando o intervalo que ela pediu.
pub(super) fn wait_turn(core: &Core, source: &str, min_interval_ms: u64, cancel: &Cancel) -> AppResult<()> {
    let interval = Duration::from_millis(min_interval_ms).min(MAX_SOURCE_INTERVAL);
    wait_interval(core.downloads.reserve_request(source, interval), cancel)
}

/// Busca uma página, com as mesmas regras para quem baixa e para quem lê online. Devolve a imagem e a extensão real dela.
pub(super) fn fetch_page(core: &Core, extension: &Installed, page: &Page, position: usize, cancel: Option<&Cancel>) -> AppResult<(Vec<u8>, &'static str)> {
    let headers: Vec<(String, String)> = page.headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect();
    let limits = Limits { max_bytes: MAX_PAGE_BYTES, timeout: PAGE_TIMEOUT };
    let fetched = if extension.bridged {
        suwayomi::image(core, &page.url, limits, cancel)
    } else {
        core.net.get(&page.url, Access::Hosts(&extension.grant.allowed_hosts), limits, Request { headers: &headers, cancel, check_content_type: true, ..Default::default() })
    };
    let response = fetched.map_err(|e| page_error(position, e))?;
    if !response.is_success() {
        return Err(AppError::user("download_network", format!("Página {}: o servidor respondeu {}.", position + 1, response.status)));
    }
    let extension_name = image_extension(&response.body).ok_or_else(|| AppError::user("download_page", format!("Página {}: o arquivo recebido não é uma imagem.", position + 1)))?;
    covers::decode_limited(&response.body).map_err(|why| AppError::user("download_page", format!("Página {}: imagem inválida ({why}).", position + 1)))?;
    Ok((response.body, extension_name))
}

fn wait_interval(next: Instant, cancel: &Cancel) -> AppResult<()> {
    while Instant::now() < next {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::user("cancelled", "Operação cancelada."));
        }
        std::thread::sleep(WAIT_STEP.min(next.saturating_duration_since(Instant::now())));
    }
    Ok(())
}

fn existing_pages(temp: &Path) -> HashMap<usize, PathBuf> {
    let Ok(entries) = std::fs::read_dir(temp) else { return HashMap::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let number = path.file_stem()?.to_str()?.parse::<usize>().ok()?;
            let extension = path.extension()?.to_str()?;
            matches!(extension, "png" | "jpg" | "webp" | "gif" | "bmp").then(|| (number, path.clone()))
        })
        .collect()
}

fn download(core: &Arc<Core>, job: &Job) -> AppResult<()> {
    let id = job.id;
    let info = chapter_info(&core.db(), id)?;
    let extension = install::runnable(core, &info.extension_id)?;
    let source = sources::source_row(&core.db(), &info.extension_id, &info.source_id)?;
    let pages = ordered_pages(if extension.bridged { suwayomi::pages(core, &info.chapter_remote, Some(&job.cancel))? } else { core.engine()?.pages(&extension, Some(&job.cancel), &info.source_id, &info.chapter_remote)? })?;
    let temp = temp_dir(core, id);
    std::fs::create_dir_all(&temp)?;

    let total = pages.len() as i64;
    let mut stored = existing_pages(&temp);
    let mut bytes: u64 = stored.values().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    let done = pages.iter().enumerate().filter(|(position, _)| stored.contains_key(&(position + 1))).count() as i64;
    core.db().execute("UPDATE chapter_downloads SET pages_total = ?2, pages_done = ?3, bytes = ?4, updated_at = ?5 WHERE id = ?1", params![id, total, done, bytes as i64, now_ms()])?;
    notify(core, id);

    let mut pages_done = done;
    for (position, page) in pages.iter().enumerate() {
        if stored.contains_key(&(position + 1)) {
            continue;
        }
        if job.cancel.load(Ordering::Relaxed) {
            return Err(AppError::user("cancelled", "Operação cancelada."));
        }
        wait_turn(core, &job.source, source.min_interval_ms, &job.cancel)?;
        let (body, extension_name) = fetch_page(core, &extension, page, position, Some(&job.cancel))?;
        bytes += body.len() as u64;
        if bytes > MAX_CHAPTER_BYTES {
            return Err(AppError::user("download_page", "O capítulo passa do limite de tamanho."));
        }
        let name = temp.join(format!("{:04}.{extension_name}", position + 1));
        let partial = temp.join(format!("{:04}.part", position + 1));
        std::fs::write(&partial, &body)?;
        std::fs::rename(&partial, &name)?;
        stored.insert(position + 1, name);
        pages_done += 1;
        // A linha some junto com a obra, quando ela é bloqueada: o capítulo não pode chegar à biblioteca.
        if core.db().execute("UPDATE chapter_downloads SET pages_done = ?2, bytes = ?3, updated_at = ?4 WHERE id = ?1", params![id, pages_done, bytes as i64, now_ms()])? == 0 {
            return Err(AppError::user("download_not_found", "Este download não está mais na lista."));
        }
        notify(core, id);
    }

    let target = target_path(core, &info);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = temp.join("capitulo.cbz.part");
    let mut ordered: Vec<(usize, PathBuf)> = stored.into_iter().filter(|(number, _)| *number <= pages.len()).collect();
    ordered.sort_by_key(|(number, _)| *number);
    write_cbz(&partial, &ordered, &comic_info_xml(&info))?;
    validate_cbz(&partial, pages.len())?;
    status_of(core, id)?;
    std::fs::rename(&partial, &target)?;
    register(core, &info, &target)
}

/// `downloads/<obra>/<capítulo> [<hash>].cbz`. Os nomes são gerados aqui: nada vindo da fonte vira caminho sem passar pelo filtro.
fn target_path(core: &Core, info: &ChapterInfo) -> PathBuf {
    let series_key = sha256_hex(format!("{}\0{}\0{}", info.extension_id, info.source_id, info.series_remote).as_bytes());
    let chapter_key = sha256_hex(format!("{series_key}\0{}", info.chapter_remote).as_bytes());
    let name = format!("{} - {} [{}].cbz", safe_name(&info.series_title), safe_name(&info.chapter_title), &chapter_key[..12]);
    core.paths.data_dir.join("downloads").join(&series_key[..16]).join(name)
}

/// Letras, dígitos e pouca pontuação. Limita o tamanho e não deixa ponto ou espaço nas pontas, que o Windows recusa.
pub fn safe_name(text: &str) -> String {
    let filtered: String = text.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | ',' | '(' | ')') { c } else { ' ' }).take(80).collect();
    let collapsed = filtered.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(['.', ' ']);
    if trimmed.is_empty() { "sem nome".to_string() } else { trimmed.to_string() }
}

fn escape_xml(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            other => other.to_string(),
        })
        .collect()
}

fn comic_info_xml(info: &ChapterInfo) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ComicInfo>\n");
    let mut field = |name: &str, value: Option<&str>| {
        if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
            xml.push_str(&format!("  <{name}>{}</{name}>\n", escape_xml(value)));
        }
    };
    field("Title", Some(&info.chapter_title));
    field("Series", Some(&info.series_title));
    field("Number", info.chapter_number.as_deref());
    field("Writer", info.author.as_deref());
    field("Summary", info.summary.as_deref());
    field("LanguageISO", info.language.as_deref());
    xml.push_str("</ComicInfo>\n");
    xml
}

/// Sem compressão, porque as imagens já vêm comprimidas, e sem data variável: o mesmo capítulo gera os mesmos bytes.
pub fn write_cbz(path: &Path, pages: &[(usize, PathBuf)], comic_info: &str) -> AppResult<()> {
    let file = File::create(path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored).last_modified_time(DateTime::default()).large_file(false);
    let failed = |e: zip::result::ZipError| AppError::user("download_cbz", format!("Não foi possível gerar o arquivo: {e}."));
    for (number, source) in pages {
        let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("png");
        zip.start_file(format!("{number:04}.{extension}"), options).map_err(failed)?;
        zip.write_all(&std::fs::read(source)?)?;
    }
    zip.start_file("ComicInfo.xml", options).map_err(failed)?;
    zip.write_all(comic_info.as_bytes())?;
    zip.finish().map_err(failed)?;
    Ok(())
}

/// O arquivo só entra na biblioteca depois de ser aberto pelo mesmo código que o leitor usa.
pub fn validate_cbz(path: &Path, expected_pages: usize) -> AppResult<()> {
    let invalid = |why: String| AppError::user("download_cbz", format!("O arquivo gerado não é válido: {why}."));
    let listing = formats::list(ComicFormat::Cbz, path).map_err(|e| invalid(e.to_string()))?;
    if listing.pages.len() != expected_pages {
        return Err(invalid(format!("{} páginas em vez de {expected_pages}", listing.pages.len())));
    }
    let first = listing.pages.first().ok_or_else(|| invalid("sem páginas".into()))?;
    let bytes = formats::read_entry(ComicFormat::Cbz, path, first).map_err(|e| invalid(e.to_string()))?;
    covers::decode_limited(&bytes).map_err(invalid)?;
    Ok(())
}

fn register(core: &Core, info: &ChapterInfo, target: &Path) -> AppResult<()> {
    // Conexão própria: gerar a capa não pode segurar o banco principal.
    let conn = db::open(&core.paths.db)?;
    let item = import::process_one(&conn, &core.paths.covers, target, DuplicatePolicy::Skip);
    let Some(comic_id) = item.comic_id.filter(|_| item.status != "failed") else {
        let _ = std::fs::remove_file(target);
        return Err(AppError::user("download_import", item.message.unwrap_or_else(|| "Não foi possível adicionar à biblioteca.".into())));
    };
    // Conteúdo idêntico já está em outro lugar da biblioteca: o capítulo aponta para ele e a cópia sai.
    let file_path = if item.status == "duplicate" {
        let _ = std::fs::remove_file(target);
        None
    } else {
        Some(item.path)
    };
    conn.execute("UPDATE online_chapters SET comic_id = ?2, file_path = ?3 WHERE id = ?1", params![info.chapter_id, comic_id, file_path])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_names_become_harmless() {
        assert_eq!(safe_name("../../etc/passwd"), "etc passwd");
        assert_eq!(safe_name("..\\..\\Windows\\System32"), "Windows System32");
        assert_eq!(safe_name("C:\\x"), "C x");
        assert_eq!(safe_name("  ...  "), "sem nome");
        assert_eq!(safe_name("<>:\"/\\|?*"), "sem nome");
        assert_eq!(safe_name("Capítulo 1: A volta."), "Capítulo 1 A volta");
        assert_eq!(safe_name(&"a".repeat(500)).len(), 80);
    }

    #[test]
    fn the_format_comes_from_the_content() {
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(image_extension(b"\xff\xd8\xff\xe0...."), Some("jpg"));
        assert_eq!(image_extension(b"<html>erro</html>"), None);
        assert_eq!(image_extension(b"MZ executavel"), None);
        assert_eq!(image_extension(b""), None);
    }

    #[test]
    fn xml_values_are_escaped() {
        assert_eq!(escape_xml("A & B <c> \"d\" 'e'"), "A &amp; B &lt;c&gt; &quot;d&quot; &apos;e&apos;");
        assert_eq!(escape_xml("a\u{0}b"), "ab");
    }
}
