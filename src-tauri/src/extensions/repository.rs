//! Repositórios de extensões: manifesto, índice assinado, confiança e catálogo local.
//! Formato documentado em `docs/extensions/REPOSITORY_FORMAT.md`.

use std::collections::HashSet;
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use reqwest::Url;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::net::{Access, Limits, Net, NetError, Request, Response};
use super::signing;
use crate::core::Core;
use crate::db::now_ms;
use crate::error::{AppError, AppResult};

pub const MANIFEST_FILE: &str = "prancha-repo.json";
pub const RUNTIME: &str = "wasm-component";
pub const API_VERSION: &str = "1";
pub const MAX_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;
pub const MIHON_MESSAGE: &str = "Este repositório distribui extensões para Mihon e não possui pacotes compatíveis com o Prancha.";

const SCHEMA_VERSION: u64 = 1;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 1024;
const MAX_ICON_CHARS: usize = 44 * 1024; // 32 KB em base64
pub const MAX_TEXT_CHARS: usize = 2000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Repositórios sincronizados ao mesmo tempo.
const SYNC_CONCURRENCY: usize = 4;

// ---------------------------------------------------------------------------
// Formatos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    website: Option<String>,
    index_url: String,
    public_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    schema_version: u64,
    repository_id: String,
    generated_at: String,
    extensions: Vec<ExtensionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    pub languages: Vec<String>,
    pub runtime: String,
    pub api_version: String,
    pub min_app_version: String,
    #[serde(default)]
    pub max_app_version: Option<String>,
    pub artifact_url: String,
    pub size: u64,
    pub sha256: String,
    pub signature_url: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub supported_kinds: Vec<String>,
    pub content_rating: String,
    pub sources: Vec<EntrySource>,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntrySource {
    pub id: String,
    pub name: String,
    pub language: String,
    pub content_rating: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub network: bool,
    pub storage: bool,
    pub authentication: bool,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

// ---------------------------------------------------------------------------
// Validação
// ---------------------------------------------------------------------------

fn invalid(message: impl Into<String>) -> AppError {
    AppError::user("repo_invalid", message)
}

fn mihon() -> AppError {
    AppError::user("repo_mihon", MIHON_MESSAGE)
}

fn network_error(error: NetError) -> AppError {
    match error {
        NetError::Blocked(why) => AppError::user("repo_blocked", format!("Endereço não permitido: {why}.")),
        NetError::TooLarge => invalid("O arquivo é maior que o limite permitido."),
        NetError::Cancelled => AppError::user("cancelled", "Operação cancelada."),
        NetError::Timeout => AppError::user("repo_network", "O servidor demorou demais para responder."),
        NetError::Network(why) => {
            log::warn!("repositório: {why}");
            AppError::user("repo_network", "Não foi possível conectar ao repositório.")
        }
    }
}

/// Identificadores estáveis: letras minúsculas, dígitos, `.`, `-` e `_`, sem começar por pontuação.
pub fn is_valid_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

pub fn short_text(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= limit && !text.chars().any(char::is_control)
}

/// `exemplo.com` ou `*.exemplo.com`, sem esquema, porta ou caminho.
fn is_valid_host_pattern(pattern: &str, net: &Net) -> bool {
    if net.allows_loopback() && pattern == "127.0.0.1" {
        return true;
    }
    let host = pattern.strip_prefix("*.").unwrap_or(pattern);
    host.contains('.')
        && host.len() <= 253
        && host.split('.').all(|label| !label.is_empty() && label.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        && !host.rsplit('.').next().unwrap_or_default().chars().all(|c| c.is_ascii_digit())
}

fn is_valid_icon(icon: &str) -> bool {
    icon.len() <= MAX_ICON_CHARS && ["data:image/png;base64,", "data:image/jpeg;base64,", "data:image/webp;base64,"].iter().any(|p| icon.starts_with(p))
}

fn check_public_url(net: &Net, url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| net.check(&u, Access::Public).is_ok())
}

fn validate_entry(net: &Net, entry: &ExtensionEntry) -> Result<(), String> {
    if !is_valid_id(&entry.id) {
        return Err(format!("o identificador `{}` não é válido", entry.id));
    }
    let name = &entry.id;
    if !short_text(&entry.name, 120) {
        return Err(format!("{name}: nome inválido"));
    }
    if semver::Version::parse(&entry.version).is_err() {
        return Err(format!("{name}: versão inválida"));
    }
    if semver::Version::parse(&entry.min_app_version).is_err() || entry.max_app_version.as_deref().is_some_and(|v| semver::Version::parse(v).is_err()) {
        return Err(format!("{name}: versão do Prancha inválida"));
    }
    if entry.description.as_deref().is_some_and(|d| !d.is_empty() && !short_text(d, MAX_TEXT_CHARS)) || entry.authors.iter().any(|a| !short_text(a, 120)) {
        return Err(format!("{name}: texto inválido"));
    }
    if entry.languages.is_empty() || entry.languages.iter().any(|l| !short_text(l, 20)) {
        return Err(format!("{name}: idiomas inválidos"));
    }
    if !check_public_url(net, &entry.artifact_url) || !check_public_url(net, &entry.signature_url) {
        return Err(format!("{name}: endereço de pacote não permitido"));
    }
    if entry.size == 0 || entry.size > MAX_ARTIFACT_BYTES {
        return Err(format!("{name}: tamanho de pacote fora do limite"));
    }
    if entry.sha256.len() != 64 || !entry.sha256.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
        return Err(format!("{name}: SHA-256 inválido"));
    }
    if entry.icon.as_deref().is_some_and(|icon| !is_valid_icon(icon)) {
        return Err(format!("{name}: ícone inválido"));
    }
    if !is_known_rating(&entry.content_rating) {
        return Err(format!("{name}: classificação de conteúdo inválida"));
    }
    if entry.sources.is_empty() {
        return Err(format!("{name}: nenhuma fonte declarada"));
    }
    for source in &entry.sources {
        if !is_valid_id(&source.id) || !short_text(&source.name, 120) || !short_text(&source.language, 20) || !is_known_rating(&source.content_rating) {
            return Err(format!("{name}: fonte inválida"));
        }
    }
    let capabilities = &entry.capabilities;
    if capabilities.allowed_hosts.iter().any(|h| !is_valid_host_pattern(h, net)) {
        return Err(format!("{name}: host autorizado inválido"));
    }
    if capabilities.network && capabilities.allowed_hosts.is_empty() {
        return Err(format!("{name}: acesso à rede sem hosts autorizados"));
    }
    // A tela diria "sem rede", e o Prancha ainda baixaria capas e páginas desses hosts.
    if !capabilities.network && !capabilities.allowed_hosts.is_empty() {
        return Err(format!("{name}: hosts autorizados sem acesso à rede"));
    }
    if !short_text(&entry.runtime, 40) || !short_text(&entry.api_version, 20) || entry.supported_kinds.iter().any(|kind| !short_text(kind, 40)) {
        return Err(format!("{name}: runtime, API ou tipos de conteúdo inválidos"));
    }
    Ok(())
}

fn is_known_rating(rating: &str) -> bool {
    matches!(rating, "general" | "teen" | "adult" | "unknown")
}

/// Motivo pelo qual esta versão do Prancha não pode usar a extensão.
pub fn incompatibility(entry: &ExtensionEntry, app_version: &str) -> Option<String> {
    if entry.runtime != RUNTIME {
        return Some(format!("Usa o runtime “{}”, que o Prancha não conhece.", entry.runtime));
    }
    if entry.api_version != API_VERSION {
        return Some(format!("Usa a versão {} da API de extensões. Esta versão do Prancha usa a {API_VERSION}.", entry.api_version));
    }
    let app = semver::Version::parse(app_version).ok()?;
    if semver::Version::parse(&entry.min_app_version).is_ok_and(|min| app < min) {
        return Some(format!("Exige o Prancha {} ou mais novo.", entry.min_app_version));
    }
    if entry.max_app_version.as_deref().and_then(|v| semver::Version::parse(v).ok()).is_some_and(|max| app > max) {
        return Some(format!("Funciona até o Prancha {}.", entry.max_app_version.as_deref().unwrap_or_default()));
    }
    None
}

/// `2026-11-01T12:00:00Z`, com frações de segundo opcionais, em milissegundos Unix.
fn parse_utc(text: &str) -> Option<i64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut date = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (date.next()??, date.next()??, date.next()??);
    let (time, fraction) = time.split_once('.').unwrap_or((time, ""));
    let mut time = time.splitn(3, ':').map(|p| p.parse::<i64>().ok());
    let (hh, mm, ss) = (time.next()??, time.next()??, time.next()??);
    if !(0..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 || hh < 0 || mm < 0 || ss < 0 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis = if fraction.is_empty() { 0 } else { format!("{fraction:0<3}")[..3].parse::<i64>().ok()? };
    // Dias desde a época (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + hh * 3600 + mm * 60 + ss) * 1000 + millis)
}

// ---------------------------------------------------------------------------
// Rede
// ---------------------------------------------------------------------------

fn fetch(net: &Net, url: &str, max_bytes: u64, etag: Option<&str>) -> AppResult<Response> {
    net.get(url, Access::Public, Limits { max_bytes, timeout: REQUEST_TIMEOUT }, Request { etag, ..Default::default() }).map_err(network_error)
}

fn parse_address(address: &str) -> AppResult<Url> {
    let text = address.trim();
    let with_scheme = if text.contains("://") { text.to_string() } else { format!("https://{text}") };
    Url::parse(&with_scheme)
        .ok()
        .filter(|url| url.host_str().is_some_and(|host| host.contains('.') || host == "localhost"))
        .ok_or_else(|| AppError::user("repo_address", "Esse endereço não é válido. Informe algo como https://exemplo.com/prancha-repo.json."))
}

fn sibling(base: &Url, file: &str) -> Option<Url> {
    let mut url = base.clone();
    url.path_segments_mut().ok()?.pop_if_empty().push(file);
    Some(url)
}

fn names_a_mihon_file(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    path.ends_with(".apk") || path.ends_with("/index.min.json")
}

/// `repo.json` do Mihon (`{"meta": {...}}`) ou `index.min.json` (lista com `pkg` e `apk`).
fn is_mihon_document(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.first().is_some_and(|item| item.get("pkg").is_some() && item.get("apk").is_some()),
        Value::Object(fields) => !fields.contains_key("schemaVersion") && fields.get("meta").is_some_and(|meta| meta.get("name").is_some()),
        _ => false,
    }
}

/// Um endereço-base sem `prancha-repo.json` ainda pode ser de um repositório do Mihon.
fn base_is_mihon(net: &Net, base: &Url) -> bool {
    let is_mihon = |file: &str| {
        let Some(url) = sibling(base, file) else { return false };
        match fetch(net, url.as_str(), MAX_MANIFEST_BYTES, None) {
            Ok(response) => response.is_success() && serde_json::from_slice::<Value>(&response.body).is_ok_and(|value| is_mihon_document(&value)),
            Err(_) => false,
        }
    };
    is_mihon("repo.json") || is_mihon("index.min.json")
}

fn fetch_manifest(net: &Net, address: &str) -> AppResult<(String, Manifest, VerifyingKey)> {
    let url = parse_address(address)?;
    if names_a_mihon_file(&url) {
        return Err(mihon());
    }
    let direct = url.path().to_ascii_lowercase().ends_with(".json");
    let target = if direct { url.clone() } else { sibling(&url, MANIFEST_FILE).ok_or_else(|| invalid("Esse endereço não é válido."))? };
    let response = fetch(net, target.as_str(), MAX_MANIFEST_BYTES, None)?;
    if !response.is_success() {
        if !direct && base_is_mihon(net, &url) {
            return Err(mihon());
        }
        return Err(AppError::user("repo_missing", format!("Não há um {MANIFEST_FILE} nesse endereço (resposta {}).", response.status)));
    }
    let value: Value = serde_json::from_slice(&response.body).map_err(|_| invalid(format!("O {MANIFEST_FILE} não é um JSON válido.")))?;
    if is_mihon_document(&value) {
        return Err(mihon());
    }
    match value.get("schemaVersion").and_then(Value::as_u64) {
        Some(SCHEMA_VERSION) => {}
        Some(_) => return Err(AppError::user("repo_incompatible", "Este repositório usa um formato que esta versão do Prancha não entende.")),
        None => return Err(invalid(format!("O {MANIFEST_FILE} não tem uma versão de formato válida."))),
    }
    let manifest: Manifest = serde_json::from_value(value).map_err(|e| invalid(format!("O {MANIFEST_FILE} está incompleto: {e}.")))?;
    if !is_valid_id(&manifest.id) || !short_text(&manifest.name, 120) || manifest.description.as_deref().is_some_and(|d| !d.is_empty() && !short_text(d, MAX_TEXT_CHARS)) {
        return Err(invalid(format!("O {MANIFEST_FILE} tem identificador, nome ou descrição inválidos.")));
    }
    if manifest.website.as_deref().is_some_and(|w| !Url::parse(w).is_ok_and(|u| u.scheme() == "https")) {
        return Err(invalid("O site do repositório precisa ser um endereço HTTPS."));
    }
    if !check_public_url(net, &manifest.index_url) {
        return Err(invalid("O endereço do índice não é permitido."));
    }
    let key = signing::parse_public_key(&manifest.public_key).ok_or_else(|| invalid("A chave pública do repositório não é válida."))?;
    Ok((target.to_string(), manifest, key))
}

struct Fetched {
    generated_at: i64,
    etag: Option<String>,
    entries: Vec<ExtensionEntry>,
}

/// Índice validado por inteiro: uma única entrada errada recusa o arquivo todo.
/// `None` quando o servidor responde que nada mudou.
fn fetch_index(net: &Net, key: &VerifyingKey, repository_id: &str, index_url: &str, etag: Option<&str>) -> AppResult<Option<Fetched>> {
    let response = fetch(net, index_url, MAX_INDEX_BYTES, etag)?;
    if response.status == 304 {
        return Ok(None);
    }
    if !response.is_success() {
        return Err(AppError::user("repo_missing", format!("O índice do repositório não foi encontrado (resposta {}).", response.status)));
    }
    let signature = fetch(net, &format!("{index_url}.sig"), MAX_SIGNATURE_BYTES, None)?;
    if !signature.is_success() {
        return Err(AppError::user("repo_signature", "O índice não tem assinatura."));
    }
    if !signing::verify_index(key, &response.body, &signature.body) {
        return Err(AppError::user("repo_signature", "A assinatura do índice não confere. O catálogo não foi alterado."));
    }
    let index: IndexFile = serde_json::from_slice(&response.body).map_err(|e| invalid(format!("O índice não é válido: {e}.")))?;
    if index.schema_version != SCHEMA_VERSION {
        return Err(AppError::user("repo_incompatible", "O índice usa um formato que esta versão do Prancha não entende."));
    }
    if index.repository_id != repository_id {
        return Err(invalid("O índice pertence a outro repositório."));
    }
    let generated_at = parse_utc(&index.generated_at).ok_or_else(|| invalid("A data do índice não é válida."))?;
    let mut seen = HashSet::new();
    for entry in &index.extensions {
        validate_entry(net, entry).map_err(|why| invalid(format!("O índice não é válido: {why}.")))?;
        if !seen.insert(entry.id.as_str()) {
            return Err(invalid(format!("O índice não é válido: {} aparece duas vezes.", entry.id)));
        }
    }
    // Conteúdo adulto não chega ao catálogo, e não há opção que mude isso.
    let entries = index.extensions.into_iter().filter(|entry| entry.content_rating != "adult").collect();
    Ok(Some(Fetched { generated_at, etag: response.etag, entries }))
}

// ---------------------------------------------------------------------------
// Consulta
// ---------------------------------------------------------------------------

/// O que a pessoa revisa antes de confiar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryPreview {
    pub url: String,
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub website: Option<String>,
    pub public_key: String,
    pub fingerprint: String,
    pub extension_count: usize,
    pub already_added: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    pub id: String,
    pub url: String,
    pub name: String,
    pub description: Option<String>,
    pub website: Option<String>,
    pub public_key: String,
    pub fingerprint: String,
    pub pending_public_key: Option<String>,
    pub pending_fingerprint: Option<String>,
    pub enabled: bool,
    pub status: String,
    pub last_error: Option<String>,
    pub last_sync_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub extension_count: i64,
    pub added_at: i64,
}

const INFO_COLUMNS: &str = "id, url, name, description, website, public_key, key_fingerprint, pending_public_key, enabled, status, last_error, last_sync_at, last_success_at, extension_count, added_at";

fn info_from_row(row: &Row) -> rusqlite::Result<RepositoryInfo> {
    let pending_public_key: Option<String> = row.get(7)?;
    let pending_fingerprint = pending_public_key.as_deref().and_then(signing::parse_public_key).map(|key| signing::fingerprint(&key));
    Ok(RepositoryInfo {
        id: row.get(0)?,
        url: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        website: row.get(4)?,
        public_key: row.get(5)?,
        fingerprint: row.get(6)?,
        pending_public_key,
        pending_fingerprint,
        enabled: row.get::<_, i64>(8)? != 0,
        status: row.get(9)?,
        last_error: row.get(10)?,
        last_sync_at: row.get(11)?,
        last_success_at: row.get(12)?,
        extension_count: row.get(13)?,
        added_at: row.get(14)?,
    })
}

pub fn list(conn: &Connection) -> AppResult<Vec<RepositoryInfo>> {
    let mut statement = conn.prepare(&format!("SELECT {INFO_COLUMNS} FROM source_repositories ORDER BY name COLLATE NATSORT"))?;
    let rows = statement.query_map([], info_from_row)?.collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<RepositoryInfo> {
    conn.query_row(&format!("SELECT {INFO_COLUMNS} FROM source_repositories WHERE id = ?1"), [id], info_from_row)
        .optional()?
        .ok_or_else(|| AppError::user("repo_not_found", "Esse repositório não está mais na lista."))
}

// ---------------------------------------------------------------------------
// Confiança
// ---------------------------------------------------------------------------

/// Consulta o endereço sem gravar nada. Nada aqui é prova de confiança: é o que a pessoa vai avaliar.
pub fn preview(core: &Core, address: &str) -> AppResult<RepositoryPreview> {
    let (url, manifest, key) = fetch_manifest(&core.net, address)?;
    let fetched = fetch_index(&core.net, &key, &manifest.id, &manifest.index_url, None)?.ok_or_else(|| invalid("O índice não pôde ser lido."))?;
    let already_added = core.db().query_row("SELECT 1 FROM source_repositories WHERE id = ?1 OR url = ?2", [&manifest.id, &url], |_| Ok(())).optional()?.is_some();
    Ok(RepositoryPreview {
        url,
        id: manifest.id,
        name: manifest.name,
        description: manifest.description,
        website: manifest.website,
        public_key: manifest.public_key,
        fingerprint: signing::fingerprint(&key),
        extension_count: fetched.entries.len(),
        already_added,
    })
}

/// Fixa a chave que a pessoa revisou. Se o endereço passou a anunciar outra, nada é gravado.
pub fn add(core: &Core, address: &str, confirmed_fingerprint: &str) -> AppResult<RepositoryInfo> {
    let (url, manifest, key) = fetch_manifest(&core.net, address)?;
    let fingerprint = signing::fingerprint(&key);
    if fingerprint != confirmed_fingerprint {
        return Err(AppError::user("repo_fingerprint", "A chave deste repositório mudou desde a revisão. Revise de novo antes de confiar."));
    }
    let fetched = fetch_index(&core.net, &key, &manifest.id, &manifest.index_url, None)?.ok_or_else(|| invalid("O índice não pôde ser lido."))?;
    let mut conn = core.db();
    let exists = conn.query_row("SELECT 1 FROM source_repositories WHERE id = ?1 OR url = ?2", [&manifest.id, &url], |_| Ok(())).optional()?.is_some();
    if exists {
        return Err(AppError::user("repo_exists", "Esse repositório já está na lista."));
    }
    let now = now_ms();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO source_repositories (id, url, name, description, website, index_url, public_key, key_fingerprint, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![manifest.id, url, manifest.name, manifest.description, manifest.website, manifest.index_url, manifest.public_key, fingerprint, now],
    )?;
    store_catalog(&tx, &manifest, &fetched, now)?;
    tx.commit()?;
    get(&conn, &manifest.id)
}

/// A pessoa confirmou a chave nova. Só vale para a impressão digital que ela viu.
pub fn accept_key(core: &Core, id: &str, confirmed_fingerprint: &str) -> AppResult<RepositoryInfo> {
    {
        let conn = core.db();
        let repository = get(&conn, id)?;
        if repository.pending_fingerprint.as_deref() != Some(confirmed_fingerprint) {
            return Err(AppError::user("repo_fingerprint", "A chave pendente mudou. Sincronize e revise de novo."));
        }
        conn.execute(
            "UPDATE source_repositories SET public_key = pending_public_key, key_fingerprint = ?2, pending_public_key = NULL, status = 'pending', last_error = NULL, index_etag = NULL WHERE id = ?1",
            params![id, confirmed_fingerprint],
        )?;
    }
    sync(core, id)
}

pub fn remove(conn: &Connection, id: &str) -> AppResult<()> {
    // As extensões instaladas guardam a própria procedência e continuam funcionando.
    conn.execute("DELETE FROM source_repositories WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_enabled(conn: &Connection, id: &str, enabled: bool) -> AppResult<RepositoryInfo> {
    conn.execute("UPDATE source_repositories SET enabled = ?2 WHERE id = ?1", params![id, enabled])?;
    get(conn, id)
}

// ---------------------------------------------------------------------------
// Sincronização
// ---------------------------------------------------------------------------

/// Troca o catálogo de uma vez: um índice recusado nunca chega aqui.
fn store_catalog(conn: &Connection, manifest: &Manifest, fetched: &Fetched, now: i64) -> AppResult<()> {
    conn.execute("DELETE FROM repository_extensions WHERE repository_id = ?1", [&manifest.id])?;
    let mut insert = conn.prepare("INSERT INTO repository_extensions (repository_id, extension_id, name, version, entry) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    for entry in &fetched.entries {
        let json = serde_json::to_string(entry).map_err(|e| invalid(e.to_string()))?;
        insert.execute(params![manifest.id, entry.id, entry.name, entry.version, json])?;
    }
    conn.execute(
        "UPDATE source_repositories SET name = ?2, description = ?3, website = ?4, index_url = ?5, status = 'ok', last_error = NULL, pending_public_key = NULL,
            last_sync_at = ?6, last_success_at = ?6, index_generated_at = ?7, index_etag = ?8, extension_count = ?9 WHERE id = ?1",
        params![manifest.id, manifest.name, manifest.description, manifest.website, manifest.index_url, now, fetched.generated_at, fetched.etag, fetched.entries.len() as i64],
    )?;
    Ok(())
}

fn sync_inner(core: &Core, id: &str) -> AppResult<()> {
    let (url, pinned, index_url, etag, last_generated) = core.db().query_row(
        "SELECT url, public_key, index_url, index_etag, index_generated_at FROM source_repositories WHERE id = ?1",
        [id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<i64>>(4)?)),
    )?;
    let (_, manifest, _) = fetch_manifest(&core.net, &url)?;
    if manifest.id != id {
        return Err(invalid("O manifesto agora declara outro repositório."));
    }
    let pinned_key = signing::parse_public_key(&pinned).ok_or_else(|| invalid("A chave registrada não é válida."))?;
    let announced = signing::parse_public_key(&manifest.public_key).ok_or_else(|| invalid("A chave pública do repositório não é válida."))?;
    if announced != pinned_key {
        let message = "A chave deste repositório mudou. A sincronização foi interrompida até você revisar a chave nova.";
        core.db().execute(
            "UPDATE source_repositories SET status = 'key_changed', pending_public_key = ?2, last_error = ?3, last_sync_at = ?4 WHERE id = ?1",
            params![id, manifest.public_key, message, now_ms()],
        )?;
        return Err(AppError::user("repo_key_changed", message));
    }
    // Um ETag só vale para o índice de onde veio.
    let etag = etag.filter(|_| index_url == manifest.index_url);
    let now = now_ms();
    let Some(fetched) = fetch_index(&core.net, &pinned_key, id, &manifest.index_url, etag.as_deref())? else {
        core.db().execute("UPDATE source_repositories SET status = 'ok', last_error = NULL, pending_public_key = NULL, last_sync_at = ?2, last_success_at = ?2 WHERE id = ?1", params![id, now])?;
        return Ok(());
    };
    if last_generated.is_some_and(|last| fetched.generated_at < last) {
        return Err(AppError::user("repo_stale", "O índice recebido é mais antigo que o último aceito e foi recusado."));
    }
    let mut conn = core.db();
    let tx = conn.transaction()?;
    store_catalog(&tx, &manifest, &fetched, now)?;
    tx.commit()?;
    Ok(())
}

/// Sincroniza um repositório. Qualquer falha fica registrada e o último catálogo válido é mantido.
pub fn sync(core: &Core, id: &str) -> AppResult<RepositoryInfo> {
    get(&core.db(), id)?;
    let outcome = sync_inner(core, id);
    let conn = core.db();
    if let Err(error) = &outcome {
        if error.code() != "repo_key_changed" {
            conn.execute("UPDATE source_repositories SET status = 'error', last_error = ?2, last_sync_at = ?3 WHERE id = ?1", params![id, error.to_string(), now_ms()])?;
        }
    }
    let info = get(&conn, id)?;
    outcome.map(|()| info)
}

/// Sincroniza os habilitados. Cada um registra o próprio resultado: um erro não atrapalha os outros.
pub fn sync_all(core: &Core) -> AppResult<Vec<RepositoryInfo>> {
    let ids: Vec<String> = list(&core.db())?.into_iter().filter(|r| r.enabled).map(|r| r.id).collect();
    for group in ids.chunks(SYNC_CONCURRENCY) {
        std::thread::scope(|scope| {
            for id in group {
                scope.spawn(move || {
                    if let Err(error) = sync(core, id) {
                        log::warn!("repositório {id}: {error}");
                    }
                });
            }
        });
    }
    list(&core.db())
}

// ---------------------------------------------------------------------------
// Catálogo
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogQuery {
    pub text: Option<String>,
    pub language: Option<String>,
    /// `name` (padrão) ou `repository`.
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    #[serde(flatten)]
    pub entry: ExtensionEntry,
    pub repository_id: String,
    pub repository_name: String,
    pub installed_version: Option<String>,
    /// A instalada veio deste repositório.
    pub installed_here: bool,
    pub update_available: bool,
    pub incompatible: Option<String>,
}

pub fn catalog(conn: &Connection, query: &CatalogQuery, app_version: &str) -> AppResult<Vec<CatalogItem>> {
    let order = if query.sort.as_deref() == Some("repository") { "s.name COLLATE NATSORT, r.name COLLATE NATSORT" } else { "r.name COLLATE NATSORT, s.name COLLATE NATSORT" };
    let text = query.text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|t| format!("%{}%", t.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    let mut statement = conn.prepare(&format!(
        "SELECT r.entry, s.id, s.name, i.version, i.repository_id
         FROM repository_extensions r
         JOIN source_repositories s ON s.id = r.repository_id AND s.enabled = 1
         LEFT JOIN installed_extensions i ON i.id = r.extension_id
         WHERE (?1 IS NULL OR r.name LIKE ?1 ESCAPE '\\' OR r.extension_id LIKE ?1 ESCAPE '\\')
           AND (?2 IS NULL OR EXISTS (SELECT 1 FROM json_each(r.entry, '$.languages') WHERE value = ?2))
         ORDER BY {order} LIMIT ?3 OFFSET ?4"
    ))?;
    let rows = statement.query_map(params![text, query.language, query.limit.unwrap_or(200).clamp(1, 5000), query.offset.unwrap_or(0).max(0)], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?))
    })?;
    let mut items = Vec::new();
    for row in rows {
        let (json, repository_id, repository_name, installed_version, installed_from) = row?;
        let entry: ExtensionEntry = serde_json::from_str(&json).map_err(|e| invalid(e.to_string()))?;
        let installed_here = installed_from.as_deref() == Some(repository_id.as_str());
        let incompatible = incompatibility(&entry, app_version);
        let newer = |installed: &str| matches!((semver::Version::parse(&entry.version), semver::Version::parse(installed)), (Ok(a), Ok(b)) if a > b);
        let update_available = installed_here && incompatible.is_none() && installed_version.as_deref().is_some_and(newer);
        items.push(CatalogItem { entry, repository_id, repository_name, installed_version, installed_here, update_available, incompatible });
    }
    Ok(items)
}

/// Idiomas do catálogo, para o filtro.
pub fn catalog_languages(conn: &Connection) -> AppResult<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT DISTINCT j.value FROM repository_extensions r JOIN source_repositories s ON s.id = r.repository_id AND s.enabled = 1, json_each(r.entry, '$.languages') j ORDER BY 1",
    )?;
    let rows = statement.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_read_as_utc_milliseconds() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2026-11-01T12:00:00Z"), Some(1_793_534_400_000));
        assert_eq!(parse_utc("2024-02-29T00:00:01.5Z"), Some(1_709_164_801_500));
        for bad in ["2026-11-01", "2026-13-01T00:00:00Z", "2026-11-01T25:00:00Z", "2026-11-01T12:00:00+03:00", "ontem", "2026-11-01T12:00:00.12éZ", "2026-11-01T12:00:00.-1Z", "99999999999999-01-01T00:00:00Z"] {
            assert_eq!(parse_utc(bad), None, "{bad}");
        }
    }

    #[test]
    fn identifiers_are_restricted() {
        for good in ["com.exemplo.fonte", "a", "demo-1_2"] {
            assert!(is_valid_id(good), "{good}");
        }
        for bad in ["", "Com.Exemplo", ".oculto", "a/b", "a b", "..", "é"] {
            assert!(!is_valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn host_patterns_have_no_scheme_port_or_address() {
        let net = Net::new(false);
        for good in ["exemplo.com", "*.cdn.exemplo.com", "a-b.exemplo.com.br"] {
            assert!(is_valid_host_pattern(good, &net), "{good}");
        }
        for bad in ["", "*", "*.com.", "localhost", "https://exemplo.com", "exemplo.com:8080", "exemplo.com/x", "127.0.0.1", "10.0.0.1", "*.exemplo.*", "EXEMPLO.com"] {
            assert!(!is_valid_host_pattern(bad, &net), "{bad}");
        }
        assert!(is_valid_host_pattern("127.0.0.1", &Net::new(true)));
    }

    #[test]
    fn the_real_mihon_files_are_recognized() {
        let repo: Value = serde_json::from_str(r#"{"index_v2":"https://github.com/keiyoushi/extensions/raw/repo/index.pb","meta":{"name":"Keiyoushi","website":"https://keiyoushi.github.io","signingKeyFingerprint":"abc"}}"#).unwrap();
        assert!(is_mihon_document(&repo));
        let index: Value = serde_json::from_str(r#"[{"name":"Tachiyomi: X","pkg":"eu.kanade.tachiyomi.extension.all.x","apk":"x.apk","lang":"all","code":1,"version":"1.4.1","nsfw":0,"sources":[]}]"#).unwrap();
        assert!(is_mihon_document(&index));
        let native: Value = serde_json::from_str(r#"{"schemaVersion":1,"id":"a","name":"b"}"#).unwrap();
        assert!(!is_mihon_document(&native));
        assert!(names_a_mihon_file(&Url::parse("https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json").unwrap()));
        assert!(names_a_mihon_file(&Url::parse("https://exemplo.com/apk/x.APK").unwrap()));
    }

    #[test]
    fn compatibility_covers_runtime_api_and_app_versions() {
        let entry = |runtime: &str, api: &str, min: &str, max: Option<&str>| ExtensionEntry {
            id: "e".into(),
            name: "E".into(),
            version: "1.0.0".into(),
            description: None,
            authors: vec![],
            languages: vec!["pt-BR".into()],
            runtime: runtime.into(),
            api_version: api.into(),
            min_app_version: min.into(),
            max_app_version: max.map(str::to_string),
            artifact_url: String::new(),
            size: 1,
            sha256: String::new(),
            signature_url: String::new(),
            icon: None,
            supported_kinds: vec![],
            content_rating: "general".into(),
            sources: vec![],
            capabilities: Capabilities { network: false, storage: false, authentication: false, allowed_hosts: vec![] },
        };
        assert!(incompatibility(&entry(RUNTIME, "1", "0.2.0", None), "0.2.0").is_none());
        assert!(incompatibility(&entry(RUNTIME, "1", "0.1.0", Some("0.2.0")), "0.2.0").is_none());
        assert!(incompatibility(&entry("apk", "1", "0.2.0", None), "0.2.0").is_some());
        assert!(incompatibility(&entry(RUNTIME, "2", "0.2.0", None), "0.2.0").is_some());
        assert!(incompatibility(&entry(RUNTIME, "1", "0.3.0", None), "0.2.0").is_some());
        assert!(incompatibility(&entry(RUNTIME, "1", "0.1.0", Some("0.1.9")), "0.2.0").is_some());
    }
}
