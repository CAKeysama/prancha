//! Integração com o MyAnimeList: conta conectada, associação das obras das fontes ao catálogo do MAL e a fila que
//! leva o progresso de leitura para a lista da pessoa. Ver docs/MYANIMELIST.md.
//!
//! - A conexão usa OAuth 2.0 com PKCE (`oauth.rs`); os tokens ficam cifrados pelo sistema (`vault.rs`), numa tabela
//!   que a interface não lê, e são renovados sozinhos.
//! - A associação (`matching.rs`) acontece ao abrir a obra, uma vez: o resultado fica guardado pelo endereço da obra.
//! - Só concluir um capítulo no leitor coloca algo na fila. A fila é uma linha por obra do MAL, guardada no banco, e
//!   uma thread própria a esvazia em segundo plano, sem nunca segurar a leitura. Antes de gravar, o progresso do MAL é
//!   consultado: o que já está mais adiantado lá não é tocado, e a obra nunca é marcada como concluída por aqui.

pub mod api;
pub mod matching;
pub mod oauth;
pub mod vault;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::{self, now_ms};
use crate::error::{AppError, AppResult};
use crate::extensions::net::Cancel;
use crate::import::Emit;
use api::{ApiError, Http, ListStatus, TokenResponse};
use matching::{Decision, Local, Scored};
use vault::Vault;

pub const EVENT_CHANGED: &str = "mal:changed";
/// Uma procura automática sem decisão vale por uma semana; depois a obra é procurada de novo ao abrir.
const LOOKUP_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Tentativas de uma atualização que falha por rede ou instabilidade do MAL, antes de aparecer como erro.
pub const MAX_ATTEMPTS: i64 = 10;
const BASE_RETRY_MS: i64 = 60_000;
const MAX_RETRY_MS: i64 = 60 * 60_000;
/// O token é renovado um pouco antes de vencer.
const REFRESH_MARGIN_MS: i64 = 5 * 60_000;
const IDLE_WAIT: Duration = Duration::from_secs(30 * 60);
const FIRST_RUN: Duration = Duration::from_secs(15);
/// Intervalo entre duas chamadas da fila, para não disparar o limite do MAL.
const SPACING: Duration = Duration::from_millis(700);
const MAX_PER_RUN: usize = 50;
const MAX_CANDIDATES: usize = 6;

/// O identificador do aplicativo cadastrado no MAL. É público (vai no endereço de autorização), mas não fica no
/// código: vem da variável `PRANCHA_MAL_CLIENT_ID` na compilação (a variável `MAL_CLIENT_ID` do repositório no
/// GitHub). Em builds de depuração também vale a variável de ambiente na execução. Sem ele, a integração não aparece.
pub fn client_id_from_env() -> Option<String> {
    let runtime = if cfg!(debug_assertions) { std::env::var("PRANCHA_MAL_CLIENT_ID").ok() } else { None };
    runtime
        .or_else(|| option_env!("PRANCHA_MAL_CLIENT_ID").map(str::to_string))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 128 && s.chars().all(|c| c.is_ascii_alphanumeric()))
}

#[derive(Serialize, Deserialize)]
struct Tokens {
    access: String,
    refresh: String,
}

// ---------------------------------------------------------------------------
// O que a interface vê
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub mal_id: i64,
    pub title: Option<String>,
    pub chapters: i64,
    pub error: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Esta versão do Prancha tem o identificador do aplicativo no MAL.
    pub available: bool,
    /// `disconnected`, `connected` ou `expired` (o MAL recusou a renovação: é preciso conectar de novo).
    pub state: &'static str,
    pub user_name: Option<String>,
    pub connected_at: Option<i64>,
    pub last_sync_at: Option<i64>,
    /// Atualizações esperando a vez, inclusive as que falharam e vão ser tentadas de novo.
    pub pending: i64,
    /// O erro mais recente de uma atualização que ainda vai ser tentada.
    pub pending_error: Option<String>,
    /// Atualizações que desistiram e precisam da pessoa.
    pub failures: Vec<Failure>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub mal_id: Option<i64>,
    pub title: Option<String>,
    pub media_type: Option<String>,
    pub num_chapters: Option<i64>,
    /// `auto`, `manual` ou `none` (a pessoa disse que a obra não está no MAL).
    pub origin: String,
    pub confidence: i64,
    pub synced_chapters: Option<i64>,
    pub linked_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub mal_id: i64,
    pub title: String,
    pub alternative: Vec<String>,
    pub media_type: Option<String>,
    pub num_chapters: Option<i64>,
    pub status: Option<String>,
    pub year: Option<String>,
    pub authors: Vec<String>,
    pub score: u8,
}

impl Candidate {
    fn of(scored: &Scored) -> Candidate {
        let m = &scored.manga;
        let mut alternative: Vec<String> = Vec::new();
        for t in m.titles().into_iter().skip(1) {
            if t != m.title && !alternative.iter().any(|a| a == t) {
                alternative.push(t.to_string());
            }
        }
        alternative.truncate(4);
        Candidate {
            mal_id: m.id,
            title: m.title.clone(),
            alternative,
            media_type: m.media_type.clone(),
            num_chapters: m.num_chapters.filter(|n| *n > 0),
            status: m.status.clone(),
            year: m.start_date.as_deref().map(|d| d.chars().take(4).collect()),
            authors: m.author_names(),
            score: scored.score,
        }
    }
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    /// `pending` ou `failed`.
    pub status: String,
    pub chapters: i64,
    pub attempts: i64,
    pub error: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SeriesStatus {
    pub available: bool,
    pub connected: bool,
    /// `linked`, `none`, `ambiguous`, `not_found`, `unchecked` ou `error`.
    pub state: &'static str,
    pub link: Option<Link>,
    pub candidates: Vec<Candidate>,
    pub sync: Option<SyncState>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Banco
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Address {
    extension_id: String,
    source_id: String,
    remote_id: String,
}

struct SeriesInfo {
    address: Address,
    local: Local,
    summary: Option<String>,
}

fn series_info(conn: &Connection, id: i64) -> AppResult<SeriesInfo> {
    let (extension_id, source_id, remote_id, title, author, summary): (String, String, String, String, Option<String>, Option<String>) = conn
        .query_row("SELECT extension_id, source_id, remote_id, title, author, summary FROM online_series WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .optional()?
        .ok_or_else(|| AppError::user("series_not_found", "Esta obra não está mais registrada."))?;
    let mut statement = conn.prepare("SELECT number, title FROM online_chapters WHERE series_id = ?1")?;
    let max_chapter = statement
        .query_map([id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(Result::ok)
        .filter_map(|(number, title)| matching::chapter_number(number.as_deref(), &title))
        .max();
    Ok(SeriesInfo { address: Address { extension_id, source_id, remote_id }, local: Local { title, author, max_chapter }, summary })
}

fn load_link(conn: &Connection, a: &Address) -> AppResult<Option<Link>> {
    Ok(conn
        .query_row(
            "SELECT mal_id, mal_title, media_type, num_chapters, origin, confidence, synced_chapters, linked_at FROM mal_links WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3",
            params![a.extension_id, a.source_id, a.remote_id],
            |r| {
                Ok(Link {
                    mal_id: r.get(0)?,
                    title: r.get(1)?,
                    media_type: r.get(2)?,
                    num_chapters: r.get(3)?,
                    origin: r.get(4)?,
                    confidence: r.get(5)?,
                    synced_chapters: r.get(6)?,
                    linked_at: r.get(7)?,
                })
            },
        )
        .optional()?)
}

fn load_sync(conn: &Connection, a: &Address) -> AppResult<Option<SyncState>> {
    Ok(conn
        .query_row(
            "SELECT status, chapters, attempts, last_error FROM mal_sync_queue WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3 ORDER BY id DESC LIMIT 1",
            params![a.extension_id, a.source_id, a.remote_id],
            |r| Ok(SyncState { status: r.get(0)?, chapters: r.get(1)?, attempts: r.get(2)?, error: r.get(3)? }),
        )
        .optional()?)
}

fn load_lookup(conn: &Connection, a: &Address, now: i64) -> AppResult<Option<Vec<Candidate>>> {
    let found: Option<String> = conn
        .query_row(
            "SELECT candidates FROM mal_lookups WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3 AND checked_at > ?4",
            params![a.extension_id, a.source_id, a.remote_id, now - LOOKUP_TTL_MS],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.map(|json| serde_json::from_str(&json).unwrap_or_default()))
}

fn delete_lookup(conn: &Connection, a: &Address) -> AppResult<()> {
    conn.execute("DELETE FROM mal_lookups WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3", params![a.extension_id, a.source_id, a.remote_id])?;
    Ok(())
}

struct Account {
    state: String,
    expires_at: i64,
    secret: Vec<u8>,
}

fn load_account(conn: &Connection) -> AppResult<Option<Account>> {
    Ok(conn
        .query_row("SELECT state, expires_at, secret FROM mal_account WHERE id = 1", [], |r| Ok(Account { state: r.get(0)?, expires_at: r.get(1)?, secret: r.get(2)? }))
        .optional()?)
}

fn has_account(conn: &Connection) -> AppResult<bool> {
    Ok(conn.query_row("SELECT EXISTS (SELECT 1 FROM mal_account)", [], |r| r.get(0))?)
}

/// Um capítulo foi concluído no leitor. Se a obra está associada e há conta, entra na fila o número de capítulos
/// lidos que ele representa. Sem rede: a fila é esvaziada depois, em segundo plano. Devolve se a fila mudou.
///
/// Repetições não duplicam nada: há no máximo uma linha por obra do MAL, e ela só muda quando o alvo aumenta.
/// Um número que não passa do último confirmado no MAL nem entra.
pub fn enqueue_completion(conn: &Connection, chapter_id: i64) -> AppResult<bool> {
    if !has_account(conn)? {
        return Ok(false);
    }
    let row: Option<(String, String, String, Option<String>, String, bool)> = conn
        .query_row(
            "SELECT s.extension_id, s.source_id, s.remote_id, c.number, c.title, c.completed_at IS NOT NULL
               FROM online_chapters c JOIN online_series s ON s.id = c.series_id WHERE c.id = ?1",
            [chapter_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;
    // Só capítulo concluído de verdade: abrir, pré-carregar ou começar a ler nunca chega aqui.
    let Some((extension_id, source_id, remote_id, number, title, completed)) = row else { return Ok(false) };
    if !completed {
        return Ok(false);
    }
    let address = Address { extension_id, source_id, remote_id };
    let Some(link) = load_link(conn, &address)? else { return Ok(false) };
    let Some(mal_id) = link.mal_id else { return Ok(false) };
    let Some(chapters) = matching::chapter_number(number.as_deref(), &title) else { return Ok(false) };
    if link.synced_chapters.is_some_and(|synced| synced >= chapters) {
        return Ok(false);
    }
    let now = now_ms();
    let changed = conn.execute(
        "INSERT INTO mal_sync_queue (mal_id, mal_title, extension_id, source_id, remote_id, chapters, status, attempts, next_attempt_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 0, ?7, ?7, ?7)
         ON CONFLICT(mal_id) DO UPDATE SET
            chapters = excluded.chapters, mal_title = excluded.mal_title, extension_id = excluded.extension_id, source_id = excluded.source_id,
            remote_id = excluded.remote_id, status = 'pending', attempts = 0, next_attempt_at = excluded.next_attempt_at, last_error = NULL,
            updated_at = excluded.updated_at
         WHERE excluded.chapters > mal_sync_queue.chapters",
        params![mal_id, link.title, address.extension_id, address.source_id, address.remote_id, chapters, now],
    )?;
    Ok(changed > 0)
}

fn retry_delay(attempts: i64) -> i64 {
    let exp = attempts.clamp(1, 20) - 1;
    (BASE_RETRY_MS.saturating_mul(1i64 << exp)).min(MAX_RETRY_MS)
}

// ---------------------------------------------------------------------------
// Decisão da sincronização
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// O MAL já está igual ou mais adiantado (ou a obra está concluída lá): nada é enviado.
    Skip { remote: i64 },
    /// Envia os capítulos e, só quando a obra ainda não estava sendo lida, a situação "lendo".
    Update { chapters: i64, status: Option<&'static str> },
}

/// Nunca diminui o progresso do MAL e nunca marca a obra como concluída: isso fica com a pessoa.
pub fn plan(target: i64, total: i64, remote: Option<&ListStatus>) -> Plan {
    // A fonte pode numerar além do total do MAL (extras, numeração própria): o máximo é o total.
    let target = if total > 0 { target.min(total) } else { target };
    match remote {
        Some(r) if r.status.as_deref() == Some("completed") && !r.is_rereading => Plan::Skip { remote: r.num_chapters_read },
        Some(r) if r.num_chapters_read >= target => Plan::Skip { remote: r.num_chapters_read },
        Some(r) => Plan::Update { chapters: target, status: matches!(r.status.as_deref(), None | Some("plan_to_read")).then_some("reading") },
        None => Plan::Update { chapters: target, status: Some("reading") },
    }
}

#[derive(Debug)]
enum SyncError {
    NotConnected,
    Expired,
    Api(ApiError),
    App(AppError),
}

impl From<AppError> for SyncError {
    fn from(e: AppError) -> Self {
        SyncError::App(e)
    }
}

impl From<rusqlite::Error> for SyncError {
    fn from(e: rusqlite::Error) -> Self {
        SyncError::App(e.into())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Done { remote: i64 },
    Retry(String),
    Failed(String),
    /// A conta saiu ou expirou: a fila espera, sem gastar tentativas.
    Paused,
}

struct QueueItem {
    id: i64,
    mal_id: i64,
    chapters: i64,
    attempts: i64,
}

// ---------------------------------------------------------------------------
// Serviço
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Signal {
    pending: bool,
    stop: bool,
}

pub struct Mal {
    db_path: PathBuf,
    client_id: Option<String>,
    http: Box<dyn Http>,
    vault: Box<dyn Vault>,
    emit: Emit,
    redirect_port: u16,
    authorize_timeout: Duration,
    spacing: Duration,
    /// Uma renovação de token por vez.
    auth: Mutex<()>,
    connecting: Mutex<Option<Cancel>>,
    signal: Mutex<Signal>,
    wake: Condvar,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn not_available() -> AppError {
    AppError::user("mal_unavailable", "Esta versão do Prancha não tem a integração com o MyAnimeList configurada.")
}

fn api_error(e: ApiError) -> AppError {
    match e {
        ApiError::Unauthorized => AppError::user("mal_auth_failed", "O MyAnimeList não aceitou a autorização. Tente conectar de novo."),
        ApiError::NotFound => AppError::user("mal_not_found", "Esta obra não foi encontrada no MyAnimeList."),
        other => AppError::user("mal_network", other.message()),
    }
}

impl Mal {
    pub fn new(db_path: PathBuf, client_id: Option<String>, http: Box<dyn Http>, vault: Box<dyn Vault>, emit: Emit) -> Self {
        Mal {
            db_path,
            client_id,
            http,
            vault,
            emit,
            redirect_port: oauth::REDIRECT_PORT,
            authorize_timeout: oauth::AUTHORIZE_TIMEOUT,
            spacing: SPACING,
            auth: Mutex::new(()),
            connecting: Mutex::new(None),
            signal: Mutex::new(Signal::default()),
            wake: Condvar::new(),
        }
    }

    pub fn from_env(db_path: PathBuf, emit: Emit) -> Self {
        Mal::new(db_path, client_id_from_env(), Box::new(api::ReqwestHttp::new()), vault::system(), emit)
    }

    fn conn(&self) -> AppResult<Connection> {
        db::open(&self.db_path)
    }

    fn client_id(&self) -> AppResult<&str> {
        self.client_id.as_deref().ok_or_else(not_available)
    }

    fn changed(&self) {
        (self.emit)(EVENT_CHANGED, json!({}));
    }

    // -- Conta --------------------------------------------------------------------------------------------------

    pub fn status(&self, conn: &Connection) -> AppResult<Status> {
        let account: Option<(String, String, i64, Option<i64>)> = conn
            .query_row("SELECT state, user_name, connected_at, last_sync_at FROM mal_account WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .optional()?;
        let pending: i64 = conn.query_row("SELECT count(*) FROM mal_sync_queue WHERE status = 'pending'", [], |r| r.get(0))?;
        let pending_error: Option<String> = conn
            .query_row("SELECT last_error FROM mal_sync_queue WHERE status = 'pending' AND last_error IS NOT NULL ORDER BY updated_at DESC LIMIT 1", [], |r| r.get(0))
            .optional()?;
        let mut statement = conn.prepare("SELECT mal_id, mal_title, chapters, last_error FROM mal_sync_queue WHERE status = 'failed' ORDER BY updated_at DESC")?;
        let failures = statement
            .query_map([], |r| Ok(Failure { mal_id: r.get(0)?, title: r.get(1)?, chapters: r.get(2)?, error: r.get(3)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        let (state, user_name, connected_at, last_sync_at) = match account {
            Some((state, name, at, sync)) => (if state == "expired" { "expired" } else { "connected" }, Some(name), Some(at), sync),
            None => ("disconnected", None, None, None),
        };
        Ok(Status { available: self.client_id.is_some(), state, user_name, connected_at, last_sync_at, pending, pending_error, failures })
    }

    fn store_tokens(&self, conn: &Connection, user: Option<&api::User>, tokens: TokenResponse) -> AppResult<()> {
        let plain = serde_json::to_vec(&Tokens { access: tokens.access_token, refresh: tokens.refresh_token }).map_err(|_| AppError::user("internal", "Falha interna."))?;
        let secret = self.vault.seal(&plain)?;
        let now = now_ms();
        let expires_at = now + tokens.expires_in.clamp(60, 365 * 24 * 3600) * 1000;
        match user {
            Some(user) => {
                conn.execute(
                    "INSERT INTO mal_account (id, user_id, user_name, secret, expires_at, connected_at, state) VALUES (1, ?1, ?2, ?3, ?4, ?5, 'connected')
                     ON CONFLICT(id) DO UPDATE SET user_id = excluded.user_id, user_name = excluded.user_name, secret = excluded.secret,
                        expires_at = excluded.expires_at, connected_at = excluded.connected_at, state = 'connected'",
                    params![user.id, user.name, secret, expires_at, now],
                )?;
            }
            None => {
                conn.execute("UPDATE mal_account SET secret = ?1, expires_at = ?2, state = 'connected' WHERE id = 1", params![secret, expires_at])?;
            }
        }
        Ok(())
    }

    /// Abre a autorização no navegador (`open`) e espera a volta. A senha da pessoa fica no site do MAL.
    pub fn connect(&self, open: &dyn Fn(&str) -> AppResult<()>) -> AppResult<Status> {
        let client_id = self.client_id()?.to_string();
        let cancel: Cancel = Arc::new(AtomicBool::new(false));
        if let Some(previous) = lock(&self.connecting).replace(cancel.clone()) {
            previous.store(true, Ordering::Relaxed);
        }
        let result = (|| -> AppResult<()> {
            let callback = oauth::listen(self.redirect_port)?;
            let redirect = oauth::redirect_uri(callback.port());
            let verifier = oauth::new_verifier();
            let state = oauth::new_state();
            open(&api::authorize_url(&client_id, &state, &verifier, &redirect))?;
            let code = callback.wait(&state, &cancel, self.authorize_timeout)?;
            drop(callback);
            let tokens = api::exchange_code(&*self.http, &client_id, &code, &verifier, &redirect).map_err(api_error)?;
            let user = api::me(&*self.http, &tokens.access_token).map_err(api_error)?;
            let _guard = lock(&self.auth);
            self.store_tokens(&self.conn()?, Some(&user), tokens)
        })();
        {
            let mut current = lock(&self.connecting);
            if current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                *current = None;
            }
        }
        result?;
        self.changed();
        self.notify();
        self.status(&self.conn()?)
    }

    pub fn cancel_connect(&self) {
        if let Some(cancel) = lock(&self.connecting).take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Apaga os tokens e as atualizações que esperavam: elas eram para esta conta. As associações ficam.
    /// Devolve quantas atualizações foram descartadas.
    pub fn disconnect(&self, conn: &mut Connection) -> AppResult<i64> {
        self.cancel_connect();
        let _guard = lock(&self.auth);
        let tx = conn.transaction()?;
        let dropped = tx.execute("DELETE FROM mal_sync_queue", [])? as i64;
        tx.execute("DELETE FROM mal_account", [])?;
        tx.commit()?;
        self.changed();
        Ok(dropped)
    }

    fn mark_expired(&self, conn: &Connection) -> AppResult<()> {
        if conn.execute("UPDATE mal_account SET state = 'expired' WHERE id = 1 AND state <> 'expired'", [])? > 0 {
            log::warn!("myanimelist: a sessão expirou; é preciso conectar de novo");
            self.changed();
        }
        Ok(())
    }

    /// O token de acesso válido, renovado quando está para vencer (ou quando `force`, depois de um 401).
    fn access_token(&self, force: bool) -> Result<String, SyncError> {
        let _guard = lock(&self.auth);
        let conn = self.conn()?;
        let Some(account) = load_account(&conn)? else { return Err(SyncError::NotConnected) };
        if account.state == "expired" {
            return Err(SyncError::Expired);
        }
        // Um banco copiado de outro computador ou de outra conta do Windows não abre: é como uma sessão vencida.
        let tokens: Option<Tokens> = self.vault.open(&account.secret).ok().and_then(|plain| serde_json::from_slice(&plain).ok());
        let Some(tokens) = tokens else {
            self.mark_expired(&conn)?;
            return Err(SyncError::Expired);
        };
        if !force && account.expires_at - REFRESH_MARGIN_MS > now_ms() {
            return Ok(tokens.access);
        }
        let client_id = self.client_id()?;
        match api::refresh(&*self.http, client_id, &tokens.refresh) {
            Ok(fresh) => {
                let access = fresh.access_token.clone();
                self.store_tokens(&conn, None, fresh)?;
                Ok(access)
            }
            Err(ApiError::Unauthorized) => {
                self.mark_expired(&conn)?;
                Err(SyncError::Expired)
            }
            Err(e) => Err(SyncError::Api(e)),
        }
    }

    /// Chama a API com o token e, num 401, renova uma vez e repete. Recusado de novo, a sessão expirou.
    fn with_token<T>(&self, call: impl Fn(&str) -> Result<T, ApiError>) -> Result<T, SyncError> {
        let token = self.access_token(false)?;
        match call(&token) {
            Err(ApiError::Unauthorized) => {
                let token = self.access_token(true)?;
                call(&token).map_err(|e| match e {
                    ApiError::Unauthorized => {
                        if let Ok(conn) = self.conn() {
                            let _ = self.mark_expired(&conn);
                        }
                        SyncError::Expired
                    }
                    other => SyncError::Api(other),
                })
            }
            other => other.map_err(SyncError::Api),
        }
    }

    // -- Associação ---------------------------------------------------------------------------------------------

    /// A associação da obra. Com `lookup`, conta conectada e nada decidido ainda, procura no MAL (uma vez por
    /// semana no máximo) e associa sozinho quando há uma candidata clara.
    pub fn series_status(&self, series_id: i64, lookup: bool) -> AppResult<SeriesStatus> {
        let conn = self.conn()?;
        let info = series_info(&conn, series_id)?;
        let connected = has_account(&conn)?;
        let available = self.client_id.is_some();
        let base = |state: &'static str, link: Option<Link>, candidates: Vec<Candidate>, error: Option<String>, conn: &Connection| -> AppResult<SeriesStatus> {
            Ok(SeriesStatus { available, connected, state, link, candidates, sync: load_sync(conn, &info.address)?, error })
        };
        if let Some(link) = load_link(&conn, &info.address)? {
            let state = if link.mal_id.is_some() { "linked" } else { "none" };
            return base(state, Some(link), vec![], None, &conn);
        }
        if let Some(candidates) = load_lookup(&conn, &info.address, now_ms())? {
            let state = if candidates.is_empty() { "not_found" } else { "ambiguous" };
            return base(state, None, candidates, None, &conn);
        }
        if !lookup || !connected || !available {
            return base("unchecked", None, vec![], None, &conn);
        }
        drop(conn);
        let found = self.auto_lookup(&info);
        let conn = self.conn()?;
        match found {
            Err(e) => base("error", None, vec![], Some(e.message()), &conn),
            Ok((Decision::Confident, ranked)) => {
                let best = &ranked[0];
                conn.execute(
                    "INSERT OR IGNORE INTO mal_links (extension_id, source_id, remote_id, mal_id, mal_title, media_type, num_chapters, origin, confidence, linked_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'auto', ?8, ?9)",
                    params![
                        info.address.extension_id,
                        info.address.source_id,
                        info.address.remote_id,
                        best.manga.id,
                        best.manga.title,
                        best.manga.media_type,
                        best.manga.num_chapters,
                        best.score,
                        now_ms()
                    ],
                )?;
                delete_lookup(&conn, &info.address)?;
                self.changed();
                let link = load_link(&conn, &info.address)?;
                base("linked", link, vec![], None, &conn)
            }
            Ok((decision, ranked)) => {
                let candidates: Vec<Candidate> = ranked.iter().take(MAX_CANDIDATES).map(Candidate::of).collect();
                conn.execute(
                    "INSERT OR REPLACE INTO mal_lookups (extension_id, source_id, remote_id, checked_at, candidates) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![info.address.extension_id, info.address.source_id, info.address.remote_id, now_ms(), serde_json::to_string(&candidates).unwrap_or_else(|_| "[]".into())],
                )?;
                let state = if decision == Decision::NotFound { "not_found" } else { "ambiguous" };
                base(state, None, candidates, None, &conn)
            }
        }
    }

    fn auto_lookup(&self, info: &SeriesInfo) -> Result<(Decision, Vec<Scored>), ApiError> {
        let client_id = self.client_id.as_deref().unwrap_or_default();
        // O identificador que a própria fonte deu vale mais que qualquer comparação de nomes.
        if let Some(id) = info.summary.as_deref().and_then(matching::external_id) {
            match api::manga(&*self.http, client_id, id) {
                Ok(manga) if !matches!(manga.media_type.as_deref(), Some("novel" | "light_novel")) => return Ok((Decision::Confident, vec![Scored { manga, score: 100 }])),
                Ok(_) | Err(ApiError::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        let Some(query) = matching::query_for(&info.local.title) else { return Ok((Decision::NotFound, vec![])) };
        let ranked = matching::rank(&info.local, api::search(&*self.http, client_id, &query)?);
        Ok((matching::decide(&ranked), ranked))
    }

    /// Pesquisa manual, para corrigir a associação. Aceita um título, o número da obra no MAL ou o endereço dela.
    pub fn search(&self, series_id: i64, query: &str) -> AppResult<Vec<Candidate>> {
        let client_id = self.client_id()?;
        let local = series_info(&self.conn()?, series_id)?.local;
        let query = query.trim();
        let direct = matching::external_id(query).or_else(|| query.parse::<i64>().ok().filter(|id| *id > 0));
        let results = match direct {
            Some(id) => match api::manga(&*self.http, client_id, id) {
                Ok(manga) => vec![manga],
                Err(ApiError::NotFound) => vec![],
                Err(e) => return Err(api_error(e)),
            },
            None => {
                let text: String = query.chars().take(64).collect();
                if text.chars().filter(|c| c.is_alphanumeric()).count() < 3 {
                    return Err(AppError::user("mal_query_short", "Digite pelo menos 3 letras para pesquisar no MyAnimeList."));
                }
                api::search(&*self.http, client_id, &text).map_err(api_error)?
            }
        };
        let mut scored: Vec<Scored> = results.into_iter().map(|m| Scored { score: matching::score(&local, &m), manga: m }).collect();
        scored.sort_by_key(|s| std::cmp::Reverse(s.score));
        Ok(scored.iter().map(Candidate::of).collect())
    }

    /// A pessoa escolheu a obra do MAL. Atualizações pendentes para outra obra (a associação errada) saem da fila.
    pub fn link(&self, series_id: i64, mal_id: i64) -> AppResult<SeriesStatus> {
        let client_id = self.client_id()?;
        let manga = api::manga(&*self.http, client_id, mal_id).map_err(api_error)?;
        let mut conn = self.conn()?;
        let info = series_info(&conn, series_id)?;
        let a = &info.address;
        let previous = load_link(&conn, a)?;
        let synced = previous.filter(|p| p.mal_id == Some(mal_id)).and_then(|p| p.synced_chapters);
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO mal_links (extension_id, source_id, remote_id, mal_id, mal_title, media_type, num_chapters, origin, confidence, synced_chapters, linked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'manual', 100, ?8, ?9)",
            params![a.extension_id, a.source_id, a.remote_id, manga.id, manga.title, manga.media_type, manga.num_chapters, synced, now_ms()],
        )?;
        tx.execute(
            "DELETE FROM mal_sync_queue WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3 AND mal_id <> ?4",
            params![a.extension_id, a.source_id, a.remote_id, manga.id],
        )?;
        delete_lookup(&tx, a)?;
        tx.commit()?;
        drop(conn);
        self.changed();
        self.series_status(series_id, false)
    }

    /// A obra não está no MAL: nada é procurado nem sincronizado até a pessoa mudar de ideia.
    pub fn unlink(&self, series_id: i64) -> AppResult<SeriesStatus> {
        let mut conn = self.conn()?;
        let info = series_info(&conn, series_id)?;
        let a = &info.address;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO mal_links (extension_id, source_id, remote_id, mal_id, origin, linked_at) VALUES (?1, ?2, ?3, NULL, 'none', ?4)",
            params![a.extension_id, a.source_id, a.remote_id, now_ms()],
        )?;
        tx.execute("DELETE FROM mal_sync_queue WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3", params![a.extension_id, a.source_id, a.remote_id])?;
        delete_lookup(&tx, a)?;
        tx.commit()?;
        drop(conn);
        self.changed();
        self.series_status(series_id, false)
    }

    /// Esquece a decisão e a última procura: a próxima abertura procura de novo.
    pub fn reset_link(&self, series_id: i64) -> AppResult<SeriesStatus> {
        let conn = self.conn()?;
        let info = series_info(&conn, series_id)?;
        let a = &info.address;
        conn.execute("DELETE FROM mal_links WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3", params![a.extension_id, a.source_id, a.remote_id])?;
        conn.execute("DELETE FROM mal_sync_queue WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3", params![a.extension_id, a.source_id, a.remote_id])?;
        delete_lookup(&conn, a)?;
        drop(conn);
        self.changed();
        self.series_status(series_id, true)
    }

    // -- Fila ---------------------------------------------------------------------------------------------------

    /// As que desistiram voltam para a fila, e as que esperavam são tentadas agora.
    pub fn retry(&self, conn: &Connection) -> AppResult<usize> {
        let n = conn.execute("UPDATE mal_sync_queue SET status = 'pending', attempts = 0, next_attempt_at = ?1, updated_at = ?1", [now_ms()])?;
        self.changed();
        self.notify();
        Ok(n)
    }

    pub fn discard_failed(&self, conn: &Connection) -> AppResult<usize> {
        let n = conn.execute("DELETE FROM mal_sync_queue WHERE status = 'failed'", [])?;
        self.changed();
        Ok(n)
    }

    /// Acorda a thread da fila.
    pub fn notify(&self) {
        lock(&self.signal).pending = true;
        self.wake.notify_all();
    }

    pub fn stop(&self) {
        lock(&self.signal).stop = true;
        self.wake.notify_all();
    }

    /// Thread da fila: dorme até ter algo vencido, um aviso (capítulo concluído, conexão, "tentar de novo") ou o
    /// intervalo. Na abertura do app, retoma o que ficou pendente.
    pub fn spawn(self: &Arc<Self>) {
        if self.client_id.is_none() {
            return;
        }
        let this = self.clone();
        std::thread::Builder::new()
            .name("myanimelist".into())
            .spawn(move || {
                let mut wait_for = FIRST_RUN;
                loop {
                    {
                        let signal = lock(&this.signal);
                        let (mut signal, _) = this.wake.wait_timeout_while(signal, wait_for, |s| !s.pending && !s.stop).unwrap_or_else(|e| e.into_inner());
                        if signal.stop {
                            return;
                        }
                        signal.pending = false;
                    }
                    wait_for = this.run_queue();
                }
            })
            .expect("thread do MyAnimeList");
    }

    /// Processa o que está vencido e diz quanto esperar até a próxima vez.
    pub fn run_queue(&self) -> Duration {
        match self.process_due() {
            Ok(Some(next_at)) => Duration::from_millis((next_at - now_ms()).clamp(1_000, IDLE_WAIT.as_millis() as i64) as u64),
            Ok(None) => IDLE_WAIT,
            Err(e) => {
                log::warn!("myanimelist: fila não processada: {}", e.code());
                Duration::from_secs(5 * 60)
            }
        }
    }

    fn process_due(&self) -> AppResult<Option<i64>> {
        let conn = self.conn()?;
        match load_account(&conn)? {
            Some(account) if account.state == "connected" => {}
            _ => return Ok(None),
        }
        let mut processed = 0;
        while processed < MAX_PER_RUN {
            let item: Option<QueueItem> = conn
                .query_row(
                    "SELECT id, mal_id, chapters, attempts FROM mal_sync_queue WHERE status = 'pending' AND next_attempt_at <= ?1 ORDER BY next_attempt_at, id LIMIT 1",
                    [now_ms()],
                    |r| Ok(QueueItem { id: r.get(0)?, mal_id: r.get(1)?, chapters: r.get(2)?, attempts: r.get(3)? }),
                )
                .optional()?;
            let Some(item) = item else { break };
            if processed > 0 && !self.spacing.is_zero() {
                std::thread::sleep(self.spacing);
            }
            processed += 1;
            let outcome = self.sync_one(&item);
            self.apply(&conn, &item, &outcome)?;
            if outcome == Outcome::Paused {
                break;
            }
        }
        if processed > 0 {
            self.changed();
        }
        Ok(conn.query_row("SELECT min(next_attempt_at) FROM mal_sync_queue WHERE status = 'pending'", [], |r| r.get(0))?)
    }

    fn sync_one(&self, item: &QueueItem) -> Outcome {
        let outcome = (|| -> Result<i64, SyncError> {
            let (total, remote) = self.with_token(|token| api::list_status(&*self.http, token, item.mal_id))?;
            match plan(item.chapters, total.unwrap_or(0), remote.as_ref()) {
                Plan::Skip { remote } => Ok(remote),
                Plan::Update { chapters, status } => {
                    self.with_token(|token| api::update_list_status(&*self.http, token, item.mal_id, chapters, status))?;
                    Ok(chapters)
                }
            }
        })();
        match outcome {
            Ok(remote) => Outcome::Done { remote },
            Err(SyncError::NotConnected | SyncError::Expired) => Outcome::Paused,
            Err(SyncError::Api(e)) if e.is_transient() => Outcome::Retry(e.message()),
            Err(SyncError::Api(e)) => Outcome::Failed(e.message()),
            Err(SyncError::App(e)) => Outcome::Retry(e.to_string()),
        }
    }

    fn apply(&self, conn: &Connection, item: &QueueItem, outcome: &Outcome) -> AppResult<()> {
        let now = now_ms();
        match outcome {
            Outcome::Done { remote } => {
                // Se outro capítulo foi concluído enquanto isto rodava, o alvo subiu e a linha fica para a próxima volta.
                let removed = conn.execute("DELETE FROM mal_sync_queue WHERE id = ?1 AND chapters <= ?2", params![item.id, item.chapters])?;
                if removed == 0 {
                    conn.execute("UPDATE mal_sync_queue SET attempts = 0, last_error = NULL, next_attempt_at = ?2, updated_at = ?2 WHERE id = ?1", params![item.id, now])?;
                }
                conn.execute("UPDATE mal_links SET synced_chapters = max(coalesce(synced_chapters, 0), ?2) WHERE mal_id = ?1", params![item.mal_id, remote])?;
                conn.execute("UPDATE mal_account SET last_sync_at = ?1 WHERE id = 1", [now])?;
            }
            Outcome::Retry(message) => {
                let attempts = item.attempts + 1;
                if attempts >= MAX_ATTEMPTS {
                    conn.execute(
                        "UPDATE mal_sync_queue SET status = 'failed', attempts = ?2, last_error = ?3, updated_at = ?4 WHERE id = ?1",
                        params![item.id, attempts, message, now],
                    )?;
                } else {
                    conn.execute(
                        "UPDATE mal_sync_queue SET attempts = ?2, last_error = ?3, next_attempt_at = ?4, updated_at = ?5 WHERE id = ?1",
                        params![item.id, attempts, message, now + retry_delay(attempts), now],
                    )?;
                }
            }
            Outcome::Failed(message) => {
                conn.execute(
                    "UPDATE mal_sync_queue SET status = 'failed', attempts = attempts + 1, last_error = ?2, updated_at = ?3 WHERE id = ?1",
                    params![item.id, message, now],
                )?;
            }
            Outcome::Paused => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
