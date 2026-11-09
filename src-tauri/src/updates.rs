//! Atualizações do aplicativo. As GitHub Releases são a fonte oficial: o plugin do Tauri lê o `latest.json`
//! da release estável mais recente e confere a assinatura do pacote. Aqui ficam a decisão do que oferecer,
//! as conferências extras e o estado que precisa sobreviver a um fechamento no meio da atualização.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri_plugin_updater::{Error as UpdaterError, Update};

use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::library;
use crate::notifications::{self, Event, Kind, NewNotification};

const STATE_KEY: &str = "updates.state";
const RELEASES_KEY: &str = "updates.releases";
/// `set_setting` recusa valores acima de 64 KB.
const MAX_CACHE_BYTES: usize = 60 * 1024;
const MAX_NOTES_BYTES: usize = 16 * 1024;
const HISTORY_TTL_MS: i64 = 60 * 60 * 1000;
const MAX_LOG_BYTES: u64 = 256 * 1024;

pub const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// O prazo do plugin vale para o download inteiro, então precisa caber uma conexão lenta.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Builds de depuração não usam a internet: testes e desenvolvimento ficam isolados.
pub const ENABLED: bool = !cfg!(debug_assertions);

/// Uma versão publicada, com as notas como estão na release.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReleaseInfo {
    pub version: String,
    pub name: String,
    pub date: Option<String>,
    pub notes: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateState {
    /// Última versão confirmada em execução.
    pub installed_version: Option<String>,
    pub updated_at: Option<i64>,
    pub last_check_at: Option<i64>,
    pub last_seen_version: Option<String>,
    pub available: Option<ReleaseInfo>,
    pub declined_version: Option<String>,
    pub downloaded_version: Option<String>,
    /// O instalador foi aberto, mas a nova versão ainda não rodou.
    pub pending_version: Option<String>,
    pub failed_version: Option<String>,
    pub welcome_version: Option<String>,
    pub notes_shown_version: Option<String>,
    pub history_fetched_at: Option<i64>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Welcome {
    pub version: String,
    pub release: Option<ReleaseInfo>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub current_version: String,
    pub available: Option<ReleaseInfo>,
    pub declined_version: Option<String>,
    pub failed_version: Option<String>,
    pub welcome: Option<Welcome>,
    pub updated_at: Option<i64>,
    pub last_check_at: Option<i64>,
    pub needs_admin: bool,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub releases: Vec<ReleaseInfo>,
    /// A consulta falhou: a lista é a última guardada neste computador.
    pub stale: bool,
}

#[derive(Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum Progress {
    Downloading { downloaded: u64, total: Option<u64> },
    Verifying,
    Installing,
}

/// O que só vale enquanto o aplicativo está aberto.
#[derive(Default)]
pub struct Session {
    pub update: Mutex<Option<Update>>,
    pub abort_download: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

// ---------------------------------------------------------------------------
// Versões
// ---------------------------------------------------------------------------

/// Só versão estável e maior que a instalada vira atualização.
pub fn is_newer(current: &Version, remote: &Version) -> bool {
    remote.pre.is_empty() && remote > current
}

fn parse(version: &Option<String>) -> Option<Version> {
    Version::parse(version.as_deref()?).ok()
}

// ---------------------------------------------------------------------------
// Estado
// ---------------------------------------------------------------------------

fn load_state(conn: &Connection) -> AppResult<UpdateState> {
    let value = library::get_setting(conn, STATE_KEY)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

fn save_state(conn: &Connection, state: &UpdateState) -> AppResult<()> {
    library::set_setting(conn, STATE_KEY, &json!(state))
}

fn change_state(conn: &Connection, change: impl FnOnce(&mut UpdateState)) -> AppResult<()> {
    let mut state = load_state(conn)?;
    change(&mut state);
    save_state(conn, &state)
}

fn had_previous_use(conn: &Connection) -> AppResult<bool> {
    let used = conn.query_row("SELECT EXISTS(SELECT 1 FROM settings) OR EXISTS(SELECT 1 FROM comics)", [], |r| r.get(0))?;
    Ok(used)
}

/// Roda a cada início: compara a versão em execução com o que estava registrado. É o único
/// lugar que dá uma atualização por concluída, e a prova é a versão nova estar rodando.
pub fn reconcile(conn: &Connection, current: &Version) -> AppResult<()> {
    let mut state = load_state(conn)?;
    let updated = match parse(&state.installed_version) {
        Some(previous) => previous < *current,
        // Sem registro: primeira execução, ou uma instalação anterior ao atualizador.
        None => had_previous_use(conn)?,
    };
    if updated {
        state.welcome_version = Some(current.to_string());
        state.updated_at = Some(now_ms());
    }
    state.installed_version = Some(current.to_string());
    if let Some(pending) = state.pending_version.take() {
        if parse(&Some(pending.clone())).is_some_and(|p| p > *current) {
            state.failed_version = Some(pending);
        }
    }
    let settled = |version: &Option<String>| parse(version).is_none_or(|v| v <= *current);
    if settled(&state.declined_version) {
        state.declined_version = None;
    }
    if settled(&state.downloaded_version) {
        state.downloaded_version = None;
    }
    if settled(&state.failed_version) {
        state.failed_version = None;
    }
    if state.available.as_ref().is_some_and(|a| settled(&Some(a.version.clone()))) {
        state.available = None;
    }
    if parse(&state.welcome_version).is_some_and(|v| v != *current) {
        state.welcome_version = None;
    }
    save_state(conn, &state)?;
    // A central guarda o resultado. A chave é a versão: reabrir o app não repete o aviso.
    let now = now_ms();
    if updated {
        notifications::record(conn, &update_notice(Kind::UpdateInstalled, &current.to_string()), now)?;
    }
    if let Some(failed) = &state.failed_version {
        notifications::record(conn, &update_notice(Kind::UpdateFailed, failed), now)?;
    }
    Ok(())
}

/// Aviso de uma versão, com a chave de deduplicação pelo tipo e pelo número.
pub fn update_notice(kind: Kind, version: &str) -> NewNotification {
    NewNotification::new(kind).extra(json!({ "version": version })).event(Event::new(format!("{}:{version}", kind.as_str())))
}

pub fn snapshot(conn: &Connection, current: &Version, needs_admin: bool) -> AppResult<Snapshot> {
    let state = load_state(conn)?;
    let releases = load_releases(conn)?;
    let welcome = state.welcome_version.map(|version| Welcome {
        release: releases.iter().find(|r| r.version == version).cloned(),
        version,
    });
    Ok(Snapshot {
        current_version: current.to_string(),
        available: state.available,
        declined_version: state.declined_version,
        failed_version: state.failed_version,
        welcome,
        updated_at: state.updated_at,
        last_check_at: state.last_check_at,
        needs_admin,
    })
}

/// `found` vazio confirma que não há nada a instalar, e o indicador some.
pub fn record_check(conn: &Connection, current: &Version, found: Option<ReleaseInfo>) -> AppResult<()> {
    change_state(conn, |state| {
        state.last_check_at = Some(now_ms());
        state.last_seen_version = Some(found.as_ref().map_or_else(|| current.to_string(), |r| r.version.clone()));
        state.available = found;
    })
}

pub fn decline(conn: &Connection, version: &str) -> AppResult<()> {
    change_state(conn, |state| {
        state.declined_version = Some(version.to_string());
        state.failed_version = None;
    })
}

pub fn notes_shown(conn: &Connection, version: &str) -> AppResult<()> {
    change_state(conn, |state| {
        state.notes_shown_version = Some(version.to_string());
        state.welcome_version = None;
    })
}

/// Registrado antes de abrir o instalador: se a nova versão não rodar depois disso, a próxima abertura sabe que falhou.
pub fn mark_ready_to_install(conn: &Connection, version: &str) -> AppResult<()> {
    change_state(conn, |state| {
        state.downloaded_version = Some(version.to_string());
        state.pending_version = Some(version.to_string());
        state.failed_version = None;
    })
}

pub fn install_not_started(conn: &Connection) -> AppResult<()> {
    change_state(conn, |state| {
        state.downloaded_version = None;
        state.pending_version = None;
    })
}

// ---------------------------------------------------------------------------
// Notas guardadas localmente
// ---------------------------------------------------------------------------

pub fn load_releases(conn: &Connection) -> AppResult<Vec<ReleaseInfo>> {
    let value = library::get_setting(conn, RELEASES_KEY)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default())
}

/// Guarda as versões mais novas que couberem: `releases` vem em ordem da mais nova para a mais antiga.
fn save_releases(conn: &Connection, releases: &[ReleaseInfo]) -> AppResult<()> {
    let mut kept = releases.len();
    let mut value = json!(releases);
    while value.to_string().len() > MAX_CACHE_BYTES && kept > 0 {
        kept -= 1;
        value = json!(releases[..kept]);
    }
    library::set_setting(conn, RELEASES_KEY, &value)
}

/// Guarda as notas de uma versão antes do download: a janela de novidades precisa delas mesmo sem internet.
pub fn remember_release(conn: &Connection, release: &ReleaseInfo) -> AppResult<()> {
    let mut releases = load_releases(conn)?;
    releases.retain(|r| r.version != release.version);
    releases.insert(0, release.clone());
    save_releases(conn, &releases)
}

pub fn history_is_fresh(conn: &Connection) -> AppResult<bool> {
    Ok(load_state(conn)?.history_fetched_at.is_some_and(|at| now_ms() - at < HISTORY_TTL_MS))
}

pub fn store_history(conn: &Connection, releases: &[ReleaseInfo]) -> AppResult<()> {
    save_releases(conn, releases)?;
    change_state(conn, |state| state.history_fetched_at = Some(now_ms()))
}

// ---------------------------------------------------------------------------
// Origem dos pacotes
// ---------------------------------------------------------------------------

/// `https://github.com/<dono>/<repo>/releases/latest/download/latest.json` vira `https://github.com/<dono>/<repo>/releases`.
fn releases_base(endpoint: &str) -> Option<&str> {
    endpoint.split_once("/latest/download/").map(|(base, _)| base)
}

/// O pacote precisa estar na pasta da versão anunciada, nas releases configuradas no próprio aplicativo.
pub fn is_official_package(endpoint: &str, version: &str, url: &str) -> bool {
    let Some(base) = releases_base(endpoint) else { return false };
    let Some(file) = url.strip_prefix(&format!("{base}/download/v{version}/")) else { return false };
    !file.contains('/') && (file.ends_with(".exe") || file.ends_with(".msi"))
}

pub fn release_page(endpoint: &str, version: &str) -> Option<String> {
    Some(format!("{}/tag/v{version}", releases_base(endpoint)?))
}

pub fn releases_api(endpoint: &str) -> Option<String> {
    let base = releases_base(endpoint)?;
    let repo = base.strip_prefix("https://github.com/").and_then(|b| b.strip_suffix("/releases"));
    Some(match repo {
        Some(repo) => format!("https://api.github.com/repos/{repo}/releases?per_page=50"),
        // Só o servidor local do teste de atualização cai aqui: ver tests/update/.
        None => format!("{base}/api"),
    })
}

fn clip_notes(mut notes: String) -> String {
    if notes.len() > MAX_NOTES_BYTES {
        let mut end = MAX_NOTES_BYTES;
        while !notes.is_char_boundary(end) {
            end -= 1;
        }
        notes.truncate(end);
    }
    notes
}

pub fn release_from_update(update: &Update, endpoint: &str) -> ReleaseInfo {
    ReleaseInfo {
        version: update.version.clone(),
        name: format!("Prancha v{}", update.version),
        date: update.raw_json["pub_date"].as_str().map(str::to_string),
        notes: clip_notes(update.body.clone().unwrap_or_default()),
        url: release_page(endpoint, &update.version).unwrap_or_default(),
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Lê a resposta da API do GitHub: só versões estáveis, da mais nova para a mais antiga.
pub fn parse_releases(raw: &str) -> Result<Vec<ReleaseInfo>, serde_json::Error> {
    let list: Vec<GithubRelease> = serde_json::from_str(raw)?;
    let mut releases: Vec<(Version, ReleaseInfo)> = list
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.trim_start_matches('v')).ok().filter(|v| v.pre.is_empty())?;
            let info = ReleaseInfo {
                version: version.to_string(),
                name: r.name.unwrap_or_else(|| format!("Prancha v{version}")),
                date: r.published_at,
                notes: clip_notes(r.body.unwrap_or_default()),
                url: r.html_url,
            };
            Some((version, info))
        })
        .collect();
    releases.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(releases.into_iter().map(|(_, info)| info).collect())
}

pub async fn fetch_releases(url: &str) -> Result<Vec<ReleaseInfo>, String> {
    // O plugin instala este mesmo provedor TLS ao consultar; quem chegar primeiro vale.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder().user_agent("Prancha").timeout(CHECK_TIMEOUT).build().map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let body = response.text().await.map_err(|e| e.to_string())?;
    parse_releases(&body).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Integridade
// ---------------------------------------------------------------------------

/// O SHA-256 do pacote anunciado no `latest.json`. A assinatura continua sendo a garantia principal.
pub fn expected_sha256(manifest: &Value, url: &str) -> Option<String> {
    let platforms = manifest["platforms"].as_object()?;
    let platform = platforms.values().find(|p| p["url"] == url)?;
    platform["sha256"].as_str().map(str::to_string)
}

pub fn sha256_matches(bytes: &[u8], expected: &str) -> bool {
    let actual: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    actual.eq_ignore_ascii_case(expected)
}

// ---------------------------------------------------------------------------
// Erros
// ---------------------------------------------------------------------------

pub fn describe(error: &UpdaterError) -> AppError {
    match error {
        UpdaterError::Reqwest(_) | UpdaterError::Network(_) | UpdaterError::ReleaseNotFound => {
            AppError::user("network", "Não foi possível falar com o GitHub. Verifique sua conexão e tente novamente.")
        }
        UpdaterError::Minisign(_)
        | UpdaterError::Base64(_)
        | UpdaterError::SignatureUtf8(_)
        | UpdaterError::SignedVersionMismatch { .. }
        | UpdaterError::MissingSignedVersion => {
            AppError::user("signature", "A atualização não passou na verificação de segurança e foi descartada. Nada foi alterado.")
        }
        UpdaterError::TargetNotFound(_) | UpdaterError::TargetsNotFound(_) => {
            AppError::user("no_package", "Esta versão não tem pacote para o seu sistema.")
        }
        UpdaterError::Io(e) if e.kind() == ErrorKind::StorageFull => {
            AppError::user("disk", "Falta espaço em disco para instalar a atualização. Libere espaço e tente novamente.")
        }
        UpdaterError::Io(e) if e.kind() == ErrorKind::PermissionDenied => {
            AppError::user("permission", "O Windows não permitiu abrir o instalador. Verifique suas permissões e tente novamente.")
        }
        _ => AppError::user("install", "Não foi possível abrir o instalador. A versão atual continua funcionando."),
    }
}

/// Detalhes técnicos para diagnóstico, sem incomodar quem está lendo. Recomeça ao passar do limite.
pub fn log(data_dir: &Path, message: &str) {
    let path = data_dir.join("updates.log");
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = fs::remove_file(&path);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {message}", now_ms());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn release(version: &str) -> ReleaseInfo {
        ReleaseInfo { version: version.into(), name: format!("Prancha v{version}"), date: None, notes: "## Novidades\n- Algo".into(), url: String::new() }
    }

    #[test]
    fn compares_versions_by_number_and_never_offers_previews() {
        assert!(!is_newer(&v("1.2.3"), &v("1.2.3")), "igual");
        assert!(!is_newer(&v("1.2.3"), &v("1.2.2")), "inferior");
        assert!(is_newer(&v("1.2.3"), &v("1.2.4")), "PATCH");
        assert!(is_newer(&v("1.2.3"), &v("1.3.0")), "MINOR");
        assert!(is_newer(&v("1.2.3"), &v("2.0.0")), "MAJOR");
        assert!(is_newer(&v("1.2.9"), &v("1.2.10")), "dois dígitos em PATCH");
        assert!(is_newer(&v("1.9.0"), &v("1.10.0")), "dois dígitos em MINOR");
        assert!(is_newer(&v("9.0.0"), &v("10.0.0")), "dois dígitos em MAJOR");
        assert!(!is_newer(&v("1.2.10"), &v("1.2.9")), "texto diria o contrário");
        assert!(!is_newer(&v("1.2.3"), &v("1.3.0-beta.1")), "pré-lançamento");
    }

    #[test]
    fn first_run_does_not_count_as_an_update() {
        let conn = db::open_in_memory().unwrap();
        reconcile(&conn, &v("0.2.0")).unwrap();
        let snap = snapshot(&conn, &v("0.2.0"), false).unwrap();
        assert_eq!(snap.welcome, None);
        assert_eq!(snap.updated_at, None);
    }

    #[test]
    fn coming_from_a_version_without_updater_counts_as_an_update() {
        let conn = db::open_in_memory().unwrap();
        library::set_setting(&conn, "appearance.theme", &json!("light")).unwrap();
        reconcile(&conn, &v("0.2.0")).unwrap();
        assert_eq!(snapshot(&conn, &v("0.2.0"), false).unwrap().welcome.unwrap().version, "0.2.0");
    }

    #[test]
    fn welcome_appears_once_after_a_confirmed_update_and_survives_being_closed_unseen() {
        let conn = db::open_in_memory().unwrap();
        reconcile(&conn, &v("1.0.0")).unwrap();
        remember_release(&conn, &release("1.1.0")).unwrap();
        mark_ready_to_install(&conn, "1.1.0").unwrap();

        // O instalador rodou e a nova versão abriu.
        reconcile(&conn, &v("1.1.0")).unwrap();
        let snap = snapshot(&conn, &v("1.1.0"), false).unwrap();
        let welcome = snap.welcome.unwrap();
        assert_eq!(welcome.version, "1.1.0");
        assert_eq!(welcome.release.unwrap().notes, "## Novidades\n- Algo", "notas guardadas, sem internet");
        assert!(snap.updated_at.is_some());
        assert_eq!(snap.failed_version, None);

        // Fechou sem ver: aparece de novo.
        reconcile(&conn, &v("1.1.0")).unwrap();
        assert!(snapshot(&conn, &v("1.1.0"), false).unwrap().welcome.is_some());

        // Viu: não repete.
        notes_shown(&conn, "1.1.0").unwrap();
        reconcile(&conn, &v("1.1.0")).unwrap();
        assert_eq!(snapshot(&conn, &v("1.1.0"), false).unwrap().welcome, None);
        assert_eq!(load_state(&conn).unwrap().notes_shown_version.as_deref(), Some("1.1.0"));
    }

    #[test]
    fn installer_that_did_not_finish_is_reported_and_never_counted_as_updated() {
        let conn = db::open_in_memory().unwrap();
        reconcile(&conn, &v("1.0.0")).unwrap();
        mark_ready_to_install(&conn, "1.1.0").unwrap();
        reconcile(&conn, &v("1.0.0")).unwrap();
        let snap = snapshot(&conn, &v("1.0.0"), false).unwrap();
        assert_eq!(snap.failed_version.as_deref(), Some("1.1.0"));
        assert_eq!(snap.welcome, None);
        assert_eq!(snap.updated_at, None);
        assert_eq!(load_state(&conn).unwrap().pending_version, None);

        // "Depois" encerra o aviso sem apagar a decisão.
        decline(&conn, "1.1.0").unwrap();
        reconcile(&conn, &v("1.0.0")).unwrap();
        let snap = snapshot(&conn, &v("1.0.0"), false).unwrap();
        assert_eq!(snap.failed_version, None);
        assert_eq!(snap.declined_version.as_deref(), Some("1.1.0"));
    }

    fn notices(conn: &Connection) -> Vec<(String, String)> {
        let page = notifications::list(conn, &Default::default()).unwrap();
        page.items.into_iter().map(|n| (n.kind, n.body)).collect()
    }

    #[test]
    fn the_center_records_installed_and_failed_updates_once() {
        let conn = db::open_in_memory().unwrap();
        reconcile(&conn, &v("1.0.0")).unwrap();
        assert!(notices(&conn).is_empty(), "a primeira execução não é atualização");
        mark_ready_to_install(&conn, "1.1.0").unwrap();
        reconcile(&conn, &v("1.0.0")).unwrap();
        reconcile(&conn, &v("1.0.0")).unwrap();
        assert_eq!(notices(&conn), vec![("update_failed".to_string(), "A instalação da v1.1.0 não terminou. A versão atual continua funcionando.".to_string())]);
        reconcile(&conn, &v("1.1.0")).unwrap();
        reconcile(&conn, &v("1.1.0")).unwrap();
        let kinds: Vec<String> = notices(&conn).into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, vec!["update_installed", "update_failed"], "reabrir não repete");
    }

    #[test]
    fn installed_version_clears_what_it_settles() {
        let conn = db::open_in_memory().unwrap();
        reconcile(&conn, &v("1.0.0")).unwrap();
        record_check(&conn, &v("1.0.0"), Some(release("1.1.0"))).unwrap();
        decline(&conn, "1.1.0").unwrap();
        let snap = snapshot(&conn, &v("1.0.0"), false).unwrap();
        assert_eq!(snap.available.unwrap().version, "1.1.0");

        reconcile(&conn, &v("1.1.0")).unwrap();
        let snap = snapshot(&conn, &v("1.1.0"), false).unwrap();
        assert_eq!(snap.available, None);
        assert_eq!(snap.declined_version, None);
    }

    #[test]
    fn a_check_that_finds_nothing_clears_the_available_version() {
        let conn = db::open_in_memory().unwrap();
        record_check(&conn, &v("1.0.0"), Some(release("1.1.0"))).unwrap();
        record_check(&conn, &v("1.0.0"), None).unwrap();
        let state = load_state(&conn).unwrap();
        assert_eq!(state.available, None);
        assert_eq!(state.last_seen_version.as_deref(), Some("1.0.0"));
        assert!(state.last_check_at.is_some());
    }

    #[test]
    fn saved_state_survives_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let conn = db::open(&path).unwrap();
            reconcile(&conn, &v("1.0.0")).unwrap();
            record_check(&conn, &v("1.0.0"), Some(release("1.1.0"))).unwrap();
            decline(&conn, "1.1.0").unwrap();
        }
        let conn = db::open(&path).unwrap();
        reconcile(&conn, &v("1.0.0")).unwrap();
        let snap = snapshot(&conn, &v("1.0.0"), false).unwrap();
        assert_eq!(snap.available.unwrap().version, "1.1.0");
        assert_eq!(snap.declined_version.as_deref(), Some("1.1.0"));
    }

    #[test]
    fn notes_cache_keeps_the_newest_versions_that_fit() {
        let conn = db::open_in_memory().unwrap();
        let big: Vec<ReleaseInfo> = (0..40).rev().map(|i| ReleaseInfo { notes: "x".repeat(4000), ..release(&format!("1.0.{i}")) }).collect();
        store_history(&conn, &big).unwrap();
        let kept = load_releases(&conn).unwrap();
        assert!(!kept.is_empty() && kept.len() < 40);
        assert_eq!(kept[0].version, "1.0.39");
        assert!(history_is_fresh(&conn).unwrap());
    }

    const ENDPOINT: &str = "https://github.com/CAKeysama/prancha/releases/latest/download/latest.json";

    #[test]
    fn only_packages_of_the_announced_official_release_are_accepted() {
        let ok = "https://github.com/CAKeysama/prancha/releases/download/v1.3.0/Prancha_1.3.0_x64-setup.exe";
        assert!(is_official_package(ENDPOINT, "1.3.0", ok));
        assert!(is_official_package(ENDPOINT, "1.3.0", "https://github.com/CAKeysama/prancha/releases/download/v1.3.0/Prancha_1.3.0_x64_pt-BR.msi"));
        for bad in [
            "https://github.com/outra-pessoa/prancha/releases/download/v1.3.0/Prancha_1.3.0_x64-setup.exe",
            "https://exemplo.com/CAKeysama/prancha/releases/download/v1.3.0/Prancha_1.3.0_x64-setup.exe",
            "http://github.com/CAKeysama/prancha/releases/download/v1.3.0/Prancha_1.3.0_x64-setup.exe",
            "https://github.com/CAKeysama/prancha/releases/download/v1.2.0/Prancha_1.2.0_x64-setup.exe",
            "https://github.com/CAKeysama/prancha/releases/download/v1.3.0/sub/Prancha_1.3.0_x64-setup.exe",
            "https://github.com/CAKeysama/prancha/releases/download/v1.3.0/Prancha_1.3.0_x64.tar.gz",
        ] {
            assert!(!is_official_package(ENDPOINT, "1.3.0", bad), "{bad}");
        }
        assert!(!is_official_package("https://exemplo.com/latest.json", "1.3.0", ok));
    }

    #[test]
    fn release_urls_come_from_the_configured_repository() {
        assert_eq!(release_page(ENDPOINT, "1.3.0").unwrap(), "https://github.com/CAKeysama/prancha/releases/tag/v1.3.0");
        assert_eq!(releases_api(ENDPOINT).unwrap(), "https://api.github.com/repos/CAKeysama/prancha/releases?per_page=50");
    }

    #[test]
    fn github_response_keeps_only_stable_versions_in_numeric_order() {
        let raw = json!([
            { "tag_name": "v1.2.9", "name": "Prancha v1.2.9", "body": "antiga", "published_at": "2026-01-01T00:00:00Z", "html_url": "u1", "draft": false, "prerelease": false },
            { "tag_name": "v1.3.0-beta.1", "name": "b", "body": "", "html_url": "u2", "draft": false, "prerelease": false },
            { "tag_name": "v1.4.0", "name": "p", "body": "", "html_url": "u3", "draft": false, "prerelease": true },
            { "tag_name": "v1.5.0", "name": "r", "body": "", "html_url": "u4", "draft": true, "prerelease": false },
            { "tag_name": "ferramentas", "name": "x", "body": "", "html_url": "u5" },
            { "tag_name": "v1.2.10", "name": null, "body": null, "published_at": null, "html_url": "u6", "draft": false, "prerelease": false },
        ])
        .to_string();
        let releases = parse_releases(&raw).unwrap();
        assert_eq!(releases.iter().map(|r| r.version.as_str()).collect::<Vec<_>>(), ["1.2.10", "1.2.9"]);
        assert_eq!(releases[0].name, "Prancha v1.2.10");
        assert_eq!(releases[0].notes, "");
        assert!(parse_releases("{\"message\":\"rate limit\"}").is_err());
    }

    #[test]
    fn sha256_is_checked_against_the_announced_package() {
        let url = "https://github.com/CAKeysama/prancha/releases/download/v1.3.0/a.exe";
        // SHA-256 de "abc", do padrão FIPS 180.
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let manifest = json!({ "platforms": { "windows-x86_64": { "url": url, "signature": "s", "sha256": abc } } });
        assert_eq!(expected_sha256(&manifest, url).as_deref(), Some(abc));
        assert_eq!(expected_sha256(&manifest, "outra"), None);
        assert_eq!(expected_sha256(&json!({}), url), None);
        assert!(sha256_matches(b"abc", abc));
        assert!(sha256_matches(b"abc", &abc.to_uppercase()));
        assert!(!sha256_matches(b"abd", abc));
    }

    #[test]
    fn errors_become_clear_messages_by_cause() {
        let code = |e: UpdaterError| describe(&e).code();
        assert_eq!(code(UpdaterError::Network("x".into())), "network");
        assert_eq!(code(UpdaterError::ReleaseNotFound), "network");
        assert_eq!(code(UpdaterError::MissingSignedVersion), "signature");
        assert_eq!(code(UpdaterError::TargetNotFound("x".into())), "no_package");
        assert_eq!(code(UpdaterError::Io(std::io::Error::from(ErrorKind::StorageFull))), "disk");
        assert_eq!(code(UpdaterError::Io(std::io::Error::from(ErrorKind::PermissionDenied))), "permission");
        assert_eq!(code(UpdaterError::Io(std::io::Error::from(ErrorKind::Other))), "install");
    }

    #[test]
    fn log_restarts_when_it_grows_too_much() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("updates.log");
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        log(dir.path(), "detalhe");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("detalhe\n") && text.len() < 64);
    }
}
