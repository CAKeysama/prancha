//! Telemetria anônima e voluntária. Desligada até a pessoa aceitar; nada é gravado nem enviado antes disso.
//!
//! O que entra são contadores agregados (aberturas, uso de fontes por identificador técnico, códigos de erro,
//! tempos), somados em memória por dia e enviados em segundo plano a cada poucos minutos para a função da
//! Vercel (`site/api/telemetry/ingest.mjs`). Nada daqui bloqueia a interface, a leitura ou a abertura do app:
//! sem rede, sem servidor ou com o Supabase fora do ar, os eventos esperam a próxima vez ou são descartados.
//! Ver docs/TELEMETRY.md.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Value, json};

use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::extensions::net::{Cancel, NetError, wait};
use crate::library;

pub const CONSENT_KEY: &str = "telemetry.consent";
const ID_KEY: &str = "telemetry.installId";
const PENDING_KEY: &str = "telemetry.pending";
const RUNNING_KEY: &str = "telemetry.running";
const FORGET_KEY: &str = "telemetry.forgetId";

/// Endereço oficial. Builds de depuração só enviam com `PRANCHA_TELEMETRY_URL`, para testes locais.
pub const OFFICIAL_ENDPOINT: &str = "https://prancha-beta.vercel.app/api/telemetry";

/// Primeiro envio um pouco depois de abrir, para não disputar a rede com a inicialização.
const FIRST_FLUSH: Duration = Duration::from_secs(90);
const FLUSH_EVERY: Duration = Duration::from_secs(10 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(6 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Combinações diferentes guardadas ao mesmo tempo. Passou disso, o resto é descartado até o próximo envio.
const MAX_KEYS: usize = 400;
const MAX_EVENTS_PER_BATCH: usize = 200;
/// Lotes que falharam e esperam outra tentativa, com o mesmo identificador (o servidor ignora repetidos).
const MAX_OUTBOX: usize = 4;
const MAX_COUNT: u64 = 100_000;
const MAX_MS: u64 = 3_600_000;

pub const ERROR_CATEGORIES: &[&str] = &["network", "source", "extension", "reader", "library", "import", "database", "update", "interface", "crash", "other"];
pub const METRICS: &[&str] = &["startup", "library_load", "explore_results", "series_open", "chapter_open", "reader_first_page"];

// ---------------------------------------------------------------------------
// Sanitização
// ---------------------------------------------------------------------------

/// Um identificador técnico: minúsculas, dígitos e `._:-`, sem nada que pareça caminho, endereço, e-mail ou segredo.
/// Qualquer outra coisa vira `None` e o evento é descartado, em vez de ser "limpo" e mandar um pedaço do original.
pub fn token(raw: &str, max: usize) -> Option<String> {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() || value.len() > max || !value.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    if !value.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | ':' | '-')) {
        return None;
    }
    // Uma sequência longa de letras e dígitos sem separador é mais provável um token ou hash que um código.
    if value.split(|c: char| matches!(c, '.' | '_' | ':' | '-')).any(|part| part.len() > 32) {
        return None;
    }
    Some(value)
}

/// Fonte como `extensão/fonte`, os dois identificadores técnicos que vêm do repositório. Nunca o nome, o site ou a obra.
pub fn source_key(extension_id: &str, source_id: &str) -> Option<String> {
    let clean = |raw: &str| -> Option<String> {
        let value: String = raw.trim().to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect();
        let value = value.trim_matches('-').to_string();
        (!value.is_empty() && value.len() <= 64 && value.starts_with(|c: char| c.is_ascii_alphanumeric())).then_some(value)
    };
    Some(format!("{}/{}", clean(extension_id)?, clean(source_id)?))
}

/// Dia UTC (`AAAA-MM-DD`) e hora de um instante em milissegundos, sem depender de biblioteca de datas.
pub fn utc_day_hour(ms: i64) -> (String, u8) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let hour = (secs.rem_euclid(86_400) / 3600) as u8;
    // Howard Hinnant, "civil_from_days".
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (format!("{y:04}-{m:02}-{d:02}"), hour)
}

fn new_uuid() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("gerador aleatório do sistema");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() && !c.is_ascii_uppercase() })
}

// ---------------------------------------------------------------------------
// Eventos agregados
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Session { day: String },
    Source { day: String, source: String },
    Error { day: String, hour: u8, category: String, component: String, code: String, status: u16 },
    Perf { day: String, metric: String },
}

/// Somas de uma chave. Sessão: aberturas, saídas inesperadas. Fonte: consultas, obras, capítulos, erros.
/// Erro: ocorrências. Desempenho: amostras, soma e máximo em ms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Agg([u64; 4]);

impl Agg {
    fn merge(&mut self, key: &Key, other: Agg) {
        let max_index = if matches!(key, Key::Perf { .. }) { 2 } else { 4 };
        for i in 0..4 {
            self.0[i] = if i == max_index { self.0[i].max(other.0[i]) } else { (self.0[i] + other.0[i]).min(if matches!(key, Key::Perf { .. }) && i == 1 { MAX_MS * MAX_COUNT } else { MAX_COUNT }) };
        }
    }
}

fn to_event(key: &Key, agg: &Agg) -> Value {
    let [a, b, c, d] = agg.0;
    match key {
        Key::Session { day } => json!({ "type": "session", "day": day, "launches": a, "uncleanExits": b }),
        Key::Source { day, source } => json!({ "type": "source", "day": day, "source": source, "queries": a, "opens": b, "chapters": c, "errors": d }),
        Key::Error { day, hour, category, component, code, status } => {
            json!({ "type": "error", "day": day, "hour": hour, "category": category, "component": component, "code": code, "httpStatus": status, "count": a })
        }
        Key::Perf { day, metric } => json!({ "type": "perf", "day": day, "metric": metric, "count": a, "totalMs": b, "maxMs": c }),
    }
}

/// O caminho de volta: lê um evento (guardado no disco ou vindo da interface) e recusa o que não for exatamente o esperado.
fn from_event(value: &Value, allow: &[&str]) -> Option<(Key, Agg)> {
    let obj = value.as_object()?;
    let kind = obj.get("type")?.as_str()?;
    if !allow.contains(&kind) {
        return None;
    }
    let day = obj.get("day")?.as_str()?;
    if day.len() != 10 || !day.bytes().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == b'-' } else { c.is_ascii_digit() }) {
        return None;
    }
    let day = day.to_string();
    let num = |name: &str, max: u64| -> Option<u64> {
        match obj.get(name) {
            None => Some(0),
            Some(v) => v.as_u64().filter(|n| *n <= max),
        }
    };
    let allowed_keys: &[&str] = match kind {
        "session" => &["type", "day", "launches", "uncleanExits"],
        "source" => &["type", "day", "source", "queries", "opens", "chapters", "errors"],
        "error" => &["type", "day", "hour", "category", "component", "code", "httpStatus", "count"],
        "perf" => &["type", "day", "metric", "count", "totalMs", "maxMs"],
        _ => return None,
    };
    if obj.keys().any(|k| !allowed_keys.contains(&k.as_str())) {
        return None;
    }
    match kind {
        "session" => Some((Key::Session { day }, Agg([num("launches", 1000)?, num("uncleanExits", 1000)?, 0, 0]))),
        "source" => {
            let source = obj.get("source")?.as_str()?;
            let (ext, src) = source.split_once('/')?;
            let source = source_key(ext, src).filter(|s| s == source)?;
            Some((Key::Source { day, source }, Agg([num("queries", MAX_COUNT)?, num("opens", MAX_COUNT)?, num("chapters", MAX_COUNT)?, num("errors", MAX_COUNT)?])))
        }
        "error" => {
            let category = obj.get("category")?.as_str()?;
            if !ERROR_CATEGORIES.contains(&category) {
                return None;
            }
            let hour = u8::try_from(num("hour", 23)?).ok()?;
            let count = num("count", MAX_COUNT)?.max(1);
            Some((
                Key::Error {
                    day,
                    hour,
                    category: category.to_string(),
                    component: token(obj.get("component")?.as_str()?, 40)?,
                    code: token(obj.get("code")?.as_str()?, 64)?,
                    status: u16::try_from(num("httpStatus", 599)?).ok()?,
                },
                Agg([count, 0, 0, 0]),
            ))
        }
        "perf" => {
            let metric = obj.get("metric")?.as_str()?;
            if !METRICS.contains(&metric) {
                return None;
            }
            let count = num("count", MAX_COUNT)?;
            if count == 0 {
                return None;
            }
            Some((Key::Perf { day, metric: metric.to_string() }, Agg([count, num("totalMs", MAX_MS * MAX_COUNT)?, num("maxMs", MAX_MS)?, 0])))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Estado
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Consent {
    Unset,
    Granted,
    Denied,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub consent: Consent,
    pub decided_at: Option<i64>,
    pub install_id: Option<String>,
    pub pending_events: usize,
    pub last_sent_at: Option<i64>,
    /// Há um endereço para enviar (builds oficiais). Sem ele, nada sai do computador.
    pub endpoint: bool,
    /// Um pedido de exclusão não chegou ao servidor e pode ser repetido.
    pub forget_pending: bool,
}

/// Versão, sistema e canal, preenchidos pelo app Tauri ao abrir.
#[derive(Clone, Debug)]
pub struct AppMeta {
    pub version: String,
    pub platform: &'static str,
    pub arch: Option<&'static str>,
    pub channel: &'static str,
}

impl AppMeta {
    pub fn current(version: String, channel: &'static str) -> Self {
        let platform = match std::env::consts::OS {
            "windows" => "windows",
            "macos" => "macos",
            _ => "linux",
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => Some("x86_64"),
            "aarch64" => Some("aarch64"),
            "x86" => Some("x86"),
            "arm" => Some("arm"),
            _ => None,
        };
        // Pré-lançamentos não passam na validação do servidor: vão como a versão base.
        let version = version.split(['-', '+']).next().unwrap_or_default().to_string();
        AppMeta { version, platform, arch, channel }
    }
}

/// Envio pela rede, trocado por um falso nos testes.
pub trait Transport: Send + Sync {
    /// Devolve o código HTTP. `cancel` interrompe a requisição em andamento.
    fn post(&self, url: &str, body: String, cancel: &Cancel) -> Result<u16, NetError>;
}

struct HttpTransport {
    client: OnceLock<Result<reqwest::Client, String>>,
}

impl Transport for HttpTransport {
    fn post(&self, url: &str, body: String, cancel: &Cancel) -> Result<u16, NetError> {
        let client = self
            .client
            .get_or_init(|| {
                let _ = rustls::crypto::ring::default_provider().install_default();
                reqwest::Client::builder()
                    .user_agent(concat!("Prancha/", env!("CARGO_PKG_VERSION")))
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(REQUEST_TIMEOUT)
                    .build()
                    .map_err(|e| e.to_string())
            })
            .clone()
            .map_err(NetError::Network)?;
        let request = client.post(url).header(reqwest::header::CONTENT_TYPE, "application/json").body(body);
        wait(
            async move {
                let response = request.send().await.map_err(|e| if e.is_timeout() { NetError::Timeout } else { NetError::Network("falha de rede".into()) })?;
                Ok(response.status().as_u16())
            },
            Some(cancel),
            Instant::now() + REQUEST_TIMEOUT,
        )
    }
}

struct Outgoing {
    body: String,
    events: usize,
}

#[derive(Default)]
struct Inner {
    enabled: bool,
    install_id: Option<String>,
    pending: BTreeMap<Key, Agg>,
    outbox: VecDeque<Outgoing>,
    /// Muda a cada revogação ou troca de identificador: um envio que já tinha começado não grava o resultado.
    generation: u64,
    cancel: Cancel,
    last_sent_at: Option<i64>,
    failures: u32,
    dropped: u64,
    stop: bool,
    flush_now: bool,
}

pub struct Telemetry {
    inner: Mutex<Inner>,
    wake: Condvar,
    meta: OnceLock<AppMeta>,
    endpoint: Option<String>,
    transport: Box<dyn Transport>,
    started: Instant,
    startup_recorded: AtomicBool,
}

impl Telemetry {
    pub fn new(endpoint: Option<String>, transport: Box<dyn Transport>) -> Self {
        Telemetry { inner: Mutex::new(Inner::default()), wake: Condvar::new(), meta: OnceLock::new(), endpoint, transport, started: Instant::now(), startup_recorded: AtomicBool::new(false) }
    }

    /// O endereço oficial nos builds de distribuição; `PRANCHA_TELEMETRY_URL` só em depuração.
    pub fn from_env() -> Self {
        let endpoint = if cfg!(debug_assertions) {
            std::env::var("PRANCHA_TELEMETRY_URL").ok().filter(|u| !u.is_empty())
        } else {
            Some(OFFICIAL_ENDPOINT.to_string())
        };
        Telemetry::new(endpoint, Box::new(HttpTransport { client: OnceLock::new() }))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn enabled(&self) -> bool {
        self.lock().enabled
    }

    /// Lê o consentimento e o que sobrou da última sessão. Chamado ao abrir, antes de qualquer registro.
    pub fn load(&self, conn: &Connection) -> AppResult<()> {
        let granted = consent_of(conn)?.0 == Consent::Granted;
        let id = library::get_setting(conn, ID_KEY)?.and_then(|v| v.as_str().map(str::to_string)).filter(|id| is_uuid(id));
        let mut inner = self.lock();
        inner.enabled = granted && id.is_some();
        inner.install_id = if inner.enabled { id } else { None };
        if inner.enabled {
            if let Some(Value::Array(events)) = library::get_setting(conn, PENDING_KEY)? {
                for event in &events {
                    if let Some((key, agg)) = from_event(event, &["session", "source", "error", "perf"]) {
                        add(&mut inner, key, agg);
                    }
                }
            }
        }
        drop(inner);
        delete_setting(conn, PENDING_KEY)?;
        Ok(())
    }

    /// Ao abrir: conta a abertura e, se a anterior não fechou direito, a saída inesperada.
    pub fn on_launch(&self, conn: &Connection, meta: AppMeta) -> AppResult<()> {
        let _ = self.meta.set(meta);
        if !self.enabled() {
            delete_setting(conn, RUNNING_KEY)?;
            return Ok(());
        }
        let unclean = library::get_setting(conn, RUNNING_KEY)?.and_then(|v| v.as_bool()).unwrap_or(false);
        let (day, hour) = utc_day_hour(now_ms());
        self.add(Key::Session { day: day.clone() }, Agg([1, u64::from(unclean), 0, 0]));
        if unclean {
            self.add(Key::Error { day, hour, category: "crash".into(), component: "app".into(), code: "unclean_exit".into(), status: 0 }, Agg([1, 0, 0, 0]));
        }
        library::set_setting(conn, RUNNING_KEY, &json!(true))
    }

    /// Ao fechar normalmente: guarda o que não foi enviado (só no disco, sem rede) e marca a saída limpa.
    pub fn on_exit(&self, conn: &Connection) -> AppResult<()> {
        let mut inner = self.lock();
        inner.stop = true;
        inner.cancel.store(true, Ordering::Relaxed);
        let events: Vec<Value> = if inner.enabled { inner.pending.iter().map(|(k, a)| to_event(k, a)).collect() } else { Vec::new() };
        drop(inner);
        self.wake.notify_all();
        if !events.is_empty() {
            library::set_setting(conn, PENDING_KEY, &Value::Array(events))?;
        }
        delete_setting(conn, RUNNING_KEY)
    }

    fn add(&self, key: Key, agg: Agg) {
        let mut inner = self.lock();
        if inner.enabled {
            add(&mut inner, key, agg);
        }
    }

    pub fn status(&self, conn: &Connection) -> AppResult<Status> {
        let (consent, decided_at) = consent_of(conn)?;
        let forget_pending = library::get_setting(conn, FORGET_KEY)?.is_some();
        let inner = self.lock();
        Ok(Status {
            consent,
            decided_at,
            install_id: inner.install_id.clone(),
            pending_events: inner.pending.len() + inner.outbox.iter().map(|o| o.events).sum::<usize>(),
            last_sent_at: inner.last_sent_at,
            endpoint: self.endpoint.is_some(),
            forget_pending,
        })
    }

    /// Aceitar cria um identificador novo; recusar ou revogar para os envios na hora, descarta o que estava
    /// esperando (inclusive um envio já começado) e apaga o identificador deste computador.
    pub fn set_consent(&self, conn: &Connection, granted: bool) -> AppResult<Status> {
        library::set_setting(conn, CONSENT_KEY, &json!({ "state": if granted { "granted" } else { "denied" }, "at": now_ms() }))?;
        if granted {
            let id = library::get_setting(conn, ID_KEY)?.and_then(|v| v.as_str().map(str::to_string)).filter(|id| is_uuid(id)).unwrap_or_else(new_uuid);
            library::set_setting(conn, ID_KEY, &json!(id))?;
            library::set_setting(conn, RUNNING_KEY, &json!(true))?;
            let mut inner = self.lock();
            inner.enabled = true;
            inner.install_id = Some(id);
        } else {
            self.discard(conn)?;
        }
        self.status(conn)
    }

    fn discard(&self, conn: &Connection) -> AppResult<Option<String>> {
        let old = {
            let mut inner = self.lock();
            inner.enabled = false;
            inner.pending.clear();
            inner.outbox.clear();
            inner.generation += 1;
            inner.cancel.store(true, Ordering::Relaxed);
            inner.cancel = Cancel::default();
            inner.failures = 0;
            inner.install_id.take()
        };
        delete_setting(conn, ID_KEY)?;
        delete_setting(conn, PENDING_KEY)?;
        delete_setting(conn, RUNNING_KEY)?;
        Ok(old)
    }

    /// Troca o identificador. O que estava esperando é descartado, para não ligar o identificador antigo ao novo.
    pub fn reset_id(&self, conn: &Connection) -> AppResult<Status> {
        if !self.enabled() {
            return Err(AppError::user("telemetry_disabled", "A telemetria está desligada."));
        }
        let id = new_uuid();
        library::set_setting(conn, ID_KEY, &json!(id))?;
        {
            let mut inner = self.lock();
            inner.pending.clear();
            inner.outbox.clear();
            inner.generation += 1;
            inner.cancel.store(true, Ordering::Relaxed);
            inner.cancel = Cancel::default();
            inner.install_id = Some(id);
        }
        self.status(conn)
    }

    /// Parte local de `forget`: revoga e devolve o identificador que precisa ser apagado no servidor.
    pub fn begin_forget(&self, conn: &Connection) -> AppResult<Option<String>> {
        let stored = library::get_setting(conn, FORGET_KEY)?.and_then(|v| v.as_str().map(str::to_string)).filter(|id| is_uuid(id));
        let current = self.discard(conn)?;
        library::set_setting(conn, CONSENT_KEY, &json!({ "state": "denied", "at": now_ms() }))?;
        let id = current.or(stored);
        if let Some(id) = &id {
            library::set_setting(conn, FORGET_KEY, &json!(id))?;
        }
        Ok(id)
    }

    /// Envia o pedido de exclusão. Bloqueia a thread que chamou (os comandos rodam fora da principal).
    pub fn send_forget(&self, id: &str) -> AppResult<()> {
        let Some(base) = &self.endpoint else {
            return Err(AppError::user("telemetry_offline", "Este build do Prancha não envia telemetria, então não há dados no servidor."));
        };
        let body = json!({ "installId": id }).to_string();
        match self.transport.post(&format!("{base}/forget"), body, &Cancel::default()) {
            Ok(200) => Ok(()),
            Ok(503) => Err(AppError::user("telemetry_unavailable", "O servidor da telemetria não está disponível agora. Tente de novo mais tarde.")),
            Ok(status) => Err(AppError::user("telemetry_unavailable", format!("O servidor recusou o pedido (HTTP {status}). Tente de novo mais tarde."))),
            Err(_) => Err(AppError::user("telemetry_unavailable", "Sem conexão com o servidor da telemetria. Tente de novo mais tarde.")),
        }
    }

    pub fn finish_forget(&self, conn: &Connection) -> AppResult<()> {
        delete_setting(conn, FORGET_KEY)
    }

    // -- Registros. Todos voltam na hora quando a telemetria está desligada. ---------------------------------

    pub fn source_query(&self, extension_id: &str, source_id: &str, error: Option<&AppError>) {
        self.source_event(extension_id, source_id, [1, 0, 0, 0], error);
    }

    pub fn source_open(&self, extension_id: &str, source_id: &str, failed: bool) {
        self.source_event(extension_id, source_id, [0, 1, 0, u64::from(failed)], None);
    }

    pub fn source_chapter(&self, extension_id: &str, source_id: &str, error: Option<&AppError>) {
        self.source_event(extension_id, source_id, [0, 0, 1, 0], error);
    }

    fn source_event(&self, extension_id: &str, source_id: &str, mut counts: [u64; 4], error: Option<&AppError>) {
        if !self.enabled() {
            return;
        }
        if let Some(e) = error {
            // Cancelar uma busca ao trocar de tela não é falha.
            if is_cancel(e) {
                return;
            }
            counts[3] += 1;
            self.error("source", "sources", e.code(), 0);
        }
        let Some(source) = source_key(extension_id, source_id) else { return };
        let (day, _) = utc_day_hour(now_ms());
        self.add(Key::Source { day, source }, Agg(counts));
    }

    /// Uma falha, só por categoria, componente e código técnico.
    pub fn error(&self, category: &str, component: &str, code: &str, http_status: u16) {
        if !self.enabled() || !ERROR_CATEGORIES.contains(&category) {
            return;
        }
        let (Some(component), Some(code)) = (token(component, 40), token(code, 64)) else { return };
        let (day, hour) = utc_day_hour(now_ms());
        self.add(Key::Error { day, hour, category: category.into(), component, code, status: http_status.min(599) }, Agg([1, 0, 0, 0]));
    }

    pub fn timing(&self, metric: &str, ms: u64) {
        if !self.enabled() || !METRICS.contains(&metric) {
            return;
        }
        let ms = ms.min(MAX_MS);
        let (day, _) = utc_day_hour(now_ms());
        self.add(Key::Perf { day, metric: metric.into() }, Agg([1, ms, ms, 0]));
    }

    /// A interface terminou de desenhar a primeira tela: é o tempo de inicialização. Só conta uma vez.
    pub fn ready(&self) {
        if !self.startup_recorded.swap(true, Ordering::Relaxed) {
            self.timing("startup", self.started.elapsed().as_millis() as u64);
        }
    }

    /// Eventos vindos da interface: só falhas e tempos, conferidos como qualquer outro.
    pub fn record_from_ui(&self, events: &[Value]) -> usize {
        if !self.enabled() {
            return 0;
        }
        let mut accepted = 0;
        for event in events.iter().take(50) {
            if let Some((key, agg)) = from_event(event, &["error", "perf"]) {
                if matches!(&key, Key::Perf { metric, .. } if metric == "startup") {
                    continue;
                }
                self.add(key, agg);
                accepted += 1;
            }
        }
        accepted
    }

    // -- Envio ------------------------------------------------------------------------------------------------

    /// Monta os lotes com o que está esperando. Devolve o que mandar e a geração em que foi montado.
    fn prepare(&self) -> Option<(u64, Cancel, Vec<String>)> {
        let mut inner = self.lock();
        let meta = self.meta.get()?;
        if !inner.enabled || self.endpoint.is_none() {
            return None;
        }
        let install_id = inner.install_id.clone()?;
        let pending = std::mem::take(&mut inner.pending);
        let events: Vec<Value> = pending.iter().map(|(k, a)| to_event(k, a)).collect();
        for chunk in events.chunks(MAX_EVENTS_PER_BATCH) {
            let body = json!({
                "schema": 1,
                "batchId": new_uuid(),
                "installId": install_id,
                "app": { "version": meta.version, "platform": meta.platform, "arch": meta.arch, "channel": meta.channel },
                "events": chunk,
            });
            inner.outbox.push_back(Outgoing { body: body.to_string(), events: chunk.len() });
        }
        while inner.outbox.len() > MAX_OUTBOX {
            if let Some(old) = inner.outbox.pop_front() {
                inner.dropped += old.events as u64;
            }
        }
        if inner.outbox.is_empty() {
            return None;
        }
        Some((inner.generation, inner.cancel.clone(), inner.outbox.iter().map(|o| o.body.clone()).collect()))
    }

    /// Um ciclo de envio. Devolve se deu certo, para o laço decidir a espera até o próximo.
    pub fn flush(&self) -> bool {
        let Some((generation, cancel, bodies)) = self.prepare() else { return true };
        let url = format!("{}/ingest", self.endpoint.as_deref().unwrap_or_default());
        for body in bodies {
            let outcome = self.transport.post(&url, body, &cancel);
            let mut inner = self.lock();
            if inner.generation != generation || !inner.enabled {
                return true;
            }
            match outcome {
                // Aceito, ou recusado de um jeito que não muda tentando de novo: sai da fila.
                Ok(200) | Ok(400) | Ok(413) | Ok(415) => {
                    if outcome == Ok(200) {
                        inner.last_sent_at = Some(now_ms());
                    }
                    inner.outbox.pop_front();
                    inner.failures = 0;
                }
                _ => {
                    inner.failures = inner.failures.saturating_add(1);
                    return false;
                }
            }
        }
        true
    }

    fn next_wait(&self, ok: bool) -> Duration {
        if ok {
            return FLUSH_EVERY;
        }
        let failures = self.lock().failures.min(6);
        (FLUSH_EVERY * 2u32.pow(failures)).min(MAX_BACKOFF)
    }

    /// Envia pedindo já, por exemplo logo depois de aceitar (para a pessoa ver que funciona).
    pub fn request_flush(&self) {
        self.lock().flush_now = true;
        self.wake.notify_all();
    }

    /// Thread de envio. Dorme quase sempre; acorda no intervalo, num pedido ou ao fechar.
    pub fn spawn(self: &Arc<Self>) {
        let this = self.clone();
        std::thread::Builder::new()
            .name("telemetria".into())
            .spawn(move || {
                let mut wait_for = FIRST_FLUSH;
                loop {
                    {
                        let inner = this.lock();
                        let (mut inner, _) = this.wake.wait_timeout_while(inner, wait_for, |i| !i.stop && !i.flush_now).unwrap_or_else(|e| e.into_inner());
                        if inner.stop {
                            return;
                        }
                        inner.flush_now = false;
                    }
                    let ok = this.flush();
                    wait_for = this.next_wait(ok);
                }
            })
            .expect("thread da telemetria");
    }

    #[cfg(test)]
    fn pending_events(&self) -> Vec<Value> {
        self.lock().pending.iter().map(|(k, a)| to_event(k, a)).collect()
    }
}

fn add(inner: &mut Inner, key: Key, agg: Agg) {
    if let Some(existing) = inner.pending.get_mut(&key) {
        existing.merge(&key, agg);
    } else if inner.pending.len() < MAX_KEYS {
        inner.pending.insert(key, agg);
    } else {
        inner.dropped += 1;
    }
}

fn consent_of(conn: &Connection) -> AppResult<(Consent, Option<i64>)> {
    let value = library::get_setting(conn, CONSENT_KEY)?;
    let state = value.as_ref().and_then(|v| v.get("state")).and_then(Value::as_str);
    let at = value.as_ref().and_then(|v| v.get("at")).and_then(Value::as_i64);
    Ok(match state {
        Some("granted") => (Consent::Granted, at),
        Some("denied") => (Consent::Denied, at),
        _ => (Consent::Unset, None),
    })
}

fn delete_setting(conn: &Connection, key: &str) -> AppResult<()> {
    conn.execute("DELETE FROM settings WHERE key = ?", [key])?;
    Ok(())
}

/// Cancelamentos (trocar de tela no meio de uma busca) não contam como falha.
pub fn is_cancel(error: &AppError) -> bool {
    matches!(error.code(), "cancelled" | "extension_cancelled")
}

/// Chaves que só este módulo grava: `set_setting` recusa, para a interface não ligar a telemetria por fora do consentimento.
pub fn is_reserved(key: &str) -> bool {
    key.starts_with("telemetry.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Fake {
        sent: Mutex<Vec<(String, Value)>>,
        status: Mutex<Vec<Result<u16, NetError>>>,
        calls: AtomicUsize,
    }

    impl Transport for Arc<Fake> {
        fn post(&self, url: &str, body: String, _cancel: &Cancel) -> Result<u16, NetError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.sent.lock().unwrap().push((url.to_string(), serde_json::from_str(&body).unwrap()));
            self.status.lock().unwrap().pop().unwrap_or(Ok(200))
        }
    }

    fn setup() -> (Telemetry, Arc<Fake>, Connection) {
        let fake = Arc::new(Fake::default());
        let t = Telemetry::new(Some("https://example.test/api/telemetry".into()), Box::new(fake.clone()));
        let conn = crate::db::open_in_memory().unwrap();
        (t, fake, conn)
    }

    fn meta() -> AppMeta {
        AppMeta::current("0.9.0".into(), "nsis")
    }

    #[test]
    fn sem_consentimento_nada_e_registrado_nem_enviado() {
        let (t, fake, conn) = setup();
        t.load(&conn).unwrap();
        t.on_launch(&conn, meta()).unwrap();
        t.source_query("ext", "fonte", None);
        t.error("network", "sources", "timeout", 0);
        t.timing("explore_results", 300);
        t.ready();
        assert_eq!(t.record_from_ui(&[json!({ "type": "perf", "day": "2026-11-09", "metric": "series_open", "count": 1, "totalMs": 5, "maxMs": 5 })]), 0);
        assert!(t.pending_events().is_empty());
        assert!(t.flush());
        assert_eq!(fake.calls.load(Ordering::Relaxed), 0);
        assert_eq!(t.status(&conn).unwrap().consent, Consent::Unset);
        assert!(library::get_setting(&conn, ID_KEY).unwrap().is_none());
        assert!(library::get_setting(&conn, RUNNING_KEY).unwrap().is_none());
    }

    #[test]
    fn com_consentimento_os_eventos_saem_agregados() {
        let (t, fake, conn) = setup();
        t.load(&conn).unwrap();
        let status = t.set_consent(&conn, true).unwrap();
        let id = status.install_id.clone().unwrap();
        assert!(is_uuid(&id));
        t.on_launch(&conn, meta()).unwrap();
        t.source_query("Ext.Uma", "Fonte 1", None);
        t.source_query("Ext.Uma", "Fonte 1", None);
        t.source_open("Ext.Uma", "Fonte 1", false);
        t.timing("explore_results", 200);
        t.timing("explore_results", 400);
        assert!(t.flush());
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let (url, batch) = &sent[0];
        assert_eq!(url, "https://example.test/api/telemetry/ingest");
        assert_eq!(batch["installId"], json!(id));
        assert_eq!(batch["app"]["version"], "0.9.0");
        let events = batch["events"].as_array().unwrap();
        let source = events.iter().find(|e| e["type"] == "source").unwrap();
        assert_eq!(source["source"], "ext.uma/fonte-1");
        assert_eq!(source["queries"], 2);
        assert_eq!(source["opens"], 1);
        let perf = events.iter().find(|e| e["type"] == "perf").unwrap();
        assert_eq!((perf["count"].as_u64(), perf["totalMs"].as_u64(), perf["maxMs"].as_u64()), (Some(2), Some(600), Some(400)));
        drop(sent);
        assert!(t.status(&conn).unwrap().last_sent_at.is_some());
        assert!(t.pending_events().is_empty());
    }

    #[test]
    fn revogar_para_na_hora_e_apaga_o_que_esperava() {
        let (t, fake, conn) = setup();
        t.load(&conn).unwrap();
        t.set_consent(&conn, true).unwrap();
        t.on_launch(&conn, meta()).unwrap();
        t.error("network", "sources", "timeout", 0);
        let status = t.set_consent(&conn, false).unwrap();
        assert_eq!(status.consent, Consent::Denied);
        assert_eq!(status.install_id, None);
        assert_eq!(status.pending_events, 0);
        t.error("network", "sources", "timeout", 0);
        assert!(t.flush());
        assert_eq!(fake.calls.load(Ordering::Relaxed), 0);
        assert!(library::get_setting(&conn, ID_KEY).unwrap().is_none());
        // Não volta sozinha ao abrir de novo.
        let again = Telemetry::new(Some("https://example.test".into()), Box::new(fake.clone()));
        again.load(&conn).unwrap();
        assert!(!again.enabled());
    }

    #[test]
    fn revogar_durante_um_envio_descarta_o_resultado() {
        let fake = Arc::new(Fake::default());
        let t = Arc::new(Telemetry::new(Some("https://example.test".into()), Box::new(fake.clone())));
        let conn = crate::db::open_in_memory().unwrap();
        t.load(&conn).unwrap();
        t.set_consent(&conn, true).unwrap();
        t.on_launch(&conn, meta()).unwrap();
        // Monta o lote, revoga e só então o envio "termina" com falha: nada volta para a fila.
        fake.status.lock().unwrap().push(Err(NetError::Timeout));
        let prepared = t.prepare().unwrap();
        t.set_consent(&conn, false).unwrap();
        assert!(prepared.1.load(Ordering::Relaxed), "o envio em andamento foi cancelado");
        assert_eq!(t.status(&conn).unwrap().pending_events, 0);
    }

    #[test]
    fn falha_de_rede_guarda_o_lote_e_repete_o_mesmo_identificador() {
        let (t, fake, conn) = setup();
        t.load(&conn).unwrap();
        t.set_consent(&conn, true).unwrap();
        t.on_launch(&conn, meta()).unwrap();
        fake.status.lock().unwrap().push(Err(NetError::Network("x".into())));
        assert!(!t.flush());
        assert!(t.next_wait(false) > FLUSH_EVERY);
        assert!(t.flush());
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].1["batchId"], sent[1].1["batchId"]);
        assert_eq!(t.lock().outbox.len(), 0);
    }

    #[test]
    fn saida_inesperada_conta_na_proxima_abertura() {
        let (t, fake, conn) = setup();
        t.load(&conn).unwrap();
        t.set_consent(&conn, true).unwrap();
        t.on_launch(&conn, meta()).unwrap();
        // O app "caiu": não houve on_exit. A próxima abertura acha a marca.
        let next = Telemetry::new(Some("https://example.test".into()), Box::new(fake.clone()));
        next.load(&conn).unwrap();
        next.on_launch(&conn, meta()).unwrap();
        let events = next.pending_events();
        assert!(events.iter().any(|e| e["type"] == "session" && e["uncleanExits"] == 1));
        assert!(events.iter().any(|e| e["type"] == "error" && e["code"] == "unclean_exit"));
        // Fechando direito, o que não foi enviado fica no disco e a marca some.
        next.on_exit(&conn).unwrap();
        assert!(library::get_setting(&conn, RUNNING_KEY).unwrap().is_none());
        let third = Telemetry::new(Some("https://example.test".into()), Box::new(fake.clone()));
        third.load(&conn).unwrap();
        third.on_launch(&conn, meta()).unwrap();
        assert!(third.pending_events().iter().any(|e| e["type"] == "session" && e["launches"] == 2));
    }

    #[test]
    fn sanitizacao_recusa_dados_pessoais() {
        assert_eq!(token("Timeout", 64).as_deref(), Some("timeout"));
        assert_eq!(token("extension_load", 64).as_deref(), Some("extension_load"));
        for bad in ["C:\\Users\\gustavo\\HQs\\x.cbz", "/home/gustavo/x", "gustavo@example.com", "https://site.com/?token=abc", "Bearer abc", "senha 123", "", "eyJhbGciOiJIUzI1NiJ9eyJzdWIiOiIxMjM0NTY3ODkwIn0", "-x"] {
            assert_eq!(token(bad, 64), None, "{bad}");
        }
        assert_eq!(source_key("prancha.demonstracao", "Acervo Principal").as_deref(), Some("prancha.demonstracao/acervo-principal"));
        assert_eq!(source_key("", "x"), None);
        // Da interface só entram falhas e tempos, com os campos conhecidos.
        let (t, _fake, conn) = setup();
        t.load(&conn).unwrap();
        t.set_consent(&conn, true).unwrap();
        let day = utc_day_hour(now_ms()).0;
        let accepted = t.record_from_ui(&[
            json!({ "type": "error", "day": day, "hour": 3, "category": "reader", "component": "pdf", "code": "render_failed", "count": 1 }),
            json!({ "type": "error", "day": day, "hour": 3, "category": "reader", "component": "pdf", "code": "Falha ao abrir C:\\Users\\a.pdf", "count": 1 }),
            json!({ "type": "error", "day": day, "hour": 3, "category": "reader", "component": "pdf", "code": "x", "count": 1, "message": "título da obra" }),
            json!({ "type": "source", "day": day, "source": "a/b", "queries": 1 }),
            json!({ "type": "session", "day": day, "launches": 5 }),
            json!({ "type": "perf", "day": day, "metric": "startup", "count": 1, "totalMs": 1, "maxMs": 1 }),
        ]);
        assert_eq!(accepted, 1);
        let text = serde_json::to_string(&t.pending_events()).unwrap();
        assert!(!text.contains("Users") && !text.contains("título"));
    }

    #[test]
    fn trocar_o_identificador_descarta_o_que_esperava() {
        let (t, _fake, conn) = setup();
        t.load(&conn).unwrap();
        let first = t.set_consent(&conn, true).unwrap().install_id.unwrap();
        t.error("network", "sources", "timeout", 0);
        let second = t.reset_id(&conn).unwrap();
        assert_ne!(second.install_id.as_deref(), Some(first.as_str()));
        assert_eq!(second.pending_events, 0);
    }

    #[test]
    fn apagar_os_dados_envia_o_identificador_e_revoga() {
        let (t, fake, conn) = setup();
        t.load(&conn).unwrap();
        let id = t.set_consent(&conn, true).unwrap().install_id.unwrap();
        let pending = t.begin_forget(&conn).unwrap().unwrap();
        assert_eq!(pending, id);
        assert!(!t.enabled());
        assert!(t.status(&conn).unwrap().forget_pending);
        fake.status.lock().unwrap().push(Err(NetError::Timeout));
        assert!(t.send_forget(&pending).is_err());
        // Repetir usa o identificador guardado.
        let again = t.begin_forget(&conn).unwrap().unwrap();
        assert_eq!(again, id);
        t.send_forget(&again).unwrap();
        t.finish_forget(&conn).unwrap();
        assert!(!t.status(&conn).unwrap().forget_pending);
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.last().unwrap().0, "https://example.test/api/telemetry/forget");
        assert_eq!(sent.last().unwrap().1, json!({ "installId": id }));
    }

    #[test]
    fn limite_de_chaves_em_memoria() {
        let (t, _fake, conn) = setup();
        t.load(&conn).unwrap();
        t.set_consent(&conn, true).unwrap();
        for i in 0..(MAX_KEYS + 50) {
            t.source_query("ext", &format!("fonte{i}"), None);
        }
        assert_eq!(t.pending_events().len(), MAX_KEYS);
    }

    #[test]
    fn dia_utc() {
        assert_eq!(utc_day_hour(0), ("1970-01-01".to_string(), 0));
        assert_eq!(utc_day_hour(1_794_232_800_000), ("2026-11-09".to_string(), 14));
        assert_eq!(utc_day_hour(951_782_400_000).0, "2000-02-29");
    }
}
