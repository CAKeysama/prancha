//! Comunicação com a API oficial do MyAnimeList (v2) e com o servidor OAuth. Só o que o Prancha usa:
//! trocar e renovar tokens, o nome da conta, a pesquisa e os detalhes de mangás e o progresso na lista da pessoa.
//! Ver https://myanimelist.net/apiconfig/references/api/v2 e https://myanimelist.net/apiconfig/references/authorization.
//!
//! Nada daqui escreve tokens em log ou em mensagem de erro: o cabeçalho de autorização é montado só no envio, e os
//! erros dizem o que aconteceu sem repetir a requisição.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::extensions::net::{NetError, wait};

pub const AUTHORIZE_URL: &str = "https://myanimelist.net/v1/oauth2/authorize";
pub const TOKEN_URL: &str = "https://myanimelist.net/v1/oauth2/token";
pub const API_URL: &str = "https://api.myanimelist.net/v2";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY: usize = 2 * 1024 * 1024;
/// Campos pedidos na pesquisa e nos detalhes: o que ajuda a separar obras de nomes parecidos.
pub const CATALOG_FIELDS: &str = "alternative_titles,media_type,num_chapters,status,authors{first_name,last_name},start_date";
const SEARCH_LIMIT: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
}

/// Como a requisição se identifica. O valor nunca aparece no `Debug`.
#[derive(Clone)]
pub enum Auth {
    None,
    /// `X-MAL-CLIENT-ID`: consultas ao catálogo, que não precisam da conta.
    Client(String),
    /// `Authorization: Bearer`: a lista da pessoa.
    Bearer(String),
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Auth::None => "None",
            Auth::Client(_) => "Client(…)",
            Auth::Bearer(_) => "Bearer(…)",
        })
    }
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub auth: Auth,
    /// Corpo `application/x-www-form-urlencoded`. Pode conter o código ou o token de renovação: não vai para o `Debug`.
    pub form: Option<Form>,
}

#[derive(Clone)]
pub struct Form(pub String);

impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Form(…)")
    }
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Envio pela rede, trocado por um falso nos testes.
pub trait Http: Send + Sync {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, NetError>;
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
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, NetError> {
        let client = self
            .client
            .get_or_init(|| {
                let _ = rustls::crypto::ring::default_provider().install_default();
                reqwest::Client::builder()
                    .user_agent(concat!("Prancha/", env!("CARGO_PKG_VERSION")))
                    // Os endereços do MAL respondem direto; um redirecionamento levaria o token a outro lugar.
                    .redirect(reqwest::redirect::Policy::none())
                    .https_only(true)
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(REQUEST_TIMEOUT)
                    .build()
                    .map_err(|e| e.to_string())
            })
            .clone()
            .map_err(NetError::Network)?;
        let mut builder = match request.method {
            Method::Get => client.get(&request.url),
            Method::Post => client.post(&request.url),
            Method::Put => client.put(&request.url),
        };
        builder = match &request.auth {
            Auth::None => builder,
            Auth::Client(id) => builder.header("X-MAL-CLIENT-ID", id),
            Auth::Bearer(token) => builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}")),
        };
        if let Some(Form(body)) = request.form {
            builder = builder.header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded").body(body);
        }
        let failed = |e: reqwest::Error| if e.is_timeout() { NetError::Timeout } else { NetError::Network("sem conexão com o MyAnimeList".into()) };
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
    /// Sem rede, tempo esgotado, resposta grande demais.
    Net(NetError),
    /// 401, ou o servidor OAuth recusou o código ou o token de renovação.
    Unauthorized,
    NotFound,
    /// 403 ("DoS detected") ou 429: pedidos demais. Espera e tenta de novo.
    Throttled,
    Server(u16),
    /// Outro 4xx: tentar de novo não muda nada.
    Rejected(u16),
    /// A resposta não tinha o formato esperado.
    Invalid,
}

impl ApiError {
    /// Vale a pena tentar de novo mais tarde.
    pub fn is_transient(&self) -> bool {
        matches!(self, ApiError::Net(_) | ApiError::Throttled | ApiError::Server(_))
    }

    /// Mensagem para a pessoa, sem nada da requisição.
    pub fn message(&self) -> String {
        match self {
            ApiError::Net(NetError::Timeout) => "O MyAnimeList demorou demais para responder.".into(),
            ApiError::Net(_) => "Sem conexão com o MyAnimeList.".into(),
            ApiError::Unauthorized => "O MyAnimeList não aceitou a conexão. Conecte a conta de novo.".into(),
            ApiError::NotFound => "A obra não existe mais no MyAnimeList.".into(),
            ApiError::Throttled => "O MyAnimeList pediu um intervalo entre as atualizações.".into(),
            ApiError::Server(status) => format!("O MyAnimeList está com problemas (erro {status})."),
            ApiError::Rejected(status) => format!("O MyAnimeList recusou a atualização (erro {status})."),
            ApiError::Invalid => "O MyAnimeList respondeu algo inesperado.".into(),
        }
    }
}

fn check(response: HttpResponse) -> Result<Vec<u8>, ApiError> {
    match response.status {
        200..=299 => Ok(response.body),
        401 => Err(ApiError::Unauthorized),
        404 => Err(ApiError::NotFound),
        403 | 429 => Err(ApiError::Throttled),
        500..=599 => Err(ApiError::Server(response.status)),
        status => Err(ApiError::Rejected(status)),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|_| ApiError::Invalid)
}

fn form(pairs: &[(&str, &str)]) -> Form {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        serializer.append_pair(k, v);
    }
    Form(serializer.finish())
}

// ---------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// Segundos. A documentação fala em uma hora; na prática vem perto de um mês. Vale o que chegar.
    pub expires_in: i64,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq)]
pub struct AltTitles {
    #[serde(default)]
    pub synonyms: Vec<String>,
    #[serde(default)]
    pub en: String,
    #[serde(default)]
    pub ja: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq)]
pub struct AuthorNode {
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub last_name: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Author {
    pub node: AuthorNode,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ListStatus {
    pub status: Option<String>,
    #[serde(default)]
    pub num_chapters_read: i64,
    #[serde(default)]
    pub is_rereading: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Manga {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub alternative_titles: Option<AltTitles>,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub num_chapters: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing)]
    pub my_list_status: Option<ListStatus>,
}

impl Manga {
    /// Todos os nomes pelos quais a obra é conhecida no MAL.
    pub fn titles(&self) -> Vec<&str> {
        let mut out = vec![self.title.as_str()];
        if let Some(alt) = &self.alternative_titles {
            out.push(alt.en.as_str());
            out.push(alt.ja.as_str());
            out.extend(alt.synonyms.iter().map(String::as_str));
        }
        out.retain(|t| !t.trim().is_empty());
        out
    }

    pub fn author_names(&self) -> Vec<String> {
        self.authors
            .iter()
            .map(|a| format!("{} {}", a.node.first_name, a.node.last_name).trim().to_string())
            .filter(|n| !n.is_empty())
            .collect()
    }
}

#[derive(Deserialize)]
struct Node {
    node: Manga,
}

#[derive(Deserialize)]
struct SearchPage {
    #[serde(default)]
    data: Vec<Node>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct User {
    pub id: Option<i64>,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Chamadas
// ---------------------------------------------------------------------------

pub fn authorize_url(client_id: &str, state: &str, verifier: &str, redirect_uri: &str) -> String {
    let mut url = Url::parse(AUTHORIZE_URL).expect("endereço fixo");
    // O MAL só aceita o método `plain`: o desafio é o próprio verificador, que nasce a cada conexão.
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("state", state)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("code_challenge", verifier)
        .append_pair("code_challenge_method", "plain");
    url.into()
}

fn token_request(http: &dyn Http, pairs: &[(&str, &str)]) -> Result<TokenResponse, ApiError> {
    let response = http.send(HttpRequest { method: Method::Post, url: TOKEN_URL.into(), auth: Auth::None, form: Some(form(pairs)) }).map_err(ApiError::Net)?;
    // O servidor OAuth responde 400 ou 401 para código, verificador ou token de renovação inválidos.
    if matches!(response.status, 400 | 401) {
        return Err(ApiError::Unauthorized);
    }
    let token: TokenResponse = parse(&check(response)?)?;
    if token.access_token.is_empty() || token.refresh_token.is_empty() {
        return Err(ApiError::Invalid);
    }
    Ok(token)
}

/// Troca o código da autorização pelos tokens. Aplicativo público: sem segredo do cliente, com o verificador PKCE.
pub fn exchange_code(http: &dyn Http, client_id: &str, code: &str, verifier: &str, redirect_uri: &str) -> Result<TokenResponse, ApiError> {
    token_request(
        http,
        &[("client_id", client_id), ("grant_type", "authorization_code"), ("code", code), ("redirect_uri", redirect_uri), ("code_verifier", verifier)],
    )
}

pub fn refresh(http: &dyn Http, client_id: &str, refresh_token: &str) -> Result<TokenResponse, ApiError> {
    token_request(http, &[("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", refresh_token)])
}

fn get(http: &dyn Http, url: Url, auth: Auth) -> Result<Vec<u8>, ApiError> {
    check(http.send(HttpRequest { method: Method::Get, url: url.into(), auth, form: None }).map_err(ApiError::Net)?)
}

fn api_url(path: &str) -> Url {
    Url::parse(&format!("{API_URL}{path}")).expect("endereço fixo")
}

pub fn me(http: &dyn Http, token: &str) -> Result<User, ApiError> {
    parse(&get(http, api_url("/users/@me"), Auth::Bearer(token.into()))?)
}

/// Pesquisa no catálogo. Não precisa da conta: vai com o identificador do cliente.
pub fn search(http: &dyn Http, client_id: &str, query: &str) -> Result<Vec<Manga>, ApiError> {
    let mut url = api_url("/manga");
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("limit", &SEARCH_LIMIT.to_string())
        .append_pair("fields", CATALOG_FIELDS)
        .append_pair("nsfw", "true");
    let page: SearchPage = parse(&get(http, url, Auth::Client(client_id.into()))?)?;
    Ok(page.data.into_iter().map(|n| n.node).collect())
}

pub fn manga(http: &dyn Http, client_id: &str, id: i64) -> Result<Manga, ApiError> {
    let mut url = api_url(&format!("/manga/{id}"));
    url.query_pairs_mut().append_pair("fields", CATALOG_FIELDS);
    parse(&get(http, url, Auth::Client(client_id.into()))?)
}

/// O total de capítulos e o que está na lista da pessoa (nada, se a obra não está na lista).
pub fn list_status(http: &dyn Http, token: &str, id: i64) -> Result<(Option<i64>, Option<ListStatus>), ApiError> {
    let mut url = api_url(&format!("/manga/{id}"));
    url.query_pairs_mut().append_pair("fields", "num_chapters,my_list_status");
    let manga: Manga = parse(&get(http, url, Auth::Bearer(token.into()))?)?;
    Ok((manga.num_chapters, manga.my_list_status))
}

/// Grava os capítulos lidos e, quando pedido, a situação. Só os campos enviados mudam no MAL.
/// A documentação descreve PATCH e o exemplo dela usa PUT; o servidor aceita PUT, que é o que os clientes usam.
pub fn update_list_status(http: &dyn Http, token: &str, id: i64, chapters: i64, status: Option<&str>) -> Result<(), ApiError> {
    let chapters = chapters.to_string();
    let mut pairs = vec![("num_chapters_read", chapters.as_str())];
    if let Some(status) = status {
        pairs.push(("status", status));
    }
    let request = HttpRequest { method: Method::Put, url: api_url(&format!("/manga/{id}/my_list_status")).into(), auth: Auth::Bearer(token.into()), form: Some(form(&pairs)) };
    check(http.send(request).map_err(ApiError::Net)?).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_endereco_de_autorizacao_usa_pkce_plain() {
        let url = Url::parse(&authorize_url("cliente", "estado", "verificador-123", "http://localhost:1/cb")).unwrap();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["response_type"], "code");
        assert_eq!(pairs["code_challenge"], "verificador-123");
        assert_eq!(pairs["code_challenge_method"], "plain");
        assert_eq!(pairs["state"], "estado");
        assert_eq!(pairs["redirect_uri"], "http://localhost:1/cb");
        assert!(!pairs.contains_key("client_secret"));
    }

    #[test]
    fn segredos_nao_aparecem_no_debug() {
        let request = HttpRequest { method: Method::Post, url: TOKEN_URL.into(), auth: Auth::Bearer("token-secreto".into()), form: Some(Form("refresh_token=outro-segredo".into())) };
        let text = format!("{request:?}");
        assert!(!text.contains("token-secreto") && !text.contains("outro-segredo"), "{text}");
    }

    #[test]
    fn status_http_viram_erros_com_mensagem_sem_detalhes() {
        let r = |status| check(HttpResponse { status, body: vec![] });
        assert_eq!(r(401), Err(ApiError::Unauthorized));
        assert_eq!(r(403), Err(ApiError::Throttled));
        assert_eq!(r(429), Err(ApiError::Throttled));
        assert_eq!(r(503), Err(ApiError::Server(503)));
        assert_eq!(r(400), Err(ApiError::Rejected(400)));
        assert!(ApiError::Throttled.is_transient() && !ApiError::Rejected(400).is_transient() && !ApiError::Unauthorized.is_transient());
    }
}
