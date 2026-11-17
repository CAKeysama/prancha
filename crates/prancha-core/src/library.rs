//! Repositório do catálogo: HQs, progresso, favoritos, coleções, tags e configurações.
//! Todas as funções recebem uma `Connection`, o que as torna testáveis com banco em memória.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Row, ToSql, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::text::{clean_opt, fold};

// ---------------------------------------------------------------------------
// Tipos expostos à interface
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub page_index: i64,
    pub page_count: i64,
    pub scroll_ratio: f64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicSummary {
    pub id: i64,
    pub title: String,
    pub file_name: String,
    pub format: String,
    pub series: Option<String>,
    pub issue_number: Option<String>,
    pub volume: Option<String>,
    pub writer: Option<String>,
    pub page_count: Option<i64>,
    pub cover_kind: String,
    pub cover_rev: i64,
    pub cover_error: Option<String>,
    pub favorite: bool,
    pub status: String,
    pub missing: bool,
    pub added_at: i64,
    pub last_opened_at: Option<i64>,
    pub progress: Option<Progress>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionRef {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicDetails {
    #[serde(flatten)]
    pub summary: ComicSummary,
    pub path: String,
    pub file_size: i64,
    pub year: Option<i64>,
    pub artist: Option<String>,
    pub publisher: Option<String>,
    pub summary_text: Option<String>,
    pub cover_page: Option<i64>,
    pub tags: Vec<String>,
    pub collections: Vec<CollectionRef>,
    pub field_sources: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionInfo {
    pub id: i64,
    pub name: String,
    pub comic_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    /// Até 4 itens para a prévia de capas.
    pub preview: Vec<CoverRef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverRef {
    pub id: i64,
    pub title: String,
    pub cover_kind: String,
    pub cover_rev: i64,
    /// Uma obra de uma fonte: a capa vem por outro caminho.
    pub online: bool,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LibraryQuery {
    pub text: Option<String>,
    pub statuses: Vec<String>,
    pub favorites_only: bool,
    pub formats: Vec<String>,
    pub collection_id: Option<i64>,
    pub opened_only: bool,
    pub sort: Option<String>,
    pub descending: bool,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeData {
    pub total: i64,
    pub continue_reading: Vec<ComicSummary>,
    pub recently_added: Vec<ComicSummary>,
    pub favorites: Vec<ComicSummary>,
    pub recently_opened: Vec<ComicSummary>,
}

/// Campos editáveis. A interface envia o formulário completo.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataPatch {
    pub title: String,
    pub series: Option<String>,
    pub issue_number: Option<String>,
    pub volume: Option<String>,
    pub year: Option<i64>,
    pub writer: Option<String>,
    pub artist: Option<String>,
    pub publisher: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
}

/// Dados de uma HQ nova, produzidos pela importação.
#[derive(Debug, Clone, Default)]
pub struct NewComic {
    pub path: String,
    pub file_name: String,
    pub format: String,
    pub file_size: i64,
    pub fingerprint: String,
    pub page_count: Option<i64>,
    pub title: String,
    pub series: Option<String>,
    pub issue_number: Option<String>,
    pub volume: Option<String>,
    pub year: Option<i64>,
    pub writer: Option<String>,
    pub artist: Option<String>,
    pub publisher: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub field_sources: HashMap<String, String>,
}

/// Linha mínima usada pelo leitor, pelo protocolo de páginas e pela importação.
#[derive(Debug, Clone)]
pub struct ComicFileInfo {
    pub id: i64,
    pub path: String,
    pub format: String,
    pub fingerprint: String,
    pub page_count: Option<i64>,
    pub status: String,
    pub missing: bool,
    pub cover_file: Option<String>,
    pub title: String,
}

// ---------------------------------------------------------------------------
// Consultas
// ---------------------------------------------------------------------------

const SUMMARY_SELECT: &str = "SELECT c.id, c.title, c.file_name, c.format, c.series, c.issue_number, c.volume, c.writer, \
     c.page_count, c.cover_kind, c.cover_rev, c.cover_error, c.favorite, c.status, c.missing, c.added_at, \
     c.last_opened_at, p.page_index, p.page_count, p.scroll_ratio, p.updated_at \
     FROM comics c LEFT JOIN reading_progress p ON p.comic_id = c.id";

fn summary_from_row(r: &Row<'_>) -> rusqlite::Result<ComicSummary> {
    let progress = match r.get::<_, Option<i64>>(17)? {
        Some(page_index) => Some(Progress {
            page_index,
            page_count: r.get(18)?,
            scroll_ratio: r.get(19)?,
            updated_at: r.get(20)?,
        }),
        None => None,
    };
    Ok(ComicSummary {
        id: r.get(0)?,
        title: r.get(1)?,
        file_name: r.get(2)?,
        format: r.get(3)?,
        series: r.get(4)?,
        issue_number: r.get(5)?,
        volume: r.get(6)?,
        writer: r.get(7)?,
        page_count: r.get(8)?,
        cover_kind: r.get(9)?,
        cover_rev: r.get(10)?,
        cover_error: r.get(11)?,
        favorite: r.get::<_, i64>(12)? == 1,
        status: r.get(13)?,
        missing: r.get::<_, i64>(14)? == 1,
        added_at: r.get(15)?,
        last_opened_at: r.get(16)?,
        progress,
    })
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

pub fn list_comics(conn: &Connection, q: &LibraryQuery) -> AppResult<Vec<ComicSummary>> {
    let mut sql = String::from(SUMMARY_SELECT);
    let mut wheres: Vec<String> = Vec::new();
    let mut args: Vec<Box<dyn ToSql>> = Vec::new();

    if let Some(text) = q.text.as_deref() {
        for token in fold(text).split_whitespace() {
            wheres.push("c.search_text LIKE ? ESCAPE '\\'".into());
            args.push(Box::new(format!("%{}%", escape_like(token))));
        }
    }
    let statuses: Vec<&String> =
        q.statuses.iter().filter(|s| matches!(s.as_str(), "unread" | "reading" | "completed")).collect();
    if !statuses.is_empty() {
        wheres.push(format!("c.status IN ({})", vec!["?"; statuses.len()].join(",")));
        for s in statuses {
            args.push(Box::new(s.clone()));
        }
    }
    if q.favorites_only {
        wheres.push("c.favorite = 1".into());
    }
    if !q.formats.is_empty() {
        wheres.push(format!("c.format IN ({})", vec!["?"; q.formats.len()].join(",")));
        for f in &q.formats {
            args.push(Box::new(f.clone()));
        }
    }
    if let Some(cid) = q.collection_id {
        wheres.push("EXISTS (SELECT 1 FROM comic_collections cc WHERE cc.comic_id = c.id AND cc.collection_id = ?)".into());
        args.push(Box::new(cid));
    }
    if q.opened_only {
        wheres.push("c.last_opened_at IS NOT NULL".into());
    }
    if !wheres.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&wheres.join(" AND "));
    }
    let dir = if q.descending { "DESC" } else { "ASC" };
    let order = match q.sort.as_deref().unwrap_or("title") {
        "added" => format!("c.added_at {dir}, c.id {dir}"),
        "opened" => format!("c.last_opened_at IS NULL, c.last_opened_at {dir}, c.id"),
        "progress" => format!(
            "CASE c.status WHEN 'completed' THEN 1.0 WHEN 'unread' THEN 0.0 \
             ELSE (COALESCE(p.page_index, 0) + 1.0) / MAX(COALESCE(p.page_count, c.page_count, 1), 1) END {dir}, \
             c.title COLLATE NATSORT"
        ),
        "recentProgress" => format!("COALESCE(p.updated_at, c.last_opened_at, 0) {dir}"),
        _ => format!("c.title COLLATE NATSORT {dir}, c.id"),
    };
    sql.push_str(" ORDER BY ");
    sql.push_str(&order);
    if let Some(limit) = q.limit {
        sql.push_str(" LIMIT ?");
        args.push(Box::new(limit.clamp(1, 100_000)));
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter().map(|a| a.as_ref())), summary_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn home(conn: &Connection) -> AppResult<HomeData> {
    let total: i64 = conn.query_row("SELECT count(*) FROM comics", [], |r| r.get(0))?;
    let q = |query: LibraryQuery| list_comics(conn, &query);
    Ok(HomeData {
        total,
        continue_reading: q(LibraryQuery {
            statuses: vec!["reading".into()],
            sort: Some("recentProgress".into()),
            descending: true,
            limit: Some(12),
            ..Default::default()
        })?,
        recently_added: q(LibraryQuery {
            sort: Some("added".into()),
            descending: true,
            limit: Some(12),
            ..Default::default()
        })?,
        favorites: q(LibraryQuery { favorites_only: true, limit: Some(12), ..Default::default() })?,
        recently_opened: q(LibraryQuery {
            opened_only: true,
            sort: Some("opened".into()),
            descending: true,
            limit: Some(12),
            ..Default::default()
        })?,
    })
}

pub fn summary(conn: &Connection, id: i64) -> AppResult<ComicSummary> {
    let sql = format!("{SUMMARY_SELECT} WHERE c.id = ?");
    conn.query_row(&sql, [id], summary_from_row).optional()?.ok_or(AppError::NotFound)
}

pub fn details(conn: &Connection, id: i64) -> AppResult<ComicDetails> {
    let summary = summary(conn, id)?;
    let (path, file_size, year, artist, publisher, summary_text, cover_page, sources): (
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        String,
    ) = conn.query_row(
        "SELECT path, file_size, year, artist, publisher, summary, cover_page, field_sources FROM comics WHERE id = ?",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
    )?;
    Ok(ComicDetails {
        summary,
        path,
        file_size,
        year,
        artist,
        publisher,
        summary_text,
        cover_page,
        tags: tags_of(conn, id)?,
        collections: collections_of(conn, id)?,
        field_sources: serde_json::from_str(&sources).unwrap_or(Value::Object(Default::default())),
    })
}

const FILE_INFO_SELECT: &str = "SELECT id, path, format, fingerprint, page_count, status, missing, cover_file, title FROM comics";

fn file_info_from_row(r: &Row<'_>) -> rusqlite::Result<ComicFileInfo> {
    Ok(ComicFileInfo {
        id: r.get(0)?,
        path: r.get(1)?,
        format: r.get(2)?,
        fingerprint: r.get(3)?,
        page_count: r.get(4)?,
        status: r.get(5)?,
        missing: r.get::<_, i64>(6)? == 1,
        cover_file: r.get(7)?,
        title: r.get(8)?,
    })
}

pub fn comic_title(conn: &Connection, id: i64) -> AppResult<String> {
    conn.query_row("SELECT title FROM comics WHERE id = ?1", [id], |r| r.get(0)).optional()?.ok_or(AppError::NotFound)
}

pub fn file_info(conn: &Connection, id: i64) -> AppResult<ComicFileInfo> {
    let sql = format!("{FILE_INFO_SELECT} WHERE id = ?");
    conn.query_row(&sql, [id], file_info_from_row).optional()?.ok_or(AppError::NotFound)
}

pub fn all_file_infos(conn: &Connection) -> AppResult<Vec<ComicFileInfo>> {
    let mut stmt = conn.prepare(&format!("{FILE_INFO_SELECT} ORDER BY id"))?;
    let rows = stmt.query_map([], file_info_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn find_by_path(conn: &Connection, path: &str) -> AppResult<Option<i64>> {
    Ok(conn.query_row("SELECT id FROM comics WHERE path = ?", [path], |r| r.get(0)).optional()?)
}

pub fn find_by_fingerprint(conn: &Connection, fingerprint: &str) -> AppResult<Vec<ComicFileInfo>> {
    let mut stmt = conn.prepare(&format!("{FILE_INFO_SELECT} WHERE fingerprint = ? ORDER BY id"))?;
    let rows = stmt.query_map([fingerprint], file_info_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn tags_of(conn: &Connection, id: i64) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t JOIN comic_tags ct ON ct.tag_id = t.id WHERE ct.comic_id = ? ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([id], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn all_tags(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT t.name FROM tags t JOIN comic_tags ct ON ct.tag_id = t.id ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn collections_of(conn: &Connection, id: i64) -> AppResult<Vec<CollectionRef>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name FROM collections c JOIN comic_collections cc ON cc.collection_id = c.id \
         WHERE cc.comic_id = ? ORDER BY c.name COLLATE NATSORT",
    )?;
    let rows = stmt.query_map([id], |r| Ok(CollectionRef { id: r.get(0)?, name: r.get(1)? }))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// ---------------------------------------------------------------------------
// Escrita
// ---------------------------------------------------------------------------

fn set_tags(conn: &Connection, id: i64, tags: &[String]) -> AppResult<()> {
    conn.execute("DELETE FROM comic_tags WHERE comic_id = ?", [id])?;
    let mut seen: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() || tag.chars().count() > 64 || seen.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            continue;
        }
        seen.push(tag.to_string());
        conn.execute("INSERT INTO tags(name) VALUES (?) ON CONFLICT(name) DO NOTHING", [tag])?;
        let tag_id: i64 = conn.query_row("SELECT id FROM tags WHERE name = ?", [tag], |r| r.get(0))?;
        conn.execute("INSERT OR IGNORE INTO comic_tags(comic_id, tag_id) VALUES (?, ?)", params![id, tag_id])?;
    }
    // Remove tags órfãs.
    conn.execute("DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM comic_tags)", [])?;
    Ok(())
}

/// Recalcula o texto de busca (título, arquivo, série, autores, editora, tags) sem acentos.
fn refresh_search_text(conn: &Connection, id: i64) -> AppResult<()> {
    let parts: Vec<Option<String>> = conn.query_row(
        "SELECT title, file_name, series, writer, artist, publisher, issue_number FROM comics WHERE id = ?",
        [id],
        |r| Ok((0..7).map(|i| r.get::<_, Option<String>>(i).ok().flatten()).collect()),
    )?;
    let mut text: Vec<String> = parts.into_iter().flatten().collect();
    text.extend(tags_of(conn, id)?);
    conn.execute("UPDATE comics SET search_text = ? WHERE id = ?", params![fold(&text.join(" \u{1f} ")), id])?;
    Ok(())
}

pub fn insert_comic(conn: &Connection, c: &NewComic) -> AppResult<i64> {
    let now = now_ms();
    conn.execute(
        "INSERT INTO comics (path, file_name, format, file_size, fingerprint, page_count, title, series, \
         issue_number, volume, year, writer, artist, publisher, summary, field_sources, added_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17)",
        params![
            c.path,
            c.file_name,
            c.format,
            c.file_size,
            c.fingerprint,
            c.page_count,
            c.title,
            c.series,
            c.issue_number,
            c.volume,
            c.year,
            c.writer,
            c.artist,
            c.publisher,
            c.summary,
            serde_json::to_string(&c.field_sources).unwrap_or_else(|_| "{}".into()),
            now
        ],
    )?;
    let id = conn.last_insert_rowid();
    set_tags(conn, id, &c.tags)?;
    refresh_search_text(conn, id)?;
    Ok(id)
}

pub fn set_cover(
    conn: &Connection,
    id: i64,
    kind: &str,
    file: Option<&str>,
    page: Option<i64>,
    error: Option<&str>,
    params_version: i64,
) -> AppResult<()> {
    conn.execute(
        "UPDATE comics SET cover_kind = ?1, cover_file = ?2, cover_page = ?3, cover_error = ?4, cover_params = ?5, \
         cover_rev = cover_rev + 1, updated_at = ?6 WHERE id = ?7",
        params![kind, file, page, error, params_version, now_ms(), id],
    )?;
    Ok(())
}

pub fn set_page_count(conn: &Connection, id: i64, count: i64) -> AppResult<()> {
    conn.execute("UPDATE comics SET page_count = ?1, updated_at = ?2 WHERE id = ?3", params![count, now_ms(), id])?;
    conn.execute("UPDATE reading_progress SET page_count = ?1 WHERE comic_id = ?2", params![count, id])?;
    Ok(())
}

pub fn relink(conn: &Connection, id: i64, path: &str, file_name: &str, size: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE comics SET path = ?1, file_name = ?2, file_size = ?3, missing = 0, updated_at = ?4 WHERE id = ?5",
        params![path, file_name, size, now_ms(), id],
    )?;
    refresh_search_text(conn, id)
}

/// A HQ passou a apontar para um arquivo com outro conteúdo.
pub fn set_content(conn: &Connection, id: i64, fingerprint: &str, format: &str, page_count: Option<i64>) -> AppResult<()> {
    conn.execute(
        "UPDATE comics SET fingerprint = ?1, format = ?2, page_count = ?3 WHERE id = ?4",
        params![fingerprint, format, page_count, id],
    )?;
    Ok(())
}

pub fn set_missing(conn: &Connection, id: i64, missing: bool) -> AppResult<()> {
    conn.execute("UPDATE comics SET missing = ?1 WHERE id = ?2 AND missing <> ?1", params![missing as i64, id])?;
    Ok(())
}

pub fn update_metadata(conn: &mut Connection, id: i64, patch: &MetadataPatch) -> AppResult<()> {
    let title = patch.title.trim();
    if title.is_empty() {
        return Err(AppError::user("invalid", "O título não pode ficar vazio."));
    }
    if let Some(y) = patch.year {
        if !(1800..=2200).contains(&y) {
            return Err(AppError::user("invalid", "Informe um ano entre 1800 e 2200."));
        }
    }
    let tx = conn.transaction()?;
    let (cur, sources): (Vec<Option<String>>, String) = tx.query_row(
        "SELECT title, series, issue_number, volume, CAST(year AS TEXT), writer, artist, publisher, summary, field_sources \
         FROM comics WHERE id = ?",
        [id],
        |r| Ok(((0..9).map(|i| r.get::<_, Option<String>>(i).ok().flatten()).collect(), r.get(9)?)),
    )
    .optional()?
    .ok_or(AppError::NotFound)?;
    let mut sources: HashMap<String, String> = serde_json::from_str(&sources).unwrap_or_default();
    let new_values: [(&str, Option<String>); 9] = [
        ("title", Some(title.to_string())),
        ("series", clean_opt(patch.series.clone())),
        ("issueNumber", clean_opt(patch.issue_number.clone())),
        ("volume", clean_opt(patch.volume.clone())),
        ("year", patch.year.map(|y| y.to_string())),
        ("writer", clean_opt(patch.writer.clone())),
        ("artist", clean_opt(patch.artist.clone())),
        ("publisher", clean_opt(patch.publisher.clone())),
        ("summary", clean_opt(patch.summary.clone())),
    ];
    for (i, (field, value)) in new_values.iter().enumerate() {
        if cur.get(i).cloned().flatten() != *value {
            sources.insert((*field).to_string(), "user".into());
        }
    }
    let old_tags = tags_of(&tx, id)?;
    let mut new_tags: Vec<String> = patch.tags.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    new_tags.dedup();
    let mut folded: Vec<String> = new_tags.iter().map(|t| t.to_lowercase()).collect();
    folded.sort();
    folded.dedup();
    if old_tags.iter().map(|t| t.to_lowercase()).ne(folded) {
        sources.insert("tags".into(), "user".into());
    }
    tx.execute(
        "UPDATE comics SET title = ?1, series = ?2, issue_number = ?3, volume = ?4, year = ?5, writer = ?6, artist = ?7, \
         publisher = ?8, summary = ?9, field_sources = ?10, updated_at = ?11 WHERE id = ?12",
        params![
            title,
            new_values[1].1,
            new_values[2].1,
            new_values[3].1,
            patch.year,
            new_values[5].1,
            new_values[6].1,
            new_values[7].1,
            new_values[8].1,
            serde_json::to_string(&sources).unwrap_or_else(|_| "{}".into()),
            now_ms(),
            id
        ],
    )?;
    set_tags(&tx, id, &new_tags)?;
    refresh_search_text(&tx, id)?;
    tx.commit()?;
    Ok(())
}

pub fn set_favorite(conn: &Connection, id: i64, favorite: bool) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE comics SET favorite = ?1, updated_at = ?2 WHERE id = ?3",
        params![favorite as i64, now_ms(), id],
    )?;
    if n == 0 { Err(AppError::NotFound) } else { Ok(()) }
}

/// Marca como não lida (apaga o progresso) ou concluída (posiciona na última página).
pub fn set_read_status(conn: &Connection, id: i64, status: &str) -> AppResult<()> {
    let info = file_info(conn, id)?;
    let now = now_ms();
    match status {
        "unread" => {
            conn.execute("DELETE FROM reading_progress WHERE comic_id = ?", [id])?;
            conn.execute("UPDATE comics SET status = 'unread', updated_at = ?1 WHERE id = ?2", params![now, id])?;
        }
        "completed" => {
            if let Some(count) = info.page_count.filter(|c| *c > 0) {
                conn.execute(
                    "INSERT INTO reading_progress(comic_id, page_index, page_count, scroll_ratio, updated_at) \
                     VALUES (?1, ?2, ?3, 0, ?4) ON CONFLICT(comic_id) DO UPDATE SET page_index = ?2, page_count = ?3, \
                     scroll_ratio = 0, updated_at = ?4",
                    params![id, count - 1, count, now],
                )?;
            }
            conn.execute("UPDATE comics SET status = 'completed', updated_at = ?1 WHERE id = ?2", params![now, id])?;
        }
        _ => return Err(AppError::user("invalid", "Estado de leitura inválido.")),
    }
    Ok(())
}

/// Registra a abertura no leitor. Devolve a posição onde a leitura deve começar.
/// HQs concluídas recomeçam do início (o estado "Concluído" é mantido).
pub fn mark_opened(conn: &Connection, id: i64, page_count: i64) -> AppResult<Progress> {
    let now = now_ms();
    let info = file_info(conn, id)?;
    conn.execute("UPDATE comics SET last_opened_at = ?1 WHERE id = ?2", params![now, id])?;
    if info.status == "unread" {
        conn.execute("UPDATE comics SET status = 'reading' WHERE id = ?", [id])?;
    }
    let existing: Option<Progress> = conn
        .query_row(
            "SELECT page_index, page_count, scroll_ratio, updated_at FROM reading_progress WHERE comic_id = ?",
            [id],
            |r| Ok(Progress { page_index: r.get(0)?, page_count: r.get(1)?, scroll_ratio: r.get(2)?, updated_at: r.get(3)? }),
        )
        .optional()?;
    let start = match existing {
        Some(p) if info.status != "completed" => Progress {
            page_index: p.page_index.clamp(0, (page_count - 1).max(0)),
            page_count,
            scroll_ratio: p.scroll_ratio,
            updated_at: p.updated_at,
        },
        _ => Progress { page_index: 0, page_count, scroll_ratio: 0.0, updated_at: now },
    };
    if info.status != "completed" {
        conn.execute(
            "INSERT INTO reading_progress(comic_id, page_index, page_count, scroll_ratio, updated_at) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(comic_id) DO UPDATE SET page_index = ?2, page_count = ?3, scroll_ratio = ?4",
            params![id, start.page_index, page_count, start.scroll_ratio, start.updated_at],
        )?;
    }
    Ok(start)
}

/// Salva a posição de leitura. Chegar à última página conclui a leitura.
pub fn save_progress(conn: &Connection, id: i64, page_index: i64, page_count: i64, scroll_ratio: f64) -> AppResult<String> {
    if page_count <= 0 {
        return Err(AppError::user("invalid", "Número de páginas inválido."));
    }
    let page_index = page_index.clamp(0, page_count - 1);
    let scroll_ratio = if scroll_ratio.is_finite() { scroll_ratio.clamp(0.0, 1.0) } else { 0.0 };
    let info = file_info(conn, id)?;
    let status = if page_index == page_count - 1 || info.status == "completed" { "completed" } else { "reading" };
    let now = now_ms();
    conn.execute(
        "INSERT INTO reading_progress(comic_id, page_index, page_count, scroll_ratio, updated_at) VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(comic_id) DO UPDATE SET page_index = ?2, page_count = ?3, scroll_ratio = ?4, updated_at = ?5",
        params![id, page_index, page_count, scroll_ratio, now],
    )?;
    conn.execute(
        "UPDATE comics SET status = ?1, page_count = ?2, updated_at = ?3 WHERE id = ?4",
        params![status, page_count, now, id],
    )?;
    Ok(status.to_string())
}

/// Volta para a primeira página mantendo a HQ "Em andamento".
pub fn reset_progress(conn: &Connection, id: i64) -> AppResult<()> {
    let info = file_info(conn, id)?;
    let count = info.page_count.unwrap_or(0).max(0);
    let now = now_ms();
    conn.execute(
        "INSERT INTO reading_progress(comic_id, page_index, page_count, scroll_ratio, updated_at) VALUES (?1, 0, ?2, 0, ?3) \
         ON CONFLICT(comic_id) DO UPDATE SET page_index = 0, scroll_ratio = 0, updated_at = ?3",
        params![id, count, now],
    )?;
    conn.execute("UPDATE comics SET status = 'reading', updated_at = ?1 WHERE id = ?2", params![now, id])?;
    Ok(())
}

pub fn delete_comic(conn: &Connection, id: i64) -> AppResult<()> {
    let n = conn.execute("DELETE FROM comics WHERE id = ?", [id])?;
    conn.execute("DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM comic_tags)", [])?;
    if n == 0 { Err(AppError::NotFound) } else { Ok(()) }
}

// ---------------------------------------------------------------------------
// Coleções
// ---------------------------------------------------------------------------

fn validate_collection_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::user("invalid", "Dê um nome à coleção."));
    }
    if name.chars().count() > 80 {
        return Err(AppError::user("invalid", "O nome da coleção pode ter no máximo 80 caracteres."));
    }
    Ok(name.to_string())
}

fn map_unique(e: rusqlite::Error, name: &str) -> AppError {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
            AppError::user("duplicate", format!("Já existe uma coleção chamada “{name}”."))
        }
        other => other.into(),
    }
}

pub fn list_collections(conn: &Connection) -> AppResult<Vec<CollectionInfo>> {
    // Obras de fontes só contam com o plugin instalado.
    let shown = "EXISTS (SELECT 1 FROM settings WHERE key = 'plugins.online' AND value = 'true')";
    let mut stmt = conn.prepare(&format!(
        "SELECT c.id, c.name, c.created_at, c.updated_at, \
         (SELECT count(*) FROM comic_collections cc WHERE cc.collection_id = c.id) + \
         (SELECT count(*) FROM series_collections sc JOIN online_series o ON o.id = sc.series_id WHERE sc.collection_id = c.id AND {shown}) \
         FROM collections c ORDER BY c.name COLLATE NATSORT",
    ))?;
    let rows: Vec<(i64, String, i64, i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    let mut preview_stmt = conn.prepare(&format!(
        "SELECT id, title, cover_kind, cover_rev, online FROM ( \
           SELECT c.id, c.title, c.cover_kind, c.cover_rev, 0 AS online, cc.added_at FROM comics c JOIN comic_collections cc ON cc.comic_id = c.id WHERE cc.collection_id = ?1 \
           UNION ALL \
           SELECT o.id, o.title, CASE WHEN o.cover_url IS NULL THEN 'placeholder' ELSE 'format' END, 0, 1, sc.added_at FROM online_series o JOIN series_collections sc ON sc.series_id = o.id WHERE sc.collection_id = ?1 AND {shown}) \
         ORDER BY added_at DESC LIMIT 4",
    ))?;
    rows.into_iter()
        .map(|(id, name, created_at, updated_at, comic_count)| {
            let preview = preview_stmt
                .query_map([id], |r| {
                    Ok(CoverRef { id: r.get(0)?, title: r.get(1)?, cover_kind: r.get(2)?, cover_rev: r.get(3)?, online: r.get::<_, i64>(4)? != 0 })
                })?
                .collect::<Result<_, _>>()?;
            Ok(CollectionInfo { id, name, comic_count, created_at, updated_at, preview })
        })
        .collect()
}

pub fn create_collection(conn: &Connection, name: &str) -> AppResult<i64> {
    let name = validate_collection_name(name)?;
    let now = now_ms();
    conn.execute("INSERT INTO collections(name, created_at, updated_at) VALUES (?1, ?2, ?2)", params![name, now])
        .map_err(|e| map_unique(e, &name))?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_collection(conn: &Connection, id: i64, name: &str) -> AppResult<()> {
    let name = validate_collection_name(name)?;
    let n = conn
        .execute("UPDATE collections SET name = ?1, updated_at = ?2 WHERE id = ?3", params![name, now_ms(), id])
        .map_err(|e| map_unique(e, &name))?;
    if n == 0 { Err(AppError::NotFound) } else { Ok(()) }
}

/// Exclui apenas a coleção; as HQs continuam na biblioteca.
pub fn delete_collection(conn: &Connection, id: i64) -> AppResult<()> {
    let n = conn.execute("DELETE FROM collections WHERE id = ?", [id])?;
    if n == 0 { Err(AppError::NotFound) } else { Ok(()) }
}

/// Define exatamente as coleções de uma HQ.
pub fn set_comic_collections(conn: &mut Connection, comic_id: i64, collection_ids: &[i64]) -> AppResult<()> {
    file_info(conn, comic_id)?;
    let tx = conn.transaction()?;
    let now = now_ms();
    let placeholders = vec!["?"; collection_ids.len()].join(",");
    if collection_ids.is_empty() {
        tx.execute("DELETE FROM comic_collections WHERE comic_id = ?", [comic_id])?;
    } else {
        let sql = format!("DELETE FROM comic_collections WHERE comic_id = ? AND collection_id NOT IN ({placeholders})");
        let mut args: Vec<i64> = vec![comic_id];
        args.extend_from_slice(collection_ids);
        tx.execute(&sql, params_from_iter(args.iter()))?;
    }
    for cid in collection_ids {
        tx.execute(
            "INSERT OR IGNORE INTO comic_collections(comic_id, collection_id, added_at) VALUES (?1, ?2, ?3)",
            params![comic_id, cid, now],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn add_to_collection(conn: &mut Connection, collection_id: i64, comic_ids: &[i64]) -> AppResult<()> {
    let tx = conn.transaction()?;
    let exists: Option<i64> =
        tx.query_row("SELECT id FROM collections WHERE id = ?", [collection_id], |r| r.get(0)).optional()?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    let now = now_ms();
    for id in comic_ids {
        tx.execute(
            "INSERT OR IGNORE INTO comic_collections(comic_id, collection_id, added_at) VALUES (?1, ?2, ?3)",
            params![id, collection_id, now],
        )?;
    }
    tx.execute("UPDATE collections SET updated_at = ?1 WHERE id = ?2", params![now, collection_id])?;
    tx.commit()?;
    Ok(())
}

pub fn remove_from_collection(conn: &Connection, collection_id: i64, comic_id: i64) -> AppResult<()> {
    conn.execute(
        "DELETE FROM comic_collections WHERE collection_id = ?1 AND comic_id = ?2",
        params![collection_id, comic_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Configurações (JSON por chave)
// ---------------------------------------------------------------------------

fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 64 && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn get_settings(conn: &Connection) -> AppResult<HashMap<String, Value>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (k, v) = row?;
        if let Ok(v) = serde_json::from_str(&v) {
            out.insert(k, v);
        }
    }
    Ok(out)
}

pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<Value>> {
    let v: Option<String> = conn.query_row("SELECT value FROM settings WHERE key = ?", [key], |r| r.get(0)).optional()?;
    Ok(v.and_then(|s| serde_json::from_str(&s).ok()))
}

pub fn set_setting(conn: &Connection, key: &str, value: &Value) -> AppResult<()> {
    if !valid_key(key) {
        return Err(AppError::user("invalid", "Chave de configuração inválida."));
    }
    let json = serde_json::to_string(value).map_err(|e| AppError::user("invalid", e.to_string()))?;
    if json.len() > 64 * 1024 {
        return Err(AppError::user("invalid", "Valor de configuração grande demais."));
    }
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, json],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Capas pendentes / manutenção
// ---------------------------------------------------------------------------

/// HQs com capa pendente ou gerada com parâmetros antigos. Quem desenha a capa de um PDF é a interface.
pub fn pending_covers(conn: &Connection, pdf: bool) -> AppResult<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM comics WHERE missing = 0 AND (format = 'pdf') = ?1 AND (cover_kind = 'pending' OR cover_params <> ?2) ORDER BY id",
    )?;
    let rows = stmt.query_map(params![pdf, crate::covers::COVER_PARAMS_VERSION], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Após limpar o cache de miniaturas: todas as capas voltam a "pendente" para regeneração.
pub fn reset_all_covers(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE comics SET cover_kind = 'pending', cover_file = NULL, cover_page = NULL, cover_error = NULL, \
         cover_rev = cover_rev + 1",
        [],
    )?)
}

// ---------------------------------------------------------------------------
// Histórico de importação
// ---------------------------------------------------------------------------

pub fn create_import_job(conn: &Connection) -> AppResult<i64> {
    conn.execute("INSERT INTO import_jobs(status, created_at) VALUES ('running', ?)", [now_ms()])?;
    Ok(conn.last_insert_rowid())
}

pub fn add_import_item(
    conn: &Connection,
    job_id: i64,
    path: &str,
    status: &str,
    message: Option<&str>,
    comic_id: Option<i64>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO import_job_items(job_id, path, status, message, comic_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![job_id, path, status, message, comic_id],
    )?;
    Ok(())
}

pub fn finish_import_job(
    conn: &Connection,
    job_id: i64,
    status: &str,
    counts: (i64, i64, i64, i64, i64),
) -> AppResult<()> {
    conn.execute(
        "UPDATE import_jobs SET status = ?1, total = ?2, processed = ?3, added = ?4, skipped = ?5, failed = ?6, finished_at = ?7 \
         WHERE id = ?8",
        params![status, counts.0, counts.1, counts.2, counts.3, counts.4, now_ms(), job_id],
    )?;
    Ok(())
}

/// Importações que estavam rodando quando o app fechou são marcadas como interrompidas.
pub fn mark_interrupted_jobs(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute("UPDATE import_jobs SET status = 'interrupted', finished_at = ? WHERE status = 'running'", [now_ms()])?)
}
