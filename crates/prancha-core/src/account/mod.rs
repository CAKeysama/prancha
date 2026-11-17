//! A conta do Prancha: login com Google ou Discord pela nuvem (Supabase), para sincronizar a leitura online entre
//! aparelhos. Ver docs/SINCRONIZACAO.md.
//!
//! - O login acontece no navegador da pessoa (a senha nunca passa pelo Prancha), com PKCE: o código que volta só vale
//!   junto com o verificador que ficou no aparelho. A volta chega a um endereço local que só existe durante o login
//!   (`http://localhost:3642/auth`, como o MyAnimeList usa a 3641) ou, onde o sistema entregar o endereço ao app
//!   (`app.prancha://auth` no Android), por `deliver_redirect`.
//! - Os tokens ficam cifrados pelo cofre do sistema, como os do MyAnimeList, e são renovados sozinhos.
//! - A conta é opcional: sem ela, nada sai do aparelho.

pub mod api;

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::Url;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::{self, now_ms};
use crate::error::{AppError, AppResult};
use crate::extensions::net::Cancel;
use crate::import::Emit;
use crate::mal::vault::{self, Vault};
use api::{ApiError, Http, Project, Provider, Session};

pub const EVENT_CHANGED: &str = "account:changed";
/// Porta e caminho fixos: o endereço precisa estar na lista de retornos permitidos do projeto no Supabase.
pub const REDIRECT_PORT: u16 = 3642;
pub const REDIRECT_PATH: &str = "/auth";
/// O retorno que o Android entrega ao app.
pub const APP_REDIRECT: &str = "app.prancha://auth";
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// O token é renovado um pouco antes de vencer.
const REFRESH_MARGIN_MS: i64 = 5 * 60_000;

pub fn redirect_uri(port: u16) -> String {
    format!("http://localhost:{port}{REDIRECT_PATH}")
}

#[derive(Serialize, Deserialize)]
struct Tokens {
    access: String,
    refresh: String,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// `signed_out`, `signed_in` ou `expired` (a nuvem recusou a renovação: é preciso entrar de novo).
    pub state: &'static str,
    pub email: Option<String>,
    pub name: Option<String>,
    pub provider: Option<String>,
    pub signed_in_at: Option<i64>,
    pub device_id: String,
}

/// Erro de uma chamada autenticada: separa "a conta não está pronta" do resto.
#[derive(Debug)]
pub enum CallError {
    SignedOut,
    Expired,
    Api(ApiError),
    App(AppError),
}

impl From<AppError> for CallError {
    fn from(e: AppError) -> Self {
        CallError::App(e)
    }
}

impl From<rusqlite::Error> for CallError {
    fn from(e: rusqlite::Error) -> Self {
        CallError::App(e.into())
    }
}

impl CallError {
    pub fn into_app(self) -> AppError {
        match self {
            CallError::SignedOut => AppError::user("account_signed_out", "Entre na sua conta para usar a sincronização."),
            CallError::Expired => AppError::user("account_expired", "A sessão da sua conta terminou. Entre de novo."),
            CallError::Api(e) => AppError::user("account_network", e.message()),
            CallError::App(e) => e,
        }
    }

    pub fn message(&self) -> String {
        match self {
            CallError::Api(e) => e.message(),
            CallError::App(e) => e.to_string(),
            CallError::SignedOut => "Sem conta conectada.".into(),
            CallError::Expired => "A sessão da sua conta terminou. Entre de novo.".into(),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// O login em andamento: o cancelamento e por onde chega um retorno entregue pelo sistema.
struct Pending {
    cancel: Cancel,
    delivered: Sender<String>,
}

pub struct Account {
    db_path: PathBuf,
    project: Project,
    http: Box<dyn Http>,
    vault: Box<dyn Vault>,
    emit: Emit,
    redirect_port: u16,
    timeout: Duration,
    /// Uma renovação de token por vez.
    auth: Mutex<()>,
    pending: Mutex<Option<Pending>>,
}

impl Account {
    pub fn new(db_path: PathBuf, project: Project, http: Box<dyn Http>, vault: Box<dyn Vault>, emit: Emit) -> Self {
        Account { db_path, project, http, vault, emit, redirect_port: REDIRECT_PORT, timeout: SIGN_IN_TIMEOUT, auth: Mutex::new(()), pending: Mutex::new(None) }
    }

    pub fn from_env(db_path: PathBuf, emit: Emit) -> Self {
        Account::new(db_path, Project::from_env(), Box::new(api::ReqwestHttp::new()), vault::system(), emit)
    }

    #[cfg(test)]
    pub fn with_port(mut self, port: u16, timeout: Duration) -> Self {
        self.redirect_port = port;
        self.timeout = timeout;
        self
    }

    fn conn(&self) -> AppResult<Connection> {
        db::open(&self.db_path)
    }

    fn changed(&self) {
        (self.emit)(EVENT_CHANGED, json!({}));
    }

    pub fn status(&self, conn: &Connection) -> AppResult<Status> {
        let device_id: String = conn.query_row("SELECT device_id FROM sync_state WHERE id = 1", [], |r| r.get(0))?;
        let row: Option<(String, Option<String>, Option<String>, String, i64)> = conn
            .query_row("SELECT state, email, name, provider, signed_in_at FROM sync_account WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .optional()?;
        Ok(match row {
            Some((state, email, name, provider, at)) => {
                Status { state: if state == "expired" { "expired" } else { "signed_in" }, email, name, provider: Some(provider), signed_in_at: Some(at), device_id }
            }
            None => Status { state: "signed_out", email: None, name: None, provider: None, signed_in_at: None, device_id },
        })
    }

    pub fn is_signed_in(conn: &Connection) -> AppResult<bool> {
        Ok(conn.query_row("SELECT EXISTS (SELECT 1 FROM sync_account WHERE state = 'signed_in')", [], |r| r.get(0))?)
    }

    fn store(&self, conn: &Connection, session: Session, provider: Option<Provider>) -> AppResult<()> {
        let plain = serde_json::to_vec(&Tokens { access: session.access_token, refresh: session.refresh_token }).map_err(|_| AppError::user("internal", "Falha interna."))?;
        let secret = self.vault.seal(&plain)?;
        let now = now_ms();
        let expires_at = now + session.expires_in.clamp(60, 7 * 24 * 3600) * 1000;
        match (provider, session.user) {
            (Some(provider), Some(user)) => {
                let previous: Option<String> = conn.query_row("SELECT user_id FROM sync_account WHERE id = 1", [], |r| r.get(0)).optional()?;
                // Outra conta neste aparelho: o que veio da anterior não vale para esta.
                if previous.is_some_and(|p| p != user.id) {
                    forget_remote(conn)?;
                }
                conn.execute(
                    "INSERT INTO sync_account (id, user_id, email, name, provider, secret, expires_at, signed_in_at, state) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'signed_in')
                     ON CONFLICT(id) DO UPDATE SET user_id = excluded.user_id, email = excluded.email, name = excluded.name, provider = excluded.provider,
                        secret = excluded.secret, expires_at = excluded.expires_at, signed_in_at = excluded.signed_in_at, state = 'signed_in'",
                    params![user.id, user.email, user.display_name(), user.provider().unwrap_or_else(|| provider.id().to_string()), secret, expires_at, now],
                )?;
                // Uma entrada nova confere tudo o que há no aparelho.
                conn.execute("UPDATE sync_state SET full_push = 1, last_error = NULL WHERE id = 1", [])?;
            }
            _ => {
                conn.execute("UPDATE sync_account SET secret = ?1, expires_at = ?2, state = 'signed_in' WHERE id = 1", params![secret, expires_at])?;
            }
        }
        Ok(())
    }

    /// Abre o login no navegador (`open`) e espera a volta. A senha fica no site do provedor.
    pub fn sign_in(&self, provider: &str, open: &dyn Fn(&str) -> AppResult<()>) -> AppResult<Status> {
        let provider = Provider::parse(provider).ok_or_else(|| AppError::user("bad_request", "Forma de login desconhecida."))?;
        let cancel: Cancel = Arc::new(AtomicBool::new(false));
        let (delivered, received) = channel();
        if let Some(previous) = lock(&self.pending).replace(Pending { cancel: cancel.clone(), delivered }) {
            previous.cancel.store(true, Ordering::Relaxed);
        }
        let result = (|| -> AppResult<()> {
            let listener = Loopback::listen(self.redirect_port)?;
            let redirect = redirect_uri(listener.port);
            let verifier = crate::mal::oauth::new_verifier();
            open(&api::authorize_url(&self.project, provider, &verifier, &redirect))?;
            let code = listener.wait(&received, &cancel, self.timeout)?;
            drop(listener);
            let session = api::exchange(&*self.http, &self.project, &code, &verifier).map_err(|e| match e {
                ApiError::Unauthorized => AppError::user("account_denied", "O login não foi aceito. Tente entrar de novo."),
                other => AppError::user("account_network", other.message()),
            })?;
            if session.user.is_none() {
                return Err(AppError::user("account_network", ApiError::Invalid.message()));
            }
            let _guard = lock(&self.auth);
            self.store(&self.conn()?, session, Some(provider))
        })();
        {
            let mut current = lock(&self.pending);
            if current.as_ref().is_some_and(|p| Arc::ptr_eq(&p.cancel, &cancel)) {
                *current = None;
            }
        }
        result?;
        self.changed();
        self.status(&self.conn()?)
    }

    pub fn cancel_sign_in(&self) {
        if let Some(pending) = lock(&self.pending).take() {
            pending.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// O sistema entregou ao app o endereço de volta do login (`app.prancha://auth?code=...`). Devolve se havia um
    /// login esperando por ele.
    pub fn deliver_redirect(&self, url: &str) -> bool {
        let Some(code) = code_from(url) else { return false };
        match lock(&self.pending).as_ref() {
            Some(pending) => pending.delivered.send(code).is_ok(),
            None => false,
        }
    }

    fn mark_expired(&self, conn: &Connection) -> AppResult<()> {
        if conn.execute("UPDATE sync_account SET state = 'expired' WHERE id = 1 AND state <> 'expired'", [])? > 0 {
            log::warn!("conta: a sessão expirou; é preciso entrar de novo");
            self.changed();
        }
        Ok(())
    }

    /// O token de acesso válido, renovado quando está para vencer (ou quando `force`, depois de um 401).
    fn access_token(&self, force: bool) -> Result<String, CallError> {
        let _guard = lock(&self.auth);
        let conn = self.conn()?;
        let row: Option<(String, i64, Vec<u8>)> =
            conn.query_row("SELECT state, expires_at, secret FROM sync_account WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        let Some((state, expires_at, secret)) = row else { return Err(CallError::SignedOut) };
        if state == "expired" {
            return Err(CallError::Expired);
        }
        // Um banco copiado de outro computador ou de outra conta do sistema não abre: é como uma sessão vencida.
        let tokens: Option<Tokens> = self.vault.open(&secret).ok().and_then(|plain| serde_json::from_slice(&plain).ok());
        let Some(tokens) = tokens else {
            self.mark_expired(&conn)?;
            return Err(CallError::Expired);
        };
        if !force && expires_at - REFRESH_MARGIN_MS > now_ms() {
            return Ok(tokens.access);
        }
        match api::refresh(&*self.http, &self.project, &tokens.refresh) {
            Ok(fresh) => {
                let access = fresh.access_token.clone();
                self.store(&conn, fresh, None)?;
                Ok(access)
            }
            Err(ApiError::Unauthorized) => {
                self.mark_expired(&conn)?;
                Err(CallError::Expired)
            }
            Err(e) => Err(CallError::Api(e)),
        }
    }

    /// Chama uma função da nuvem com o token e, num 401, renova uma vez e repete. Recusado de novo, a sessão expirou.
    pub fn call(&self, function: &str, args: Value) -> Result<Value, CallError> {
        let token = self.access_token(false)?;
        match api::rpc(&*self.http, &self.project, &token, function, args.clone()) {
            Err(ApiError::Unauthorized) => {
                let token = self.access_token(true)?;
                api::rpc(&*self.http, &self.project, &token, function, args).map_err(|e| match e {
                    ApiError::Unauthorized => {
                        if let Ok(conn) = self.conn() {
                            let _ = self.mark_expired(&conn);
                        }
                        CallError::Expired
                    }
                    other => CallError::Api(other),
                })
            }
            other => other.map_err(CallError::Api),
        }
    }

    /// Sai da conta neste aparelho. O que está no aparelho fica; o que veio da nuvem e ainda não foi aplicado sai.
    pub fn sign_out(&self, conn: &Connection) -> AppResult<()> {
        self.cancel_sign_in();
        if let Ok(token) = self.access_token(false) {
            if let Err(e) = api::logout(&*self.http, &self.project, &token) {
                log::warn!("conta: a sessão não foi encerrada na nuvem: {}", e.message());
            }
        }
        let _guard = lock(&self.auth);
        conn.execute("DELETE FROM sync_account", [])?;
        forget_remote(conn)?;
        self.changed();
        Ok(())
    }

    /// Apaga na nuvem tudo o que a conta tem, inclusive a própria conta, e sai neste aparelho.
    pub fn delete_everything(&self, conn: &Connection) -> AppResult<()> {
        self.call("sync_forget_me", json!({})).map_err(CallError::into_app)?;
        let _guard = lock(&self.auth);
        conn.execute("DELETE FROM sync_account", [])?;
        forget_remote(conn)?;
        self.changed();
        Ok(())
    }

    pub fn devices(&self) -> AppResult<Value> {
        self.call("sync_devices", json!({})).map_err(CallError::into_app)
    }

    pub fn remove_device(&self, id: &str) -> AppResult<bool> {
        let removed = self.call("sync_remove_device", json!({ "p_id": id })).map_err(CallError::into_app)?;
        Ok(removed.as_bool().unwrap_or(false))
    }

    pub fn export(&self) -> AppResult<Value> {
        self.call("sync_export", json!({})).map_err(CallError::into_app)
    }
}

/// O que veio da nuvem pertence à conta: sem ela, sai, e a próxima entrada recomeça do zero.
fn forget_remote(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "DELETE FROM sync_remote_series; DELETE FROM sync_remote_chapters; DELETE FROM sync_remote_items;
         UPDATE sync_state SET cursor = 0, full_push = 1, last_sync_at = NULL, last_error = NULL WHERE id = 1;",
    )?;
    Ok(())
}

/// O código de um endereço de volta (`...?code=...`), se for um retorno de login válido.
fn code_from(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    let ours = (url.scheme() == "app.prancha" && url.host_str() == Some("auth")) || (matches!(url.scheme(), "http") && url.path() == REDIRECT_PATH);
    if !ours {
        return None;
    }
    url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.into_owned()).filter(|c| !c.is_empty() && c.len() <= 4096)
}

/// O endereço de volta só existe durante o login, e só no próprio aparelho (127.0.0.1 e ::1), nunca na rede.
struct Loopback {
    listeners: Vec<TcpListener>,
    port: u16,
}

#[derive(Debug, PartialEq, Eq)]
enum Arrival {
    Code(String),
    Denied(Option<String>),
    Ignored,
}

fn page(title: &str, text: &str) -> String {
    format!(
        "<!doctype html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Prancha</title>\
         <style>body{{font-family:system-ui,sans-serif;background:#16181d;color:#e8e9ec;display:grid;place-items:center;height:100vh;margin:0;padding:1rem;box-sizing:border-box}}\
         main{{max-width:28rem;text-align:center}}p{{color:#a3a7b0}}</style></head>\
         <body><main><h1>{title}</h1><p>{text}</p></main></body></html>"
    )
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn handle(mut stream: TcpStream) -> Arrival {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 8192 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let mut parts = text.lines().next().unwrap_or_default().split_whitespace();
    let (method, target) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
    let url = Url::parse(&format!("http://localhost{target}")).ok().filter(|_| method == "GET" && target.starts_with('/'));
    let Some(url) = url.filter(|u| u.path() == REDIRECT_PATH) else {
        respond(&mut stream, "404 Not Found", &page("Página não encontrada", "Este endereço só existe enquanto você entra na sua conta pelo Prancha."));
        return Arrival::Ignored;
    };
    let param = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
    if let Some(error) = param("error") {
        respond(&mut stream, "200 OK", &page("Login cancelado", "O acesso não foi autorizado. Você pode fechar esta aba."));
        return Arrival::Denied(param("error_description").or(Some(error)));
    }
    match param("code").filter(|c| !c.is_empty() && c.len() <= 4096) {
        Some(code) => {
            respond(&mut stream, "200 OK", &page("Pronto!", "Você entrou na sua conta. Pode fechar esta aba e voltar ao Prancha."));
            Arrival::Code(code)
        }
        None => {
            respond(&mut stream, "400 Bad Request", &page("Resposta incompleta", "O login não devolveu o código de autorização. Tente de novo pelo Prancha."));
            Arrival::Ignored
        }
    }
}

impl Loopback {
    fn listen(port: u16) -> AppResult<Loopback> {
        let busy = || AppError::user("account_port_busy", format!("A porta {port} deste aparelho está em uso por outro programa. Feche-o e tente entrar de novo."));
        let v4 = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).map_err(|_| busy())?;
        let port = v4.local_addr()?.port();
        let mut listeners = vec![v4];
        // "localhost" pode ir para o IPv6 primeiro. Se o sistema não tiver IPv6, o IPv4 basta.
        if let Ok(v6) = TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, port))) {
            listeners.push(v6);
        }
        for l in &listeners {
            l.set_nonblocking(true)?;
        }
        Ok(Loopback { listeners, port })
    }

    /// Espera o código pelo endereço local ou entregue pelo sistema. Termina no código, na recusa, no cancelamento
    /// ou no prazo.
    fn wait(&self, delivered: &Receiver<String>, cancel: &Cancel, timeout: Duration) -> AppResult<String> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::user("cancelled", "Login cancelado."));
            }
            if Instant::now() >= deadline {
                return Err(AppError::user("account_timeout", "O tempo para entrar pelo navegador acabou. Tente de novo."));
            }
            if let Ok(code) = delivered.try_recv() {
                return Ok(code);
            }
            for listener in &self.listeners {
                match listener.accept() {
                    Ok((stream, _)) => match handle(stream) {
                        Arrival::Code(code) => return Ok(code),
                        Arrival::Denied(reason) => {
                            if let Some(reason) = reason {
                                log::info!("conta: login recusado: {reason}");
                            }
                            return Err(AppError::user("account_denied", "O login foi cancelado ou não foi autorizado."));
                        }
                        Arrival::Ignored => {}
                    },
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => {}
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
pub mod tests;
