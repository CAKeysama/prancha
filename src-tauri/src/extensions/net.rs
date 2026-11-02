//! Toda requisição feita em nome de um repositório ou de uma extensão passa por aqui.
//! Só HTTPS na porta 443, sem proxy, e os destinos são conferidos de novo a cada redirecionamento.
//! O DNS é resolvido por este módulo: um nome que aponte para a rede local não chega a conectar.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{CONTENT_TYPE, ETAG, HeaderName, HeaderValue, IF_NONE_MATCH, LOCATION};
use reqwest::{StatusCode, Url};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const WAIT_STEP: Duration = Duration::from_millis(50);
pub const MAX_REDIRECTS: u8 = 3;
/// Únicos cabeçalhos que uma extensão pode definir.
const ALLOWED_HEADERS: &[&str] = &["referer", "accept"];

pub type Cancel = Arc<AtomicBool>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// O destino não obedece às regras.
    Blocked(String),
    Network(String),
    Timeout,
    TooLarge,
    Cancelled,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Blocked(why) => write!(f, "Endereço bloqueado: {why}"),
            NetError::Network(why) => write!(f, "Falha de conexão: {why}"),
            NetError::Timeout => f.write_str("O servidor demorou demais para responder."),
            NetError::TooLarge => f.write_str("A resposta é maior que o limite permitido."),
            NetError::Cancelled => f.write_str("Operação cancelada."),
        }
    }
}

/// Quem pode ser consultado nesta requisição.
#[derive(Clone, Copy)]
pub enum Access<'a> {
    /// Repositórios: qualquer host público.
    Public,
    /// Extensões: só os hosts declarados e aceitos na instalação.
    Hosts(&'a [String]),
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub max_bytes: u64,
    pub timeout: Duration,
}

#[derive(Default)]
pub struct Request<'a> {
    pub headers: &'a [(String, String)],
    /// Validador da resposta anterior: um 304 evita baixar de novo.
    pub etag: Option<&'a str>,
    pub cancel: Option<&'a Cancel>,
    /// Confere o tipo de conteúdo antes de baixar o corpo.
    pub check_content_type: bool,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub url: String,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

// ---------------------------------------------------------------------------
// Destinos permitidos
// ---------------------------------------------------------------------------

fn ipv4_is_public(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        || a >= 240
        || (a == 100 && (64..128).contains(&b)) // CGNAT
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (18..20).contains(&b)))
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ipv4_is_public(ip),
        IpAddr::V6(ip) => ipv6_is_public(ip),
    }
}

fn ipv6_is_public(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return ipv4_is_public(v4);
    }
    let s = ip.segments();
    // NAT64 e 6to4 embutem um IPv4, que também pode ser local.
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return ipv4_is_public(Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8));
    }
    if s[0] == 0x2002 {
        return ipv4_is_public(Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8));
    }
    // Só 2000::/3 é roteável na internet. Dentro dele ficam de fora o Teredo e a faixa de documentação.
    (s[0] & 0xe000) == 0x2000 && !(s[0] == 0x2001 && (s[1] == 0 || s[1] == 0xdb8))
}

fn is_local_name(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    !host.contains('.') || [".localhost", ".local", ".internal", ".intranet", ".lan", ".home.arpa"].iter().any(|s| host.ends_with(s))
}

fn host_is_allowed(host: &str, allowed: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    allowed.iter().any(|a| match a.strip_prefix("*.") {
        Some(domain) => host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.')),
        None => *a == host,
    })
}

/// Tipos que uma fonte de leitura tem motivo para receber.
fn content_type_is_expected(value: &str) -> bool {
    let kind = value.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    kind.starts_with("text/")
        || kind.starts_with("image/")
        || kind.ends_with("+json")
        || kind.ends_with("+xml")
        || matches!(kind.as_str(), "application/json" | "application/xml" | "application/octet-stream" | "binary/octet-stream")
}

// ---------------------------------------------------------------------------
// DNS
// ---------------------------------------------------------------------------

struct PublicResolver;

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs = tauri::async_runtime::spawn_blocking(move || (host.as_str(), 0).to_socket_addrs().map(Iterator::collect::<Vec<_>>)).await??;
            let public: Vec<SocketAddr> = addrs.into_iter().filter(|a| is_public_ip(a.ip())).collect();
            if public.is_empty() {
                return Err("o nome aponta para um endereço local".into());
            }
            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

// ---------------------------------------------------------------------------
// Cliente
// ---------------------------------------------------------------------------

pub struct Net {
    /// Só os testes, ou um build de depuração com `PRANCHA_E2E_ALLOW_LOOPBACK`, ligam isto.
    allow_loopback: bool,
    client: OnceLock<Result<reqwest::Client, String>>,
}

impl Net {
    pub fn new(allow_loopback: bool) -> Self {
        Net { allow_loopback, client: OnceLock::new() }
    }

    pub fn from_env() -> Self {
        Net::new(cfg!(debug_assertions) && std::env::var_os("PRANCHA_E2E_ALLOW_LOOPBACK").is_some())
    }

    pub fn allows_loopback(&self) -> bool {
        self.allow_loopback
    }

    /// Conexões são reaproveitadas entre as chamadas.
    fn client(&self) -> Result<reqwest::Client, NetError> {
        let built = self.client.get_or_init(|| {
            // O plugin de atualização instala este mesmo provedor; quem chegar primeiro vale.
            let _ = rustls::crypto::ring::default_provider().install_default();
            reqwest::Client::builder()
                .user_agent(concat!("Prancha/", env!("CARGO_PKG_VERSION")))
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .connect_timeout(CONNECT_TIMEOUT)
                .dns_resolver(Arc::new(PublicResolver))
                .build()
                .map_err(|e| e.to_string())
        });
        built.clone().map_err(NetError::Network)
    }

    /// Só a exceção dos testes aceita HTTP, e só em 127.0.0.1.
    fn is_test_loopback(&self, url: &Url) -> bool {
        self.allow_loopback && url.host_str() == Some("127.0.0.1")
    }

    /// Confere um destino sem conectar.
    pub fn check(&self, url: &Url, access: Access) -> Result<(), NetError> {
        let blocked = |why: &str| Err(NetError::Blocked(why.to_string()));
        let Some(host) = url.host_str() else { return blocked("sem host") };
        if !url.username().is_empty() || url.password().is_some() {
            return blocked("credenciais no endereço");
        }
        if !self.is_test_loopback(url) {
            if url.scheme() != "https" {
                return blocked("só HTTPS é aceito");
            }
            if url.port().is_some_and(|p| p != 443) {
                return blocked("só a porta 443 é aceita");
            }
            match url.host() {
                Some(url::Host::Ipv4(ip)) if !ipv4_is_public(ip) => return blocked("endereço local"),
                Some(url::Host::Ipv6(ip)) if !ipv6_is_public(ip) => return blocked("endereço local"),
                Some(url::Host::Domain(name)) if is_local_name(name) => return blocked("nome local"),
                _ => {}
            }
        }
        if let Access::Hosts(allowed) = access {
            if !host_is_allowed(host, allowed) {
                return Err(NetError::Blocked(format!("{host} não está entre os hosts autorizados")));
            }
        }
        Ok(())
    }

    /// GET com limites. Bloqueia a thread até terminar, ser cancelado ou estourar o prazo.
    pub fn get(&self, url: &str, access: Access, limits: Limits, request: Request) -> Result<Response, NetError> {
        let parsed = Url::parse(url).map_err(|_| NetError::Blocked("endereço inválido".into()))?;
        self.check(&parsed, access)?;
        let client = self.client()?;
        let deadline = Instant::now() + limits.timeout;
        let mut current = parsed;
        for _ in 0..=MAX_REDIRECTS {
            let (status, location, response) = self.one_hop(&client, &current, limits, &request, deadline)?;
            let Some(location) = location else { return Ok(response.expect("resposta sem redirecionamento")) };
            debug_assert!(status.is_redirection());
            let next = current.join(&location).map_err(|_| NetError::Blocked("redirecionamento inválido".into()))?;
            self.check(&next, access)?;
            current = next;
        }
        Err(NetError::Blocked("redirecionamentos demais".into()))
    }

    fn one_hop(&self, client: &reqwest::Client, url: &Url, limits: Limits, request: &Request, deadline: Instant) -> Result<(StatusCode, Option<String>, Option<Response>), NetError> {
        let mut builder = client.get(url.clone());
        for (name, value) in request.headers {
            let name = name.to_ascii_lowercase();
            if !ALLOWED_HEADERS.contains(&name.as_str()) {
                return Err(NetError::Blocked(format!("o cabeçalho {name} não é permitido")));
            }
            let value = HeaderValue::from_str(value).map_err(|_| NetError::Blocked("cabeçalho inválido".into()))?;
            builder = builder.header(HeaderName::from_bytes(name.as_bytes()).map_err(|_| NetError::Blocked("cabeçalho inválido".into()))?, value);
        }
        if let Some(etag) = request.etag.and_then(|e| HeaderValue::from_str(e).ok()) {
            builder = builder.header(IF_NONE_MATCH, etag);
        }
        let (url_text, check_type, max) = (url.to_string(), request.check_content_type, limits.max_bytes);
        let (tx, rx) = channel();
        let task = tauri::async_runtime::spawn(async move {
            let _ = tx.send(read_response(builder, url_text, check_type, max).await);
        });
        loop {
            match rx.recv_timeout(WAIT_STEP) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Timeout) => {
                    if request.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                        task.abort();
                        return Err(NetError::Cancelled);
                    }
                    if Instant::now() >= deadline {
                        task.abort();
                        return Err(NetError::Timeout);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Err(NetError::Network("a requisição foi interrompida".into())),
            }
        }
    }
}

async fn read_response(builder: reqwest::RequestBuilder, url: String, check_type: bool, max: u64) -> Result<(StatusCode, Option<String>, Option<Response>), NetError> {
    let mut response = builder.send().await.map_err(|e| {
        if e.is_timeout() { NetError::Timeout } else { NetError::Network(root_cause(&e)) }
    })?;
    let status = response.status();
    if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
        let location = response.headers().get(LOCATION).and_then(|v| v.to_str().ok()).map(str::to_string);
        let Some(location) = location else { return Err(NetError::Network("redirecionamento sem destino".into())) };
        return Ok((status, Some(location), None));
    }
    let content_type = response.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string);
    if check_type && status.is_success() && content_type.as_deref().is_some_and(|c| !content_type_is_expected(c)) {
        return Err(NetError::Blocked(format!("tipo de conteúdo inesperado: {}", content_type.unwrap_or_default())));
    }
    let etag = response.headers().get(ETAG).and_then(|v| v.to_str().ok()).map(str::to_string);
    if response.content_length().is_some_and(|n| n > max) {
        return Err(NetError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| if e.is_timeout() { NetError::Timeout } else { NetError::Network(root_cause(&e)) })? {
        // O tamanho declarado não vale como garantia: o limite é contado sobre o que chega.
        if body.len() as u64 + chunk.len() as u64 > max {
            return Err(NetError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok((status, None, Some(Response { status: status.as_u16(), url, content_type, etag, body })))
}

/// A causa mais específica, sem o endereço completo que o `reqwest` repete em cada nível.
fn root_cause(error: &(dyn std::error::Error + 'static)) -> String {
    let mut current = error;
    while let Some(source) = current.source() {
        current = source;
    }
    current.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn local_ipv4_addresses_are_not_public() {
        for bad in [
            "0.0.0.0", "0.1.2.3", "10.0.0.1", "100.64.0.1", "100.127.255.255", "127.0.0.1", "127.8.8.8", "169.254.169.254", "172.16.0.1", "172.31.255.255",
            "192.0.0.1", "192.0.2.1", "192.168.0.1", "198.18.0.1", "198.19.255.255", "198.51.100.1", "203.0.113.1", "224.0.0.1", "240.0.0.1", "255.255.255.255",
        ] {
            assert!(!is_public_ip(ip(bad)), "{bad}");
        }
        for good in ["1.1.1.1", "8.8.8.8", "100.63.255.255", "100.128.0.1", "172.15.255.255", "172.32.0.1", "192.167.0.1", "198.20.0.1", "223.255.255.255"] {
            assert!(is_public_ip(ip(good)), "{good}");
        }
    }

    #[test]
    fn local_ipv6_addresses_are_not_public_even_when_disguised() {
        for bad in [
            "::", "::1", "fe80::1", "fec0::1", "fc00::1", "fd12:3456::1", "ff02::1", "2001:db8::1", "2001:0:4136:e378:8000:63bf:3fff:fdd2",
            "::ffff:127.0.0.1", "::ffff:10.0.0.1", "::ffff:169.254.169.254", "64:ff9b::7f00:1", "64:ff9b::a9fe:a9fe", "2002:7f00:1::", "2002:c0a8:1::", "::127.0.0.1",
            "64:ff9b:1::a00:1", "::ffff:0:a00:1", "100::1",
        ] {
            assert!(!is_public_ip(ip(bad)), "{bad}");
        }
        for good in ["2606:4700:4700::1111", "::ffff:8.8.8.8", "64:ff9b::808:808", "2002:808:808::"] {
            assert!(is_public_ip(ip(good)), "{good}");
        }
    }

    #[test]
    fn only_https_on_443_to_public_names_is_accepted() {
        let net = Net::new(false);
        let check = |u: &str| net.check(&Url::parse(u).unwrap(), Access::Public);
        assert!(check("https://exemplo.com/x.json").is_ok());
        assert!(check("https://exemplo.com:443/x.json").is_ok());
        for bad in [
            "http://exemplo.com/",
            "ftp://exemplo.com/",
            "file:///C:/Windows/win.ini",
            "https://exemplo.com:8443/",
            "https://usuario:senha@exemplo.com/",
            "https://localhost/",
            "https://prancha.localhost/",
            "https://impressora.local/",
            "https://servidor/",
            "https://router.lan/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://169.254.169.254/latest/meta-data/",
            "https://2130706433/", // 127.0.0.1 em decimal: o parser normaliza
            "https://0x7f.0.0.1/",
        ] {
            assert!(matches!(check(bad), Err(NetError::Blocked(_))), "{bad}");
        }
    }

    #[test]
    fn hosts_must_match_exactly_or_by_declared_subdomain() {
        let net = Net::new(false);
        let allowed = vec!["exemplo.com".to_string(), "*.cdn.exemplo.net".to_string()];
        let check = |u: &str| net.check(&Url::parse(u).unwrap(), Access::Hosts(&allowed));
        assert!(check("https://exemplo.com/a").is_ok());
        assert!(check("https://EXEMPLO.com./a").is_ok());
        assert!(check("https://img.cdn.exemplo.net/a").is_ok());
        for bad in ["https://sub.exemplo.com/", "https://exemplo.com.evil.net/", "https://evilexemplo.com/", "https://cdn.exemplo.net/", "https://xcdn.exemplo.net/"] {
            assert!(matches!(check(bad), Err(NetError::Blocked(_))), "{bad}");
        }
    }

    #[test]
    fn the_test_exception_covers_only_the_numeric_loopback() {
        let net = Net::new(true);
        let check = |u: &str| net.check(&Url::parse(u).unwrap(), Access::Public);
        assert!(check("http://127.0.0.1:8080/").is_ok());
        for bad in ["http://localhost:8080/", "http://[::1]:8080/", "http://10.0.0.1/", "https://169.254.169.254/", "http://exemplo.com/"] {
            assert!(check(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn unexpected_content_types_are_refused() {
        for good in ["text/html; charset=utf-8", "application/json", "application/ld+json", "image/webp", "application/xml", "application/rss+xml", "application/octet-stream"] {
            assert!(content_type_is_expected(good), "{good}");
        }
        for bad in ["application/x-msdownload", "application/zip", "video/mp4", "audio/mpeg", "application/x-sh"] {
            assert!(!content_type_is_expected(bad), "{bad}");
        }
    }

    #[test]
    fn a_name_that_resolves_to_a_local_address_is_refused_by_the_resolver() {
        let name: Name = "localhost".parse().unwrap();
        let outcome = tauri::async_runtime::block_on(PublicResolver.resolve(name));
        assert!(outcome.is_err());
    }
}
