//! Chama as fontes, filtra o que chega e guarda o que a biblioteca online precisa lembrar.
//! Tudo que vem de uma extensão é tratado como dado de terceiros: tamanho limitado, sem endereços fora das regras
//! e sem conteúdo adulto.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use reqwest::Url;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

use super::install;
use super::net::{Access, Cancel, Limits, Request};
use super::runtime::{ContentRating, Filter, Installed, PublicationStatus, SearchPage, SearchResult};
use crate::core::Core;
use crate::covers;
use crate::db::now_ms;
use crate::error::{AppError, AppResult};

const MAX_RESULTS: usize = 200;
const MAX_CHAPTERS: usize = 5000;
const MAX_TITLE_CHARS: usize = 300;
const MAX_TEXT_CHARS: usize = 4000;
const MAX_NAMES: usize = 32;
const MAX_REMOTE_ID_BYTES: usize = 1024;
const MAX_COVER_BYTES: u64 = 16 * 1024 * 1024;
const COVER_TIMEOUT: Duration = Duration::from_secs(30);
const CONCURRENT_COVERS: usize = 4;
/// Obras só vistas em uma busca saem do banco depois deste tempo.
const STALE_AFTER_MS: i64 = 30 * 24 * 60 * 60 * 1000;

// ---------------------------------------------------------------------------
// Cancelamento das consultas da interface
// ---------------------------------------------------------------------------

/// Consultas em andamento, para a interface poder desistir de uma tela que já saiu.
#[derive(Default)]
pub struct Requests(Mutex<HashMap<String, Cancel>>);

impl Requests {
    pub fn begin(&self, id: &str) -> Cancel {
        let cancel = Cancel::default();
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), cancel.clone());
        cancel
    }

    pub fn end(&self, id: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
    }

    pub fn cancel(&self, id: &str) {
        if let Some(cancel) = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// Limita quantas capas são buscadas ao mesmo tempo: uma grade inteira não vira uma rajada contra o servidor.
pub struct CoverGate {
    used: Mutex<usize>,
    freed: Condvar,
}

impl Default for CoverGate {
    fn default() -> Self {
        CoverGate { used: Mutex::new(0), freed: Condvar::new() }
    }
}

struct CoverPermit<'a>(&'a CoverGate);

impl Drop for CoverPermit<'_> {
    fn drop(&mut self) {
        *self.0.used.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        self.0.freed.notify_one();
    }
}

impl CoverGate {
    fn enter(&self) -> CoverPermit<'_> {
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        while *used >= CONCURRENT_COVERS {
            used = self.freed.wait(used).unwrap_or_else(|e| e.into_inner());
        }
        *used += 1;
        CoverPermit(self)
    }
}

// ---------------------------------------------------------------------------
// Fontes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub extension_id: String,
    pub extension_name: String,
    pub repository_name: String,
    pub source_id: String,
    pub name: String,
    pub language: String,
    pub description: Option<String>,
    pub kinds: Vec<String>,
    pub rating: String,
    pub direction: String,
    pub supports_latest: bool,
    pub supports_search: bool,
}

/// Fontes que podem aparecer em Explorar: de extensões habilitadas e com classificação confiável.
pub fn available_sources(conn: &Connection) -> AppResult<Vec<SourceSummary>> {
    let mut statement = conn.prepare(
        "SELECT s.extension_id, e.name, e.repository_name, s.source_id, s.name, s.language, s.description, s.kinds, s.rating, s.direction, s.supports_latest, s.supports_search
         FROM extension_sources s JOIN installed_extensions e ON e.id = s.extension_id
         WHERE e.enabled = 1 AND s.rating IN ('general', 'teen')
         ORDER BY s.name COLLATE NATSORT, e.name COLLATE NATSORT",
    )?;
    let rows = statement.query_map([], |r| {
        Ok(SourceSummary {
            extension_id: r.get(0)?,
            extension_name: r.get(1)?,
            repository_name: r.get(2)?,
            source_id: r.get(3)?,
            name: r.get(4)?,
            language: r.get(5)?,
            description: r.get(6)?,
            kinds: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
            rating: r.get(8)?,
            direction: r.get(9)?,
            supports_latest: r.get::<_, i64>(10)? != 0,
            supports_search: r.get::<_, i64>(11)? != 0,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub struct SourceRow {
    pub language: String,
    pub rating: String,
    pub supports_latest: bool,
    pub supports_search: bool,
    pub min_interval_ms: u64,
}

pub fn source_row(conn: &Connection, extension_id: &str, source_id: &str) -> AppResult<SourceRow> {
    let row = conn
        .query_row(
            "SELECT language, rating, supports_latest, supports_search, min_interval_ms FROM extension_sources WHERE extension_id = ?1 AND source_id = ?2",
            [extension_id, source_id],
            |r| Ok(SourceRow { language: r.get(0)?, rating: r.get(1)?, supports_latest: r.get::<_, i64>(2)? != 0, supports_search: r.get::<_, i64>(3)? != 0, min_interval_ms: r.get::<_, i64>(4)?.max(0) as u64 }),
        )
        .optional()?;
    row.ok_or_else(|| AppError::user("source_not_found", "Esta fonte não está disponível."))
}

/// A fonte existe, tem classificação confiável e a extensão está habilitada.
fn usable(core: &Core, extension_id: &str, source_id: &str) -> AppResult<(Installed, SourceRow)> {
    let row = source_row(&core.db(), extension_id, source_id)?;
    if !install::rating_is_available(&row.rating) {
        return Err(AppError::user("source_disabled", "Esta fonte fica desabilitada até o repositório informar a classificação de conteúdo."));
    }
    Ok((install::runnable(core, extension_id)?, row))
}

// ---------------------------------------------------------------------------
// Limpeza do que vem da extensão
// ---------------------------------------------------------------------------

fn clean(text: Option<&str>, limit: usize) -> Option<String> {
    let text: String = text?.chars().filter(|c| !c.is_control() || *c == '\n').take(limit).collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn clean_names(names: &[String]) -> Vec<String> {
    names.iter().filter_map(|n| clean(Some(n), 120)).take(MAX_NAMES).collect()
}

/// Uma capa só vale se puder ser baixada sob as mesmas regras de rede da extensão.
fn safe_cover(core: &Core, extension: &Installed, url: Option<&str>) -> Option<String> {
    let url = clean(url, MAX_TEXT_CHARS)?;
    let parsed = Url::parse(&url).ok()?;
    core.net.check(&parsed, Access::Hosts(&extension.grant.allowed_hosts)).ok()?;
    Some(url)
}

fn status_name(status: PublicationStatus) -> &'static str {
    match status {
        PublicationStatus::Ongoing => "ongoing",
        PublicationStatus::Completed => "completed",
        PublicationStatus::Hiatus => "hiatus",
        PublicationStatus::Cancelled => "cancelled",
    }
}

// ---------------------------------------------------------------------------
// Obras
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesCard {
    pub id: i64,
    pub extension_id: String,
    pub source_id: String,
    pub source_name: Option<String>,
    pub remote_id: String,
    pub title: String,
    pub has_cover: bool,
    pub summary: Option<String>,
    pub author: Option<String>,
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub language: Option<String>,
    pub following: bool,
    pub chapter_sort: String,
    pub last_checked_at: Option<i64>,
    pub last_update_at: Option<i64>,
}

const CARD_SELECT: &str = "SELECT o.id, o.extension_id, o.source_id, s.name, o.remote_id, o.title, o.cover_url, o.summary, o.author, o.genres, o.status, o.language, o.following, o.chapter_sort, o.last_checked_at, o.last_update_at
    FROM online_series o LEFT JOIN extension_sources s ON s.extension_id = o.extension_id AND s.source_id = o.source_id";

fn card_from_row(r: &Row) -> rusqlite::Result<SeriesCard> {
    Ok(SeriesCard {
        id: r.get(0)?,
        extension_id: r.get(1)?,
        source_id: r.get(2)?,
        source_name: r.get(3)?,
        remote_id: r.get(4)?,
        title: r.get(5)?,
        has_cover: r.get::<_, Option<String>>(6)?.is_some(),
        summary: r.get(7)?,
        author: r.get(8)?,
        genres: serde_json::from_str(&r.get::<_, String>(9)?).unwrap_or_default(),
        status: r.get(10)?,
        language: r.get(11)?,
        following: r.get::<_, i64>(12)? != 0,
        chapter_sort: r.get(13)?,
        last_checked_at: r.get(14)?,
        last_update_at: r.get(15)?,
    })
}

pub fn card(conn: &Connection, id: i64) -> AppResult<SeriesCard> {
    conn.query_row(&format!("{CARD_SELECT} WHERE o.id = ?1"), [id], card_from_row)
        .optional()?
        .ok_or_else(|| AppError::user("series_not_found", "Esta obra não está mais registrada."))
}

fn cover_cache_file(core: &Core, id: i64) -> std::path::PathBuf {
    core.paths.covers.join(format!("online-{id}.jpg"))
}

struct NewSeries<'a> {
    extension_id: &'a str,
    source_id: &'a str,
    remote_id: &'a str,
    title: &'a str,
    cover_url: Option<String>,
    summary: Option<String>,
    author: Option<String>,
    genres: Option<String>,
    status: Option<&'static str>,
    language: Option<&'a str>,
}

/// O que a fonte deixou de informar não apaga o que já se sabia.
fn upsert_series(core: &Core, conn: &Connection, series: &NewSeries) -> AppResult<i64> {
    let previous: Option<(i64, Option<String>)> = conn
        .query_row(
            "SELECT id, cover_url FROM online_series WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3",
            [series.extension_id, series.source_id, series.remote_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, Some(old_cover))) = &previous {
        if series.cover_url.as_ref().is_some_and(|new| new != old_cover) {
            let _ = std::fs::remove_file(cover_cache_file(core, *id));
        }
    }
    let now = now_ms();
    let id = conn.query_row(
        "INSERT INTO online_series (extension_id, source_id, remote_id, title, cover_url, summary, author, genres, status, language, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, coalesce(?8, '[]'), ?9, ?10, ?11, ?11)
         ON CONFLICT (extension_id, source_id, remote_id) DO UPDATE SET
            title = excluded.title,
            cover_url = coalesce(?5, cover_url),
            summary = coalesce(?6, summary),
            author = coalesce(?7, author),
            genres = coalesce(?8, genres),
            status = coalesce(?9, status),
            language = coalesce(?10, language),
            updated_at = excluded.updated_at
         RETURNING id",
        params![series.extension_id, series.source_id, series.remote_id, series.title, series.cover_url, series.summary, series.author, series.genres, series.status, series.language, now],
        |r| r.get(0),
    )?;
    Ok(id)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePage {
    pub results: Vec<SeriesCard>,
    pub has_more: bool,
}

fn is_listed(result: &SearchResult) -> bool {
    !matches!(result.rating, Some(ContentRating::Adult)) && result.id.len() <= MAX_REMOTE_ID_BYTES && !result.id.is_empty()
}

fn store_page(core: &Core, extension: &Installed, source_id: &str, language: &str, page: SearchPage) -> AppResult<BrowsePage> {
    let mut conn = core.db();
    let tx = conn.transaction()?;
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for result in page.results.iter().filter(|r| is_listed(r)).take(MAX_RESULTS) {
        let Some(title) = clean(Some(&result.title), MAX_TITLE_CHARS) else { continue };
        if !seen.insert(result.id.as_str()) {
            continue;
        }
        ids.push(upsert_series(
            core,
            &tx,
            &NewSeries {
                extension_id: &extension.id,
                source_id,
                remote_id: &result.id,
                title: &title,
                cover_url: safe_cover(core, extension, result.cover_url.as_deref()),
                summary: clean(result.summary.as_deref(), MAX_TEXT_CHARS),
                author: clean(result.author.as_deref(), 300),
                genres: None,
                status: result.status.map(status_name),
                language: Some(language),
            },
        )?);
    }
    tx.commit()?;
    let results = ids.into_iter().map(|id| card(&conn, id)).collect::<AppResult<_>>()?;
    Ok(BrowsePage { results, has_more: page.has_more })
}

fn kind_filter(kind: Option<&str>) -> Vec<Filter> {
    kind.map(|value| Filter { name: "kind".into(), value: value.into() }).into_iter().collect()
}

pub fn popular(core: &Core, extension_id: &str, source_id: &str, page: u32, kind: Option<&str>, cancel: Option<&Cancel>) -> AppResult<BrowsePage> {
    let (extension, source) = usable(core, extension_id, source_id)?;
    let found = core.engine()?.popular(&extension, cancel, source_id, page, &kind_filter(kind))?;
    store_page(core, &extension, source_id, &source.language, found)
}

pub fn latest(core: &Core, extension_id: &str, source_id: &str, page: u32, kind: Option<&str>, cancel: Option<&Cancel>) -> AppResult<BrowsePage> {
    let (extension, source) = usable(core, extension_id, source_id)?;
    if !source.supports_latest {
        return Err(AppError::user("source_unsupported", "Esta fonte não informa atualizações recentes."));
    }
    let found = core.engine()?.latest(&extension, cancel, source_id, page, &kind_filter(kind))?;
    store_page(core, &extension, source_id, &source.language, found)
}

pub fn search(core: &Core, extension_id: &str, source_id: &str, query: &str, page: u32, kind: Option<&str>, cancel: Option<&Cancel>) -> AppResult<BrowsePage> {
    let (extension, source) = usable(core, extension_id, source_id)?;
    if !source.supports_search {
        return Err(AppError::user("source_unsupported", "Esta fonte não oferece pesquisa."));
    }
    let found = core.engine()?.search(&extension, cancel, source_id, query.trim(), page, &kind_filter(kind))?;
    store_page(core, &extension, source_id, &source.language, found)
}

// ---------------------------------------------------------------------------
// Detalhes e capítulos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterProgress {
    pub page_index: i64,
    pub page_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterDownload {
    pub id: i64,
    pub status: String,
    pub pages_done: i64,
    pub pages_total: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterView {
    pub id: i64,
    pub remote_id: String,
    pub title: String,
    pub number: Option<String>,
    pub published_at: Option<i64>,
    pub order: i64,
    /// `available`, `downloaded` (o arquivo existe) ou `offline` (na biblioteca).
    pub state: String,
    pub comic_id: Option<i64>,
    pub progress: Option<ChapterProgress>,
    pub download: Option<ChapterDownload>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesView {
    #[serde(flatten)]
    pub series: SeriesCard,
    pub chapters: Vec<ChapterView>,
    /// A fonte não respondeu: o que aparece é o último conhecido.
    pub source_error: Option<String>,
}

pub fn chapters_of(conn: &Connection, series_id: i64) -> AppResult<Vec<ChapterView>> {
    let mut statement = conn.prepare(
        "SELECT c.id, c.remote_id, c.title, c.number, c.published_at, c.sort_order, c.file_path, c.comic_id, p.page_index, p.page_count,
                d.id, d.status, d.pages_done, d.pages_total, d.error
         FROM online_chapters c
         LEFT JOIN reading_progress p ON p.comic_id = c.comic_id
         LEFT JOIN chapter_downloads d ON d.id = (SELECT max(x.id) FROM chapter_downloads x WHERE x.chapter_id = c.id)
         WHERE c.series_id = ?1
         ORDER BY c.sort_order, c.id",
    )?;
    let rows = statement.query_map([series_id], |r| {
        let file_path: Option<String> = r.get(6)?;
        let comic_id: Option<i64> = r.get(7)?;
        let downloaded = file_path.as_deref().is_some_and(|path| std::path::Path::new(path).exists());
        let state = if comic_id.is_some() {
            "offline"
        } else if downloaded {
            "downloaded"
        } else {
            "available"
        };
        Ok(ChapterView {
            id: r.get(0)?,
            remote_id: r.get(1)?,
            title: r.get(2)?,
            number: r.get(3)?,
            published_at: r.get(4)?,
            order: r.get(5)?,
            state: state.into(),
            comic_id,
            progress: match (r.get::<_, Option<i64>>(8)?, r.get::<_, Option<i64>>(9)?) {
                (Some(page_index), Some(page_count)) => Some(ChapterProgress { page_index, page_count }),
                _ => None,
            },
            download: match r.get::<_, Option<i64>>(10)? {
                Some(id) => Some(ChapterDownload { id, status: r.get(11)?, pages_done: r.get(12)?, pages_total: r.get(13)?, error: r.get(14)? }),
                None => None,
            },
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// O capítulo só some do registro se ninguém depende dele localmente.
fn store_chapters(conn: &mut Connection, series_id: i64, chapters: &[super::runtime::Chapter]) -> AppResult<()> {
    let tx = conn.transaction()?;
    let mut seen = HashSet::new();
    let mut newest: Option<i64> = None;
    for chapter in chapters.iter().take(MAX_CHAPTERS) {
        if chapter.id.is_empty() || chapter.id.len() > MAX_REMOTE_ID_BYTES || !seen.insert(chapter.id.as_str()) {
            continue;
        }
        let Some(title) = clean(Some(&chapter.title), MAX_TITLE_CHARS) else { continue };
        newest = newest.max(chapter.published_at);
        tx.execute(
            "INSERT INTO online_chapters (series_id, remote_id, title, number, published_at, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (series_id, remote_id) DO UPDATE SET title = excluded.title, number = excluded.number, published_at = excluded.published_at, sort_order = excluded.sort_order",
            params![series_id, chapter.id, title, clean(chapter.number.as_deref(), 40), chapter.published_at, chapter.order],
        )?;
    }
    let existing: Vec<(i64, String)> = {
        let mut statement = tx.prepare("SELECT id, remote_id FROM online_chapters WHERE series_id = ?1 AND comic_id IS NULL AND file_path IS NULL")?;
        let rows = statement.query_map([series_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (id, remote) in existing {
        if !seen.contains(remote.as_str()) {
            tx.execute("DELETE FROM online_chapters WHERE id = ?1", [id])?;
        }
    }
    tx.execute("UPDATE online_series SET last_checked_at = ?2, last_update_at = coalesce(?3, last_update_at), updated_at = ?2 WHERE id = ?1", params![series_id, now_ms(), newest])?;
    tx.commit()?;
    Ok(())
}

/// O que há registrado, sem consultar a fonte. Funciona sem internet e depois de remover a extensão.
pub fn stored_series(conn: &Connection, id: i64) -> AppResult<SeriesView> {
    Ok(SeriesView { series: card(conn, id)?, chapters: chapters_of(conn, id)?, source_error: None })
}

fn remove_if_unused(core: &Core, conn: &Connection, id: i64) -> AppResult<()> {
    let removed = conn.execute(
        "DELETE FROM online_series WHERE id = ?1 AND following = 0 AND NOT EXISTS (SELECT 1 FROM online_chapters c WHERE c.series_id = ?1 AND (c.comic_id IS NOT NULL OR c.file_path IS NOT NULL))",
        [id],
    )?;
    if removed > 0 {
        let _ = std::fs::remove_file(cover_cache_file(core, id));
    }
    Ok(())
}

/// Atualiza detalhes e capítulos. Se a fonte falhar, mostra o último conhecido e diz o motivo.
pub fn open_series(core: &Core, id: i64, cancel: Option<&Cancel>) -> AppResult<SeriesView> {
    let series = card(&core.db(), id)?;
    let remote = (|| -> AppResult<_> {
        let (extension, source) = usable(core, &series.extension_id, &series.source_id)?;
        let engine = core.engine()?;
        let details = engine.details(&extension, cancel, &series.source_id, &series.remote_id)?;
        if matches!(details.rating, Some(ContentRating::Adult)) {
            return Err(AppError::user("source_rating", "O Prancha não oferece conteúdo adulto."));
        }
        let chapters = engine.chapters(&extension, cancel, &series.source_id, &series.remote_id)?;
        Ok((extension, source, details, chapters))
    })();
    match remote {
        Ok((extension, source, details, chapters)) => {
            let mut conn = core.db();
            let title = clean(Some(&details.title), MAX_TITLE_CHARS).unwrap_or_else(|| series.title.clone());
            upsert_series(
                core,
                &conn,
                &NewSeries {
                    extension_id: &series.extension_id,
                    source_id: &series.source_id,
                    remote_id: &series.remote_id,
                    title: &title,
                    cover_url: safe_cover(core, &extension, details.cover_url.as_deref()),
                    summary: clean(details.synopsis.as_deref(), MAX_TEXT_CHARS),
                    author: Some(clean_names(&details.authors).join(", ")).filter(|a| !a.is_empty()),
                    genres: Some(clean_names(&details.genres)).filter(|g| !g.is_empty()).and_then(|g| serde_json::to_string(&g).ok()),
                    status: details.status.map(status_name),
                    language: clean(details.language.as_deref(), 20).as_deref().or(Some(source.language.as_str())),
                },
            )?;
            store_chapters(&mut conn, id, &chapters)?;
            stored_series(&conn, id)
        }
        Err(error) if error.code() == "extension_cancelled" => Err(error),
        Err(error) if error.code() == "source_rating" => {
            remove_if_unused(core, &core.db(), id)?;
            Err(error)
        }
        Err(error) => {
            let mut view = stored_series(&core.db(), id)?;
            view.source_error = Some(error.to_string());
            Ok(view)
        }
    }
}

// ---------------------------------------------------------------------------
// Acompanhamento
// ---------------------------------------------------------------------------

pub fn set_following(conn: &Connection, id: i64, following: bool) -> AppResult<SeriesCard> {
    conn.execute("UPDATE online_series SET following = ?2 WHERE id = ?1", params![id, following])?;
    card(conn, id)
}

pub fn set_chapter_sort(conn: &Connection, id: i64, sort: &str) -> AppResult<SeriesCard> {
    if !matches!(sort, "asc" | "desc") {
        return Err(AppError::user("bad_request", "Ordenação inválida."));
    }
    conn.execute("UPDATE online_series SET chapter_sort = ?2 WHERE id = ?1", params![id, sort])?;
    card(conn, id)
}

pub fn list_following(conn: &Connection) -> AppResult<Vec<SeriesCard>> {
    let mut statement = conn.prepare(&format!("{CARD_SELECT} WHERE o.following = 1 ORDER BY o.title COLLATE NATSORT"))?;
    let rows = statement.query_map([], card_from_row)?.collect::<Result<_, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicOrigin {
    pub series_id: i64,
    pub series_title: String,
    pub chapter_title: String,
    pub source_name: Option<String>,
}

/// De onde veio uma HQ da biblioteca, se foi baixada de uma fonte.
pub fn comic_origin(conn: &Connection, comic_id: i64) -> AppResult<Option<ComicOrigin>> {
    Ok(conn
        .query_row(
            "SELECT o.id, o.title, c.title, s.name FROM online_chapters c JOIN online_series o ON o.id = c.series_id
             LEFT JOIN extension_sources s ON s.extension_id = o.extension_id AND s.source_id = o.source_id
             WHERE c.comic_id = ?1 LIMIT 1",
            [comic_id],
            |r| Ok(ComicOrigin { series_id: r.get(0)?, series_title: r.get(1)?, chapter_title: r.get(2)?, source_name: r.get(3)? }),
        )
        .optional()?)
}

/// Tira do banco as obras vistas só de passagem, há muito tempo.
pub fn prune_stale(core: &Core, conn: &Connection) -> AppResult<usize> {
    let ids: Vec<i64> = {
        let mut statement = conn.prepare(
            "SELECT o.id FROM online_series o WHERE o.following = 0 AND o.updated_at < ?1
               AND NOT EXISTS (SELECT 1 FROM online_chapters c WHERE c.series_id = o.id
                    AND (c.comic_id IS NOT NULL OR c.file_path IS NOT NULL OR EXISTS (SELECT 1 FROM chapter_downloads d WHERE d.chapter_id = c.id)))",
        )?;
        let rows = statement.query_map([now_ms() - STALE_AFTER_MS], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for id in &ids {
        conn.execute("DELETE FROM online_series WHERE id = ?1", [id])?;
        let _ = std::fs::remove_file(cover_cache_file(core, *id));
    }
    Ok(ids.len())
}

// ---------------------------------------------------------------------------
// Capas
// ---------------------------------------------------------------------------

/// Miniatura da obra. A interface nunca fala com o servidor da fonte: a CSP continua sem endereços externos.
pub fn cover(core: &Core, id: i64) -> AppResult<Option<Vec<u8>>> {
    let cached = cover_cache_file(core, id);
    if let Ok(bytes) = std::fs::read(&cached) {
        return Ok(Some(bytes));
    }
    let row: Option<(Option<String>, String)> = core.db().query_row("SELECT cover_url, extension_id FROM online_series WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((Some(url), extension_id)) = row else { return Ok(None) };
    // Extensão removida ou desabilitada: fica só o que já estava em cache.
    let Ok(extension) = install::runnable(core, &extension_id) else { return Ok(None) };
    if !extension.grant.network {
        return Ok(None);
    }
    let _permit = core.cover_gate.enter();
    let limits = Limits { max_bytes: MAX_COVER_BYTES, timeout: COVER_TIMEOUT };
    let Ok(response) = core.net.get(&url, Access::Hosts(&extension.grant.allowed_hosts), limits, Request { check_content_type: true, ..Default::default() }) else {
        return Ok(None);
    };
    if !response.is_success() {
        return Ok(None);
    }
    let Ok(thumbnail) = covers::make_thumbnail(&response.body) else { return Ok(None) };
    let temporary = cached.with_extension(format!("jpg.{:?}.tmp", std::thread::current().id()));
    if std::fs::write(&temporary, &thumbnail).is_ok() && std::fs::rename(&temporary, &cached).is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    Ok(Some(thumbnail))
}
