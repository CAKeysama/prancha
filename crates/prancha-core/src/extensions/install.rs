//! Ciclo de vida de uma extensão. Nada substitui o que funciona antes de todas as conferências passarem.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

use super::net::{Access, Limits, NetError, Request};
use super::repository::{self, Capabilities, ExtensionEntry, MAX_ARTIFACT_BYTES};
use super::runtime::{ContentKind, ContentRating, Grant, Installed, ReadingDirection, SourceInfo};
use super::{signing, suwayomi};
use crate::core::Core;
use crate::db::now_ms;
use crate::error::{AppError, AppResult};

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_SIGNATURE_BYTES: u64 = 1024;
const MAX_SOURCE_KINDS: usize = 8;

fn refused(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::user(code, message)
}

fn network_error(error: NetError) -> AppError {
    match error {
        NetError::Blocked(why) => refused("extension_blocked", format!("Endereço não permitido: {why}.")),
        NetError::TooLarge => refused("extension_size", "O pacote é maior do que o índice declara."),
        NetError::Cancelled => refused("cancelled", "Operação cancelada."),
        NetError::Timeout => refused("extension_network", "O download demorou demais."),
        NetError::Network(why) => {
            log::warn!("extensão: {why}");
            refused("extension_network", "Não foi possível baixar a extensão.")
        }
    }
}

// ---------------------------------------------------------------------------
// Consulta
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSource {
    pub id: String,
    pub name: String,
    pub language: String,
    pub description: Option<String>,
    pub kinds: Vec<String>,
    pub rating: String,
    pub direction: String,
    pub supports_latest: bool,
    pub supports_search: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub icon: Option<String>,
    pub repository_id: String,
    pub repository_name: String,
    pub fingerprint: String,
    pub runtime: String,
    pub capabilities: Capabilities,
    pub enabled: bool,
    pub load_error: Option<String>,
    pub installed_at: i64,
    pub updated_at: i64,
    pub sources: Vec<InstalledSource>,
    pub available_version: Option<String>,
}

fn sources_of(conn: &Connection, extension_id: &str) -> AppResult<Vec<InstalledSource>> {
    let mut statement = conn.prepare(
        "SELECT source_id, name, language, description, kinds, rating, direction, supports_latest, supports_search FROM extension_sources WHERE extension_id = ?1 ORDER BY name COLLATE NATSORT",
    )?;
    let rows = statement.query_map([extension_id], |r| {
        Ok(InstalledSource {
            id: r.get(0)?,
            name: r.get(1)?,
            language: r.get(2)?,
            description: r.get(3)?,
            kinds: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
            rating: r.get(5)?,
            direction: r.get(6)?,
            supports_latest: r.get::<_, i64>(7)? != 0,
            supports_search: r.get::<_, i64>(8)? != 0,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn extension_from_row(conn: &Connection, row: &Row) -> AppResult<InstalledExtension> {
    let id: String = row.get(0)?;
    let json: String = row.get(6)?;
    let entry: ExtensionEntry = serde_json::from_str(&json).map_err(|e| refused("extension_data", e.to_string()))?;
    let sources = sources_of(conn, &id)?;
    let repository_id: String = row.get(1)?;
    let version: String = row.get(5)?;
    let available_version = conn
        .query_row("SELECT e.version FROM repository_extensions e JOIN source_repositories s ON s.id = e.repository_id AND s.enabled = 1 WHERE e.repository_id = ?1 AND e.extension_id = ?2", [&repository_id, &id], |r| r.get::<_, String>(0))
        .optional()?
        .filter(|available| is_newer(available, &version));
    Ok(InstalledExtension {
        id,
        name: row.get(4)?,
        version,
        description: entry.description,
        authors: entry.authors,
        icon: entry.icon,
        repository_id,
        repository_name: row.get(2)?,
        fingerprint: row.get(3)?,
        runtime: entry.runtime,
        capabilities: entry.capabilities,
        enabled: row.get::<_, i64>(7)? != 0,
        load_error: row.get(8)?,
        installed_at: row.get(9)?,
        updated_at: row.get(10)?,
        sources,
        available_version,
    })
}

const COLUMNS: &str = "id, repository_id, repository_name, key_fingerprint, name, version, entry, enabled, load_error, installed_at, updated_at";

pub fn list(conn: &Connection) -> AppResult<Vec<InstalledExtension>> {
    let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM installed_extensions ORDER BY name COLLATE NATSORT"))?;
    let mut rows = statement.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(extension_from_row(conn, row)?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<InstalledExtension> {
    let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM installed_extensions WHERE id = ?1"))?;
    let mut rows = statement.query([id])?;
    match rows.next()? {
        Some(row) => extension_from_row(conn, row),
        None => Err(refused("extension_not_found", "Essa extensão não está mais instalada.")),
    }
}

fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((semver::Version::parse(candidate), semver::Version::parse(current)), (Ok(a), Ok(b)) if a > b)
}

fn extensions_dir(core: &Core) -> PathBuf {
    core.paths.data_dir.join("extensions")
}

/// Como executar uma extensão habilitada. As capacidades vêm do índice assinado, nunca do componente.
pub fn runnable(core: &Core, id: &str) -> AppResult<Installed> {
    let row = core
        .db()
        .query_row("SELECT file_name, sha256, entry, enabled FROM installed_extensions WHERE id = ?1", [id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)? != 0))
        })
        .optional()?;
    let Some((file_name, sha256, json, enabled)) = row else {
        return Err(refused("extension_not_found", "A extensão desta fonte não está instalada."));
    };
    if !enabled {
        return Err(refused("extension_disabled", "A extensão desta fonte está desabilitada."));
    }
    let entry: ExtensionEntry = serde_json::from_str(&json).map_err(|e| refused("extension_data", e.to_string()))?;
    let capabilities = entry.capabilities;
    Ok(Installed {
        id: id.to_string(),
        file: extensions_dir(core).join(file_name),
        sha256,
        grant: Grant { network: capabilities.network, storage: capabilities.storage, allowed_hosts: capabilities.allowed_hosts },
        bridged: entry.runtime == suwayomi::RUNTIME,
    })
}

// ---------------------------------------------------------------------------
// Conferências
// ---------------------------------------------------------------------------

struct CatalogRow {
    entry: ExtensionEntry,
    repository_name: String,
    repository_url: String,
    public_key: String,
    fingerprint: String,
    status: String,
}

fn catalog_row(conn: &Connection, repository_id: &str, extension_id: &str) -> AppResult<CatalogRow> {
    let row = conn
        .query_row(
            "SELECT e.entry, s.name, s.url, s.public_key, s.key_fingerprint, s.status FROM repository_extensions e JOIN source_repositories s ON s.id = e.repository_id WHERE e.repository_id = ?1 AND e.extension_id = ?2",
            [repository_id, extension_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?)),
        )
        .optional()?;
    let Some((json, repository_name, repository_url, public_key, fingerprint, status)) = row else {
        return Err(refused("extension_not_found", "Essa extensão não está mais no catálogo do repositório."));
    };
    let entry = serde_json::from_str(&json).map_err(|e| refused("extension_data", e.to_string()))?;
    Ok(CatalogRow { entry, repository_name, repository_url, public_key, fingerprint, status })
}

fn name_of_rating(rating: ContentRating) -> &'static str {
    match rating {
        ContentRating::General => "general",
        ContentRating::Teen => "teen",
        ContentRating::Adult => "adult",
        ContentRating::Unknown => "unknown",
    }
}

fn name_of_kind(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Manga => "manga",
        ContentKind::Manhwa => "manhwa",
        ContentKind::Manhua => "manhua",
        ContentKind::Comic => "comic",
        ContentKind::Other => "other",
    }
}

fn name_of_direction(direction: ReadingDirection) -> &'static str {
    match direction {
        ReadingDirection::LeftToRight => "ltr",
        ReadingDirection::RightToLeft => "rtl",
        ReadingDirection::Vertical => "vertical",
    }
}

/// O que o componente informa vai para o banco e para a tela: valem os mesmos limites do índice.
fn source_is_presentable(source: &SourceInfo) -> bool {
    repository::short_text(&source.name, 120)
        && repository::short_text(&source.language, 20)
        && source.description.as_deref().is_none_or(|text| text.is_empty() || repository::short_text(text, repository::MAX_TEXT_CHARS))
        && source.kinds.len() <= MAX_SOURCE_KINDS
}

/// O que o componente diz ter precisa ser o que o índice assinado promete.
fn check_declared_sources(entry: &ExtensionEntry, found: &[SourceInfo]) -> AppResult<()> {
    let mismatch = || refused("extension_mismatch", "O pacote não corresponde ao que o repositório declara.");
    let mut seen = HashSet::new();
    for source in found {
        if !seen.insert(source.id.as_str()) {
            return Err(mismatch());
        }
        let declared = entry.sources.iter().find(|d| d.id == source.id).ok_or_else(mismatch)?;
        if declared.content_rating != name_of_rating(source.rating) || !repository::is_valid_id(&source.id) || !source_is_presentable(source) {
            return Err(mismatch());
        }
    }
    if found.len() != entry.sources.len() {
        return Err(mismatch());
    }
    Ok(())
}

/// O que vale para qualquer extensão, antes de baixar qualquer coisa.
fn check_allowed(row: &CatalogRow) -> AppResult<()> {
    let entry = &row.entry;
    if row.status == "key_changed" {
        return Err(refused("extension_provenance", "A chave deste repositório mudou. Revise a chave nova antes de instalar."));
    }
    if let Some(reason) = repository::incompatibility(entry, env!("CARGO_PKG_VERSION")) {
        return Err(refused("extension_incompatible", reason));
    }
    if entry.capabilities.authentication {
        return Err(refused("extension_incompatible", "Esta extensão exige login, e o Prancha não oferece suporte a isso."));
    }
    Ok(())
}

/// Baixa e confere tudo. Só depois disto algo é gravado em disco.
fn fetch_verified(core: &Core, row: &CatalogRow, repository_id: &str) -> AppResult<(Vec<u8>, Vec<SourceInfo>)> {
    let entry = &row.entry;
    let key = signing::parse_public_key(&row.public_key).ok_or_else(|| refused("extension_provenance", "A chave registrada do repositório não é válida."))?;
    let get = |url: &str, max_bytes: u64| {
        let response = core.net.get(url, Access::Public, Limits { max_bytes, timeout: DOWNLOAD_TIMEOUT }, Request::default()).map_err(network_error)?;
        if response.is_success() { Ok(response.body) } else { Err(refused("extension_network", format!("O repositório respondeu {} ao baixar a extensão.", response.status))) }
    };
    let wasm = get(&entry.artifact_url, entry.size.min(MAX_ARTIFACT_BYTES))?;
    if wasm.len() as u64 != entry.size {
        return Err(refused("extension_size", "O pacote não tem o tamanho que o índice declara."));
    }
    if signing::sha256_hex(&wasm) != entry.sha256 {
        return Err(refused("extension_hash", "O SHA-256 do pacote não confere."));
    }
    let signature = get(&entry.signature_url, MAX_SIGNATURE_BYTES)?;
    if !signing::verify_package(&key, repository_id, &entry.id, &entry.version, &entry.sha256, &signature) {
        return Err(refused("extension_signature", "A assinatura do pacote não confere."));
    }
    let sources = core.engine()?.list_sources(&wasm)?;
    check_declared_sources(entry, &sources)?;
    Ok((wasm, sources))
}

/// Grava com outro nome e renomeia. Pacotes são nomeados pelo conteúdo, então um nunca sobrescreve outro.
fn store_package(core: &Core, wasm: &[u8], sha256: &str) -> AppResult<String> {
    let dir = extensions_dir(core);
    std::fs::create_dir_all(&dir)?;
    let name = format!("{sha256}.wasm");
    let temporary = dir.join(format!("{sha256}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, wasm)?;
    if let Err(error) = std::fs::rename(&temporary, dir.join(&name)) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(name)
}

struct Package {
    file_name: String,
    sha256: String,
    sources: Vec<SourceInfo>,
}

/// O pacote pronto para registrar. Uma extensão do Suwayomi fica com ele: aqui ela não tem arquivo.
fn fetch_package(core: &Core, row: &CatalogRow, repository_id: &str, updating: bool) -> AppResult<Package> {
    check_allowed(row)?;
    if row.entry.runtime == suwayomi::RUNTIME {
        let mut sources = suwayomi::install_extension(core, &row.repository_url, &row.fingerprint, &row.entry.id, updating)?;
        sources.retain(|source| repository::is_valid_id(&source.id) && source_is_presentable(source));
        return Ok(Package { file_name: String::new(), sha256: String::new(), sources });
    }
    let (wasm, sources) = fetch_verified(core, row, repository_id)?;
    let file_name = store_package(core, &wasm, &row.entry.sha256)?;
    Ok(Package { file_name, sha256: row.entry.sha256.clone(), sources })
}

fn write_sources(conn: &Connection, extension_id: &str, sources: &[SourceInfo]) -> AppResult<()> {
    conn.execute("DELETE FROM extension_sources WHERE extension_id = ?1", [extension_id])?;
    let mut insert = conn.prepare(
        "INSERT INTO extension_sources (extension_id, source_id, name, language, description, kinds, rating, direction, supports_latest, supports_search, min_interval_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    for source in sources {
        let kinds = serde_json::to_string(&source.kinds.iter().map(|k| name_of_kind(*k)).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".into());
        insert.execute(params![
            extension_id,
            source.id,
            source.name,
            source.language,
            source.description,
            kinds,
            name_of_rating(source.rating),
            name_of_direction(source.direction),
            source.supports_latest,
            source.supports_search,
            source.min_interval_ms,
        ])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Ações
// ---------------------------------------------------------------------------

pub fn install(core: &Core, repository_id: &str, extension_id: &str) -> AppResult<InstalledExtension> {
    let row = catalog_row(&core.db(), repository_id, extension_id)?;
    let existing: Option<String> = core.db().query_row("SELECT repository_id FROM installed_extensions WHERE id = ?1", [extension_id], |r| r.get(0)).optional()?;
    if let Some(origin) = existing {
        return Err(if origin == repository_id {
            refused("extension_installed", "Esta extensão já está instalada.")
        } else {
            refused("extension_provenance", "Esta extensão já está instalada a partir de outro repositório. Remova-a antes de instalar deste.")
        });
    }
    let package = fetch_package(core, &row, repository_id, false)?;
    let outcome = (|| -> AppResult<()> {
        let mut conn = core.db();
        let tx = conn.transaction()?;
        let now = now_ms();
        let json = serde_json::to_string(&row.entry).map_err(|e| refused("extension_data", e.to_string()))?;
        tx.execute(
            "INSERT INTO installed_extensions (id, repository_id, repository_name, public_key, key_fingerprint, name, version, entry, file_name, sha256, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![row.entry.id, repository_id, row.repository_name, row.public_key, row.fingerprint, row.entry.name, row.entry.version, json, package.file_name, package.sha256, now],
        )?;
        write_sources(&tx, &row.entry.id, &package.sources)?;
        tx.commit()?;
        Ok(())
    })();
    if let Err(error) = outcome {
        remove_package_if_unused(core, &package.file_name);
        return Err(error);
    }
    get(&core.db(), extension_id)
}

/// Só a partir do repositório e da chave registrados na instalação. Se algo falhar, a versão atual continua valendo.
pub fn update(core: &Core, extension_id: &str) -> AppResult<InstalledExtension> {
    let current = core
        .db()
        .query_row("SELECT repository_id, public_key, version, file_name, sha256 FROM installed_extensions WHERE id = ?1", [extension_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
        })
        .optional()?;
    let Some((repository_id, public_key, version, old_file, old_sha256)) = current else {
        return Err(refused("extension_not_found", "Essa extensão não está mais instalada."));
    };
    let row = catalog_row(&core.db(), &repository_id, extension_id)?;
    if row.public_key != public_key {
        return Err(refused("extension_provenance", "O repositório trocou de chave desde a instalação. Remova a extensão e instale-a de novo para aceitar a nova procedência."));
    }
    if !is_newer(&row.entry.version, &version) {
        return Err(refused("extension_current", "Esta já é a versão mais recente."));
    }
    let package = fetch_package(core, &row, &repository_id, true)?;
    let file_name = package.file_name;
    let outcome = (|| -> AppResult<()> {
        let mut conn = core.db();
        let tx = conn.transaction()?;
        let json = serde_json::to_string(&row.entry).map_err(|e| refused("extension_data", e.to_string()))?;
        tx.execute(
            "UPDATE installed_extensions SET name = ?2, version = ?3, entry = ?4, file_name = ?5, sha256 = ?6, load_error = NULL, updated_at = ?7 WHERE id = ?1",
            params![extension_id, row.entry.name, row.entry.version, json, file_name, package.sha256, now_ms()],
        )?;
        write_sources(&tx, extension_id, &package.sources)?;
        tx.commit()?;
        Ok(())
    })();
    if let Err(error) = outcome {
        if file_name != old_file {
            remove_package_if_unused(core, &file_name);
        }
        return Err(error);
    }
    if file_name != old_file {
        remove_package_if_unused(core, &old_file);
        if let Ok(engine) = core.engine() {
            engine.forget(&old_sha256);
        }
    }
    get(&core.db(), extension_id)
}

fn remove_package_if_unused(core: &Core, file_name: &str) {
    if file_name.is_empty() {
        return;
    }
    let in_use = core.db().query_row("SELECT 1 FROM installed_extensions WHERE file_name = ?1", [file_name], |_| Ok(())).optional().ok().flatten().is_some();
    if !in_use {
        if let Err(error) = std::fs::remove_file(extensions_dir(core).join(file_name)) {
            log::warn!("extensão: o pacote antigo ficou em disco: {error}");
        }
    }
}

/// Obras, capítulos e arquivos baixados continuam na biblioteca. Saem as fontes e os dados privados da extensão.
pub fn remove(core: &Core, id: &str) -> AppResult<()> {
    let found: Option<(String, String)> = core.db().query_row("SELECT file_name, sha256 FROM installed_extensions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((file_name, sha256)) = found else { return Ok(()) };
    if file_name.is_empty() {
        suwayomi::uninstall_extension(core, id);
    }
    core.db().execute("DELETE FROM installed_extensions WHERE id = ?1", [id])?;
    remove_package_if_unused(core, &file_name);
    if let Ok(engine) = core.engine() {
        engine.forget(&sha256);
    }
    Ok(())
}

pub fn set_enabled(conn: &Connection, id: &str, enabled: bool) -> AppResult<InstalledExtension> {
    conn.execute("UPDATE installed_extensions SET enabled = ?2 WHERE id = ?1", params![id, enabled])?;
    get(conn, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_with_endless_text_is_not_presentable() {
        let source = |name: &str, language: &str, description: Option<&str>, kinds: usize| SourceInfo {
            id: "fonte".into(),
            name: name.into(),
            language: language.into(),
            description: description.map(str::to_string),
            kinds: vec![ContentKind::Comic; kinds],
            rating: ContentRating::General,
            direction: ReadingDirection::LeftToRight,
            supports_latest: false,
            supports_search: false,
            min_interval_ms: 0,
        };
        assert!(source_is_presentable(&source("Fonte", "pt-BR", None, 1)));
        assert!(source_is_presentable(&source("Fonte", "pt-BR", Some(""), MAX_SOURCE_KINDS)));
        assert!(!source_is_presentable(&source(&"n".repeat(121), "pt-BR", None, 1)));
        assert!(!source_is_presentable(&source("", "pt-BR", None, 1)));
        assert!(!source_is_presentable(&source("Fonte", &"l".repeat(21), None, 1)));
        assert!(!source_is_presentable(&source("Fonte", "pt-BR", Some(&"d".repeat(2001)), 1)));
        assert!(!source_is_presentable(&source("Fonte", "pt-BR", None, MAX_SOURCE_KINDS + 1)));
    }
}
