//! Executa componentes WebAssembly. O componente só enxerga o que `extensions/wit/world.wit` declara:
//! não há WASI, então um import de sistema de arquivos ou de rede faz a carga falhar.
//! Cada chamada usa uma instância nova, com limites próprios, na thread de trabalho de quem chamou.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, params};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, ResourceLimiter, Store, Trap, UpdateDeadline};

use super::net::{Access, Cancel, Limits, Net, NetError, Request};
use super::signing::sha256_hex;
use crate::db;
use crate::error::{AppError, AppResult};

wasmtime::component::bindgen!({ path: "../extensions/wit", world: "extension" });

use prancha::source::{host, types};

pub use types::{ContentKind, ContentRating, PublicationStatus, ReadingDirection, SearchResult};

/// Intervalo em que a execução confere prazo e cancelamento.
const TICK: Duration = Duration::from_millis(50);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_WASM_STACK: usize = 512 * 1024;
const MAX_TABLE_ELEMENTS: usize = 100_000;
const MAX_INSTANCES: usize = 64;
const MAX_QUERY_BYTES: usize = 512;
const MAX_ID_BYTES: usize = 1024;
const MAX_FILTERS: usize = 32;
const MAX_STORAGE_KEY_BYTES: usize = 256;
const MAX_STORAGE_VALUE_BYTES: usize = 256 * 1024;
const MAX_STORAGE_TOTAL_BYTES: i64 = 1024 * 1024;
const MAX_LOG_LINES: u32 = 64;
const MAX_MESSAGE_CHARS: usize = 300;

#[derive(Debug, Clone, Copy)]
pub struct EngineLimits {
    /// Instruções por chamada.
    pub fuel: u64,
    pub memory_bytes: usize,
    pub call_timeout: Duration,
    pub requests_per_call: u32,
    pub response_bytes: u64,
    pub concurrent_calls: usize,
    pub concurrent_per_extension: usize,
}

impl Default for EngineLimits {
    fn default() -> Self {
        EngineLimits {
            fuel: 10_000_000_000,
            memory_bytes: 64 * 1024 * 1024,
            call_timeout: Duration::from_secs(45),
            requests_per_call: 20,
            response_bytes: 8 * 1024 * 1024,
            concurrent_calls: 4,
            concurrent_per_extension: 2,
        }
    }
}

/// O que uma extensão pode fazer. Vem do índice assinado, gravado na instalação.
#[derive(Debug, Clone, Default)]
pub struct Grant {
    pub network: bool,
    pub storage: bool,
    pub allowed_hosts: Vec<String>,
}

/// Uma extensão instalada, pronta para ser executada.
#[derive(Debug, Clone)]
pub struct Installed {
    pub id: String,
    pub file: PathBuf,
    pub sha256: String,
    pub grant: Grant,
    /// Roda no Suwayomi, não neste motor: sem arquivo e sem as regras de `grant`.
    pub bridged: bool,
}

// ---------------------------------------------------------------------------
// Host
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Cancelled,
    Timeout,
    Memory,
}

struct HostState {
    extension_id: String,
    grant: Grant,
    limits: EngineLimits,
    net: Arc<Net>,
    db_path: PathBuf,
    storage: Option<Connection>,
    cancel: Cancel,
    deadline: Instant,
    requests: u32,
    log_lines: u32,
    stop: Option<Stop>,
}

impl HostState {
    fn storage(&mut self) -> Option<&Connection> {
        if !self.grant.storage {
            return None;
        }
        if self.storage.is_none() {
            self.storage = db::open(&self.db_path).inspect_err(|e| log::warn!("armazenamento de extensão: {e}")).ok();
        }
        self.storage.as_ref()
    }
}

impl ResourceLimiter for HostState {
    fn memory_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        let allowed = desired <= self.limits.memory_bytes;
        if !allowed {
            self.stop = Some(Stop::Memory);
        }
        Ok(allowed)
    }

    fn table_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        Ok(desired <= MAX_TABLE_ELEMENTS)
    }

    fn instances(&self) -> usize {
        MAX_INSTANCES
    }

    fn tables(&self) -> usize {
        MAX_INSTANCES
    }

    fn memories(&self) -> usize {
        MAX_INSTANCES
    }
}

impl types::Host for HostState {}

impl host::Host for HostState {
    fn http_get(&mut self, request: host::HttpRequest) -> Result<host::HttpResponse, host::HttpError> {
        if !self.grant.network {
            return Err(host::HttpError::Blocked("A extensão não declarou acesso à rede.".into()));
        }
        if self.requests >= self.limits.requests_per_call {
            return Err(host::HttpError::TooManyRequests);
        }
        self.requests += 1;
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(host::HttpError::Timeout);
        }
        let headers: Vec<(String, String)> = request.headers.into_iter().map(|h| (h.name, h.value)).collect();
        let limits = Limits { max_bytes: self.limits.response_bytes, timeout: remaining.min(REQUEST_TIMEOUT) };
        let outcome = self.net.get(
            &request.url,
            Access::Hosts(&self.grant.allowed_hosts),
            limits,
            Request { headers: &headers, cancel: Some(&self.cancel), check_content_type: true, ..Default::default() },
        );
        match outcome {
            Ok(response) => Ok(host::HttpResponse { status: response.status, url: response.url, content_type: response.content_type, body: response.body }),
            Err(NetError::Blocked(why)) => Err(host::HttpError::Blocked(why)),
            Err(NetError::Timeout) => Err(host::HttpError::Timeout),
            Err(NetError::TooLarge) => Err(host::HttpError::TooLarge),
            Err(error @ (NetError::Network(_) | NetError::Cancelled)) => Err(host::HttpError::Network(error.to_string())),
        }
    }

    fn storage_get(&mut self, key: String) -> Option<Vec<u8>> {
        let id = self.extension_id.clone();
        let conn = self.storage()?;
        conn.query_row("SELECT value FROM extension_storage WHERE extension_id = ?1 AND key = ?2", params![id, key], |r| r.get(0)).optional().ok().flatten()
    }

    fn storage_set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        if !self.grant.storage {
            return Err("A extensão não declarou armazenamento.".into());
        }
        if key.is_empty() || key.len() > MAX_STORAGE_KEY_BYTES || value.len() > MAX_STORAGE_VALUE_BYTES {
            return Err("A chave ou o valor passa do limite.".into());
        }
        let id = self.extension_id.clone();
        let conn = self.storage().ok_or("O armazenamento não está disponível.")?;
        let others: i64 = conn
            .query_row(
                "SELECT coalesce(sum(length(key) + length(value)), 0) FROM extension_storage WHERE extension_id = ?1 AND key <> ?2",
                params![id, key],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if others + key.len() as i64 + value.len() as i64 > MAX_STORAGE_TOTAL_BYTES {
            return Err("O armazenamento da extensão está cheio.".into());
        }
        conn.execute(
            "INSERT INTO extension_storage (extension_id, key, value) VALUES (?1, ?2, ?3) ON CONFLICT (extension_id, key) DO UPDATE SET value = excluded.value",
            params![id, key, value],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn storage_delete(&mut self, key: String) {
        let id = self.extension_id.clone();
        if let Some(conn) = self.storage() {
            let _ = conn.execute("DELETE FROM extension_storage WHERE extension_id = ?1 AND key = ?2", params![id, key]);
        }
    }

    fn log(&mut self, message: String) {
        if self.log_lines < MAX_LOG_LINES {
            self.log_lines += 1;
            log::info!("extensão {}: {}", self.extension_id, clean_message(&message));
        }
    }
}

/// Texto de terceiros: sem caracteres de controle e com tamanho limitado.
fn clean_message(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(MAX_MESSAGE_CHARS).collect()
}

// ---------------------------------------------------------------------------
// Erros
// ---------------------------------------------------------------------------

fn load_failed(reason: impl std::fmt::Display) -> AppError {
    log::warn!("extensão: {reason:#}");
    AppError::user("extension_load", "Não foi possível carregar a extensão: ela não é compatível com esta versão do Prancha.")
}

fn crashed(reason: impl std::fmt::Display) -> AppError {
    log::warn!("extensão: {reason:#}");
    AppError::user("extension_crashed", "A extensão falhou ao executar esta operação.")
}

fn from_source(error: SourceError) -> AppError {
    let text = |message: &str, fallback: &str| {
        let message = clean_message(message);
        if message.trim().is_empty() { fallback.to_string() } else { message }
    };
    match error {
        SourceError::Network(m) => AppError::user("source_network", text(&m, "Não foi possível acessar a fonte.")),
        SourceError::Source(m) => AppError::user("source_failed", text(&m, "A fonte não respondeu como esperado.")),
        SourceError::Authentication(m) => AppError::user("source_authentication", text(&m, "A fonte exige um login, e o Prancha não oferece suporte a isso.")),
        SourceError::Data(m) => AppError::user("source_data", text(&m, "A fonte enviou dados que o Prancha não entende.")),
        SourceError::Unsupported(m) => AppError::user("source_unsupported", text(&m, "Esta fonte não oferece essa operação.")),
    }
}

// ---------------------------------------------------------------------------
// Concorrência
// ---------------------------------------------------------------------------

struct Slots {
    used: Mutex<HashMap<String, usize>>,
    freed: Condvar,
}

struct Permit<'a> {
    slots: &'a Slots,
    id: String,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut used = self.slots.used.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(count) = used.get_mut(&self.id) {
            *count -= 1;
            if *count == 0 {
                used.remove(&self.id);
            }
        }
        self.slots.freed.notify_all();
    }
}

impl Slots {
    /// Espera por uma vaga: extensões lentas não ocupam mais que a cota e o cancelamento é respeitado.
    fn acquire(&self, id: &str, limits: &EngineLimits, cancel: &Cancel) -> AppResult<Permit<'_>> {
        let give_up = Instant::now() + limits.call_timeout;
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::user("extension_cancelled", "Operação cancelada."));
            }
            let total: usize = used.values().sum();
            if total < limits.concurrent_calls && used.get(id).copied().unwrap_or(0) < limits.concurrent_per_extension {
                *used.entry(id.to_string()).or_insert(0) += 1;
                return Ok(Permit { slots: self, id: id.to_string() });
            }
            if Instant::now() >= give_up {
                return Err(AppError::user("extension_busy", "A fonte está ocupada. Tente de novo em instantes."));
            }
            used = self.freed.wait_timeout(used, Duration::from_millis(100)).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

/// Avança o epoch só enquanto há chamadas em andamento: com tudo parado, a thread dorme.
#[derive(Default)]
struct Ticker {
    active: Mutex<usize>,
    changed: Condvar,
    started: OnceLock<()>,
}

struct Active(Arc<Ticker>);

impl Drop for Active {
    fn drop(&mut self) {
        *self.0.active.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
    }
}

impl Ticker {
    fn activate(self: &Arc<Self>, engine: &Engine) -> Active {
        self.started.get_or_init(|| {
            let (ticker, engine) = (self.clone(), engine.clone());
            std::thread::spawn(move || {
                loop {
                    let mut active = ticker.active.lock().unwrap_or_else(|e| e.into_inner());
                    while *active == 0 {
                        active = ticker.changed.wait(active).unwrap_or_else(|e| e.into_inner());
                    }
                    drop(active);
                    std::thread::sleep(TICK);
                    engine.increment_epoch();
                }
            });
        });
        *self.active.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.changed.notify_all();
        Active(self.clone())
    }
}

// ---------------------------------------------------------------------------
// Motor
// ---------------------------------------------------------------------------

pub struct SourceEngine {
    engine: Engine,
    linker: Linker<HostState>,
    net: Arc<Net>,
    db_path: PathBuf,
    limits: EngineLimits,
    slots: Slots,
    ticker: Arc<Ticker>,
    /// Compilados por SHA-256. Ficam só na memória, durante a sessão.
    components: Mutex<HashMap<String, Component>>,
}

impl SourceEngine {
    pub fn new(net: Arc<Net>, db_path: PathBuf) -> AppResult<Self> {
        Self::with_limits(net, db_path, EngineLimits::default())
    }

    pub fn with_limits(net: Arc<Net>, db_path: PathBuf, limits: EngineLimits) -> AppResult<Self> {
        let mut config = Config::new();
        config.consume_fuel(true).epoch_interruption(true).max_wasm_stack(MAX_WASM_STACK);
        let engine = Engine::new(&config).map_err(load_failed)?;
        let mut linker = Linker::new(&engine);
        Extension::add_to_linker::<_, HasSelf<HostState>>(&mut linker, |state| state).map_err(load_failed)?;
        Ok(SourceEngine {
            engine,
            linker,
            net,
            db_path,
            limits,
            slots: Slots { used: Mutex::new(HashMap::new()), freed: Condvar::new() },
            ticker: Arc::default(),
            components: Mutex::new(HashMap::new()),
        })
    }

    pub fn allows_loopback(&self) -> bool {
        self.net.allows_loopback()
    }

    fn component(&self, extension: &Installed) -> AppResult<Component> {
        if let Some(component) = self.components.lock().unwrap_or_else(|e| e.into_inner()).get(&extension.sha256) {
            return Ok(component.clone());
        }
        let wasm = std::fs::read(&extension.file).map_err(load_failed)?;
        if sha256_hex(&wasm) != extension.sha256 {
            return Err(AppError::user("extension_load", "O arquivo da extensão foi alterado depois da instalação. Instale-a de novo."));
        }
        // shortcut: a compilação não é cancelável, mas o pacote tem tamanho limitado e vem de um repositório aceito.
        let component = Component::new(&self.engine, &wasm).map_err(load_failed)?;
        self.components.lock().unwrap_or_else(|e| e.into_inner()).insert(extension.sha256.clone(), component.clone());
        Ok(component)
    }

    pub fn forget(&self, sha256: &str) {
        self.components.lock().unwrap_or_else(|e| e.into_inner()).remove(sha256);
    }

    fn run<T>(&self, id: &str, grant: &Grant, component: &Component, cancel: Option<&Cancel>, call: impl FnOnce(&mut Store<HostState>, &Extension) -> wasmtime::Result<T>) -> AppResult<T> {
        let cancel = cancel.cloned().unwrap_or_default();
        let _permit = self.slots.acquire(id, &self.limits, &cancel)?;
        let _active = self.ticker.activate(&self.engine);
        let state = HostState {
            extension_id: id.to_string(),
            grant: grant.clone(),
            limits: self.limits,
            net: self.net.clone(),
            db_path: self.db_path.clone(),
            storage: None,
            cancel: cancel.clone(),
            deadline: Instant::now() + self.limits.call_timeout,
            requests: 0,
            log_lines: 0,
            stop: None,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| state);
        store.set_fuel(self.limits.fuel).map_err(crashed)?;
        store.set_epoch_deadline(1);
        store.epoch_deadline_callback(|mut context| {
            let state = context.data_mut();
            if state.cancel.load(Ordering::Relaxed) {
                state.stop = Some(Stop::Cancelled);
                return Ok(UpdateDeadline::Interrupt);
            }
            if Instant::now() >= state.deadline {
                state.stop = Some(Stop::Timeout);
                return Ok(UpdateDeadline::Interrupt);
            }
            Ok(UpdateDeadline::Continue(1))
        });
        let instance = Extension::instantiate(&mut store, component, &self.linker).map_err(load_failed)?;
        let outcome = call(&mut store, &instance);
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::user("extension_cancelled", "Operação cancelada."));
        }
        outcome.map_err(|error| match store.data().stop {
            Some(Stop::Timeout) => AppError::user("extension_timeout", "A fonte demorou demais para responder."),
            Some(Stop::Memory) => AppError::user("extension_memory", "A extensão passou do limite de memória."),
            Some(Stop::Cancelled) => AppError::user("extension_cancelled", "Operação cancelada."),
            None if error.downcast_ref::<Trap>() == Some(&Trap::OutOfFuel) => AppError::user("extension_fuel", "A extensão passou do limite de processamento."),
            None => crashed(error),
        })
    }

    /// Compila sem guardar e lista as fontes, sem nenhuma capacidade. Usado para conferir um pacote antes de instalar.
    pub fn list_sources(&self, wasm: &[u8]) -> AppResult<Vec<SourceInfo>> {
        let component = Component::new(&self.engine, wasm).map_err(load_failed)?;
        self.run("instalação", &Grant::default(), &component, None, |store, instance| instance.call_list_sources(store))
    }

    fn source_call<T>(&self, extension: &Installed, cancel: Option<&Cancel>, call: impl FnOnce(&mut Store<HostState>, &Extension) -> wasmtime::Result<Result<T, SourceError>>) -> AppResult<T> {
        let component = self.component(extension)?;
        self.run(&extension.id, &extension.grant, &component, cancel, call)?.map_err(from_source)
    }

    pub fn popular(&self, extension: &Installed, cancel: Option<&Cancel>, source: &str, page: u32, filters: &[Filter]) -> AppResult<SearchPage> {
        check_input(&[source], filters)?;
        self.source_call(extension, cancel, |store, instance| instance.call_popular(store, source, page, filters))
    }

    pub fn latest(&self, extension: &Installed, cancel: Option<&Cancel>, source: &str, page: u32, filters: &[Filter]) -> AppResult<SearchPage> {
        check_input(&[source], filters)?;
        self.source_call(extension, cancel, |store, instance| instance.call_latest(store, source, page, filters))
    }

    pub fn search(&self, extension: &Installed, cancel: Option<&Cancel>, source: &str, query: &str, page: u32, filters: &[Filter]) -> AppResult<SearchPage> {
        check_input(&[source], filters)?;
        if query.len() > MAX_QUERY_BYTES {
            return Err(AppError::user("extension_input", "A pesquisa é longa demais."));
        }
        self.source_call(extension, cancel, |store, instance| instance.call_search(store, source, query, page, filters))
    }

    pub fn details(&self, extension: &Installed, cancel: Option<&Cancel>, source: &str, id: &str) -> AppResult<ComicDetails> {
        check_input(&[source, id], &[])?;
        self.source_call(extension, cancel, |store, instance| instance.call_details(store, source, id))
    }

    pub fn chapters(&self, extension: &Installed, cancel: Option<&Cancel>, source: &str, id: &str) -> AppResult<Vec<Chapter>> {
        check_input(&[source, id], &[])?;
        self.source_call(extension, cancel, |store, instance| instance.call_chapters(store, source, id))
    }

    pub fn pages(&self, extension: &Installed, cancel: Option<&Cancel>, source: &str, chapter_id: &str) -> AppResult<Vec<Page>> {
        check_input(&[source, chapter_id], &[])?;
        self.source_call(extension, cancel, |store, instance| instance.call_pages(store, source, chapter_id))
    }
}

fn check_input(ids: &[&str], filters: &[Filter]) -> AppResult<()> {
    let filters_fit = filters.len() <= MAX_FILTERS && filters.iter().all(|f| f.name.len() <= MAX_ID_BYTES && f.value.len() <= MAX_ID_BYTES);
    if ids.iter().any(|id| id.len() > MAX_ID_BYTES) || !filters_fit {
        return Err(AppError::user("extension_input", "Os dados enviados à fonte passam do limite."));
    }
    Ok(())
}
