//! Servidor HTTP mínimo em 127.0.0.1: responde o que o teste mandar e guarda o que recebeu.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
    declare_length: bool,
}

impl Reply {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Reply { status: 200, headers: Vec::new(), body: body.into(), delay: Duration::ZERO, declare_length: true }
    }

    pub fn status(status: u16) -> Self {
        Reply { status, ..Reply::ok("") }
    }

    pub fn redirect(to: &str) -> Self {
        Reply::status(302).header("Location", to)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn after(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// O corpo termina com o fim da conexão: o cliente só descobre o tamanho lendo.
    pub fn without_length(mut self) -> Self {
        self.declare_length = false;
        self
    }
}

pub struct Server {
    pub base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// A thread de escuta vive até o fim do processo de teste.
pub fn serve(handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (log, handler) = (seen.clone(), Arc::new(handler));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (log, handler) = (log.clone(), handler.clone());
            std::thread::spawn(move || answer(stream, &log, &*handler));
        }
    });
    Server { base, seen }
}

fn answer(mut stream: TcpStream, log: &Mutex<Vec<Seen>>, handler: &(dyn Fn(&Seen) -> Reply + Send + Sync)) {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 2048];
    while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(n) => raw.extend_from_slice(&buffer[..n]),
        }
    }
    let head_end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let text = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let mut lines = text.split("\r\n");
    let path = lines.next().and_then(|l| l.split(' ').nth(1)).unwrap_or("/").to_string();
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(": ")).map(|(n, v)| (n.to_string(), v.to_string())).collect();
    let length = headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0);
    while raw.len() < head_end + length {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(n) => raw.extend_from_slice(&buffer[..n]),
        }
    }
    let request = Seen { path, headers, body: raw[head_end..].to_vec() };
    log.lock().unwrap().push(request.clone());
    let reply = handler(&request);
    std::thread::sleep(reply.delay);
    let mut head = format!("HTTP/1.1 {} X\r\nConnection: close\r\n", reply.status);
    if reply.declare_length {
        head.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
    }
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&reply.body);
}
