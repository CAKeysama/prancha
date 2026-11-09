//! Chama as fontes, filtra o que chega e guarda o que a biblioteca online precisa lembrar.
//! Tudo que vem de uma extensão é tratado como dado de terceiros: tamanho limitado, sem endereços fora das regras
//! e conteúdo adulto só com o filtro +18 desligado.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use reqwest::Url;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

use super::net::{Access, Cancel, Limits, Request};
use super::runtime::{ComicDetails, ContentRating, Filter, Installed, Page, PublicationStatus, SearchPage, SearchResult};
use super::suwayomi::{self, Browse};
use super::rating::{self, Declared, State};
use super::reading::{self, ReadState};
use super::{downloads, install, minors, rating_store};
use crate::core::Core;
use crate::covers;
use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::notifications;
use crate::library::{self, ComicFileInfo, Progress};

const MAX_RESULTS: usize = 200;
const MAX_CHAPTERS: usize = 5000;
const MAX_TITLE_CHARS: usize = 300;
const MAX_TEXT_CHARS: usize = 4000;
const MAX_NAMES: usize = 32;
const MAX_REMOTE_ID_BYTES: usize = 1024;
const MAX_COVER_BYTES: u64 = 16 * 1024 * 1024;
const COVER_TIMEOUT: Duration = Duration::from_secs(30);
const CONCURRENT_IMAGES: usize = 4;
/// Por fonte: uma lenta não segura as outras.
const CONCURRENT_CHECKS: usize = 4;
/// Falhas seguidas: a fonte está com problema, e as outras obras da página ficam para a próxima vez.
const MAX_CHECK_FAILURES: usize = 3;
const MAX_CACHED_CHAPTERS: usize = 8;
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

/// Limita quantas imagens são buscadas ao mesmo tempo: uma grade inteira não vira uma rajada contra o servidor.
pub struct Gate {
    used: Mutex<usize>,
    freed: Condvar,
}

impl Default for Gate {
    fn default() -> Self {
        Gate { used: Mutex::new(0), freed: Condvar::new() }
    }
}

struct Permit<'a>(&'a Gate);

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *self.0.used.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        self.0.freed.notify_one();
    }
}

impl Gate {
    fn enter(&self) -> Permit<'_> {
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        while *used >= CONCURRENT_IMAGES {
            used = self.freed.wait(used).unwrap_or_else(|e| e.into_inner());
        }
        *used += 1;
        Permit(self)
    }
}

/// Listas de páginas dos capítulos abertos há pouco, para cada imagem não consultar a fonte de novo.
#[derive(Default)]
pub struct ChapterPages(Mutex<HashMap<i64, Arc<Vec<Page>>>>);

impl ChapterPages {
    fn get(&self, chapter_id: i64) -> Option<Arc<Vec<Page>>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).get(&chapter_id).cloned()
    }

    fn put(&self, chapter_id: i64, pages: Arc<Vec<Page>>) {
        let mut lists = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // shortcut: esvazia tudo em vez de tirar o mais antigo; quem estiver lendo só consulta a fonte mais uma vez.
        if lists.len() >= MAX_CACHED_CHAPTERS {
            lists.clear();
        }
        lists.insert(chapter_id, pages);
    }

    fn forget(&self, chapter_id: i64) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(&chapter_id);
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

/// Fontes que podem aparecer em Explorar: de extensões habilitadas e dentro do filtro +18.
pub fn available_sources(conn: &Connection) -> AppResult<Vec<SourceSummary>> {
    let mut statement = conn.prepare(&format!(
        "SELECT s.extension_id, e.name, e.repository_name, s.source_id, s.name, s.language, s.description, s.kinds, s.rating, s.direction, s.supports_latest, s.supports_search
         FROM extension_sources s JOIN installed_extensions e ON e.id = s.extension_id
         WHERE e.enabled = 1 AND (s.rating IN ('general', 'teen') OR {adult})
         ORDER BY s.name COLLATE NATSORT, e.name COLLATE NATSORT",
        adult = library::ADULT_ALLOWED_SQL,
    ))?;
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

/// A fonte existe, está dentro do filtro +18 e a extensão está habilitada.
fn usable(core: &Core, extension_id: &str, source_id: &str) -> AppResult<(Installed, SourceRow)> {
    let row = source_row(&core.db(), extension_id, source_id)?;
    if !install::rating_is_available(&core.db(), &row.rating) {
        return Err(AppError::user("source_disabled", "Esta fonte é +18 ou não informa a classificação. Para usar, desligue o filtro +18 em Plugins."));
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
    if extension.bridged {
        // Um caminho no servidor local: é ele quem busca a imagem na fonte.
        return url.starts_with("/api/v1/").then_some(url);
    }
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
    pub favorite: bool,
    /// Capítulos que chegaram depois da primeira sincronização e ainda não foram abertos nem marcados como lidos.
    pub new_count: i64,
    /// Classificação +18: `confirmed`, `suspect`, `none` ou `insufficient`.
    pub rating_state: String,
    /// Quem decidiu: `repository`, `inferred` ou `manual`.
    pub rating_origin: String,
}

const CARD_SELECT: &str = "SELECT o.id, o.extension_id, o.source_id, s.name, o.remote_id, o.title, o.cover_url, o.summary, o.author, o.genres, o.status, o.language, o.following, o.chapter_sort, o.last_checked_at, o.last_update_at, o.favorite,
        (SELECT count(*) FROM online_chapters n WHERE n.series_id = o.id AND n.acknowledged_at IS NULL), o.rating_state, o.rating_origin
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
        favorite: r.get::<_, i64>(16)? != 0,
        new_count: r.get(17)?,
        rating_state: r.get(18)?,
        rating_origin: r.get(19)?,
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
    adult: bool,
    /// `None` quando esta informação não diz nada sobre a classificação da obra.
    classification: Option<&'static str>,
    /// A classificação que a própria obra declarou, se declarou.
    declared: Option<Declared>,
    /// A fonte acabou de declarar a obra como +18, por ela mesma ou pela fonte inteira de uma extensão nativa.
    declared_adult: bool,
}

/// O que a fonte deixou de informar não apaga o que já se sabia. Devolve a obra e a classificação com que ela ficou.
/// A classificação +18 é refeita aqui, só se algo que ela usa mudou.
fn upsert_series(core: &Core, conn: &Connection, series: &NewSeries) -> AppResult<(i64, String)> {
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
    let stored = conn.query_row(
        "INSERT INTO online_series (extension_id, source_id, remote_id, title, cover_url, summary, author, genres, status, language, created_at, updated_at, adult, classification, declared_rating, declared_adult_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, coalesce(?8, '[]'), ?9, ?10, ?11, ?11, ?12, coalesce(?13, 'pending'), ?14, CASE WHEN ?15 THEN ?11 END)
         ON CONFLICT (extension_id, source_id, remote_id) DO UPDATE SET
            title = excluded.title,
            cover_url = coalesce(?5, cover_url),
            summary = coalesce(?6, summary),
            author = coalesce(?7, author),
            genres = coalesce(?8, genres),
            status = coalesce(?9, status),
            language = coalesce(?10, language),
            updated_at = excluded.updated_at,
            adult = max(adult, ?12),
            classification = CASE WHEN classification = 'known' THEN 'known' ELSE coalesce(?13, classification) END,
            declared_rating = coalesce(?14, declared_rating),
            declared_adult_at = CASE WHEN ?15 THEN ?11 ELSE declared_adult_at END
         RETURNING id, classification",
        params![
            series.extension_id,
            series.source_id,
            series.remote_id,
            series.title,
            series.cover_url,
            series.summary,
            series.author,
            series.genres,
            series.status,
            series.language,
            now,
            series.adult,
            series.classification,
            series.declared.map(Declared::as_str),
            series.declared_adult
        ],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    rating_store::analyze(conn, stored.0)?;
    Ok(stored)
}

/// A obra sai do banco e de todas as telas, e fica só o endereço, para não voltar.
/// Capítulo já na biblioteca é um arquivo da pessoa: continua lá, sem ligação com a obra.
fn block_series(core: &Core, conn: &Connection, extension_id: &str, source_id: &str, remote_id: &str) -> AppResult<()> {
    conn.execute("INSERT OR IGNORE INTO blocked_series (extension_id, source_id, remote_id) VALUES (?1, ?2, ?3)", [extension_id, source_id, remote_id])?;
    let id: Option<i64> = conn.query_row("SELECT id FROM online_series WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3", [extension_id, source_id, remote_id], |r| r.get(0)).optional()?;
    if let Some(id) = id {
        downloads::remove_partial_pages(core, conn, id)?;
        conn.execute("DELETE FROM online_series WHERE id = ?1", [id])?;
        let _ = std::fs::remove_file(cover_cache_file(core, id));
    }
    Ok(())
}

/// Tira o que ficou guardado de antes de a obra ser reconhecida, ou de antes de a lista de palavras mudar.
pub fn forget_blocked(core: &Core, conn: &Connection) -> AppResult<usize> {
    let rows: Vec<(String, String, String, String, String)> = {
        let mut statement = conn.prepare("SELECT extension_id, source_id, remote_id, title, genres FROM online_series")?;
        let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut removed = 0;
    for (extension_id, source_id, remote_id, title, genres) in rows {
        if minors::sexualizes_minors(&title, &serde_json::from_str::<Vec<String>>(&genres).unwrap_or_default()) {
            block_series(core, conn, &extension_id, &source_id, &remote_id)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePage {
    pub results: Vec<SeriesCard>,
    pub has_more: bool,
    /// Obras que o filtro +18 escondeu. Com ele desligado, aparecem.
    pub hidden: usize,
    /// Obras que a consulta não conseguiu conferir agora: ficam de fora com o filtro ligado ou desligado, e podem aparecer em outra tentativa.
    pub unverified: usize,
}

/// A marca antiga: a da obra, se houver, ou a da fonte. Continua gravada, mas quem decide o filtro é `rating`.
fn rated_adult(source: &SourceRow, rating: &Option<ContentRating>) -> bool {
    !matches!(source.rating.as_str(), "general" | "teen") || matches!(rating, Some(ContentRating::Adult))
}

fn declared(rating: &Option<ContentRating>) -> Option<Declared> {
    rating.as_ref().map(|rating| match rating {
        ContentRating::General => Declared::General,
        ContentRating::Teen => Declared::Teen,
        ContentRating::Adult => Declared::Adult,
        ContentRating::Unknown => Declared::Unknown,
    })
}

/// Uma declaração explícita de +18, que fica no histórico da obra. A marca de uma fonte do Mihon não conta: pode ser mista.
fn declares_adult(extension: &Installed, source: &SourceRow, rating: &Option<ContentRating>) -> bool {
    matches!(rating, Some(ContentRating::Adult)) || (!extension.bridged && source.rating == "adult")
}

fn genres_json(genres: &[String]) -> Option<String> {
    if genres.is_empty() { None } else { serde_json::to_string(genres).ok() }
}

fn details_title(details: &ComicDetails, known: &str) -> String {
    clean(Some(&details.title), MAX_TITLE_CHARS).unwrap_or_else(|| known.to_string())
}

/// Onde uma obra está: a extensão, a fonte e o identificador nela.
struct SeriesPlace<'a> {
    extension: &'a Installed,
    source_id: &'a str,
    source: &'a SourceRow,
    remote_id: &'a str,
}

/// Grava o que os detalhes dizem da obra. A que indica sexualização de menores sai de vez.
fn store_details(core: &Core, conn: &Connection, place: &SeriesPlace, known_title: &str, details: &ComicDetails) -> AppResult<()> {
    let title = details_title(details, known_title);
    let genres = clean_names(&details.genres);
    if minors::sexualizes_minors(&title, &genres) {
        return block_series(core, conn, &place.extension.id, place.source_id, place.remote_id);
    }
    upsert_series(
        core,
        conn,
        &NewSeries {
            extension_id: &place.extension.id,
            source_id: place.source_id,
            remote_id: place.remote_id,
            title: &title,
            cover_url: safe_cover(core, place.extension, details.cover_url.as_deref()),
            summary: clean(details.synopsis.as_deref(), MAX_TEXT_CHARS),
            author: Some(clean_names(&details.authors).join(", ")).filter(|a| !a.is_empty()),
            genres: genres_json(&genres),
            status: details.status.map(status_name),
            language: clean(details.language.as_deref(), 20).as_deref().or(Some(place.source.language.as_str())),
            adult: rated_adult(place.source, &details.rating),
            classification: Some(if !place.extension.bridged || !genres.is_empty() { "known" } else { "unknown" }),
            declared: declared(&details.rating),
            declared_adult: declares_adult(place.extension, place.source, &details.rating),
        },
    )?;
    Ok(())
}

struct ToCheck {
    remote_id: String,
    title: String,
}

/// Consulta os detalhes das obras que a lista trouxe sem gêneros, antes de qualquer uma aparecer. O que foi respondido
/// fica gravado, mesmo se a pessoa sair da tela: a próxima visita só pergunta o que faltou. Devolve o primeiro erro.
fn check_series(core: &Core, extension: &Installed, source_id: &str, source: &SourceRow, to_check: &[ToCheck], cancel: Option<&Cancel>) -> Option<AppError> {
    let next = AtomicUsize::new(0);
    let failures = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let first_error = Mutex::new(None);
    let work = || {
        while !stop.load(Ordering::Relaxed) && !cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            let Some(item) = to_check.get(next.fetch_add(1, Ordering::Relaxed)) else { return };
            let place = SeriesPlace { extension, source_id, source, remote_id: &item.remote_id };
            match suwayomi::details(core, &item.remote_id, cancel).and_then(|details| store_details(core, &core.db(), &place, &item.title, &details)) {
                Ok(()) => failures.store(0, Ordering::Relaxed),
                Err(error) => {
                    // Com o site pedindo verificação, nenhuma das outras vai passar.
                    if failures.fetch_add(1, Ordering::Relaxed) + 1 >= MAX_CHECK_FAILURES || matches!(error.code(), "source_protected" | "extension_cancelled") {
                        stop.store(true, Ordering::Relaxed);
                    }
                    first_error.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(error);
                }
            }
        }
    };
    std::thread::scope(|scope| {
        for _ in 0..CONCURRENT_CHECKS.min(to_check.len()) {
            scope.spawn(work);
        }
    });
    first_error.into_inner().unwrap_or_else(|e| e.into_inner())
}

/// Grava a página e devolve só o que pode aparecer. `genres` é o que a lista trouxe, por obra: as extensões nativas não trazem,
/// e nelas a classificação da fonte vale para a obra que não declarar a sua.
fn store_page(core: &Core, extension: &Installed, source_id: &str, source: &SourceRow, page: SearchPage, genres: &HashMap<String, Vec<String>>, cancel: Option<&Cancel>) -> AppResult<BrowsePage> {
    let mut ids = Vec::new();
    let mut to_check = Vec::new();
    let mut hidden = 0;
    {
        let mut conn = core.db();
        let adult_allowed = library::adult_allowed(&conn);
        let tx = conn.transaction()?;
        let mut seen = HashSet::new();
        for result in page.results.iter().filter(|r| !r.id.is_empty() && r.id.len() <= MAX_REMOTE_ID_BYTES).take(MAX_RESULTS) {
            let Some(title) = clean(Some(&result.title), MAX_TITLE_CHARS) else { continue };
            if !seen.insert(result.id.as_str()) {
                continue;
            }
            let listed_genres = clean_names(genres.get(&result.id).map_or(&[], Vec::as_slice));
            let blocked: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM blocked_series WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3)", [extension.id.as_str(), source_id, result.id.as_str()], |r| r.get(0))?;
            if blocked || minors::sexualizes_minors(&title, &listed_genres) {
                block_series(core, &tx, &extension.id, source_id, &result.id)?;
                continue;
            }
            // Com o filtro ligado, o que a própria lista já confirma como +18 nem entra no banco, a não ser que a pessoa
            // tenha corrigido a classificação desta obra.
            if !adult_allowed && listed_confirmed(&tx, extension, source_id, source, result, &title, &listed_genres)? {
                hidden += 1;
                continue;
            }
            let (id, classification) = upsert_series(
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
                    genres: genres_json(&listed_genres),
                    status: result.status.map(status_name),
                    language: Some(&source.language),
                    adult: rated_adult(source, &result.rating),
                    classification: (!extension.bridged || !listed_genres.is_empty()).then_some("known"),
                    declared: declared(&result.rating),
                    declared_adult: declares_adult(extension, source, &result.rating),
                },
            )?;
            if classification == "pending" {
                to_check.push(ToCheck { remote_id: result.id.clone(), title });
            }
            ids.push(id);
        }
        tx.commit()?;
    }
    let failure = check_series(core, extension, source_id, source, &to_check, cancel);
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(AppError::user("extension_cancelled", "Operação cancelada."));
    }
    let conn = core.db();
    let mut results = Vec::new();
    let mut unverified = 0;
    // A lista nem sempre traz os gêneros: a obra recusada ao abrir não volta para ela.
    let mut state = conn.prepare(&format!("SELECT o.classification, {safe} OR {adult} FROM online_series o WHERE o.id = ?1", safe = library::SERIES_SAFE_SQL, adult = library::ADULT_ALLOWED_SQL))?;
    for id in ids {
        match state.query_row([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))).optional()? {
            Some((classification, _)) if classification == "pending" => unverified += 1,
            Some((_, true)) => results.push(card(&conn, id)?),
            Some(_) => hidden += 1,
            None => {}
        }
    }
    // Nada para mostrar, ou um site pedindo uma verificação que a pessoa precisa saber que existe.
    if let Some(error) = failure {
        if results.is_empty() || error.code() == "source_protected" {
            return Err(error);
        }
    }
    Ok(BrowsePage { results, has_more: page.has_more, hidden, unverified })
}

/// A lista sozinha já confirma a obra como +18, e não há correção da pessoa dizendo o contrário.
fn listed_confirmed(conn: &Connection, extension: &Installed, source_id: &str, source: &SourceRow, result: &SearchResult, title: &str, genres: &[String]) -> AppResult<bool> {
    let summary = clean(result.summary.as_deref(), MAX_TEXT_CHARS);
    let meta = rating::Metadata {
        origin: String::new(),
        runtime: Some(if extension.bridged { rating::Runtime::Bridged } else { rating::Runtime::Native }),
        title: title.to_string(),
        alt_titles: rating::alt_titles_from(summary.as_deref()),
        description: summary,
        genres: rating::normalize_genres(genres),
        series_rating: declared(&result.rating),
        source_rating: Declared::parse(&source.rating),
        declared_adult_at: None,
        cover: None,
        observed_at: 0,
    };
    if rating::classify(&meta, &rating::Params::DEFAULT).state != State::Confirmed {
        return Ok(false);
    }
    let corrected: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM series_rating_overrides WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3)",
        [extension.id.as_str(), source_id, result.id.as_str()],
        |r| r.get(0),
    )?;
    Ok(!corrected)
}

fn kind_filter(kind: Option<&str>) -> Vec<Filter> {
    kind.map(|value| Filter { name: "kind".into(), value: value.into() }).into_iter().collect()
}

pub fn popular(core: &Core, extension_id: &str, source_id: &str, page: u32, kind: Option<&str>, cancel: Option<&Cancel>) -> AppResult<BrowsePage> {
    let (extension, source) = usable(core, extension_id, source_id)?;
    let (found, genres) = if extension.bridged { suwayomi::browse(core, source_id, Browse::Popular, page, cancel)? } else { (core.engine()?.popular(&extension, cancel, source_id, page, &kind_filter(kind))?, HashMap::new()) };
    store_page(core, &extension, source_id, &source, found, &genres, cancel)
}

pub fn latest(core: &Core, extension_id: &str, source_id: &str, page: u32, kind: Option<&str>, cancel: Option<&Cancel>) -> AppResult<BrowsePage> {
    let (extension, source) = usable(core, extension_id, source_id)?;
    if !source.supports_latest {
        return Err(AppError::user("source_unsupported", "Esta fonte não informa atualizações recentes."));
    }
    let (found, genres) = if extension.bridged { suwayomi::browse(core, source_id, Browse::Latest, page, cancel)? } else { (core.engine()?.latest(&extension, cancel, source_id, page, &kind_filter(kind))?, HashMap::new()) };
    store_page(core, &extension, source_id, &source, found, &genres, cancel)
}

pub fn search(core: &Core, extension_id: &str, source_id: &str, query: &str, page: u32, kind: Option<&str>, cancel: Option<&Cancel>) -> AppResult<BrowsePage> {
    let (extension, source) = usable(core, extension_id, source_id)?;
    if !source.supports_search {
        return Err(AppError::user("source_unsupported", "Esta fonte não oferece pesquisa."));
    }
    let (found, genres) = if extension.bridged { suwayomi::browse(core, source_id, Browse::Search(query.trim()), page, cancel)? } else { (core.engine()?.search(&extension, cancel, source_id, query.trim(), page, &kind_filter(kind))?, HashMap::new()) };
    store_page(core, &extension, source_id, &source, found, &genres, cancel)
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
    /// `unread`, `reading` (sessão ativa no leitor), `in_progress` ou `read`.
    pub read_state: ReadState,
    /// Chegou depois da primeira sincronização e ainda não foi aberto nem marcado como lido. Independe do estado de leitura.
    pub is_new: bool,
    pub first_seen_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesView {
    #[serde(flatten)]
    pub series: SeriesCard,
    pub chapters: Vec<ChapterView>,
    #[serde(flatten)]
    pub reading: SeriesReading,
    /// A fonte não respondeu: o que aparece é o último conhecido.
    pub source_error: Option<String>,
    /// O motivo foi uma verificação anti-robô, que a pessoa pode passar no site.
    pub source_protected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesResume {
    pub chapter_id: i64,
    pub title: String,
    pub progress: Option<ChapterProgress>,
    /// O estado do capítulo onde a leitura continua, para a biblioteca dizer "Capítulo 42 · Em andamento".
    pub state: ReadState,
}

/// Até onde a pessoa leu e onde a obra está guardada, igual na página da obra e na biblioteca.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesReading {
    /// `unread`, `reading` ou `completed`, como nas HQs. `status`, na obra, é a situação da publicação.
    pub read_status: &'static str,
    pub chapter_count: usize,
    pub chapters_read: usize,
    pub resume: Option<SeriesResume>,
    pub collections: Vec<i64>,
}

/// Onde a leitura continua: no capítulo mais adiantado já iniciado ou, se ele terminou, no seguinte ainda não lido.
/// Terminados todos, ler de novo começa do primeiro. `chapters` vem na ordem de leitura.
fn reading_of(conn: &Connection, id: i64, chapters: &[ChapterView]) -> AppResult<SeriesReading> {
    let read = |chapter: &ChapterView| chapter.completed_at.is_some();
    let touched = |chapter: &ChapterView| read(chapter) || matches!(chapter.read_state, ReadState::Reading | ReadState::InProgress);
    let chapters_read = chapters.iter().filter(|chapter| read(chapter)).count();
    let reached = chapters.iter().rposition(touched);
    let resume = match reached {
        None => chapters.first(),
        Some(at) if !read(&chapters[at]) => chapters.get(at),
        Some(at) => chapters[at + 1..].iter().find(|chapter| !read(chapter)).or_else(|| chapters.iter().find(|chapter| !read(chapter))).or(chapters.first()),
    };
    let status = if chapters.is_empty() || reached.is_none() {
        "unread"
    } else if chapters_read == chapters.len() {
        "completed"
    } else {
        "reading"
    };
    let mut statement = conn.prepare("SELECT collection_id FROM series_collections WHERE series_id = ?1")?;
    let collections = statement.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(SeriesReading {
        read_status: status,
        chapter_count: chapters.len(),
        chapters_read,
        resume: resume.map(|chapter| SeriesResume {
            chapter_id: chapter.id,
            title: chapter.title.clone(),
            progress: chapter.progress.clone().filter(|_| matches!(chapter.read_state, ReadState::Reading | ReadState::InProgress)),
            state: chapter.read_state,
        }),
        collections,
    })
}

pub fn chapters_of(conn: &Connection, series_id: i64) -> AppResult<Vec<ChapterView>> {
    let mut statement = conn.prepare(
        &format!("SELECT c.id, c.remote_id, c.title, c.number, c.published_at, c.sort_order, c.file_path, c.comic_id, coalesce(p.page_index, q.page_index), coalesce(p.page_count, q.page_count),
                d.id, d.status, d.pages_done, d.pages_total, d.error,
                {active}, c.reading_started_at IS NOT NULL, c.completed_at, c.manual_unread, c.acknowledged_at IS NULL, c.first_seen_at
         FROM online_chapters c
         LEFT JOIN reading_progress p ON p.comic_id = c.comic_id
         LEFT JOIN chapter_progress q ON q.chapter_id = c.id
         LEFT JOIN chapter_downloads d ON d.id = (SELECT max(x.id) FROM chapter_downloads x WHERE x.chapter_id = c.id)
         WHERE c.series_id = ?1
         ORDER BY c.sort_order, c.id", active = reading::active_session_sql("?2")),
    )?;
    let rows = statement.query_map(params![series_id, now_ms()], |r| {
        let completed_at: Option<i64> = r.get(17)?;
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
            read_state: reading::derive_state(r.get(15)?, r.get(16)?, completed_at.is_some(), r.get::<_, i64>(18)? != 0),
            is_new: r.get(19)?,
            first_seen_at: r.get(20)?,
            completed_at,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// O capítulo só some do registro se ninguém depende dele localmente: arquivo, biblioteca ou histórico de leitura.
///
/// Novidade: um capítulo é novo quando aparece pela primeira vez numa obra acompanhada que já tinha sido sincronizada.
/// A primeira sincronização é o ponto de referência e nunca marca o histórico como novo; obras não acompanhadas não
/// geram novidades. O capítulo que já existia mantém `first_seen_at`, `acknowledged_at` e o estado de leitura.
/// Devolve os ids dos capítulos que acabaram de ser marcados como novos (para quem quiser avisar a pessoa).
pub fn store_chapters(conn: &mut Connection, series_id: i64, chapters: &[super::runtime::Chapter]) -> AppResult<Vec<i64>> {
    let tx = conn.transaction()?;
    let now = now_ms();
    let flags_new: bool = tx.query_row(
        "SELECT following = 1 AND EXISTS (SELECT 1 FROM online_chapters c WHERE c.series_id = o.id) FROM online_series o WHERE o.id = ?1",
        [series_id],
        |r| r.get(0),
    )?;
    let acknowledged = if flags_new { None } else { Some(now) };
    let mut seen = HashSet::new();
    let mut fresh = Vec::new();
    let mut newest: Option<i64> = None;
    for chapter in chapters.iter().take(MAX_CHAPTERS) {
        if chapter.id.is_empty() || chapter.id.len() > MAX_REMOTE_ID_BYTES || !seen.insert(chapter.id.as_str()) {
            continue;
        }
        let Some(title) = clean(Some(&chapter.title), MAX_TITLE_CHARS) else { continue };
        newest = newest.max(chapter.published_at);
        let known: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM online_chapters WHERE series_id = ?1 AND remote_id = ?2)", params![series_id, chapter.id], |r| r.get(0))?;
        tx.execute(
            "INSERT INTO online_chapters (series_id, remote_id, title, number, published_at, sort_order, first_seen_at, acknowledged_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (series_id, remote_id) DO UPDATE SET title = excluded.title, number = excluded.number, published_at = excluded.published_at, sort_order = excluded.sort_order",
            params![series_id, chapter.id, title, clean(chapter.number.as_deref(), 40), chapter.published_at, chapter.order, now, acknowledged],
        )?;
        if flags_new && !known {
            fresh.push(tx.last_insert_rowid());
        }
    }
    let existing: Vec<(i64, String)> = {
        let mut statement = tx.prepare(
            "SELECT id, remote_id FROM online_chapters WHERE series_id = ?1 AND comic_id IS NULL AND file_path IS NULL AND reading_started_at IS NULL AND completed_at IS NULL",
        )?;
        let rows = statement.query_map([series_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (id, remote) in existing {
        if !seen.contains(remote.as_str()) {
            tx.execute("DELETE FROM online_chapters WHERE id = ?1", [id])?;
        }
    }
    tx.execute("UPDATE online_series SET last_checked_at = ?2, last_update_at = coalesce(?3, last_update_at), updated_at = ?2 WHERE id = ?1", params![series_id, now, newest])?;
    tx.commit()?;
    Ok(fresh)
}

/// O que há registrado, sem consultar a fonte. Funciona sem internet e depois de remover a extensão.
pub fn stored_series(conn: &Connection, id: i64) -> AppResult<SeriesView> {
    let chapters = chapters_of(conn, id)?;
    Ok(SeriesView { series: card(conn, id)?, reading: reading_of(conn, id, &chapters)?, chapters, source_error: None, source_protected: false })
}

/// Como `stored_series`, para a interface: a obra fora do filtro +18 não abre, por nenhum caminho.
pub fn visible_series(conn: &Connection, id: i64) -> AppResult<SeriesView> {
    if let Some(error) = filtered(conn, id)? {
        return Err(error);
    }
    stored_series(conn, id)
}

fn remove_if_unused(core: &Core, conn: &Connection, id: i64) -> AppResult<()> {
    let removed = conn.execute(
        "DELETE FROM online_series WHERE id = ?1 AND following = 0 AND favorite = 0 AND NOT EXISTS (SELECT 1 FROM series_collections x WHERE x.series_id = ?1) AND NOT EXISTS (SELECT 1 FROM online_chapters c WHERE c.series_id = ?1 AND (c.comic_id IS NOT NULL OR c.file_path IS NOT NULL OR c.reading_started_at IS NOT NULL OR c.completed_at IS NOT NULL OR EXISTS (SELECT 1 FROM chapter_progress q WHERE q.chapter_id = c.id)))",
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
    // Escondida pelo filtro: nem a fonte é consultada, e o que está guardado também não aparece.
    if let Some(error) = filtered(&core.db(), id)? {
        return Err(error);
    }
    let remote = (|| -> AppResult<_> {
        let (extension, source) = usable(core, &series.extension_id, &series.source_id)?;
        let details = if extension.bridged { suwayomi::details(core, &series.remote_id, cancel)? } else { core.engine()?.details(&extension, cancel, &series.source_id, &series.remote_id)? };
        let genres = clean_names(&details.genres);
        if minors::sexualizes_minors(&details_title(&details, &series.title), &genres) {
            return Err(AppError::user("series_blocked", "Esta obra não está disponível no Prancha."));
        }
        // Os detalhes ficam gravados antes de qualquer capítulo: é com eles que a classificação decide.
        store_details(core, &core.db(), &SeriesPlace { extension: &extension, source_id: &series.source_id, source: &source, remote_id: &series.remote_id }, &series.title, &details)?;
        if let Some(error) = filtered(&core.db(), id)? {
            return Err(error);
        }
        let chapters = if extension.bridged { suwayomi::chapters(core, &series.remote_id, cancel)? } else { core.engine()?.chapters(&extension, cancel, &series.source_id, &series.remote_id)? };
        Ok(chapters)
    })();
    match remote {
        Ok(chapters) => {
            let mut conn = core.db();
            let fresh = store_chapters(&mut conn, id, &chapters)?;
            // Só os capítulos marcados como novos nesta sincronização; a notificação deduplica pelo id remoto.
            match notifications::chapters_discovered(&conn, id, &fresh) {
                Ok(Some(notice)) => {
                    notifications::publish_with(core, &conn, &notice);
                }
                Ok(None) => {}
                Err(e) => log::warn!("aviso de capítulos novos não gravado: {e}"),
            }
            stored_series(&conn, id)
        }
        Err(error) if matches!(error.code(), "extension_cancelled" | "source_rating" | "source_unrated") => Err(error),
        Err(error) if error.code() == "series_blocked" => {
            block_series(core, &core.db(), &series.extension_id, &series.source_id, &series.remote_id)?;
            Err(error)
        }
        Err(error) => {
            let mut view = stored_series(&core.db(), id)?;
            view.source_error = Some(error.to_string());
            view.source_protected = error.code() == "source_protected";
            Ok(view)
        }
    }
}

/// O motivo de a obra não aparecer com as preferências atuais, ou `None` se ela aparece.
pub(crate) fn filtered(conn: &Connection, id: i64) -> AppResult<Option<AppError>> {
    let row: Option<(String, bool)> = conn
        .query_row(&format!("SELECT o.rating_state, {safe} OR {adult} FROM online_series o WHERE o.id = ?1", safe = library::SERIES_SAFE_SQL, adult = library::ADULT_ALLOWED_SQL), [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(match row {
        Some((_, true)) | None => None,
        Some((state, false)) => Some(match State::parse(&state) {
            Some(State::Confirmed) => AppError::user("source_rating", "Esta obra é +18. Para ver, desligue o filtro +18 em Plugins."),
            Some(State::Suspect) => AppError::user("source_rating", "Esta obra tem indícios de conteúdo +18. Para ver, escolha mostrar obras suspeitas em Plugins."),
            _ => AppError::user("source_unrated", "A fonte não informa o suficiente para classificar esta obra. Para ver, escolha mostrar obras suspeitas em Plugins."),
        }),
    })
}

/// A página que a pessoa estava tentando abrir, para a verificação começar por ela.
pub fn site_to_verify(core: &Core, chapter_id: Option<i64>, series_id: Option<i64>, source_id: Option<String>) -> AppResult<String> {
    if let Some(id) = chapter_id {
        let chapter = chapter_row(&core.db(), id)?;
        return suwayomi::site_address(core, &chapter.source_id, None, Some(&chapter.remote_id));
    }
    if let Some(id) = series_id {
        let series = card(&core.db(), id)?;
        return suwayomi::site_address(core, &series.source_id, Some(&series.remote_id), None);
    }
    let source = source_id.ok_or_else(|| AppError::user("source_not_found", "Esta fonte não está disponível."))?;
    suwayomi::site_address(core, &source, None, None)
}

// ---------------------------------------------------------------------------
// Leitura online
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSession {
    pub id: i64,
    pub series_id: i64,
    pub series_title: String,
    pub title: String,
    pub page_count: i64,
    pub start: Progress,
    pub previous_id: Option<i64>,
    pub next_id: Option<i64>,
    /// Só com o arquivo no disco: é o que a Leitura Guiada precisa.
    pub comic_id: Option<i64>,
    pub genres: Vec<String>,
    pub reader_mode: Option<String>,
}

struct ChapterRow {
    series_id: i64,
    title: String,
    remote_id: String,
    comic_id: Option<i64>,
    extension_id: String,
    source_id: String,
    series_title: String,
    genres: String,
    reader_mode: Option<String>,
}

fn chapter_row(conn: &Connection, id: i64) -> AppResult<ChapterRow> {
    conn.query_row(
        "SELECT c.series_id, c.title, c.remote_id, c.comic_id, o.extension_id, o.source_id, o.title, o.genres, o.reader_mode
         FROM online_chapters c JOIN online_series o ON o.id = c.series_id WHERE c.id = ?1",
        [id],
        |r| Ok(ChapterRow { series_id: r.get(0)?, title: r.get(1)?, remote_id: r.get(2)?, comic_id: r.get(3)?, extension_id: r.get(4)?, source_id: r.get(5)?, series_title: r.get(6)?, genres: r.get(7)?, reader_mode: r.get(8)? }),
    )
    .optional()?
    .ok_or_else(|| AppError::user("chapter_not_found", "Este capítulo não está mais registrado."))
}

/// O arquivo do capítulo, se ele está na biblioteca e continua no disco.
fn local_file(conn: &Connection, comic_id: Option<i64>) -> Option<ComicFileInfo> {
    let info = library::file_info(conn, comic_id?).ok()?;
    Path::new(&info.path).exists().then_some(info)
}

fn fetch_pages(core: &Core, id: i64, chapter: &ChapterRow, extension: &Installed, cancel: Option<&Cancel>) -> AppResult<Arc<Vec<Page>>> {
    let pages = if extension.bridged { suwayomi::pages(core, &chapter.remote_id, cancel)? } else { core.engine()?.pages(extension, cancel, &chapter.source_id, &chapter.remote_id)? };
    let pages = Arc::new(downloads::ordered_pages(pages)?);
    core.chapter_pages.put(id, pages.clone());
    Ok(pages)
}

/// Anterior e próximo na ordem de leitura, que não muda com a ordenação escolhida na tela.
fn neighbors(conn: &Connection, series_id: i64, id: i64) -> AppResult<(Option<i64>, Option<i64>)> {
    let mut statement = conn.prepare("SELECT id FROM online_chapters WHERE series_id = ?1 ORDER BY sort_order, id")?;
    let ids: Vec<i64> = statement.query_map([series_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let Some(position) = ids.iter().position(|chapter| *chapter == id) else { return Ok((None, None)) };
    Ok((position.checked_sub(1).map(|p| ids[p]), ids.get(position + 1).copied()))
}

/// Prepara a leitura: consulta as páginas na fonte, a não ser que o capítulo esteja baixado.
pub fn open_chapter(core: &Core, id: i64, cancel: Option<&Cancel>) -> AppResult<ChapterSession> {
    let chapter = chapter_row(&core.db(), id)?;
    let local = local_file(&core.db(), chapter.comic_id);
    let (page_count, start, comic_id) = match local {
        Some(info) => {
            let count = core.pages.listing(&info)?.pages.len() as i64;
            let conn = core.db();
            // Quem leu online e baixou depois continua de onde estava.
            conn.execute(
                "INSERT OR IGNORE INTO reading_progress (comic_id, page_index, page_count, scroll_ratio, updated_at) SELECT ?2, page_index, page_count, scroll_ratio, updated_at FROM chapter_progress WHERE chapter_id = ?1",
                params![id, info.id],
            )?;
            (count, library::mark_opened(&conn, info.id, count.max(1))?, Some(info.id))
        }
        None => {
            if let Some(error) = filtered(&core.db(), chapter.series_id)? {
                return Err(error);
            }
            let (extension, _) = usable(core, &chapter.extension_id, &chapter.source_id)?;
            let count = fetch_pages(core, id, &chapter, &extension, cancel)?.len() as i64;
            let stored: Option<(i64, f64, i64)> = core.db().query_row("SELECT page_index, scroll_ratio, updated_at FROM chapter_progress WHERE chapter_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
            // Como na biblioteca, um capítulo concluído recomeça ao reler. Um não concluído volta exatamente onde parou.
            let completed: bool = core.db().query_row("SELECT completed_at IS NOT NULL FROM online_chapters WHERE id = ?1", [id], |r| r.get(0))?;
            let start = match stored {
                Some((page, ratio, updated_at)) if page < count && !(completed && page >= count - 1) => Progress { page_index: page, page_count: count, scroll_ratio: ratio, updated_at },
                _ => Progress { page_index: 0, page_count: count, scroll_ratio: 0.0, updated_at: now_ms() },
            };
            (count, start, None)
        }
    };
    let (previous_id, next_id) = neighbors(&core.db(), chapter.series_id, id)?;
    Ok(ChapterSession {
        id,
        series_id: chapter.series_id,
        series_title: chapter.series_title,
        title: chapter.title,
        page_count,
        start,
        previous_id,
        next_id,
        comic_id,
        genres: serde_json::from_str(&chapter.genres).unwrap_or_default(),
        reader_mode: chapter.reader_mode,
    })
}

/// Uma página, do arquivo se o capítulo estiver baixado e da fonte se não estiver. Nada fica gravado em disco.
pub fn chapter_page(core: &Core, id: i64, index: usize) -> AppResult<(Vec<u8>, &'static str)> {
    let chapter = chapter_row(&core.db(), id)?;
    let local = local_file(&core.db(), chapter.comic_id);
    if let Some(info) = local {
        return Ok(core.pages.read_page(&info, index)?);
    }
    let (extension, source) = usable(core, &chapter.extension_id, &chapter.source_id)?;
    let pages = match core.chapter_pages.get(id) {
        Some(pages) => pages,
        None => fetch_pages(core, id, &chapter, &extension, None)?,
    };
    let page = pages.get(index).ok_or_else(|| AppError::user("page_not_found", "Esta página não existe."))?;
    let _permit = core.page_gate.enter();
    downloads::wait_turn(core, &format!("{}/{}", chapter.extension_id, chapter.source_id), source.min_interval_ms, &Cancel::default())?;
    let (bytes, kind) = downloads::fetch_page(core, &extension, page, index, None).inspect_err(|_| {
        // Endereços de página podem expirar: a próxima tentativa pede a lista de novo.
        core.chapter_pages.forget(id);
    })?;
    let mime = match kind {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "image/jpeg",
    };
    Ok((bytes, mime))
}

pub fn save_chapter_progress(conn: &Connection, id: i64, page_index: i64, page_count: i64, scroll_ratio: f64) -> AppResult<()> {
    if page_count <= 0 {
        return Err(AppError::user("invalid", "Número de páginas inválido."));
    }
    // Gravar a posição também mantém viva a sessão de leitura, se houver uma. Não inicia nem conclui nada.
    if let Some(comic) = chapter_row(conn, id)?.comic_id {
        library::save_progress(conn, comic, page_index, page_count, scroll_ratio)?;
        reading::touch(conn, id)?;
        return Ok(());
    }
    let scroll_ratio = if scroll_ratio.is_finite() { scroll_ratio.clamp(0.0, 1.0) } else { 0.0 };
    conn.execute(
        "INSERT INTO chapter_progress (chapter_id, page_index, page_count, scroll_ratio, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (chapter_id) DO UPDATE SET page_index = ?2, page_count = ?3, scroll_ratio = ?4, updated_at = ?5",
        params![id, page_index.clamp(0, page_count - 1), page_count, scroll_ratio, now_ms()],
    )?;
    reading::touch(conn, id)?;
    Ok(())
}

pub fn reset_chapter_progress(conn: &Connection, id: i64) -> AppResult<()> {
    match chapter_row(conn, id)?.comic_id {
        Some(comic) => library::reset_progress(conn, comic),
        None => {
            conn.execute("DELETE FROM chapter_progress WHERE chapter_id = ?1", [id])?;
            Ok(())
        }
    }
}

pub fn set_reader_mode(conn: &Connection, series_id: i64, mode: &str) -> AppResult<()> {
    if !matches!(mode, "single" | "double" | "vertical" | "webtoon") {
        return Err(AppError::user("bad_request", "Modo de leitura inválido."));
    }
    conn.execute("UPDATE online_series SET reader_mode = ?2 WHERE id = ?1", params![series_id, mode])?;
    Ok(())
}

/// O plugin foi removido: a fila para e o Java sai da memória. Nada é apagado, e instalar de novo traz tudo de volta.
pub fn stop_online(core: &Core) -> AppResult<()> {
    let open: Vec<i64> = {
        let conn = core.db();
        let mut statement = conn.prepare("SELECT id FROM chapter_downloads WHERE status IN ('queued', 'running')")?;
        let ids = statement.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        ids
    };
    for id in open {
        downloads::pause(core, id)?;
    }
    suwayomi::stop(core);
    Ok(())
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
    let mut statement = conn.prepare(&format!("{CARD_SELECT} WHERE o.following = 1 AND ({safe} OR {adult}) ORDER BY o.title COLLATE NATSORT", safe = library::SERIES_SAFE_SQL, adult = library::ADULT_ALLOWED_SQL))?;
    let rows = statement.query_map([], card_from_row)?.collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Só as obras acompanhadas participam da busca por capítulos novos. Uma fonte que falha não impede as outras,
/// e o que já estava registrado (estados, novidades) não muda por causa dela.
pub fn refresh_following(core: &Core, cancel: Option<&Cancel>) -> AppResult<Vec<SeriesCard>> {
    let ids: Vec<i64> = list_following(&core.db())?.into_iter().map(|series| series.id).collect();
    let mut failed = Vec::new();
    for id in ids {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            break;
        }
        match open_series(core, id, cancel) {
            Err(error) if error.code() == "extension_cancelled" => break,
            // A fonte não respondeu: o registro continua, e a central avisa uma vez por obra e por dia.
            Ok(view) if view.source_error.is_some() => failed.push((id, view.series.title)),
            _ => {}
        }
    }
    if !failed.is_empty() {
        let day = crate::db::now_ms() / (24 * 60 * 60 * 1000);
        let mut notice = notifications::NewNotification::new(notifications::Kind::SyncFailed).grouped(notifications::SYNC_WINDOW_MS);
        for (id, title) in failed {
            notice = notice.event(notifications::Event::new(format!("sync:failed:{id}:{day}")).item(title));
        }
        notifications::publish(core, &notice);
    }
    list_following(&core.db())
}

// ---------------------------------------------------------------------------
// Obras na biblioteca
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySeries {
    pub id: i64,
    pub title: String,
    pub has_cover: bool,
    pub source_name: Option<String>,
    pub favorite: bool,
    pub added_at: i64,
    pub last_opened_at: Option<i64>,
    pub following: bool,
    pub new_count: i64,
    pub rating_state: String,
    #[serde(flatten)]
    pub reading: SeriesReading,
}

/// As obras que a pessoa guardou. O resto do que a fonte mostrou fica só em Explorar.
pub fn library_series(conn: &Connection) -> AppResult<Vec<LibrarySeries>> {
    let mut statement = conn.prepare(&format!(
        "SELECT o.id, o.title, o.cover_url IS NOT NULL, s.name, o.favorite, o.created_at,
                (SELECT max(max(coalesce(p.updated_at, q.updated_at, 0), coalesce(c.last_read_at, 0))) FROM online_chapters c
                 LEFT JOIN reading_progress p ON p.comic_id = c.comic_id
                 LEFT JOIN chapter_progress q ON q.chapter_id = c.id WHERE c.series_id = o.id),
                o.following,
                o.rating_state
         FROM online_series o LEFT JOIN extension_sources s ON s.extension_id = o.extension_id AND s.source_id = o.source_id
         WHERE ({safe} OR {adult})
           AND (o.favorite = 1 OR o.following = 1
                OR EXISTS (SELECT 1 FROM series_collections x WHERE x.series_id = o.id)
                OR EXISTS (SELECT 1 FROM online_chapters c JOIN chapter_progress q ON q.chapter_id = c.id WHERE c.series_id = o.id)
                OR EXISTS (SELECT 1 FROM online_chapters c WHERE c.series_id = o.id AND (c.reading_started_at IS NOT NULL OR c.completed_at IS NOT NULL)))",
        safe = library::SERIES_SAFE_SQL,
        adult = library::ADULT_ALLOWED_SQL,
    ))?;
    type Listed = (i64, String, bool, Option<String>, bool, i64, Option<i64>, bool, String);
    let rows: Vec<Listed> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get::<_, Option<i64>>(6)?.filter(|at| *at > 0), r.get(7)?, r.get(8)?)))?
        .collect::<Result<_, _>>()?;
    let new_counts = reading::new_counts(conn)?;
    rows.into_iter()
        .map(|(id, title, has_cover, source_name, favorite, added_at, last_opened_at, following, rating_state)| {
            let reading = reading_of(conn, id, &chapters_of(conn, id)?)?;
            let new_count = new_counts.get(&id).copied().unwrap_or(0);
            Ok(LibrarySeries { id, title, has_cover, source_name, favorite, added_at, last_opened_at, following, new_count, rating_state, reading })
        })
        .collect()
}

pub fn set_favorite(conn: &Connection, id: i64, favorite: bool) -> AppResult<()> {
    card(conn, id)?;
    conn.execute("UPDATE online_series SET favorite = ?2 WHERE id = ?1", params![id, favorite])?;
    Ok(())
}

/// Define exatamente as coleções de uma obra.
pub fn set_series_collections(conn: &mut Connection, id: i64, collection_ids: &[i64]) -> AppResult<()> {
    card(conn, id)?;
    let tx = conn.transaction()?;
    let keep = collection_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    tx.execute(&format!("DELETE FROM series_collections WHERE series_id = ?1 AND collection_id NOT IN ({keep})"), [id])?;
    for collection in collection_ids {
        tx.execute("INSERT OR IGNORE INTO series_collections (series_id, collection_id, added_at) VALUES (?1, ?2, ?3)", params![id, collection, now_ms()])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn add_series_to_collection(conn: &mut Connection, collection_id: i64, ids: &[i64]) -> AppResult<()> {
    let tx = conn.transaction()?;
    let now = now_ms();
    for id in ids {
        tx.execute("INSERT OR IGNORE INTO series_collections (series_id, collection_id, added_at) VALUES (?1, ?2, ?3)", params![id, collection_id, now])?;
    }
    tx.execute("UPDATE collections SET updated_at = ?1 WHERE id = ?2", params![now, collection_id])?;
    tx.commit()?;
    Ok(())
}

pub fn remove_series_from_collection(conn: &Connection, collection_id: i64, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM series_collections WHERE collection_id = ?1 AND series_id = ?2", params![collection_id, id])?;
    Ok(())
}

/// Marca a obra inteira como lida ou não lida, incluindo os capítulos baixados. A posição salva de cada capítulo fica.
pub fn set_series_read(conn: &mut Connection, id: i64, read: bool) -> AppResult<()> {
    card(conn, id)?;
    let ids: Vec<i64> = {
        let mut statement = conn.prepare("SELECT id FROM online_chapters WHERE series_id = ?1")?;
        let rows = statement.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        rows
    };
    reading::set_read(conn, &ids, read)?;
    Ok(())
}

/// Tira a obra da biblioteca. Capítulos baixados continuam lá, como as HQs que são.
pub fn forget_series(core: &Core, id: i64) -> AppResult<()> {
    let conn = core.db();
    conn.execute("UPDATE online_series SET favorite = 0, following = 0 WHERE id = ?1", [id])?;
    conn.execute("DELETE FROM series_collections WHERE series_id = ?1", [id])?;
    conn.execute("DELETE FROM chapter_progress WHERE chapter_id IN (SELECT id FROM online_chapters WHERE series_id = ?1)", [id])?;
    conn.execute(
        "UPDATE online_chapters SET reading_started_at = NULL, last_read_at = NULL, completed_at = NULL, manual_unread = 0 WHERE series_id = ?1",
        [id],
    )?;
    conn.execute("DELETE FROM reading_sessions WHERE chapter_id IN (SELECT id FROM online_chapters WHERE series_id = ?1)", [id])?;
    remove_if_unused(core, &conn, id)
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
            "SELECT o.id FROM online_series o WHERE o.following = 0 AND o.favorite = 0 AND o.updated_at < ?1
               AND NOT EXISTS (SELECT 1 FROM series_collections x WHERE x.series_id = o.id)
               AND NOT EXISTS (SELECT 1 FROM online_chapters c WHERE c.series_id = o.id
                    AND (c.comic_id IS NOT NULL OR c.file_path IS NOT NULL OR EXISTS (SELECT 1 FROM chapter_downloads d WHERE d.chapter_id = c.id) OR c.reading_started_at IS NOT NULL OR c.completed_at IS NOT NULL OR EXISTS (SELECT 1 FROM chapter_progress q WHERE q.chapter_id = c.id)))",
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
/// A obra fora do filtro +18 fica sem capa, mesmo com uma já em cache, e nada é pedido ao servidor.
pub fn cover(core: &Core, id: i64) -> AppResult<Option<Vec<u8>>> {
    let row: Option<(Option<String>, String, bool)> = core
        .db()
        .query_row(&format!("SELECT o.cover_url, o.extension_id, {safe} OR {adult} FROM online_series o WHERE o.id = ?1", safe = library::SERIES_SAFE_SQL, adult = library::ADULT_ALLOWED_SQL), [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let Some((url, extension_id, true)) = row else { return Ok(None) };
    let cached = cover_cache_file(core, id);
    if let Ok(bytes) = std::fs::read(&cached) {
        return Ok(Some(bytes));
    }
    let Some(url) = url else { return Ok(None) };
    // Extensão removida ou desabilitada: fica só o que já estava em cache.
    let Ok(extension) = install::runnable(core, &extension_id) else { return Ok(None) };
    if !extension.grant.network {
        return Ok(None);
    }
    let _permit = core.cover_gate.enter();
    let limits = Limits { max_bytes: MAX_COVER_BYTES, timeout: COVER_TIMEOUT };
    let fetched = if extension.bridged { suwayomi::image(core, &url, limits, None) } else { core.net.get(&url, Access::Hosts(&extension.grant.allowed_hosts), limits, Request { check_content_type: true, ..Default::default() }) };
    let Ok(response) = fetched else {
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
