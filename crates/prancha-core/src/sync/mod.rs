//! Sincronização da leitura online entre aparelhos, pela conta do Prancha. Ver docs/SINCRONIZACAO.md.
//!
//! - Gatilhos no banco (migração 0012) anotam o que a pessoa mudou e quando. Nada é calculado a cada leitura.
//! - Um ciclo puxa o que mudou na nuvem para o espelho (`sync_remote_*`), aplica no aparelho pelas datas, adiciona os
//!   repositórios e instala as extensões que faltam, e sobe o que mudou aqui. A nuvem aplica as mesmas regras de
//!   conflito, então a ordem entre aparelhos não importa.
//! - Só obras de extensões do Prancha sobem. As do Mihon dependem do banco do Suwayomi de cada PC e ficam de fora.
//! - Sem conta, nada sai do aparelho. HQs locais (PDF, CBZ, pastas) nunca sobem.

mod apply;
mod push;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Value, json};

use crate::account::{Account, CallError};
use crate::core::Core;
use crate::db::{self, now_ms};
use crate::error::AppResult;
use crate::import::EVENT_LIBRARY;

pub use apply::chapters_listed;

pub const EVENT_CHANGED: &str = "sync:changed";
/// De quanto em quanto tempo o serviço confere se há algo para subir.
const TICK: Duration = Duration::from_secs(20);
/// Sem mudança local, a nuvem é consultada a cada dois minutos.
const PULL_EVERY: Duration = Duration::from_secs(120);
/// Depois de uma falha, a espera dobra até este limite.
const MAX_BACKOFF: Duration = Duration::from_secs(10 * 60);
const PULL_LIMIT: u32 = 1000;
/// Abaixo dos limites da nuvem (500 mudanças, 256 KB por envio).
const MAX_BATCH: usize = 500;
const MAX_BATCH_BYTES: usize = 200 * 1024;
/// Obras que chegaram só com o estado dos capítulos: a lista de capítulos é buscada na fonte, poucas por ciclo.
const FETCH_PER_CYCLE: usize = 5;

/// A chave portátil da obra: 16 bytes do BLAKE3 de extensão, fonte e chave da obra, em hexadecimal.
pub fn series_key(extension_id: &str, source_id: &str, item_key: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(extension_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(source_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(item_key.as_bytes());
    hex16(hasher.finalize())
}

/// A chave portátil do capítulo, dentro da obra.
pub fn chapter_key(series_key: &str, chapter_item: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(series_key.as_bytes());
    hasher.update(b"\0");
    hasher.update(chapter_item.as_bytes());
    hex16(hasher.finalize())
}

fn hex16(hash: blake3::Hash) -> String {
    hash.as_bytes()[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// As duas chamadas da nuvem que a sincronização usa. Nos testes, uma nuvem falsa.
pub trait Remote {
    fn push(&self, device: &Value, changes: &[Value]) -> Result<Value, CallError>;
    fn pull(&self, since: i64, limit: u32) -> Result<Value, CallError>;
}

impl Remote for Account {
    fn push(&self, device: &Value, changes: &[Value]) -> Result<Value, CallError> {
        self.call("sync_push", json!({ "p_device": device, "p_changes": changes }))
    }
    fn pull(&self, since: i64, limit: u32) -> Result<Value, CallError> {
        self.call("sync_pull", json!({ "p_since": since, "p_limit": limit }))
    }
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub running: bool,
    pub last_sync_at: Option<i64>,
    pub last_error: Option<String>,
    /// Obras, pastas e catálogo esperando para subir.
    pub pending: i64,
}

pub fn status(core: &Core, conn: &Connection) -> AppResult<SyncStatus> {
    let (last_sync_at, last_error, pending) = conn.query_row(
        "SELECT last_sync_at, last_error, (SELECT count(*) FROM sync_dirty) + (SELECT count(*) FROM sync_series_gone) FROM sync_state WHERE id = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(SyncStatus { running: core.sync.running.load(Ordering::SeqCst), last_sync_at, last_error, pending })
}

/// O que um ciclo fez.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Report {
    pub pulled: usize,
    pub pushed: usize,
    pub applied: bool,
}

/// O serviço em segundo plano. Fica parado sem conta.
#[derive(Default)]
pub struct Sync {
    wake: Mutex<bool>,
    signal: Condvar,
    running: AtomicBool,
    /// Repositórios, extensões e obras que já foram tentados nesta abertura do app, para uma falha não se repetir a
    /// cada ciclo.
    tried: Mutex<HashSet<String>>,
}

impl Sync {
    /// Pede um ciclo agora (a pessoa mandou sincronizar, entrou na conta, voltou ao app).
    pub fn request(&self) {
        *self.wake.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.signal.notify_all();
    }

    fn first_try(&self, key: String) -> bool {
        self.tried.lock().unwrap_or_else(|e| e.into_inner()).insert(key)
    }

    fn wait(&self, timeout: Duration) -> bool {
        let guard = self.wake.lock().unwrap_or_else(|e| e.into_inner());
        let (mut guard, _) = self.signal.wait_timeout_while(guard, timeout, |requested| !*requested).unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *guard)
    }

    pub fn spawn(core: Arc<Core>) {
        let spawned = std::thread::Builder::new().name("prancha-sync".into()).spawn(move || service(&core));
        if let Err(e) = spawned {
            log::warn!("sincronização não iniciou: {e}");
        }
    }
}

fn service(core: &Core) {
    let mut last_pull: Option<Instant> = None;
    let mut backoff: Option<(Instant, Duration)> = None;
    let mut requested = true;
    loop {
        let result = (|| -> AppResult<()> {
            let mut conn = db::open(&core.paths.db)?;
            if !Account::is_signed_in(&conn)? {
                // Sem conta, nada fica esperando para subir: a próxima entrada confere tudo.
                conn.execute_batch("DELETE FROM sync_dirty; DELETE FROM sync_series_gone;")?;
                last_pull = None;
                return Ok(());
            }
            if let Some((at, wait)) = backoff {
                if !requested && at.elapsed() < wait {
                    return Ok(());
                }
            }
            let pending: bool = conn.query_row(
                "SELECT full_push = 1 OR EXISTS (SELECT 1 FROM sync_dirty) OR EXISTS (SELECT 1 FROM sync_series_gone) FROM sync_state WHERE id = 1",
                [],
                |r| r.get(0),
            )?;
            if !(requested || pending || last_pull.is_none_or(|at| at.elapsed() >= PULL_EVERY)) {
                return Ok(());
            }
            match run(core, &*core.account, &mut conn) {
                Ok(_) => {
                    last_pull = Some(Instant::now());
                    backoff = None;
                }
                Err(CallError::SignedOut | CallError::Expired) => {}
                Err(_) => {
                    let wait = backoff.map_or(Duration::from_secs(60), |(_, wait)| (wait * 2).min(MAX_BACKOFF));
                    backoff = Some((Instant::now(), wait));
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            log::warn!("sincronização: {e}");
        }
        requested = core.sync.wait(TICK);
    }
}

/// Um ciclo completo: puxa, aplica, completa o catálogo, sobe. Grava o resultado em `sync_state`.
pub fn run(core: &Core, remote: &dyn Remote, conn: &mut Connection) -> Result<Report, CallError> {
    if core.sync.running.swap(true, Ordering::SeqCst) {
        return Ok(Report::default());
    }
    notify(core, conn);
    let result = cycle(core, remote, conn);
    let error = match &result {
        Ok(_) => None,
        Err(CallError::SignedOut) => None,
        Err(e) => Some(e.message()),
    };
    let saved = match &result {
        Ok(_) => conn.execute("UPDATE sync_state SET last_sync_at = ?1, last_error = NULL WHERE id = 1", [now_ms()]),
        Err(_) => conn.execute("UPDATE sync_state SET last_error = ?1 WHERE id = 1", [error]),
    };
    if let Err(e) = saved {
        log::warn!("sincronização: estado não gravado: {e}");
    }
    core.sync.running.store(false, Ordering::SeqCst);
    notify(core, conn);
    result
}

fn notify(core: &Core, conn: &Connection) {
    if let Ok(status) = status(core, conn) {
        (core.emit)(EVENT_CHANGED, serde_json::to_value(status).unwrap_or(Value::Null));
    }
}

/// O que mudou no espelho neste ciclo. `all`: o espelho foi refeito, tudo é aplicado de novo.
#[derive(Debug, Default)]
pub(crate) struct Changed {
    pub all: bool,
    pub series: BTreeSet<String>,
    /// Obras com capítulos que mudaram.
    pub chapters: BTreeSet<String>,
    pub items: BTreeSet<(String, String)>,
}

impl Changed {
    fn is_empty(&self) -> bool {
        !self.all && self.series.is_empty() && self.chapters.is_empty() && self.items.is_empty()
    }
}

fn cycle(core: &Core, remote: &dyn Remote, conn: &mut Connection) -> Result<Report, CallError> {
    let (changed, pulled) = pull(remote, conn)?;
    let applied = if changed.is_empty() { false } else { apply::materialize(core, conn, &changed)? };
    if applied {
        (core.emit)(EVENT_LIBRARY, json!({}));
    }
    complete_catalog(core, conn, &changed);
    fetch_chapter_lists(core, conn);
    let pushed = push::send(remote, conn, &device(conn)?)?;
    Ok(Report { pulled, pushed, applied })
}

/// Puxa tudo desde o cursor guardado, página por página, para o espelho.
fn pull(remote: &dyn Remote, conn: &mut Connection) -> Result<(Changed, usize), CallError> {
    let mut since: i64 = conn.query_row("SELECT cursor FROM sync_state WHERE id = 1", [], |r| r.get(0))?;
    let mut changed = Changed { all: since == 0, ..Default::default() };
    let mut count = 0;
    loop {
        let page = remote.pull(since, PULL_LIMIT)?;
        let tx = conn.transaction()?;
        if page["reset"].as_bool() == Some(true) {
            // O cursor era mais antigo que a última limpeza na nuvem: o espelho recomeça do zero.
            apply::clear_mirror(&tx)?;
            changed = Changed { all: true, ..Default::default() };
        }
        let items = page["changes"].as_array().cloned().unwrap_or_default();
        for item in &items {
            apply::remember(&tx, item, &mut changed)?;
        }
        count += items.len();
        let next = page["next"].as_i64().unwrap_or(since);
        tx.execute("UPDATE sync_state SET cursor = ?1 WHERE id = 1", [next])?;
        tx.commit()?;
        let more = page["more"].as_bool() == Some(true) && next > since;
        since = next;
        if !more {
            break;
        }
    }
    Ok((changed, count))
}

fn device(conn: &Connection) -> AppResult<Value> {
    let id: String = conn.query_row("SELECT device_id FROM sync_state WHERE id = 1", [], |r| r.get(0))?;
    let platform = if cfg!(target_os = "android") {
        "android"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|n| n.trim().chars().take(80).collect::<String>())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| if platform == "android" { "Android".into() } else { "Computador".into() });
    Ok(json!({ "id": id, "name": name, "platform": platform, "version": env!("CARGO_PKG_VERSION") }))
}

/// Repositórios e extensões do Prancha que a conta tem e este aparelho não: adiciona e instala. Só acrescenta; uma
/// remoção em outro aparelho não remove nada aqui. Falhas ficam no registro e não se repetem nesta abertura do app.
fn complete_catalog(core: &Core, conn: &Connection, changed: &Changed) {
    let wanted = |kind: &str, key: &str| changed.all || changed.items.contains(&(kind.to_string(), key.to_string()));
    let items = match apply::catalog_items(conn) {
        Ok(items) => items,
        Err(e) => {
            log::warn!("sincronização: catálogo não lido: {e}");
            return;
        }
    };
    for (kind, key, data) in &items {
        if kind != "repository" || !wanted(kind, key) || data["removed"].as_bool() == Some(true) {
            continue;
        }
        let known: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM source_repositories WHERE url = ?1)", [key], |r| r.get(0)).unwrap_or(true);
        let Some(fingerprint) = data["fingerprint"].as_str() else { continue };
        if known || !core.sync.first_try(format!("repository:{key}")) {
            continue;
        }
        if let Err(e) = crate::extensions::repository::add(core, key, fingerprint) {
            log::warn!("sincronização: repositório {key} não adicionado: {e}");
        }
    }
    for (kind, key, data) in &items {
        if kind != "extension" || !wanted(kind, key) || data["removed"].as_bool() == Some(true) || data["runtime"].as_str() != Some("prancha") {
            continue;
        }
        let Some(repository_url) = data["repository_url"].as_str() else { continue };
        let found = conn.query_row(
            "SELECT (SELECT id FROM source_repositories WHERE url = ?2), EXISTS (SELECT 1 FROM installed_extensions WHERE id = ?1)",
            [key.as_str(), repository_url],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, bool>(1)?)),
        );
        let Ok((Some(repository_id), false)) = found else { continue };
        if !core.sync.first_try(format!("extension:{key}")) {
            continue;
        }
        if let Err(e) = crate::extensions::install::install(core, &repository_id, key) {
            log::warn!("sincronização: extensão {key} não instalada: {e}");
        }
    }
}

/// Obras que chegaram com estado de capítulos que ainda não estão na lista deste aparelho: busca a lista na fonte,
/// e o estado é aplicado ao guardar os capítulos (`chapters_listed`).
fn fetch_chapter_lists(core: &Core, conn: &Connection) {
    let ids = match apply::series_waiting_for_chapters(conn, FETCH_PER_CYCLE * 4) {
        Ok(ids) => ids,
        Err(e) => {
            log::warn!("sincronização: obras sem capítulos não lidas: {e}");
            return;
        }
    };
    for id in ids.into_iter().filter(|id| core.sync.first_try(format!("series:{id}"))).take(FETCH_PER_CYCLE) {
        if let Err(e) = crate::extensions::sources::open_series(core, id, None) {
            log::info!("sincronização: capítulos da obra {id} não buscados: {e}");
        }
    }
}
