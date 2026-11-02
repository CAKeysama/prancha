//! Repositório assinado servido por HTTP local, nos mesmos formatos que o `prancha-ext` gera.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signer, SigningKey};
use prancha_lib::extensions::signing::sha256_hex;
use serde_json::{Value, json};

use super::http::{Reply, Server, serve};

type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

pub struct TestRepo {
    pub server: Server,
    pub id: String,
    key: Mutex<SigningKey>,
    files: Files,
    clock: AtomicU32,
}

impl TestRepo {
    pub fn new(id: &str, seed: u8) -> Self {
        let files: Files = Default::default();
        let served = files.clone();
        let server = serve(move |seen| {
            let files = served.lock().unwrap();
            let Some(body) = files.get(&seen.path) else { return Reply::status(404) };
            let etag = format!("\"{}\"", sha256_hex(body));
            if seen.header("if-none-match") == Some(etag.as_str()) {
                return Reply::status(304);
            }
            Reply::ok(body.clone()).header("ETag", &etag)
        });
        TestRepo { server, id: id.to_string(), key: Mutex::new(SigningKey::from_bytes(&[seed; 32])), files, clock: AtomicU32::new(0) }
    }

    pub fn manifest_url(&self) -> String {
        self.server.url("/prancha-repo.json")
    }

    pub fn fingerprint(&self) -> String {
        sha256_hex(self.key.lock().unwrap().verifying_key().as_bytes())
    }

    pub fn change_key(&self, seed: u8) {
        *self.key.lock().unwrap() = SigningKey::from_bytes(&[seed; 32]);
    }

    pub fn put(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.files.lock().unwrap().insert(path.to_string(), body.into());
    }

    pub fn remove(&self, path: &str) {
        self.files.lock().unwrap().remove(path);
    }

    fn sign(&self, message: &[u8]) -> Vec<u8> {
        B64.encode(self.key.lock().unwrap().sign(message).to_bytes()).into_bytes()
    }

    /// Publica o pacote e devolve a entrada do índice. Fontes e capacidades podem ser trocadas depois.
    pub fn entry(&self, id: &str, version: &str, wasm: &[u8]) -> Value {
        let hash = sha256_hex(wasm);
        let name = format!("{id}-{version}.wasm");
        self.put(&format!("/{name}"), wasm);
        let message = format!("prancha-package-v1\0{}\0{id}\0{version}\0{hash}", self.id);
        self.put(&format!("/{name}.sig"), self.sign(message.as_bytes()));
        json!({
            "id": id,
            "name": format!("Extensão {id}"),
            "version": version,
            "description": "Extensão de teste",
            "authors": ["Testes"],
            "languages": ["pt-BR"],
            "runtime": "wasm-component",
            "apiVersion": "1",
            "minAppVersion": "0.1.0",
            "artifactUrl": self.server.url(&format!("/{name}")),
            "size": wasm.len(),
            "sha256": hash,
            "signatureUrl": self.server.url(&format!("/{name}.sig")),
            "supportedKinds": ["comic"],
            "contentRating": "general",
            "sources": [{ "id": "exemplo", "name": "Fonte de exemplo", "language": "pt-BR", "contentRating": "general" }],
            "capabilities": { "network": false, "storage": false, "authentication": false, "allowedHosts": [] }
        })
    }

    pub fn manifest(&self) -> Value {
        json!({
            "schemaVersion": 1,
            "id": self.id,
            "name": format!("Repositório {}", self.id),
            "description": "Repositório de teste",
            "website": "https://exemplo.com",
            "indexUrl": self.server.url("/prancha-index.json"),
            "publicKey": B64.encode(self.key.lock().unwrap().verifying_key().as_bytes()),
            "updatedAt": "2026-01-01T00:00:00Z",
        })
    }

    /// Índice e manifesto novos. A data avança a cada publicação.
    pub fn publish(&self, entries: &[Value]) {
        let tick = self.clock.fetch_add(1, Ordering::Relaxed);
        self.publish_dated(entries, &format!("2026-01-01T00:{:02}:{:02}Z", tick / 60, tick % 60));
    }

    pub fn publish_dated(&self, entries: &[Value], generated_at: &str) {
        let index = json!({ "schemaVersion": 1, "repositoryId": self.id, "generatedAt": generated_at, "extensions": entries });
        self.put_index(serde_json::to_vec_pretty(&index).unwrap());
        self.put("/prancha-repo.json", serde_json::to_vec_pretty(&self.manifest()).unwrap());
    }

    /// Bytes assinados como índice, sejam eles válidos ou não.
    pub fn put_index(&self, bytes: Vec<u8>) {
        let signature = self.sign(&[b"prancha-index-v1\0".as_slice(), &bytes].concat());
        self.put("/prancha-index.json", bytes);
        self.put("/prancha-index.json.sig", signature);
    }
}
