//! Comunicação com a nuvem do Prancha (Supabase): a autorização de login (OAuth com PKCE pelo GoTrue) e as funções
//! da sincronização (`/rest/v1/rpc/...`). Só com a chave pública do projeto e o token da pessoa: o app nunca tem a
//! chave de serviço.
//!
//! Nada daqui escreve tokens em log ou em mensagem de erro: o cabeçalho de autorização é montado só no envio, e o
//! `Debug` das requisições esconde o corpo.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use base64::Engine;
use reqwest::Url;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::extensions::net::{NetError, wait};

/// O projeto do Prancha na nuvem. O endereço e a chave publicável são públicos por natureza (vão em todo app que fala
/// com o Supabase); o que protege os dados é o login da pessoa e as regras do banco. Em builds de depuração, as
/// variáveis `PRANCHA_SUPABASE_URL` e `PRANCHA_SUPABASE_KEY` apontam para outro projeto.
pub const PROJECT_URL: &str = "https://irbyfncttguwcoznfiii.supabase.co";
pub const PUBLISHABLE_KEY: &str = "sb_publishable_oOYTmzh5tBAXbHL-KJHqbQ_biMLaDXP";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub url: String,
    pub key: String,
}

impl Project {
    pub fn from_env() -> Project {
        let pick = |name: &str, default: &str| {
            let runtime = if cfg!(debug_assertions) { std::env::var(name).ok() } else { None };
            runtime.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| default.to_string())
        };
        Project { url: pick("PRANCHA_SUPABASE_URL", PROJECT_URL).trim_end_matches('/').to_string(), key: pick("PRANCHA_SUPABASE_KEY", PUBLISHABLE_KEY) }
    }
}

/// Os provedores de login ligados no projeto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Google,
    Discord,
}

impl Provider {
    pub fn parse(name: &str) -> Option<Provider> {
        match name {
            "google" => Some(Provider::Google),
            "discord" => Some(Provider::Discord),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::Discord => "discord",
        }
    }
}

/// O desafio PKCE (S256) do verificador.
pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// A página de login do provedor, passando pelo Supabase. A volta traz `?code=` para `redirect`.
pub fn authorize_url(project: &Project, provider: Provider, verifier: &str, redirect: &str) -> String {
    let mut url = Url::parse(&format!("{}/auth/v1/authorize", project.url)).expect("endereço do projeto válido");
    url.query_pairs_mut()
        .append_pair("provider", provider.id())
        .append_pair("redirect_to", redirect)
        .append_pair("code_challenge", &challenge(verifier))
        .append_pair("code_challenge_method", "s256");
    url.to_string()
}

// ---------------------------------------------------------------------------
// Envio
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct HttpRequest {
    pub url: String,
    /// Token da pessoa. Sem ele, só a chave pública.
    pub bearer: Option<String>,
    /// Corpo JSON. Pode ter o código de autorização ou o token de renovação.
    pub body: Value,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRequest").field("url", &self.url).field("bearer", &self.bearer.as_ref().map(|_| "…")).field("body", &"…").finish()
    }
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Envio pela rede (sempre POST com JSON), trocado por um falso nos testes.
pub trait Http: Send + Sync {
    fn send(&self, project: &Project, request: HttpRequest) -> Result<HttpResponse, NetError>;
}

pub struct ReqwestHttp {
    client: OnceLock<Result<reqwest::Client, String>>,
}

impl ReqwestHttp {
    pub fn new() -> Self {
        ReqwestHttp { client: OnceLock::new() }
    }
}

impl Default for ReqwestHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl Http for ReqwestHttp {
    fn send(&self, project: &Project, request: HttpRequest) -> Result<HttpResponse, NetError> {
        let client = self
            .client
            .get_or_init(|| {
                let _ = rustls::crypto::ring::default_provider().install_default();
                reqwest::Client::builder()
                    .user_agent(concat!("Prancha/", env!("CARGO_PKG_VERSION")))
                    // O Supabase responde direto; um redirecionamento levaria o token a outro lugar.
                    .redirect(reqwest::redirect::Policy::none())
                    .https_only(!cfg!(debug_assertions))
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(REQUEST_TIMEOUT)
                    .build()
                    .map_err(|e| e.to_string())
            })
            .clone()
            .map_err(NetError::Network)?;
        let mut builder = client
            .post(&request.url)
            .header("apikey", &project.key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "application/json")
            .body(request.body.to_string());
        if let Some(token) = &request.bearer {
            builder = builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let failed = |e: reqwest::Error| if e.is_timeout() { NetError::Timeout } else { NetError::Network("sem conexão com a nuvem do Prancha".into()) };
        wait(
            async move {
                let mut response = builder.send().await.map_err(failed)?;
                let status = response.status().as_u16();
                let mut body = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(failed)? {
                    if body.len() + chunk.len() > MAX_BODY {
                        return Err(NetError::TooLarge);
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(HttpResponse { status, body })
            },
            None,
            Instant::now() + REQUEST_TIMEOUT,
        )
    }
}

// ---------------------------------------------------------------------------
// Erros
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    Net(NetError),
    /// O token não vale mais (ou o código / a renovação foi recusada).
    Unauthorized,
    /// A nuvem recusou o pedido com uma mensagem para a pessoa (limites da conta, dados inválidos).
    Rejected(String),
    Throttled,
    Server(u16),
    Invalid,
}

impl ApiError {
    pub fn is_transient(&self) -> bool {
        matches!(self, ApiError::Net(_) | ApiError::Throttled | ApiError::Server(_))
    }

    pub fn message(&self) -> String {
        match self {
            ApiError::Net(NetError::Timeout) => "A nuvem do Prancha demorou demais para responder.".into(),
            ApiError::Net(_) => "Sem conexão com a nuvem do Prancha.".into(),
            ApiError::Unauthorized => "A sessão da sua conta terminou. Entre de novo.".into(),
            ApiError::Rejected(message) => message.clone(),
            ApiError::Throttled => "Pedidos demais para a nuvem do Prancha. A sincronização tenta de novo daqui a pouco.".into(),
            ApiError::Server(status) => format!("A nuvem do Prancha está com problemas (erro {status})."),
            ApiError::Invalid => "A nuvem do Prancha respondeu algo inesperado.".into(),
        }
    }
}

#[derive(Deserialize, Default)]
struct ErrorBody {
    code: Option<Value>,
    message: Option<String>,
    msg: Option<String>,
    error: Option<String>,
    error_code: Option<String>,
}

/// Mensagens do banco que a pessoa pode ler (as funções da sincronização já escrevem em português).
fn readable(body: &ErrorBody) -> Option<String> {
    let code = body.code.as_ref().and_then(|c| c.as_str().map(str::to_string));
    match code.as_deref() {
        Some("54000") => body.message.clone().map(|m| format!("A nuvem recusou o envio: {m}.")),
        Some("22023") => body.message.clone().map(|m| format!("A nuvem recusou os dados enviados ({m}).")),
        _ => None,
    }
}

fn check(response: HttpResponse, auth: bool) -> Result<Vec<u8>, ApiError> {
    let status = response.status;
    if (200..=299).contains(&status) {
        return Ok(response.body);
    }
    let body: ErrorBody = serde_json::from_slice(&response.body).unwrap_or_default();
    let grant_refused = matches!(body.error.as_deref(), Some("invalid_grant"))
        || matches!(body.error_code.as_deref(), Some("refresh_token_not_found" | "refresh_token_already_used" | "flow_state_not_found" | "flow_state_expired" | "bad_code_verifier" | "session_not_found" | "user_not_found" | "bad_jwt"));
    match status {
        401 | 403 => Err(ApiError::Unauthorized),
        400 | 404 if auth && grant_refused => Err(ApiError::Unauthorized),
        429 => Err(ApiError::Throttled),
        500..=599 => Err(ApiError::Server(status)),
        _ => match readable(&body) {
            Some(message) => Err(ApiError::Rejected(message)),
            None => {
                log::warn!("nuvem: pedido recusado ({status}): {}", body.msg.or(body.message).unwrap_or_default());
                Err(ApiError::Rejected(format!("A nuvem do Prancha recusou o pedido (erro {status}).")))
            }
        },
    }
}

fn send(http: &dyn Http, project: &Project, request: HttpRequest, auth: bool) -> Result<Vec<u8>, ApiError> {
    check(http.send(project, request).map_err(ApiError::Net)?, auth)
}

// ---------------------------------------------------------------------------
// Sessão
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct User {
    pub id: String,
    pub email: Option<String>,
    #[serde(default)]
    pub app_metadata: Value,
    #[serde(default)]
    pub user_metadata: Value,
}

impl User {
    /// O nome que o provedor informou, quando há.
    pub fn display_name(&self) -> Option<String> {
        ["full_name", "name", "preferred_username", "user_name"]
            .iter()
            .find_map(|k| self.user_metadata.get(*k).and_then(Value::as_str))
            .map(|s| s.trim().chars().take(80).collect::<String>())
            .filter(|s| !s.is_empty())
    }

    pub fn provider(&self) -> Option<String> {
        self.app_metadata.get("provider").and_then(Value::as_str).map(str::to_string)
    }
}

#[derive(Deserialize, Clone)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub user: Option<User>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("expires_in", &self.expires_in).field("user", &self.user).finish_non_exhaustive()
    }
}

fn session(body: Vec<u8>) -> Result<Session, ApiError> {
    serde_json::from_slice(&body).map_err(|_| ApiError::Invalid)
}

/// Troca o código que voltou do login pela sessão.
pub fn exchange(http: &dyn Http, project: &Project, code: &str, verifier: &str) -> Result<Session, ApiError> {
    let request = HttpRequest { url: format!("{}/auth/v1/token?grant_type=pkce", project.url), bearer: None, body: json!({ "auth_code": code, "code_verifier": verifier }) };
    session(send(http, project, request, true)?)
}

/// Renova a sessão. O token de renovação muda a cada uso: o novo precisa ser guardado.
pub fn refresh(http: &dyn Http, project: &Project, refresh_token: &str) -> Result<Session, ApiError> {
    let request = HttpRequest { url: format!("{}/auth/v1/token?grant_type=refresh_token", project.url), bearer: None, body: json!({ "refresh_token": refresh_token }) };
    session(send(http, project, request, true)?)
}

/// Encerra a sessão na nuvem (só a deste aparelho).
pub fn logout(http: &dyn Http, project: &Project, access_token: &str) -> Result<(), ApiError> {
    let request = HttpRequest { url: format!("{}/auth/v1/logout?scope=local", project.url), bearer: Some(access_token.to_string()), body: json!({}) };
    send(http, project, request, true).map(|_| ())
}

/// Chama uma função da sincronização.
pub fn rpc(http: &dyn Http, project: &Project, access_token: &str, function: &str, args: Value) -> Result<Value, ApiError> {
    let request = HttpRequest { url: format!("{}/rest/v1/rpc/{function}", project.url), bearer: Some(access_token.to_string()), body: args };
    let body = send(http, project, request, false)?;
    if body.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&body).map_err(|_| ApiError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_desafio_segue_o_exemplo_da_rfc_7636() {
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn a_autorizacao_vai_pelo_projeto_com_pkce() {
        let project = Project { url: "https://exemplo.supabase.co".into(), key: "k".into() };
        let url = Url::parse(&authorize_url(&project, Provider::Discord, "verificador", "http://localhost:3642/auth")).unwrap();
        assert_eq!(url.path(), "/auth/v1/authorize");
        let pairs: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        assert!(pairs.contains(&("provider".into(), "discord".into())));
        assert!(pairs.contains(&("redirect_to".into(), "http://localhost:3642/auth".into())));
        assert!(pairs.contains(&("code_challenge_method".into(), "s256".into())));
        assert!(pairs.contains(&("code_challenge".into(), challenge("verificador"))));
        assert!(!url.as_str().contains("verificador"), "o verificador nunca sai do aparelho");
    }

    #[test]
    fn erros_viram_mensagens_para_a_pessoa() {
        let r = |status: u16, body: &str| check(HttpResponse { status, body: body.as_bytes().to_vec() }, false).unwrap_err();
        assert_eq!(r(401, "{}"), ApiError::Unauthorized);
        assert_eq!(r(400, r#"{"code":"54000","message":"aparelhos demais nesta conta; remova um em Configurações"}"#).message(), "A nuvem recusou o envio: aparelhos demais nesta conta; remova um em Configurações.");
        assert_eq!(r(503, "x"), ApiError::Server(503));
        assert!(r(400, "{}").message().contains("erro 400"));
        let refused = check(HttpResponse { status: 400, body: br#"{"code":400,"error_code":"refresh_token_not_found","msg":"Invalid Refresh Token"}"#.to_vec() }, true).unwrap_err();
        assert_eq!(refused, ApiError::Unauthorized);
    }

    #[test]
    fn o_nome_vem_do_provedor() {
        let user: User = serde_json::from_str(r#"{"id":"u","email":"a@b.c","app_metadata":{"provider":"discord"},"user_metadata":{"full_name":"  Ana  "}}"#).unwrap();
        assert_eq!(user.display_name().as_deref(), Some("Ana"));
        assert_eq!(user.provider().as_deref(), Some("discord"));
    }
}
