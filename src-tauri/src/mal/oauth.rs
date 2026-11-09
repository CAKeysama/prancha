//! Autorização OAuth 2.0 com código e PKCE, como um aplicativo nativo (RFC 8252): o Prancha abre a página do MAL
//! no navegador da pessoa, que entra e autoriza lá (a senha nunca passa pelo Prancha), e o MAL devolve o código para
//! um endereço local que só existe enquanto a conexão está em andamento.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use reqwest::Url;

use crate::error::{AppError, AppResult};
use crate::extensions::net::Cancel;

/// Porta fixa: o endereço de retorno precisa ser idêntico ao cadastrado no MAL.
pub const REDIRECT_PORT: u16 = 47215;
pub const REDIRECT_PATH: &str = "/mal/callback";
/// Tempo para a pessoa entrar e autorizar no navegador.
pub const AUTHORIZE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub fn redirect_uri(port: u16) -> String {
    format!("http://localhost:{port}{REDIRECT_PATH}")
}

const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

/// Texto aleatório só com os caracteres que o PKCE permite, sem viés (os bytes que sobrariam são descartados).
fn random_text(len: usize, alphabet: &[u8]) -> String {
    let limit = 256 - (256 % alphabet.len());
    let mut out = String::with_capacity(len);
    let mut buf = [0u8; 64];
    while out.len() < len {
        getrandom::fill(&mut buf).expect("gerador aleatório do sistema");
        for b in buf {
            if (b as usize) < limit && out.len() < len {
                out.push(alphabet[b as usize % alphabet.len()] as char);
            }
        }
    }
    out
}

/// Verificador PKCE: 43 a 128 caracteres. Um novo a cada conexão.
pub fn new_verifier() -> String {
    random_text(96, UNRESERVED)
}

/// Estado contra falsificação: a resposta só vale se trouxer o mesmo.
pub fn new_state() -> String {
    random_text(32, b"0123456789abcdef")
}

pub struct Callback {
    listeners: Vec<TcpListener>,
    port: u16,
}

/// Abre o endereço local de retorno. Só no próprio computador (127.0.0.1 e ::1), nunca na rede.
pub fn listen(port: u16) -> AppResult<Callback> {
    let busy = || AppError::user("mal_port_busy", format!("A porta {port} deste computador está em uso por outro programa. Feche-o e tente conectar de novo."));
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
    Ok(Callback { listeners, port })
}

/// O que chegou no endereço de retorno.
#[derive(Debug, PartialEq, Eq)]
enum Arrival {
    Code(String),
    Denied,
    /// Outro caminho, estado diferente, requisição estranha: responde e continua esperando.
    Ignored,
}

fn page(title: &str, text: &str) -> String {
    format!(
        "<!doctype html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\"><title>Prancha</title>\
         <style>body{{font-family:system-ui,sans-serif;background:#16181d;color:#e8e9ec;display:grid;place-items:center;height:100vh;margin:0}}\
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

fn handle(mut stream: TcpStream, expected_state: &str) -> Arrival {
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
        respond(&mut stream, "404 Not Found", &page("Página não encontrada", "Este endereço só existe enquanto o Prancha se conecta ao MyAnimeList."));
        return Arrival::Ignored;
    };
    let param = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
    if param("state").as_deref() != Some(expected_state) {
        respond(&mut stream, "400 Bad Request", &page("Pedido não reconhecido", "Esta resposta não corresponde à conexão em andamento no Prancha. Volte ao aplicativo e tente de novo."));
        return Arrival::Ignored;
    }
    if param("error").is_some() {
        respond(&mut stream, "200 OK", &page("Conexão cancelada", "O acesso não foi autorizado. Você pode fechar esta aba."));
        return Arrival::Denied;
    }
    match param("code").filter(|c| !c.is_empty() && c.len() <= 4096) {
        Some(code) => {
            respond(&mut stream, "200 OK", &page("Conta conectada", "Pronto! Pode fechar esta aba e voltar ao Prancha."));
            Arrival::Code(code)
        }
        None => {
            respond(&mut stream, "400 Bad Request", &page("Resposta incompleta", "O MyAnimeList não enviou o código de autorização. Tente de novo pelo Prancha."));
            Arrival::Ignored
        }
    }
}

impl Callback {
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Espera o MAL devolver a pessoa com o código. Termina com o código, com a recusa, no cancelamento ou no prazo.
    pub fn wait(&self, state: &str, cancel: &Cancel, timeout: Duration) -> AppResult<String> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::user("cancelled", "Conexão cancelada."));
            }
            if Instant::now() >= deadline {
                return Err(AppError::user("mal_timeout", "O tempo para autorizar no navegador acabou. Tente conectar de novo."));
            }
            for listener in &self.listeners {
                match listener.accept() {
                    Ok((stream, _)) => match handle(stream, state) {
                        Arrival::Code(code) => return Ok(code),
                        Arrival::Denied => return Err(AppError::user("mal_denied", "Você não autorizou o acesso no MyAnimeList.")),
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
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn get(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(stream, "GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = stream.read_to_string(&mut out);
        out
    }

    #[test]
    fn verificador_e_estado_seguem_o_pkce() {
        let v = new_verifier();
        assert!((43..=128).contains(&v.len()));
        assert!(v.bytes().all(|b| UNRESERVED.contains(&b)));
        assert_ne!(v, new_verifier(), "um novo a cada conexão");
        assert_eq!(new_state().len(), 32);
    }

    #[test]
    fn recebe_o_codigo_so_com_o_estado_certo() {
        let callback = listen(0).unwrap();
        let port = callback.port();
        let cancel: Cancel = Arc::new(AtomicBool::new(false));
        let client = std::thread::spawn(move || {
            let favicon = get(port, "/favicon.ico");
            let forged = get(port, &format!("{REDIRECT_PATH}?code=falso&state=outro"));
            let ok = get(port, &format!("{REDIRECT_PATH}?code=abc123&state=estado-certo"));
            (favicon, forged, ok)
        });
        let code = callback.wait("estado-certo", &cancel, Duration::from_secs(10)).unwrap();
        let (favicon, forged, ok) = client.join().unwrap();
        assert_eq!(code, "abc123");
        assert!(favicon.starts_with("HTTP/1.1 404"));
        assert!(forged.starts_with("HTTP/1.1 400"));
        assert!(ok.contains("Conta conectada"));
        assert!(!ok.contains("abc123"), "o código não volta na página");
    }

    #[test]
    fn recusa_e_cancelamento_terminam_a_espera() {
        let callback = listen(0).unwrap();
        let port = callback.port();
        let cancel: Cancel = Arc::new(AtomicBool::new(false));
        let client = std::thread::spawn(move || get(port, &format!("{REDIRECT_PATH}?error=access_denied&state=s")));
        let err = callback.wait("s", &cancel, Duration::from_secs(10)).unwrap_err();
        client.join().unwrap();
        assert_eq!(err.code(), "mal_denied");

        cancel.store(true, Ordering::Relaxed);
        assert_eq!(callback.wait("s", &cancel, Duration::from_secs(10)).unwrap_err().code(), "cancelled");
        cancel.store(false, Ordering::Relaxed);
        assert_eq!(callback.wait("s", &cancel, Duration::from_millis(10)).unwrap_err().code(), "mal_timeout");
    }
}
