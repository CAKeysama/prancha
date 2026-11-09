//! Testes de integração das extensões. Os componentes são compilados de `extensions/`.

#[path = "common/http.rs"]
#[allow(dead_code)]
mod http;
#[allow(dead_code)]
mod common;
#[path = "common/repo.rs"]
#[allow(dead_code)]
mod repo;
#[path = "common/wasm.rs"]
mod wasm;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use http::{Reply, serve};
use prancha_lib::core::Core;
use prancha_lib::error::AppError;
use prancha_lib::extensions::reading::{self, ReadState};
use prancha_lib::extensions::{downloads, install, sources, suwayomi};
use prancha_lib::library;
use prancha_lib::extensions::net::{Access, Limits, Net, NetError, Request};
use prancha_lib::extensions::repository::{self, CatalogQuery, MIHON_MESSAGE};
use prancha_lib::extensions::runtime::{ContentRating, EngineLimits, Grant, Installed, SourceEngine};
use repo::TestRepo;
use serde_json::{Value, json};

#[test]
fn template_compiles_to_a_component_without_wasi_and_runs() {
    let dir = tempfile::tempdir().unwrap();
    let engine = SourceEngine::new(Arc::new(Net::new(false)), dir.path().join("t.db")).unwrap();
    let sources = engine.list_sources(&wasm::component("prancha-extension-template")).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, "exemplo");
    assert!(matches!(sources[0].rating, ContentRating::General));
}

// ---------------------------------------------------------------------------
// Source Engine
// ---------------------------------------------------------------------------

const FIXTURE: &str = "prancha-extension-test-fixture";

struct Sandbox {
    dir: tempfile::TempDir,
    engine: SourceEngine,
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl Sandbox {
    fn new(limits: EngineLimits) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        let conn = prancha_lib::db::open(&db_path).unwrap();
        let engine = SourceEngine::with_limits(Arc::new(Net::new(true)), db_path, limits).unwrap();
        Sandbox { dir, engine, conn: conn.into() }
    }

    /// Registra a extensão de teste com as capacidades indicadas.
    fn install(&self, id: &str, grant: Grant) -> Installed {
        let wasm = wasm::component(FIXTURE);
        let sha256 = prancha_lib::extensions::signing::sha256_hex(&wasm);
        let file = self.dir.path().join(format!("{id}.wasm"));
        std::fs::write(&file, &wasm).unwrap();
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO installed_extensions (id, repository_id, repository_name, public_key, key_fingerprint, name, version, entry, file_name, sha256, installed_at, updated_at)
                 VALUES (?1, 'r', 'R', 'k', 'f', 'E', '1.0.0', '{}', 'e.wasm', ?2, 1, 1)",
                [id, sha256.as_str()],
            )
            .unwrap();
        Installed { id: id.to_string(), file, sha256, grant, bridged: false }
    }

    fn command(&self, extension: &Installed, query: &str) -> Result<String, AppError> {
        self.engine.search(extension, None, "livre", query, 1, &[]).map(|page| page.results.first().map(|r| r.title.clone()).unwrap_or_default())
    }
}

fn free() -> Grant {
    Grant::default()
}

fn network(hosts: &[&str]) -> Grant {
    Grant { network: true, storage: false, allowed_hosts: hosts.iter().map(|h| h.to_string()).collect() }
}

fn storage() -> Grant {
    Grant { network: false, storage: true, allowed_hosts: vec![] }
}

#[test]
fn a_valid_call_returns_data() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let extension = sandbox.install("e", free());
    assert_eq!(sandbox.command(&extension, "Aurora").unwrap(), "Aurora 1");
    let details = sandbox.engine.details(&extension, None, "livre", "obra").unwrap();
    assert_eq!(details.title, "Obra obra");
    assert_eq!(sandbox.engine.chapters(&extension, None, "livre", "obra").unwrap().len(), 3);
    assert_eq!(sandbox.engine.pages(&extension, None, "livre", "obra-1").unwrap().len(), 2);
}

#[test]
fn an_endless_loop_hits_the_time_limit() {
    let sandbox = Sandbox::new(EngineLimits { fuel: u64::MAX / 2, call_timeout: Duration::from_millis(300), ..Default::default() });
    let extension = sandbox.install("e", free());
    // A primeira chamada compila, o que em um build de teste leva segundos e não conta como execução.
    assert_eq!(sandbox.command(&extension, "ok").unwrap(), "ok 1");
    let started = Instant::now();
    assert_eq!(code(sandbox.command(&extension, "laco")), "extension_timeout");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
}

#[test]
fn an_endless_loop_can_be_cancelled() {
    let sandbox = Sandbox::new(EngineLimits { fuel: u64::MAX / 2, call_timeout: Duration::from_secs(60), ..Default::default() });
    let extension = sandbox.install("e", free());
    // Sem isto, a compilação gasta o tempo e o cancelamento chega antes de o laço começar.
    assert_eq!(sandbox.command(&extension, "ok").unwrap(), "ok 1");
    let cancel: Arc<AtomicBool> = Arc::default();
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let outcome = sandbox.engine.search(&extension, Some(&cancel), "livre", "laco", 1, &[]);
    assert_eq!(code(outcome.map(|_| ())), "extension_cancelled");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
}

#[test]
fn too_much_processing_runs_out_of_fuel() {
    let sandbox = Sandbox::new(EngineLimits { fuel: 50_000_000, ..Default::default() });
    let extension = sandbox.install("e", free());
    assert_eq!(code(sandbox.command(&extension, "laco")), "extension_fuel");
}

#[test]
fn too_much_memory_is_refused() {
    let sandbox = Sandbox::new(EngineLimits { memory_bytes: 8 * 1024 * 1024, ..Default::default() });
    let extension = sandbox.install("e", free());
    assert_eq!(code(sandbox.command(&extension, "memoria")), "extension_memory");
}

#[test]
fn a_stack_overflow_and_a_panic_become_errors_and_the_engine_keeps_working() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let bad = sandbox.install("ruim", free());
    let good = sandbox.install("boa", free());
    assert_eq!(code(sandbox.command(&bad, "pilha")), "extension_crashed");
    assert_eq!(code(sandbox.command(&bad, "panico")), "extension_crashed");
    assert_eq!(sandbox.command(&good, "ok").unwrap(), "ok 1");
    assert_eq!(sandbox.command(&bad, "ok").unwrap(), "ok 1", "cada chamada usa uma instância nova");
}

#[test]
fn files_and_environment_do_not_exist_for_an_extension() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let secret = sandbox.dir.path().join("segredo.txt");
    std::fs::write(&secret, "segredo").unwrap();
    let extension = sandbox.install("e", free());
    assert_eq!(sandbox.command(&extension, &format!("arquivo:{}", secret.display())).unwrap(), "sem acesso");
    assert_eq!(sandbox.command(&extension, "arquivo:t.db").unwrap(), "sem acesso");
    assert_eq!(sandbox.command(&extension, "ambiente").unwrap(), "sem acesso");
}

#[test]
fn a_component_that_imports_wasi_does_not_load() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let component = wat::parse_str(r#"(component (import "wasi:filesystem/types@0.2.0" (instance)))"#).unwrap();
    assert_eq!(code(sandbox.engine.list_sources(&component)), "extension_load");
    assert_eq!(code(sandbox.engine.list_sources(b"nao e wasm")), "extension_load");
}

#[test]
fn the_network_requires_the_capability_and_an_authorized_host() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let server = serve(|_| Reply::ok("corpo"));
    let url = server.url("/a");

    let no_capability = sandbox.install("sem", free());
    let refused = sandbox.command(&no_capability, &format!("rede:{url}")).unwrap_err();
    assert_eq!(refused.code(), "source_network");
    assert!(refused.to_string().contains("Blocked"), "{refused}");

    let other_host = sandbox.install("outro", network(&["exemplo.com"]));
    assert!(sandbox.command(&other_host, &format!("rede:{url}")).unwrap_err().to_string().contains("Blocked"));
    assert!(server.seen().is_empty(), "nada chegou ao servidor");

    let authorized = sandbox.install("autorizada", network(&["127.0.0.1"]));
    assert_eq!(sandbox.command(&authorized, &format!("rede:{url}")).unwrap(), "200 5 bytes");
    assert_eq!(server.seen().len(), 1);
}

#[test]
fn a_redirect_cannot_leave_the_authorized_hosts() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let extension = sandbox.install("e", network(&["127.0.0.1"]));
    for target in ["http://169.254.169.254/latest/meta-data/", "http://10.0.0.1/", "https://exemplo.com/"] {
        let server = serve(move |_| Reply::redirect(target));
        let refused = sandbox.command(&extension, &format!("rede:{}", server.url("/a"))).unwrap_err();
        assert!(refused.to_string().contains("Blocked"), "{target}: {refused}");
    }
}

#[test]
fn an_extension_cannot_set_private_headers() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let server = serve(|_| Reply::ok("corpo"));
    let extension = sandbox.install("e", network(&["127.0.0.1"]));
    assert!(sandbox.command(&extension, &format!("cabecalho:{}", server.url("/a"))).unwrap_err().to_string().contains("Blocked"));
    assert!(server.seen().is_empty());
}

#[test]
fn a_response_that_is_too_large_does_not_reach_the_extension() {
    let sandbox = Sandbox::new(EngineLimits { response_bytes: 1024, ..Default::default() });
    let server = serve(|_| Reply::ok(vec![b'x'; 4096]).without_length());
    let extension = sandbox.install("e", network(&["127.0.0.1"]));
    assert!(sandbox.command(&extension, &format!("rede:{}", server.url("/a"))).unwrap_err().to_string().contains("TooLarge"));
}

#[test]
fn the_number_of_requests_per_call_is_limited() {
    let sandbox = Sandbox::new(EngineLimits { requests_per_call: 3, ..Default::default() });
    let server = serve(|_| Reply::ok("corpo"));
    let extension = sandbox.install("e", network(&["127.0.0.1"]));
    assert_eq!(sandbox.command(&extension, &format!("muitas:{}", server.url("/a"))).unwrap(), "3 e depois HttpError::TooManyRequests");
    assert_eq!(server.seen().len(), 3);
}

#[test]
fn storage_is_isolated_per_extension_and_limited() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let one = sandbox.install("um", storage());
    let two = sandbox.install("dois", storage());
    let none = sandbox.install("nenhum", free());

    assert_eq!(sandbox.command(&one, "guardar:chave=valor um").unwrap(), "guardou");
    assert_eq!(sandbox.command(&one, "ler:chave").unwrap(), "valor um", "sobrevive entre instâncias");
    assert_eq!(sandbox.command(&two, "ler:chave").unwrap(), "nada", "outra extensão não enxerga");
    assert_eq!(sandbox.command(&two, "guardar:chave=valor dois").unwrap(), "guardou");
    assert_eq!(sandbox.command(&one, "ler:chave").unwrap(), "valor um");

    assert_eq!(code(sandbox.command(&none, "guardar:chave=x")), "source_data", "sem a capacidade não grava");
    assert_eq!(sandbox.command(&none, "ler:chave").unwrap(), "nada");

    assert_eq!(sandbox.command(&one, "encher").unwrap(), "5 blocos", "o limite é de 1 MB");
    assert_eq!(sandbox.command(&two, "ler:chave").unwrap(), "valor dois");
    assert_eq!(sandbox.command(&two, "apagar:chave").unwrap(), "apagou");
    assert_eq!(sandbox.command(&two, "ler:chave").unwrap(), "nada");
    assert_eq!(sandbox.command(&one, "ler:chave").unwrap(), "valor um");
}

#[test]
fn a_slow_extension_does_not_block_another() {
    let sandbox = Arc::new(Sandbox::new(EngineLimits { fuel: u64::MAX / 2, call_timeout: Duration::from_secs(2), concurrent_calls: 3, concurrent_per_extension: 1, ..Default::default() }));
    let slow = sandbox.install("lenta", free());
    let quick = sandbox.install("rapida", free());
    // A primeira chamada compila, o que em um build de teste leva segundos.
    assert_eq!(sandbox.command(&quick, "ok").unwrap(), "ok 1");
    let running = {
        let (sandbox, slow) = (sandbox.clone(), slow.clone());
        std::thread::spawn(move || sandbox.command(&slow, "laco").map(|_| ()))
    };
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    assert_eq!(sandbox.command(&quick, "ok").unwrap(), "ok 1");
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
    assert_eq!(code(running.join().unwrap()), "extension_timeout");
}

#[test]
fn calls_above_the_limit_wait_for_a_free_slot_and_can_give_up() {
    let sandbox = Arc::new(Sandbox::new(EngineLimits { fuel: u64::MAX / 2, call_timeout: Duration::from_millis(600), concurrent_calls: 1, concurrent_per_extension: 1, ..Default::default() }));
    let extension = sandbox.install("e", free());
    let running = {
        let (sandbox, extension) = (sandbox.clone(), extension.clone());
        std::thread::spawn(move || sandbox.command(&extension, "laco").map(|_| ()))
    };
    std::thread::sleep(Duration::from_millis(100));
    let cancel: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
    let outcome = sandbox.engine.search(&extension, Some(&cancel), "livre", "ok", 1, &[]);
    assert_eq!(code(outcome.map(|_| ())), "extension_cancelled", "cancelado enquanto espera");
    assert_eq!(sandbox.command(&extension, "ok").unwrap_or_else(|e| e.code().to_string()), "ok 1", "a vaga é liberada ao fim do prazo da outra");
    assert_eq!(code(running.join().unwrap()), "extension_timeout");
}

#[test]
fn source_errors_stay_separated_and_understandable() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let extension = sandbox.install("e", free());
    assert_eq!(code(sandbox.command(&extension, "erro-rede")), "source_network");
    assert_eq!(code(sandbox.command(&extension, "erro-fonte")), "source_failed");
    assert_eq!(code(sandbox.command(&extension, "erro-login")), "source_authentication");
    assert_eq!(code(sandbox.command(&extension, "erro-dados")), "source_data");
    assert_eq!(code(sandbox.command(&extension, "erro-nao-suportado")), "source_unsupported");
    assert_eq!(sandbox.command(&extension, "erro-fonte").unwrap_err().to_string(), "A fonte está em manutenção.");
    for text in ["erro-rede", "erro-login", "erro-dados", "erro-nao-suportado", "panico"] {
        let message = sandbox.command(&extension, text).unwrap_err().to_string();
        assert!(!message.contains("wasm") && !message.contains("backtrace") && !message.is_empty(), "{text}: {message}");
    }
}

#[test]
fn oversized_input_never_reaches_the_extension() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let extension = sandbox.install("e", free());
    assert_eq!(code(sandbox.command(&extension, &"x".repeat(600))), "extension_input");
    assert_eq!(code(sandbox.engine.details(&extension, None, "livre", &"x".repeat(2000)).map(|_| ())), "extension_input");
}

#[test]
fn a_changed_file_is_not_loaded() {
    let sandbox = Sandbox::new(EngineLimits::default());
    let extension = sandbox.install("e", free());
    std::fs::write(&extension.file, b"alterado").unwrap();
    assert_eq!(code(sandbox.command(&extension, "ok")), "extension_load");
}

// ---------------------------------------------------------------------------
// Rede
// ---------------------------------------------------------------------------

const LIMITS: Limits = Limits { max_bytes: 1024, timeout: Duration::from_secs(10) };

fn fetch(url: &str, access: Access) -> Result<prancha_lib::extensions::net::Response, NetError> {
    Net::new(true).get(url, access, LIMITS, Request::default())
}

#[test]
fn a_public_request_returns_the_body_and_identifies_the_app() {
    let server = serve(|_| Reply::ok("olá").header("Content-Type", "text/plain").header("ETag", "\"v1\""));
    let response = fetch(&server.url("/a"), Access::Public).unwrap();
    assert_eq!((response.status, response.body.as_slice()), (200, "olá".as_bytes()));
    assert_eq!(response.etag.as_deref(), Some("\"v1\""));
    assert_eq!(response.content_type.as_deref(), Some("text/plain"));
    assert!(server.seen()[0].header("user-agent").unwrap().starts_with("Prancha/"));
}

#[test]
fn without_the_test_exception_the_local_server_is_not_contacted() {
    let server = serve(|_| Reply::ok("não deveria"));
    let outcome = Net::new(false).get(&server.url("/a"), Access::Public, LIMITS, Request::default());
    assert!(matches!(outcome, Err(NetError::Blocked(_))), "{outcome:?}");
    assert!(server.seen().is_empty());
}

#[test]
fn a_host_outside_the_list_is_refused_without_connecting() {
    let server = serve(|_| Reply::ok("não deveria"));
    let allowed = vec!["exemplo.com".to_string()];
    let outcome = fetch(&server.url("/a"), Access::Hosts(&allowed));
    assert!(matches!(outcome, Err(NetError::Blocked(_))), "{outcome:?}");
    assert!(server.seen().is_empty());
}

#[test]
fn redirects_to_blocked_addresses_are_not_followed() {
    for target in ["http://169.254.169.254/latest/meta-data/", "http://10.0.0.1/", "https://192.168.0.1/", "http://localhost:9/", "http://[::1]/", "file:///C:/Windows/win.ini", "http://exemplo.com/"] {
        let server = serve(move |_| Reply::redirect(target));
        let outcome = fetch(&server.url("/a"), Access::Public);
        assert!(matches!(outcome, Err(NetError::Blocked(_))), "{target}: {outcome:?}");
        assert_eq!(server.seen().len(), 1);
    }
}

#[test]
fn a_redirect_is_checked_against_the_extension_hosts() {
    let allowed = vec!["127.0.0.1".to_string()];
    let server = serve(|_| Reply::redirect("https://cdn.exemplo.com/a"));
    let outcome = fetch(&server.url("/a"), Access::Hosts(&allowed));
    assert!(matches!(outcome, Err(NetError::Blocked(why)) if why.contains("cdn.exemplo.com")));
}

#[test]
fn a_valid_redirect_is_followed_up_to_the_limit() {
    let server = serve(|seen| match seen.path.as_str() {
        "/fim" => Reply::ok("chegou"),
        "/1" => Reply::redirect("/2"),
        "/2" => Reply::redirect("/3"),
        "/3" => Reply::redirect("/fim"),
        _ => Reply::redirect("/sem-fim"),
    });
    let response = fetch(&server.url("/1"), Access::Public).unwrap();
    assert_eq!(response.body, b"chegou");
    assert!(response.url.ends_with("/fim"), "{}", response.url);

    let endless = fetch(&server.url("/sem-fim"), Access::Public);
    assert!(matches!(endless, Err(NetError::Blocked(_))), "{endless:?}");
}

#[test]
fn a_response_over_the_limit_is_refused_even_when_the_size_is_not_declared() {
    let big = vec![b'x'; LIMITS.max_bytes as usize + 1];
    let exact = vec![b'x'; LIMITS.max_bytes as usize];
    let server = serve(move |seen| match seen.path.as_str() {
        "/declarado" => Reply::ok(big.clone()),
        "/sem-tamanho" => Reply::ok(big.clone()).without_length(),
        _ => Reply::ok(exact.clone()).without_length(),
    });
    assert_eq!(fetch(&server.url("/declarado"), Access::Public).unwrap_err(), NetError::TooLarge);
    assert_eq!(fetch(&server.url("/sem-tamanho"), Access::Public).unwrap_err(), NetError::TooLarge);
    assert_eq!(fetch(&server.url("/no-limite"), Access::Public).unwrap().body.len(), LIMITS.max_bytes as usize);
}

#[test]
fn a_slow_server_hits_the_time_limit() {
    let server = serve(|_| Reply::ok("tarde").after(Duration::from_secs(5)));
    let started = Instant::now();
    let outcome = Net::new(true).get(&server.url("/a"), Access::Public, Limits { max_bytes: 1024, timeout: Duration::from_millis(200) }, Request::default());
    assert_eq!(outcome.unwrap_err(), NetError::Timeout);
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
}

#[test]
fn a_request_can_be_cancelled_while_it_waits() {
    let server = serve(|_| Reply::ok("tarde").after(Duration::from_secs(5)));
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        flag.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let outcome = Net::new(true).get(&server.url("/a"), Access::Public, LIMITS, Request { cancel: Some(&cancel), ..Default::default() });
    assert_eq!(outcome.unwrap_err(), NetError::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
}

#[test]
fn unexpected_content_types_are_refused_when_asked() {
    let server = serve(|_| Reply::ok("MZ").header("Content-Type", "application/x-msdownload"));
    let checked = Net::new(true).get(&server.url("/a"), Access::Public, LIMITS, Request { check_content_type: true, ..Default::default() });
    assert!(matches!(checked, Err(NetError::Blocked(_))), "{checked:?}");
    assert!(fetch(&server.url("/a"), Access::Public).is_ok());
}

#[test]
fn an_etag_turns_into_a_conditional_request() {
    let server = serve(|seen| if seen.header("if-none-match") == Some("\"v1\"") { Reply::status(304) } else { Reply::ok("corpo") });
    let response = Net::new(true).get(&server.url("/a"), Access::Public, LIMITS, Request { etag: Some("\"v1\""), ..Default::default() }).unwrap();
    assert_eq!(response.status, 304);
    assert!(response.body.is_empty());
}

#[test]
fn an_extension_can_only_set_the_allowed_headers() {
    let server = serve(|_| Reply::ok("ok"));
    let good = vec![("Referer".to_string(), "https://exemplo.com/".to_string()), ("accept".to_string(), "image/*".to_string())];
    Net::new(true).get(&server.url("/a"), Access::Public, LIMITS, Request { headers: &good, ..Default::default() }).unwrap();
    assert_eq!(server.seen()[0].header("referer"), Some("https://exemplo.com/"));

    for name in ["Cookie", "Authorization", "Host", "User-Agent", "Forwarded"] {
        let bad = vec![(name.to_string(), "x".to_string())];
        let outcome = Net::new(true).get(&server.url("/b"), Access::Public, LIMITS, Request { headers: &bad, ..Default::default() });
        assert!(matches!(outcome, Err(NetError::Blocked(_))), "{name}: {outcome:?}");
    }
    assert_eq!(server.seen().len(), 1);
}

// ---------------------------------------------------------------------------
// Repositórios
// ---------------------------------------------------------------------------

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn core() -> (tempfile::TempDir, Core) {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::init(dir.path().to_path_buf(), Arc::new(|_, _| {})).unwrap();
    core.net = Arc::new(Net::new(true));
    (dir, core)
}

fn code<T: std::fmt::Debug>(outcome: Result<T, AppError>) -> &'static str {
    outcome.unwrap_err().code()
}

fn catalog_ids(core: &Core) -> Vec<String> {
    repository::catalog(&core.db(), &CatalogQuery::default(), APP_VERSION).unwrap().into_iter().map(|item| item.entry.id).collect()
}

/// Repositório com uma extensão, já na lista do Prancha.
fn added_repo(core: &Core, id: &str, seed: u8) -> TestRepo {
    let repo = TestRepo::new(id, seed);
    repo.publish(&[repo.entry(&format!("{id}.fonte"), "1.0.0", b"pacote")]);
    repository::add(core, &repo.manifest_url(), &repo.fingerprint()).unwrap();
    repo
}

#[test]
fn a_preview_shows_what_the_person_reviews_and_saves_nothing() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    repo.publish(&[repo.entry("com.exemplo.a", "1.0.0", b"a"), repo.entry("com.exemplo.b", "1.0.0", b"b")]);
    let preview = repository::preview(&core, &repo.manifest_url()).unwrap();
    assert_eq!(preview.id, "com.exemplo.repo");
    assert_eq!(preview.fingerprint, repo.fingerprint());
    assert_eq!(preview.extension_count, 2);
    assert_eq!(preview.website.as_deref(), Some("https://exemplo.com"));
    assert!(!preview.already_added);
    assert!(repository::list(&core.db()).unwrap().is_empty());
}

#[test]
fn a_base_address_resolves_to_the_manifest() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    repo.publish(&[]);
    assert_eq!(repository::preview(&core, &repo.server.base).unwrap().url, repo.manifest_url());
    assert_eq!(repository::preview(&core, &format!("{}/", repo.server.base)).unwrap().url, repo.manifest_url());
}

#[test]
fn adding_pins_the_reviewed_key_and_stores_the_catalog() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    repo.publish(&[repo.entry("com.exemplo.a", "1.0.0", b"a")]);
    let info = repository::add(&core, &repo.manifest_url(), &repo.fingerprint()).unwrap();
    assert_eq!((info.status.as_str(), info.extension_count, info.enabled), ("ok", 1, true));
    assert_eq!(info.fingerprint, repo.fingerprint());
    assert!(info.last_success_at.is_some());
    assert_eq!(catalog_ids(&core), ["com.exemplo.a"]);
    assert_eq!(code(repository::add(&core, &repo.manifest_url(), &repo.fingerprint())), "repo_exists");
    assert!(repository::preview(&core, &repo.manifest_url()).unwrap().already_added);
}

#[test]
fn a_key_other_than_the_reviewed_one_is_not_pinned() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    repo.publish(&[]);
    assert_eq!(code(repository::add(&core, &repo.manifest_url(), &"0".repeat(64))), "repo_fingerprint");
    assert!(repository::list(&core.db()).unwrap().is_empty());
}

#[test]
fn invalid_addresses_are_refused() {
    let (_dir, core) = core();
    for address in ["", "não é um endereço", "https://"] {
        assert_eq!(code(repository::preview(&core, address)), "repo_address", "{address}");
    }
    for address in ["http://exemplo.com/prancha-repo.json", "https://192.168.0.1/prancha-repo.json", "https://localhost/prancha-repo.json", "ftp://exemplo.com/a.json"] {
        assert_eq!(code(repository::preview(&core, address)), "repo_blocked", "{address}");
    }
}

#[test]
fn a_missing_or_malformed_manifest_is_explained() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    assert_eq!(code(repository::preview(&core, &repo.manifest_url())), "repo_missing");
    assert_eq!(code(repository::preview(&core, &repo.server.base)), "repo_missing");

    repo.put("/prancha-repo.json", "<html>não é JSON</html>");
    assert_eq!(code(repository::preview(&core, &repo.manifest_url())), "repo_invalid");

    for broken in [json!({ "name": "sem versão" }), json!({ "schemaVersion": 1, "name": "incompleto" })] {
        repo.put("/prancha-repo.json", broken.to_string());
        assert_eq!(code(repository::preview(&core, &repo.manifest_url())), "repo_invalid", "{broken}");
    }
    let mut bad_key = repo.manifest();
    bad_key["publicKey"] = json!("AAAA");
    repo.put("/prancha-repo.json", bad_key.to_string());
    assert_eq!(code(repository::preview(&core, &repo.manifest_url())), "repo_invalid");
    let mut local_index = repo.manifest();
    local_index["indexUrl"] = json!("https://10.0.0.1/prancha-index.json");
    repo.put("/prancha-repo.json", local_index.to_string());
    assert_eq!(code(repository::preview(&core, &repo.manifest_url())), "repo_invalid");
}

#[test]
fn a_newer_format_is_reported_as_incompatible() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    repo.publish(&[]);
    let mut manifest = repo.manifest();
    manifest["schemaVersion"] = json!(2);
    repo.put("/prancha-repo.json", manifest.to_string());
    assert_eq!(code(repository::preview(&core, &repo.manifest_url())), "repo_incompatible");

    repo.put("/prancha-repo.json", repo.manifest().to_string());
    repo.put_index(json!({ "schemaVersion": 2, "repositoryId": repo.id, "generatedAt": "2026-01-01T00:00:00Z", "extensions": [] }).to_string().into_bytes());
    assert_eq!(code(repository::preview(&core, &repo.manifest_url())), "repo_incompatible");
}

// Com a internet de verdade: prova TLS, DNS e o formato atual do Keiyoushi. Rode com `cargo test --test extensions -- --ignored`.
#[test]
#[ignore = "depende da internet"]
fn a_public_https_address_is_really_fetched() {
    let (_dir, core) = core();
    let preview = repository::preview(&core, "https://raw.githubusercontent.com/keiyoushi/extensions/repo").unwrap();
    assert_eq!(preview.name, "Keiyoushi");
    assert!(preview.external);
    assert!(preview.extension_count > 100, "{}", preview.extension_count);
}

// O caminho inteiro com o link de verdade: baixa o Suwayomi (uns 340 MB na primeira vez, depois fica na pasta) e fala com o site da fonte.
#[test]
#[ignore = "depende da internet e baixa o Suwayomi"]
fn a_real_keiyoushi_extension_goes_from_the_link_to_an_image() {
    let dir = std::env::temp_dir().join("prancha-suwayomi-real");
    std::fs::create_dir_all(&dir).unwrap();
    for file in ["prancha.db", "prancha.db-wal", "prancha.db-shm"] {
        let _ = std::fs::remove_file(dir.join(file));
    }
    let core = Core::init(dir, Arc::new(|_, _| {})).unwrap();

    let preview = repository::preview(&core, "https://raw.githubusercontent.com/keiyoushi/extensions/repo").unwrap();
    let added = repository::add(&core, &preview.url, &preview.fingerprint).unwrap();
    let extension = install::install(&core, &added.id, "eu.kanade.tachiyomi.extension.all.webtoons").unwrap();
    let source = extension.sources.iter().find(|s| s.language == "en").unwrap();

    let popular = sources::popular(&core, &extension.id, &source.id, 1, None, None).unwrap();
    assert!(!popular.results.is_empty());
    assert!(sources::cover(&core, popular.results[0].id).unwrap().is_some());
    assert!(!sources::search(&core, &extension.id, &source.id, "love", 1, None, None).unwrap().results.is_empty());

    let series = sources::open_series(&core, popular.results[0].id, None).unwrap();
    assert_eq!(series.source_error, None);
    let pages = suwayomi::pages(&core, &series.chapters[0].remote_id, None).unwrap();
    let image = suwayomi::image(&core, &pages[0].url, Limits { max_bytes: 32 * 1024 * 1024, timeout: Duration::from_secs(60) }, None).unwrap();
    assert!(image.is_success());
    assert!(image.body.len() > 1000, "{}", image.body.len());

    install::remove(&core, &extension.id).unwrap();
    assert!(install::list(&core.db()).unwrap().is_empty());
}

const MIHON_KEY: &str = "9add655a78e96c4ec7a53ef89dccb557cb5d767489fac5e785d671a5a75d4da2";

fn mihon_extension(id: &str, warning: &str) -> Value {
    json!({
        "name": id,
        "packageName": format!("eu.kanade.tachiyomi.extension.all.{id}"),
        "resources": { "apkUrl": "https://exemplo.com/x.apk", "iconUrl": "https://exemplo.com/x.png", "jarUrl": "https://exemplo.com/x.jar" },
        "extensionLib": "1.6",
        "versionCode": "106003",
        "versionName": "1.6.3",
        "contentWarning": warning,
        "sources": [{ "id": "2522335540328470744", "name": id, "language": "pt-BR", "homeUrl": "https://exemplo.com" }],
    })
}

fn put_mihon_key(repo: &TestRepo, fingerprint: &str) {
    repo.put("/repo.json", json!({ "index_v2": "https://exemplo.com/index.pb", "meta": { "name": "Keiyoushi", "website": "https://keiyoushi.github.io", "signingKeyFingerprint": fingerprint } }).to_string());
}

/// No formato do Keiyoushi: `repo.json` e `index.json`, com uma extensão de cada classificação.
fn mihon_repo() -> TestRepo {
    let repo = TestRepo::new("mihon", 1);
    put_mihon_key(&repo, MIHON_KEY);
    let extensions = [mihon_extension("segura", "CONTENT_WARNING_SAFE"), mihon_extension("mista", "CONTENT_WARNING_MIXED"), mihon_extension("adulta", "CONTENT_WARNING_NSFW"), json!({ "name": "quebrada" })];
    repo.put("/index.json", json!({ "name": "Keiyoushi", "signingKey": MIHON_KEY, "extensionList": { "extensions": extensions } }).to_string());
    repo
}

#[test]
fn a_mihon_repository_lists_only_what_is_marked_safe_until_the_filter_goes_off() {
    let (_dir, core) = core();
    let repo = mihon_repo();
    let preview = repository::preview(&core, &repo.server.base).unwrap();
    assert!(preview.external);
    assert_eq!((preview.name.as_str(), preview.fingerprint.as_str(), preview.extension_count), ("Keiyoushi", MIHON_KEY, 1));
    assert_eq!(preview.website.as_deref(), Some("https://keiyoushi.github.io"));

    // Os endereços que aparecem por aí levam ao mesmo repositório.
    for path in ["/repo.json", "/index.pb", "/index.min.json", "/"] {
        let same = repository::preview(&core, &repo.server.url(path)).unwrap();
        assert_eq!((same.url.as_str(), same.id.as_str()), (repo.server.url("/repo.json").as_str(), preview.id.as_str()), "{path}");
    }

    let added = repository::add(&core, &preview.url, MIHON_KEY).unwrap();
    assert_eq!((added.status.as_str(), added.extension_count, added.fingerprint.as_str()), ("ok", 1, MIHON_KEY));
    let items = repository::catalog(&core.db(), &CatalogQuery::default(), APP_VERSION).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].entry.id, "eu.kanade.tachiyomi.extension.all.segura");
    assert_eq!(items[0].entry.runtime, suwayomi::RUNTIME);
    assert_eq!(items[0].incompatible, None);
    assert_eq!(repository::catalog_languages(&core.db()).unwrap(), ["pt-BR"]);

    assert_eq!(repository::sync(&core, &added.id).unwrap().status, "ok");
    assert_eq!(code(repository::add(&core, &preview.url, MIHON_KEY)), "repo_exists");
}

#[test]
fn a_mihon_key_change_stops_everything_until_the_person_confirms() {
    let (_dir, core) = core();
    let repo = mihon_repo();
    let added = repository::add(&core, &repo.server.base, MIHON_KEY).unwrap();

    let other = "b".repeat(64);
    put_mihon_key(&repo, &other);
    assert_eq!(code(repository::sync(&core, &added.id)), "repo_key_changed");
    let info = repository::get(&core.db(), &added.id).unwrap();
    assert_eq!((info.status.as_str(), info.fingerprint.as_str()), ("key_changed", MIHON_KEY));
    assert_eq!(info.pending_fingerprint.as_deref(), Some(other.as_str()));

    // Recusada antes de baixar o componente: `core` nem tem o Suwayomi.
    assert_eq!(code(install::install(&core, &added.id, "eu.kanade.tachiyomi.extension.all.segura")), "extension_provenance");

    let accepted = repository::accept_key(&core, &added.id, &other).unwrap();
    assert_eq!((accepted.status.as_str(), accepted.fingerprint.as_str()), ("ok", other.as_str()));
}

#[test]
fn a_component_only_counts_with_the_expected_content() {
    let server = serve(|seen| if seen.path == "/falta" { Reply::status(404) } else { Reply::ok(vec![7u8; 100]) });
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pacote");
    let right = prancha_lib::extensions::signing::sha256_hex(&[7u8; 100]);
    let fetch = |path: &str, sha256: &str, size: u64| suwayomi::fetch_package(&server.url(path), &file, sha256, size, |_| {});

    fetch("/p", &right, 100).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), vec![7u8; 100]);
    assert_eq!(code(fetch("/p", &"0".repeat(64), 100)), "component_hash");
    assert_eq!(code(fetch("/p", &right, 99)), "component_download");
    assert_eq!(code(fetch("/falta", &right, 100)), "component_download");
}

/// O provedor de TLS é global, então só um processo novo mostra o caso de quem abre o app com o componente já baixado.
#[test]
fn starting_the_component_does_not_depend_on_earlier_network_use() {
    const FRESH: &str = "PRANCHA_TEST_FRESH_PROCESS";
    if std::env::var_os(FRESH).is_none() {
        let name = "starting_the_component_does_not_depend_on_earlier_network_use";
        let run = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", name, "--nocapture"]).env(FRESH, "1").output().unwrap();
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stdout));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let core = Core::init(dir.path().to_path_buf(), Arc::new(|_, _| {})).unwrap();
    let component = dir.path().join("suwayomi").join(suwayomi::VERSION);
    for file in ["jre/bin/javaw.exe", "bin/Suwayomi-Server.jar"] {
        std::fs::create_dir_all(component.join(file).parent().unwrap()).unwrap();
        std::fs::write(component.join(file), b"falso").unwrap();
    }
    // O arquivo falso não é um programa: o que importa é virar um erro, e não um pânico.
    assert!(suwayomi::browse(&core, "1", suwayomi::Browse::Popular, 1, None).is_err());
}

#[test]
fn mihon_repositories_get_the_exact_message() {
    let (_dir, core) = core();
    let mihon_repo = json!({ "index_v2": "https://exemplo.com/index.pb", "meta": { "name": "Exemplo", "website": "https://exemplo.com", "signingKeyFingerprint": "abc" } });
    let mihon_index = json!([{ "name": "X", "pkg": "eu.kanade.tachiyomi.extension.all.x", "apk": "x.apk", "lang": "all", "code": 1, "version": "1.4.1", "nsfw": 0, "sources": [] }]);

    // Quem não tem o formato do Keiyoushi, com chave e `index.json`, continua sem suporte.
    let check = |address: &str| {
        let error = repository::preview(&core, address).unwrap_err();
        assert_eq!((error.code(), error.to_string().as_str()), ("repo_mihon", MIHON_MESSAGE), "{address}");
    };
    // Pelo nome do arquivo, sem consultar a rede.
    check("https://exemplo.com/apk/tachiyomi-all.x-v1.4.1.apk");

    let repo = TestRepo::new("mihon", 1);
    repo.put("/repo.json", mihon_repo.to_string());
    check(&repo.server.url("/repo.json"));
    check(&repo.server.url("/index.min.json"));
    check(&repo.server.base);

    let only_index = TestRepo::new("mihon", 1);
    only_index.put("/index.min.json", mihon_index.to_string());
    only_index.put("/outro.json", mihon_index.to_string());
    check(&only_index.server.url("/outro.json"));
    check(&only_index.server.base);
}

#[test]
fn a_wrong_index_signature_is_refused_and_keeps_the_catalog() {
    let (_dir, core) = core();
    let repo = added_repo(&core, "com.exemplo.repo", 1);

    repo.publish(&[repo.entry("com.exemplo.novo", "1.0.0", b"novo")]);
    repo.put("/prancha-index.json.sig", "AAAA");
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_signature");
    let info = repository::get(&core.db(), &repo.id).unwrap();
    assert_eq!(info.status, "error");
    assert!(info.last_error.is_some());
    assert_eq!(catalog_ids(&core), ["com.exemplo.repo.fonte"]);

    repo.remove("/prancha-index.json.sig");
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_signature");
    assert_eq!(code(repository::preview(&core, &repo.manifest_url())), "repo_signature");
    assert_eq!(catalog_ids(&core), ["com.exemplo.repo.fonte"]);
}

#[test]
fn an_index_signed_with_another_key_is_refused() {
    let (_dir, core) = core();
    let repo = added_repo(&core, "com.exemplo.repo", 1);
    let manifest = repo.manifest();
    repo.change_key(2);
    repo.publish(&[]);
    // O manifesto continua anunciando a chave antiga, mas o índice foi assinado com outra.
    repo.put("/prancha-repo.json", manifest.to_string());
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_signature");
    assert_eq!(catalog_ids(&core), ["com.exemplo.repo.fonte"]);
}

#[test]
fn a_key_change_stops_the_sync_until_the_person_confirms() {
    let (_dir, core) = core();
    let repo = added_repo(&core, "com.exemplo.repo", 1);
    let old = repo.fingerprint();
    repo.change_key(2);
    repo.publish(&[repo.entry("com.exemplo.novo", "1.0.0", b"novo")]);

    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_key_changed");
    let info = repository::get(&core.db(), &repo.id).unwrap();
    assert_eq!(info.status, "key_changed");
    assert_eq!(info.fingerprint, old, "a chave fixada não muda sozinha");
    assert_eq!(info.pending_fingerprint.as_deref(), Some(repo.fingerprint().as_str()));
    assert_eq!(catalog_ids(&core), ["com.exemplo.repo.fonte"], "o último catálogo é mantido");

    assert_eq!(code(repository::accept_key(&core, &repo.id, &old)), "repo_fingerprint");
    assert_eq!(repository::get(&core.db(), &repo.id).unwrap().fingerprint, old);

    let accepted = repository::accept_key(&core, &repo.id, &repo.fingerprint()).unwrap();
    assert_eq!(accepted.status, "ok");
    assert_eq!(accepted.fingerprint, repo.fingerprint());
    assert_eq!(accepted.pending_fingerprint, None);
    assert_eq!(catalog_ids(&core), ["com.exemplo.novo"]);
}

#[test]
fn an_older_index_is_refused() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    repo.publish_dated(&[repo.entry("com.exemplo.atual", "2.0.0", b"novo")], "2026-06-01T00:00:00Z");
    repository::add(&core, &repo.manifest_url(), &repo.fingerprint()).unwrap();

    repo.publish_dated(&[repo.entry("com.exemplo.antigo", "1.0.0", b"velho")], "2026-05-01T00:00:00Z");
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_stale");
    assert_eq!(catalog_ids(&core), ["com.exemplo.atual"]);

    repo.publish_dated(&[repo.entry("com.exemplo.depois", "1.0.0", b"depois")], "2026-07-01T00:00:00Z");
    repository::sync(&core, &repo.id).unwrap();
    assert_eq!(catalog_ids(&core), ["com.exemplo.depois"]);
}

#[test]
fn a_signed_index_with_a_bad_entry_is_refused_whole() {
    let (_dir, core) = core();
    let repo = added_repo(&core, "com.exemplo.repo", 1);
    let good = repo.entry("com.exemplo.boa", "1.0.0", b"boa");
    let with = |field: &str, value: Value| {
        let mut entry = good.clone();
        entry[field] = value;
        entry
    };
    let bad_capabilities = json!({ "network": true, "storage": false, "authentication": false, "allowedHosts": ["*"] });
    let no_hosts = json!({ "network": true, "storage": false, "authentication": false, "allowedHosts": [] });
    let hosts_without_network = json!({ "network": false, "storage": false, "authentication": false, "allowedHosts": ["exemplo.com"] });
    for bad in [
        with("capabilities", hosts_without_network),
        with("runtime", json!("x".repeat(41))),
        with("apiVersion", json!("1".repeat(21))),
        with("supportedKinds", json!(["x".repeat(41)])),
        with("id", json!("Identificador Ruim")),
        with("version", json!("um")),
        with("sha256", json!("abc")),
        with("artifactUrl", json!("http://10.0.0.1/a.wasm")),
        with("signatureUrl", json!("https://169.254.169.254/a.sig")),
        with("size", json!(0)),
        with("size", json!(1u64 << 40)),
        with("contentRating", json!("liberado")),
        with("icon", json!("data:image/svg+xml;base64,AAAA")),
        with("sources", json!([])),
        with("capabilities", bad_capabilities),
        with("capabilities", no_hosts),
    ] {
        repo.publish(&[good.clone(), bad.clone()]);
        assert_eq!(code(repository::sync(&core, &repo.id)), "repo_invalid", "{bad}");
        assert_eq!(catalog_ids(&core), ["com.exemplo.repo.fonte"]);
    }
    repo.publish(&[good.clone(), good.clone()]);
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_invalid");

    repo.put_index(b"isto nao e json".to_vec());
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_invalid");
    repo.put_index(json!({ "schemaVersion": 1, "repositoryId": "outro.repo", "generatedAt": "2027-01-01T00:00:00Z", "extensions": [] }).to_string().into_bytes());
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_invalid");
    assert_eq!(catalog_ids(&core), ["com.exemplo.repo.fonte"]);

    repo.publish(&[good]);
    assert_eq!(repository::sync(&core, &repo.id).unwrap().status, "ok");
}

#[test]
fn a_network_failure_is_recorded_and_keeps_the_catalog() {
    let (_dir, core) = core();
    let repo = added_repo(&core, "com.exemplo.repo", 1);
    // Um endereço que ninguém escuta: a porta foi liberada logo depois de reservada.
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    core.db().execute("UPDATE source_repositories SET url = ?1 WHERE id = ?2", [format!("http://{free}/prancha-repo.json"), repo.id.clone()]).unwrap();
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_network");
    let info = repository::get(&core.db(), &repo.id).unwrap();
    assert_eq!(info.status, "error");
    assert!(info.last_success_at.is_some());
    assert_eq!(catalog_ids(&core), ["com.exemplo.repo.fonte"]);
}

#[test]
fn one_failing_repository_does_not_stop_the_others() {
    let (_dir, core) = core();
    let good = added_repo(&core, "com.exemplo.bom", 1);
    let bad = added_repo(&core, "com.exemplo.ruim", 2);
    let off = added_repo(&core, "com.exemplo.desligado", 3);
    repository::set_enabled(&core.db(), &off.id, false).unwrap();
    let off_requests = off.server.seen().len();

    good.publish(&[good.entry("com.exemplo.bom.nova", "1.0.0", b"nova")]);
    bad.put("/prancha-repo.json", "quebrado");

    let result = repository::sync_all(&core).unwrap();
    let status = |id: &str| result.iter().find(|r| r.id == id).unwrap().status.clone();
    assert_eq!(status(&good.id), "ok");
    assert_eq!(status(&bad.id), "error");
    assert_eq!(catalog_ids(&core), ["com.exemplo.bom.nova", "com.exemplo.ruim.fonte"], "o desligado não aparece");
    assert_eq!(off.server.seen().len(), off_requests, "o desligado não é consultado");
}

#[test]
fn an_unchanged_index_is_not_downloaded_again() {
    let (_dir, core) = core();
    let repo = added_repo(&core, "com.exemplo.repo", 1);
    let requests = repo.server.seen().len();
    let before = repository::get(&core.db(), &repo.id).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    let after = repository::sync(&core, &repo.id).unwrap();
    assert!(after.last_sync_at > before.last_sync_at);
    let seen = repo.server.seen();
    let index = seen[requests..].iter().find(|s| s.path == "/prancha-index.json").unwrap();
    assert!(index.header("if-none-match").is_some());
    assert!(!seen[requests..].iter().any(|s| s.path == "/prancha-index.json.sig"), "304 não pede a assinatura");
    assert_eq!(catalog_ids(&core), ["com.exemplo.repo.fonte"]);
}

#[test]
fn the_catalog_hides_adult_extensions_and_marks_incompatible_ones() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    let mut adult = repo.entry("com.exemplo.adulta", "1.0.0", b"a");
    adult["contentRating"] = json!("adult");
    let mut newer_api = repo.entry("com.exemplo.api", "1.0.0", b"b");
    newer_api["apiVersion"] = json!("2");
    let mut apk = repo.entry("com.exemplo.apk", "1.0.0", b"c");
    apk["runtime"] = json!("apk");
    let mut future = repo.entry("com.exemplo.futura", "1.0.0", b"d");
    future["minAppVersion"] = json!("99.0.0");
    let mut english = repo.entry("com.exemplo.ingles", "1.0.0", b"e");
    english["languages"] = json!(["en"]);
    english["name"] = json!("Aurora");
    repo.publish(&[adult, newer_api, apk, future, english]);
    let preview = repository::preview(&core, &repo.manifest_url()).unwrap();
    assert_eq!(preview.extension_count, 4, "a adulta não conta");
    repository::add(&core, &repo.manifest_url(), &repo.fingerprint()).unwrap();

    let items = repository::catalog(&core.db(), &CatalogQuery::default(), APP_VERSION).unwrap();
    let ids: Vec<_> = items.iter().map(|i| i.entry.id.as_str()).collect();
    assert_eq!(ids, ["com.exemplo.ingles", "com.exemplo.api", "com.exemplo.apk", "com.exemplo.futura"]);
    let reason = |id: &str| items.iter().find(|i| i.entry.id == id).unwrap().incompatible.clone();
    assert!(reason("com.exemplo.ingles").is_none());
    assert!(reason("com.exemplo.api").is_some());
    assert!(reason("com.exemplo.apk").is_some());
    assert!(reason("com.exemplo.futura").is_some());

    let search = |query: CatalogQuery| repository::catalog(&core.db(), &query, APP_VERSION).unwrap().len();
    assert_eq!(search(CatalogQuery { text: Some("auro".into()), ..Default::default() }), 1);
    assert_eq!(search(CatalogQuery { text: Some("%".into()), ..Default::default() }), 0, "curinga é texto comum");
    assert_eq!(search(CatalogQuery { language: Some("en".into()), ..Default::default() }), 1);
    assert_eq!(search(CatalogQuery { language: Some("pt-BR".into()), ..Default::default() }), 3);
    assert_eq!(search(CatalogQuery { limit: Some(2), ..Default::default() }), 2);
    assert_eq!(search(CatalogQuery { limit: Some(2), offset: Some(3), ..Default::default() }), 1);
    assert_eq!(repository::catalog_languages(&core.db()).unwrap(), ["en", "pt-BR"]);
}

// ---------------------------------------------------------------------------
// Instalação
// ---------------------------------------------------------------------------

const EXT: &str = "com.exemplo.ext";

/// As fontes que a extensão de teste declara, como o índice precisa prometer.
fn fixture_sources() -> Value {
    json!([
        { "id": "livre", "name": "Fonte livre", "language": "pt-BR", "contentRating": "general" },
        { "id": "sem-classificacao", "name": "Fonte sem-classificacao", "language": "pt-BR", "contentRating": "unknown" },
        { "id": "adulta", "name": "Fonte adulta", "language": "pt-BR", "contentRating": "adult" }
    ])
}

fn fixture_entry(repo: &TestRepo, version: &str) -> Value {
    let mut entry = repo.entry(EXT, version, &wasm::component(FIXTURE));
    entry["sources"] = fixture_sources();
    entry
}

fn repo_with_fixture(core: &Core) -> TestRepo {
    let repo = TestRepo::new("com.exemplo.repo", 1);
    repo.publish(&[fixture_entry(&repo, "1.0.0")]);
    repository::add(core, &repo.manifest_url(), &repo.fingerprint()).unwrap();
    repo
}

fn packages(core: &Core) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(core.paths.data_dir.join("extensions")) else { return vec![] };
    entries.map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect()
}

#[test]
fn a_valid_extension_installs_and_runs() {
    let (_dir, core) = core();
    let repo = repo_with_fixture(&core);
    let installed = install::install(&core, &repo.id, EXT).unwrap();
    assert_eq!(installed.version, "1.0.0");
    assert_eq!(installed.repository_id, repo.id);
    assert_eq!(installed.fingerprint, repo.fingerprint());
    assert!(installed.enabled);
    let ids: Vec<_> = installed.sources.iter().map(|s| (s.id.as_str(), s.available)).collect();
    assert_eq!(ids, [("adulta", false), ("livre", true), ("sem-classificacao", false)], "com o filtro +18 ligado, a adulta e a sem classificação ficam desabilitadas");
    assert_eq!(packages(&core).len(), 1);

    let runnable = install::runnable(&core, EXT).unwrap();
    let page = core.engine().unwrap().search(&runnable, None, "livre", "oi", 1, &[]).unwrap();
    assert_eq!(page.results[0].title, "oi 1");

    assert_eq!(code(install::install(&core, &repo.id, EXT)), "extension_installed");
    let items = repository::catalog(&core.db(), &CatalogQuery::default(), APP_VERSION).unwrap();
    assert_eq!((items[0].installed_version.as_deref(), items[0].installed_here, items[0].update_available), (Some("1.0.0"), true, false));
}

#[test]
fn a_package_that_fails_any_check_is_not_installed() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    let good = fixture_entry(&repo, "1.0.0");
    repo.publish(&[]);
    repository::add(&core, &repo.manifest_url(), &repo.fingerprint()).unwrap();

    let attempt = |entry: Value| {
        repo.publish(&[entry]);
        repository::sync(&core, &repo.id).unwrap();
        let outcome = code(install::install(&core, &repo.id, EXT));
        assert!(install::list(&core.db()).unwrap().is_empty());
        assert!(packages(&core).is_empty(), "nada fica em disco");
        outcome
    };

    let mut wrong_hash = good.clone();
    wrong_hash["sha256"] = json!("0".repeat(64));
    assert_eq!(attempt(wrong_hash), "extension_hash");

    let mut wrong_size = good.clone();
    wrong_size["size"] = json!(good["size"].as_u64().unwrap() - 1);
    assert_eq!(attempt(wrong_size), "extension_size");
    let mut wrong_size = good.clone();
    wrong_size["size"] = json!(good["size"].as_u64().unwrap() + 1);
    assert_eq!(attempt(wrong_size), "extension_size");

    let mut newer_app = good.clone();
    newer_app["minAppVersion"] = json!("99.0.0");
    assert_eq!(attempt(newer_app), "extension_incompatible");

    let mut older_app = good.clone();
    older_app["maxAppVersion"] = json!("0.1.0");
    assert_eq!(attempt(older_app), "extension_incompatible");

    let mut runtime = good.clone();
    runtime["runtime"] = json!("apk");
    assert_eq!(attempt(runtime), "extension_incompatible");

    let mut api = good.clone();
    api["apiVersion"] = json!("2");
    assert_eq!(attempt(api), "extension_incompatible");

    let mut login = good.clone();
    login["capabilities"]["authentication"] = json!(true);
    assert_eq!(attempt(login), "extension_incompatible");

    let mut other_sources = good.clone();
    other_sources["sources"] = json!([{ "id": "livre", "name": "Só uma", "language": "pt-BR", "contentRating": "general" }]);
    assert_eq!(attempt(other_sources), "extension_mismatch");

    let mut softened = good.clone();
    softened["sources"][2]["contentRating"] = json!("general");
    assert_eq!(attempt(softened), "extension_mismatch", "o índice não pode esconder uma fonte adulta");

    // Assinatura de outro pacote, depois sem assinatura.
    let signature_path = format!("/{EXT}-1.0.0.wasm.sig");
    repo.put(&signature_path, "AAAA");
    assert_eq!(attempt(good.clone()), "extension_signature");
    repo.remove(&signature_path);
    assert_eq!(attempt(good.clone()), "extension_network");
    let other = TestRepo::new("com.exemplo.outro", 9);
    let foreign = other.entry(EXT, "1.0.0", &wasm::component(FIXTURE));
    assert_ne!(foreign["signatureUrl"], good["signatureUrl"]);
    repo.put(&signature_path, signature_from(&other, &format!("/{EXT}-1.0.0.wasm.sig")));
    assert_eq!(attempt(good.clone()), "extension_signature", "chave de outro repositório");
}

fn signature_from(repo: &TestRepo, path: &str) -> Vec<u8> {
    let url = repo.server.url(path);
    fetch(&url, Access::Public).unwrap().body
}

#[test]
fn a_package_that_does_not_load_is_not_installed() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    let entry = repo.entry(EXT, "1.0.0", b"isto nao e um componente");
    repo.publish(&[entry]);
    repository::add(&core, &repo.manifest_url(), &repo.fingerprint()).unwrap();
    assert_eq!(code(install::install(&core, &repo.id, EXT)), "extension_load");
    assert!(install::list(&core.db()).unwrap().is_empty());
    assert!(packages(&core).is_empty());
}

#[test]
fn an_update_replaces_the_package_and_keeps_private_data() {
    let (_dir, core) = core();
    let repo = repo_with_fixture(&core);
    install::install(&core, &repo.id, EXT).unwrap();
    core.db().execute("INSERT INTO extension_storage (extension_id, key, value) VALUES (?1, 'k', x'01')", [EXT]).unwrap();
    assert_eq!(code(install::update(&core, EXT)), "extension_current");

    // O mesmo código com uma seção de metadados a mais: outro SHA-256, as mesmas fontes.
    let mut wasm = wasm::component(FIXTURE);
    wasm.extend([0, 4, 3, b'a', b'b', b'c']);
    let mut newer = repo.entry(EXT, "1.1.0", &wasm);
    newer["sources"] = fixture_sources();
    newer["name"] = json!("Nome novo");
    repo.publish(&[newer]);
    repository::sync(&core, &repo.id).unwrap();

    let listed = install::list(&core.db()).unwrap();
    assert_eq!((listed[0].version.as_str(), listed[0].available_version.as_deref()), ("1.0.0", Some("1.1.0")), "nada é atualizado em silêncio");
    let items = repository::catalog(&core.db(), &CatalogQuery::default(), APP_VERSION).unwrap();
    assert!(items[0].update_available);

    let updated = install::update(&core, EXT).unwrap();
    assert_eq!((updated.version.as_str(), updated.name.as_str(), updated.available_version), ("1.1.0", "Nome novo", None));
    assert_eq!(packages(&core).len(), 1);
    let stored: i64 = core.db().query_row("SELECT count(*) FROM extension_storage WHERE extension_id = ?1", [EXT], |r| r.get(0)).unwrap();
    assert_eq!(stored, 1);
}

#[test]
fn a_failed_update_keeps_the_working_version() {
    let (_dir, core) = core();
    let repo = repo_with_fixture(&core);
    let before = install::install(&core, &repo.id, EXT).unwrap();
    let files = packages(&core);

    let mut broken = repo.entry(EXT, "2.0.0", b"pacote quebrado");
    broken["sources"] = fixture_sources();
    repo.publish(&[broken.clone()]);
    repository::sync(&core, &repo.id).unwrap();
    assert_eq!(code(install::update(&core, EXT)), "extension_load");

    broken["sha256"] = json!("0".repeat(64));
    repo.publish(&[broken]);
    repository::sync(&core, &repo.id).unwrap();
    assert_eq!(code(install::update(&core, EXT)), "extension_hash");

    let after = install::get(&core.db(), EXT).unwrap();
    assert_eq!(after.version, before.version);
    assert_eq!(after.available_version.as_deref(), Some("2.0.0"));
    assert_eq!(packages(&core), files);
    let runnable = install::runnable(&core, EXT).unwrap();
    assert_eq!(core.engine().unwrap().search(&runnable, None, "livre", "oi", 1, &[]).unwrap().results[0].title, "oi 1");
}

#[test]
fn provenance_does_not_change_in_silence() {
    let (_dir, core) = core();
    let repo = repo_with_fixture(&core);
    install::install(&core, &repo.id, EXT).unwrap();

    // Outro repositório anuncia a mesma extensão com uma versão maior.
    let other = TestRepo::new("com.exemplo.outro", 2);
    let mut impostor = other.entry(EXT, "9.0.0", &wasm::component(FIXTURE));
    impostor["sources"] = fixture_sources();
    other.publish(&[impostor]);
    repository::add(&core, &other.manifest_url(), &other.fingerprint()).unwrap();
    assert_eq!(code(install::install(&core, &other.id, EXT)), "extension_provenance");
    let installed = install::get(&core.db(), EXT).unwrap();
    assert_eq!((installed.repository_id.as_str(), installed.available_version.as_deref()), (repo.id.as_str(), None));
    assert_eq!(code(install::update(&core, EXT)), "extension_current", "o outro repositório não oferece atualização");

    // O próprio repositório troca de chave: aceitar a chave não aceita o pacote em silêncio.
    repo.change_key(3);
    let mut rotated = repo.entry(EXT, "1.2.0", &wasm::component(FIXTURE));
    rotated["sources"] = fixture_sources();
    repo.publish(&[rotated]);
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_key_changed");
    assert_eq!(code(install::update(&core, EXT)), "extension_current", "o catálogo antigo continua valendo");
    repository::accept_key(&core, &repo.id, &repo.fingerprint()).unwrap();
    assert_eq!(code(install::update(&core, EXT)), "extension_provenance");
    assert_eq!(install::get(&core.db(), EXT).unwrap().version, "1.0.0");
}

#[test]
fn a_repository_whose_key_changed_cannot_install() {
    let (_dir, core) = core();
    let repo = repo_with_fixture(&core);
    repo.change_key(3);
    repo.publish(&[]);
    assert_eq!(code(repository::sync(&core, &repo.id)), "repo_key_changed");
    assert_eq!(code(install::install(&core, &repo.id, EXT)), "extension_provenance");
}

#[test]
fn a_disabled_extension_does_not_run() {
    let (_dir, core) = core();
    let repo = repo_with_fixture(&core);
    install::install(&core, &repo.id, EXT).unwrap();
    assert!(!install::set_enabled(&core.db(), EXT, false).unwrap().enabled);
    assert_eq!(code(install::runnable(&core, EXT)), "extension_disabled");
    assert!(install::set_enabled(&core.db(), EXT, true).unwrap().enabled);
    assert!(install::runnable(&core, EXT).is_ok());
}

#[test]
fn uninstalling_removes_the_package_and_private_data_only() {
    let (_dir, core) = core();
    let repo = repo_with_fixture(&core);
    install::install(&core, &repo.id, EXT).unwrap();
    core.db()
        .execute_batch(
            "INSERT INTO extension_storage (extension_id, key, value) VALUES ('com.exemplo.ext', 'k', x'01');
             INSERT INTO online_series (id, extension_id, source_id, remote_id, title, created_at, updated_at) VALUES (1, 'com.exemplo.ext', 'livre', 'obra', 'Obra', 1, 1);",
        )
        .unwrap();
    install::remove(&core, EXT).unwrap();
    assert!(install::list(&core.db()).unwrap().is_empty());
    assert!(packages(&core).is_empty());
    let count = |table: &str| -> i64 { core.db().query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap() };
    assert_eq!(count("extension_storage"), 0);
    assert_eq!(count("extension_sources"), 0);
    assert_eq!(count("online_series"), 1, "a obra continua registrada");
    assert_eq!(code(install::runnable(&core, EXT)), "extension_not_found");
    assert_eq!(catalog_ids(&core), [EXT], "pode ser instalada de novo");
    install::install(&core, &repo.id, EXT).unwrap();
}

#[test]
fn removing_a_repository_keeps_installed_extensions_running() {
    let (_dir, core) = core();
    let repo = repo_with_fixture(&core);
    install::install(&core, &repo.id, EXT).unwrap();
    repository::remove(&core.db(), &repo.id).unwrap();
    let installed = install::get(&core.db(), EXT).unwrap();
    assert_eq!((installed.repository_name.as_str(), installed.available_version), ("Repositório com.exemplo.repo", None));
    let runnable = install::runnable(&core, EXT).unwrap();
    assert_eq!(core.engine().unwrap().search(&runnable, None, "livre", "oi", 1, &[]).unwrap().results[0].title, "oi 1");
    assert_eq!(code(install::update(&core, EXT)), "extension_not_found");
}

// ---------------------------------------------------------------------------
// Fontes
// ---------------------------------------------------------------------------

fn core_with_fixture(hosts: &[&str]) -> (tempfile::TempDir, Arc<Core>, TestRepo) {
    let (dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    let mut entry = fixture_entry(&repo, "1.0.0");
    entry["capabilities"] = json!({ "network": !hosts.is_empty(), "storage": false, "authentication": false, "allowedHosts": hosts });
    repo.publish(&[entry]);
    repository::add(&core, &repo.manifest_url(), &repo.fingerprint()).unwrap();
    install::install(&core, &repo.id, EXT).unwrap();
    (dir, Arc::new(core), repo)
}

fn titles(page: &sources::BrowsePage) -> Vec<&str> {
    page.results.iter().map(|r| r.title.as_str()).collect()
}

#[test]
fn explore_lists_only_usable_sources() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let listed = sources::available_sources(&core.db()).unwrap();
    let ids: Vec<_> = listed.iter().map(|s| s.source_id.as_str()).collect();
    assert_eq!(ids, ["livre"], "sem a adulta e sem a que não tem classificação");
    assert_eq!(listed[0].extension_id, EXT);

    assert_eq!(code(sources::popular(&core, EXT, "sem-classificacao", 1, None, None)), "source_disabled");
    assert_eq!(code(sources::search(&core, EXT, "adulta", "x", 1, None, None)), "source_disabled");
    assert_eq!(code(sources::popular(&core, EXT, "inexistente", 1, None, None)), "source_not_found");

    install::set_enabled(&core.db(), EXT, false).unwrap();
    assert!(sources::available_sources(&core.db()).unwrap().is_empty());
    assert_eq!(code(sources::popular(&core, EXT, "livre", 1, None, None)), "extension_disabled");
}

#[test]
fn popular_latest_and_search_return_stored_pages() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let popular = sources::popular(&core, EXT, "livre", 1, None, None).unwrap();
    assert_eq!(titles(&popular), ["Popular 1.0", "Popular 1.1", "Popular 1.2"]);
    assert!(popular.has_more);
    let second = sources::popular(&core, EXT, "livre", 2, None, None).unwrap();
    assert_eq!(second.results.len(), 3);
    assert!(!second.has_more);
    assert_ne!(popular.results[0].id, second.results[0].id);
    let past_the_end = sources::popular(&core, EXT, "livre", 3, None, None).unwrap();
    assert!(past_the_end.results.is_empty() && !past_the_end.has_more);

    assert_eq!(titles(&sources::latest(&core, EXT, "livre", 1, None, None).unwrap())[0], "Recente 1.0");
    assert_eq!(titles(&sources::search(&core, EXT, "livre", " Aurora ", 1, None, None).unwrap()), ["Aurora 1"]);

    let again = sources::popular(&core, EXT, "livre", 1, None, None).unwrap();
    assert_eq!(again.results[0].id, popular.results[0].id, "a mesma obra mantém o identificador");
    assert_eq!(popular.results[0].language.as_deref(), Some("pt-BR"));
    assert_eq!(popular.results[0].source_name.as_deref(), Some("Fonte livre"));

    assert_eq!(titles(&sources::search(&core, EXT, "livre", "filtros", 1, Some("manga"), None).unwrap()), ["kind=manga"]);
}

#[test]
fn a_source_without_results_is_not_an_error() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let page = sources::search(&core, EXT, "livre", "vazio", 1, None, None).unwrap();
    assert!(page.results.is_empty() && !page.has_more);
}

#[test]
fn a_source_that_fails_reports_the_kind_of_failure() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    assert_eq!(code(sources::search(&core, EXT, "livre", "erro-rede", 1, None, None)), "source_network");
    assert_eq!(code(sources::search(&core, EXT, "livre", "erro-login", 1, None, None)), "source_authentication");
    assert_eq!(code(sources::search(&core, EXT, "livre", "panico", 1, None, None)), "extension_crashed");
    assert_eq!(titles(&sources::search(&core, EXT, "livre", "ok", 1, None, None).unwrap()), ["ok 1"]);
}

#[test]
fn results_are_filtered_and_cleaned() {
    let (_dir, core, _repo) = core_with_fixture(&["exemplo.com"]);
    let page = sources::search(&core, EXT, "livre", "classificados", 1, None, None).unwrap();
    assert_eq!(titles(&page), ["Juvenil", "Capa insegura"], "sem a adulta e sem título em branco");
    assert_eq!(page.results[0].status.as_deref(), Some("ongoing"));
    assert!(!page.results[1].has_cover, "capa por HTTP é descartada");
    let stored: i64 = core.db().query_row("SELECT count(*) FROM online_series WHERE title = 'Adulta'", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, 0);
}

#[test]
fn a_cancelled_search_stops_and_reports_it() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let cancel = core.requests.begin("consulta");
    let flag = core.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.requests.cancel("consulta");
    });
    assert_eq!(code(sources::search(&core, EXT, "livre", "laco", 1, None, Some(&cancel))), "extension_cancelled");
    core.requests.end("consulta");
}

fn found_series(core: &Core, query: &str) -> i64 {
    sources::search(core, EXT, "livre", query, 1, None, None).unwrap().results[0].id
}

#[test]
fn opening_a_series_stores_details_and_chapters() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let id = found_series(&core, "obra");
    let view = sources::open_series(&core, id, None).unwrap();
    assert_eq!(view.series.title, "Obra obra 1");
    assert_eq!(view.series.summary.as_deref(), Some("Sinopse"));
    assert_eq!(view.series.author.as_deref(), Some("Autoria"));
    assert_eq!(view.series.genres, ["Aventura"]);
    assert_eq!(view.series.status.as_deref(), Some("completed"));
    assert_eq!(view.series.last_update_at, Some(1_700_000_000_003));
    assert!(view.series.last_checked_at.is_some());
    assert_eq!(view.source_error, None);
    let chapters: Vec<_> = view.chapters.iter().map(|c| (c.title.as_str(), c.number.as_deref(), c.state.as_str())).collect();
    assert_eq!(chapters, [("Capítulo 1", Some("1"), "available"), ("Capítulo 2", Some("2"), "available"), ("Capítulo 3", Some("3"), "available")]);
}

#[test]
fn a_series_marked_adult_in_the_details_is_refused_and_stays_out_of_the_lists() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let id = found_series(&core, "obra");
    let remote: String = core.db().query_row("SELECT remote_id FROM online_series WHERE id = ?1", [id], |r| r.get(0)).unwrap();
    core.db().execute("UPDATE online_series SET remote_id = 'adulta' WHERE id = ?1", [id]).unwrap();
    assert_eq!(code(sources::open_series(&core, id, None)), "source_rating");

    // A lista da fonte não traz a marca: quem lembra é o Prancha.
    core.db().execute("UPDATE online_series SET remote_id = ?1 WHERE id = ?2", (&remote, id)).unwrap();
    let listed = || sources::search(&core, EXT, "livre", "obra", 1, None, None).unwrap().results.iter().any(|series| series.id == id);
    assert!(!listed());
    prancha_lib::library::set_setting(&core.db(), "content.adult", &json!(true)).unwrap();
    assert!(listed());
}

#[test]
fn an_unavailable_source_still_shows_what_is_known() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let id = found_series(&core, "obra");
    sources::open_series(&core, id, None).unwrap();
    install::set_enabled(&core.db(), EXT, false).unwrap();
    let view = sources::open_series(&core, id, None).unwrap();
    assert_eq!(view.chapters.len(), 3);
    assert!(view.source_error.is_some());
}

#[test]
fn following_and_sorting_are_remembered() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let id = found_series(&core, "obra");
    assert!(sources::list_following(&core.db()).unwrap().is_empty());
    assert!(sources::set_following(&core.db(), id, true).unwrap().following);
    assert_eq!(sources::list_following(&core.db()).unwrap()[0].id, id);
    assert_eq!(sources::set_chapter_sort(&core.db(), id, "asc").unwrap().chapter_sort, "asc");
    assert_eq!(code(sources::set_chapter_sort(&core.db(), id, "lado")), "bad_request");
    assert!(!sources::set_following(&core.db(), id, false).unwrap().following);
}

#[test]
fn old_passing_results_are_pruned_but_followed_series_stay() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let old = found_series(&core, "velha");
    let followed = found_series(&core, "seguida");
    let recent = found_series(&core, "recente");
    sources::set_following(&core.db(), followed, true).unwrap();
    core.db().execute("UPDATE online_series SET updated_at = 1 WHERE id IN (?1, ?2)", [old, followed]).unwrap();
    assert_eq!(sources::prune_stale(&core, &core.db()).unwrap(), 1);
    assert_eq!(code(sources::card(&core.db(), old)), "series_not_found");
    assert!(sources::card(&core.db(), followed).is_ok());
    assert!(sources::card(&core.db(), recent).is_ok());
}

#[test]
fn a_saved_series_shows_where_the_reading_stopped_and_is_never_pruned() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let id = found_series(&core, "obra");
    let chapters = sources::open_series(&core, id, None).unwrap().chapters;
    assert!(sources::library_series(&core.db()).unwrap().is_empty(), "só vista em uma busca não está na biblioteca");

    sources::set_favorite(&core.db(), id, true).unwrap();
    let saved = || sources::library_series(&core.db()).unwrap().remove(0);
    assert!(saved().favorite);
    assert_eq!((saved().reading.read_status, saved().reading.chapter_count, saved().reading.chapters_read), ("unread", 3, 0));
    assert_eq!(saved().reading.resume.unwrap().chapter_id, chapters[0].id);

    // Só gravar a posição (abrir o capítulo) não é começar a ler.
    sources::save_chapter_progress(&core.db(), chapters[1].id, 0, 3, 0.0).unwrap();
    assert_eq!(saved().reading.read_status, "unread");

    reading::start(&mut core.db(), chapters[1].id).unwrap();
    sources::save_chapter_progress(&core.db(), chapters[1].id, 1, 3, 0.0).unwrap();
    reading::end(&core.db(), chapters[1].id).unwrap();
    let state = saved().reading;
    assert_eq!((state.read_status, state.chapters_read), ("reading", 0));
    let resume = state.resume.unwrap();
    assert_eq!((resume.chapter_id, resume.progress.unwrap().page_index, resume.state), (chapters[1].id, 1, ReadState::InProgress));
    assert!(saved().last_opened_at.is_some());

    reading::complete(&mut core.db(), chapters[1].id).unwrap();
    let state = saved().reading;
    assert_eq!((state.read_status, state.chapters_read), ("reading", 1));
    let resume = state.resume.unwrap();
    assert_eq!(resume.chapter_id, chapters[2].id, "terminado, continua no seguinte");
    assert!(resume.progress.is_none());

    reading::set_read(&mut core.db(), &[chapters[0].id, chapters[2].id], true).unwrap();
    let state = saved().reading;
    assert_eq!((state.read_status, state.chapters_read), ("completed", 3));
    assert_eq!(state.resume.unwrap().chapter_id, chapters[0].id, "terminada, ler de novo começa do primeiro");

    sources::set_series_read(&mut core.db(), id, false).unwrap();
    assert_eq!(saved().reading.read_status, "unread");
    sources::set_series_read(&mut core.db(), id, true).unwrap();
    assert_eq!(saved().reading.read_status, "completed");
    assert_eq!(sources::open_chapter(&core, chapters[0].id, None).unwrap().start.page_index, 0, "marcado como lido recomeça do início");

    // Favorita, ela fica mesmo sem progresso e sem nada baixado.
    sources::set_series_read(&mut core.db(), id, false).unwrap();
    core.db().execute("UPDATE online_series SET updated_at = 1", []).unwrap();
    assert_eq!(sources::prune_stale(&core, &core.db()).unwrap(), 0);
    sources::forget_series(&core, id).unwrap();
    assert_eq!(code(sources::card(&core.db(), id)), "series_not_found");
    assert!(sources::library_series(&core.db()).unwrap().is_empty());
}

#[test]
fn a_series_in_a_collection_counts_while_the_plugin_is_installed() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let id = found_series(&core, "obra");
    let other = found_series(&core, "outra");
    let folder = prancha_lib::library::create_collection(&core.db(), "Mangás").unwrap();
    let info = || prancha_lib::library::list_collections(&core.db()).unwrap().remove(0);

    sources::set_series_collections(&mut core.db(), id, &[folder]).unwrap();
    sources::add_series_to_collection(&mut core.db(), folder, &[other]).unwrap();
    assert_eq!(info().comic_count, 0, "sem o plugin, as obras não aparecem");

    prancha_lib::library::set_setting(&core.db(), "plugins.online", &json!(true)).unwrap();
    assert_eq!(info().comic_count, 2);
    assert_eq!(info().preview.len(), 2);
    assert!(info().preview.iter().all(|cover| cover.online));
    assert_eq!(sources::stored_series(&core.db(), id).unwrap().reading.collections, [folder]);

    // Numa coleção, ela fica mesmo sem favorito e sem progresso.
    core.db().execute("UPDATE online_series SET updated_at = 1", []).unwrap();
    assert_eq!(sources::prune_stale(&core, &core.db()).unwrap(), 0);

    sources::remove_series_from_collection(&core.db(), folder, other).unwrap();
    assert_eq!(info().comic_count, 1);
    sources::set_series_collections(&mut core.db(), id, &[]).unwrap();
    assert_eq!(info().comic_count, 0);

    // Excluir a coleção não leva a obra junto.
    sources::set_series_collections(&mut core.db(), id, &[folder]).unwrap();
    prancha_lib::library::delete_collection(&core.db(), folder).unwrap();
    assert!(sources::stored_series(&core.db(), id).unwrap().reading.collections.is_empty());
}

#[test]
fn the_adult_filter_starts_on_and_decides_what_appears() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let set_filter = |hide: bool| prancha_lib::library::set_setting(&core.db(), "content.adult", &json!(!hide)).unwrap();
    let source_ids = || sources::available_sources(&core.db()).unwrap().into_iter().map(|s| s.source_id).collect::<Vec<_>>();
    assert_eq!(source_ids(), ["livre"]);
    assert_eq!(code(sources::search(&core, EXT, "adulta", "x", 1, None, None)), "source_disabled");

    set_filter(false);
    assert_eq!(source_ids(), ["adulta", "livre", "sem-classificacao"]);
    let found = sources::search(&core, EXT, "adulta", "x", 1, None, None).unwrap();
    let adult_source = found.results[0].id;
    assert!(titles(&sources::search(&core, EXT, "livre", "classificados", 1, None, None).unwrap()).contains(&"Adulta"));
    let marked = found_series(&core, "marcada");
    core.db().execute("UPDATE online_series SET remote_id = 'adulta' WHERE id = ?1", [marked]).unwrap();
    sources::open_series(&core, marked, None).unwrap();
    assert!(install::list(&core.db()).unwrap()[0].sources.iter().all(|source| source.available));

    // Guardadas com o filtro desligado, somem ao ligar e voltam ao desligar.
    let folder = prancha_lib::library::create_collection(&core.db(), "Pasta").unwrap();
    prancha_lib::library::set_setting(&core.db(), "plugins.online", &json!(true)).unwrap();
    for id in [adult_source, marked] {
        sources::set_favorite(&core.db(), id, true).unwrap();
        sources::set_following(&core.db(), id, true).unwrap();
        sources::set_series_collections(&mut core.db(), id, &[folder]).unwrap();
    }
    assert_eq!(sources::library_series(&core.db()).unwrap().len(), 2);
    assert_eq!(sources::list_following(&core.db()).unwrap().len(), 2);
    assert_eq!(prancha_lib::library::list_collections(&core.db()).unwrap()[0].comic_count, 2);

    set_filter(true);
    assert!(sources::library_series(&core.db()).unwrap().is_empty());
    assert!(sources::list_following(&core.db()).unwrap().is_empty());
    assert_eq!(prancha_lib::library::list_collections(&core.db()).unwrap()[0].comic_count, 0);
    assert_eq!(source_ids(), ["livre"]);
    assert_eq!(code(sources::open_series(&core, marked, None)), "source_rating");
    assert!(sources::card(&core.db(), marked).is_ok(), "uma obra guardada não é esquecida");

    set_filter(false);
    assert_eq!(sources::library_series(&core.db()).unwrap().len(), 2);
}

#[test]
fn the_catalog_shows_adult_extensions_only_with_the_filter_off() {
    let (_dir, core) = core();
    let repo = TestRepo::new("com.exemplo.repo", 1);
    let mut adult = repo.entry("com.exemplo.adulta", "1.0.0", b"a");
    adult["contentRating"] = json!("adult");
    adult["languages"] = json!(["ja"]);
    repo.publish(&[adult, repo.entry("com.exemplo.livre", "1.0.0", b"b")]);
    repository::add(&core, &repo.manifest_url(), &repo.fingerprint()).unwrap();
    let ids = || repository::catalog(&core.db(), &CatalogQuery::default(), APP_VERSION).unwrap().into_iter().map(|i| i.entry.id).collect::<Vec<_>>();
    assert_eq!(ids(), ["com.exemplo.livre"]);
    assert_eq!(repository::list(&core.db()).unwrap()[0].extension_count, 1);
    assert_eq!(repository::catalog_languages(&core.db()).unwrap(), ["pt-BR"]);
    assert_eq!(code(install::install(&core, &repo.id, "com.exemplo.adulta")), "extension_rating");

    prancha_lib::library::set_setting(&core.db(), "content.adult", &json!(true)).unwrap();
    assert_eq!(ids(), ["com.exemplo.adulta", "com.exemplo.livre"]);
    assert_eq!(repository::list(&core.db()).unwrap()[0].extension_count, 2);
    assert_eq!(repository::catalog_languages(&core.db()).unwrap(), ["ja", "pt-BR"]);
    assert_eq!(repository::preview(&core, &repo.manifest_url()).unwrap().extension_count, 2);
}

#[test]
fn covers_are_fetched_by_the_backend_and_cached() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let image = common::png(200, 300, 7);
    let body = image.clone();
    let server = serve(move |seen| if seen.path == "/capa.png" { Reply::ok(body.clone()).header("Content-Type", "image/png") } else { Reply::status(404) });
    let id = found_series(&core, "obra");
    let set_cover = |path: &str| core.db().execute("UPDATE online_series SET cover_url = ?2 WHERE id = ?1", rusqlite::params![id, server.url(path)]).unwrap();

    assert_eq!(sources::cover(&core, id).unwrap(), None, "sem capa informada");
    set_cover("/nao-existe.png");
    assert_eq!(sources::cover(&core, id).unwrap(), None);
    set_cover("/capa.png");
    let thumbnail = sources::cover(&core, id).unwrap().unwrap();
    assert_eq!(&thumbnail[..2], [0xff, 0xd8], "miniatura em JPEG");
    let requests = server.seen().len();
    assert_eq!(sources::cover(&core, id).unwrap().unwrap(), thumbnail);
    assert_eq!(server.seen().len(), requests, "a segunda vez vem do cache");

    // Sem a extensão, o que está em cache continua aparecendo.
    install::remove(&core, EXT).unwrap();
    assert!(sources::cover(&core, id).unwrap().is_some());
    std::fs::remove_file(core.paths.covers.join(format!("online-{id}.jpg"))).unwrap();
    assert_eq!(sources::cover(&core, id).unwrap(), None);
}

#[test]
fn a_cover_outside_the_extension_hosts_is_not_fetched() {
    let (_dir, core, _repo) = core_with_fixture(&["exemplo.com"]);
    let server = serve(|_| Reply::ok("x"));
    let id = found_series(&core, "obra");
    core.db().execute("UPDATE online_series SET cover_url = ?2 WHERE id = ?1", rusqlite::params![id, server.url("/capa.png")]).unwrap();
    assert_eq!(sources::cover(&core, id).unwrap(), None);
    assert!(server.seen().is_empty());
}

// ---------------------------------------------------------------------------
// Filtro +18 nas fontes do Mihon, com um Suwayomi de mentira e obras de teste
// ---------------------------------------------------------------------------

const BRIDGED: &str = "eu.kanade.tachiyomi.extension.all.teste";
const BRIDGED_SOURCE: &str = "123";

#[derive(Clone, Default)]
struct FakeWork {
    title: String,
    /// O que a lista traz. Em boa parte das fontes, nada.
    listed: Vec<String>,
    /// O que a consulta da obra traz.
    details: Vec<String>,
    /// A consulta da obra falha com esta mensagem.
    failure: Option<String>,
}

fn fake_work(title: &str, listed: &[&str], details: &[&str]) -> FakeWork {
    let names = |list: &[&str]| list.iter().map(|name| name.to_string()).collect();
    FakeWork { title: title.to_string(), listed: names(listed), details: names(details), failure: None }
}

struct FakeSuwayomi {
    server: http::Server,
    works: Arc<std::sync::Mutex<Vec<FakeWork>>>,
}

impl FakeSuwayomi {
    fn new(works: Vec<FakeWork>, check_delay: Duration) -> Self {
        let works = Arc::new(std::sync::Mutex::new(works));
        let shared = works.clone();
        let image = common::png(20, 30, 7);
        let server = serve(move |seen| {
            if seen.path.ends_with("/thumbnail") {
                return Reply::ok(image.clone()).header("Content-Type", "image/png");
            }
            let works = shared.lock().unwrap();
            let call: Value = serde_json::from_slice(&seen.body).unwrap();
            let query = call["query"].as_str().unwrap();
            let data = |field: &str, value: Value| Reply::ok(json!({ "data": { field: value } }).to_string());
            if query.contains("fetchSourceManga") {
                let mangas: Vec<Value> = works.iter().enumerate().map(|(at, work)| json!({ "id": at + 1, "title": work.title, "thumbnailUrl": format!("/api/v1/manga/{}/thumbnail", at + 1), "genre": work.listed })).collect();
                return data("fetchSourceManga", json!({ "hasNextPage": false, "mangas": mangas }));
            }
            if query.contains("fetchManga") {
                let id = call["variables"]["i"].as_u64().unwrap() as usize;
                let work = &works[id - 1];
                if let Some(message) = &work.failure {
                    return Reply::ok(json!({ "errors": [{ "message": message }], "data": null }).to_string()).after(check_delay);
                }
                let manga = json!({ "id": id, "title": work.title, "author": "Autoria", "artist": null, "description": "Sinopse", "status": "ONGOING", "genre": work.details, "thumbnailUrl": format!("/api/v1/manga/{id}/thumbnail") });
                return data("fetchManga", json!({ "manga": manga })).after(check_delay);
            }
            if query.contains("fetchChapters") {
                return data("fetchChapters", json!({ "chapters": [{ "id": 7, "name": "Capítulo 1", "chapterNumber": 1, "sourceOrder": 1, "uploadDate": "1620684028000" }] }));
            }
            Reply::status(404)
        });
        FakeSuwayomi { server, works }
    }

    fn checks(&self) -> usize {
        self.server.seen().iter().filter(|seen| String::from_utf8_lossy(&seen.body).contains("fetchManga(")).count()
    }

    fn checks_of(&self, remote_id: &str) -> usize {
        self.server.seen().iter().filter_map(|seen| serde_json::from_slice::<Value>(&seen.body).ok()).filter(|call| call["query"].as_str().is_some_and(|query| query.contains("fetchManga(")) && call["variables"]["i"].to_string() == remote_id).count()
    }

    fn covers_requested(&self) -> usize {
        self.server.seen().iter().filter(|seen| seen.path.ends_with("/thumbnail")).count()
    }
}

fn core_with_bridge(fake: &FakeSuwayomi) -> (tempfile::TempDir, Arc<Core>) {
    let (dir, core) = core();
    suwayomi::attach(&core, &fake.server.base).unwrap();
    let mut entry = TestRepo::new("com.exemplo.repo", 1).entry(BRIDGED, "1.0.0", b"x");
    entry["runtime"] = json!(suwayomi::RUNTIME);
    entry["capabilities"]["network"] = json!(true);
    core.db()
        .execute(
            "INSERT INTO installed_extensions (id, repository_id, repository_name, public_key, key_fingerprint, name, version, entry, file_name, sha256, installed_at, updated_at)
             VALUES (?1, 'r', 'R', 'k', 'f', 'E', '1.0.0', ?2, 'e', 'h', 1, 1)",
            [BRIDGED, entry.to_string().as_str()],
        )
        .unwrap();
    // A fonte se declara segura, como a do caso real: a marca não diz nada sobre cada obra.
    core.db()
        .execute("INSERT INTO extension_sources (extension_id, source_id, name, language, rating, direction, supports_latest, supports_search) VALUES (?1, ?2, 'Fonte', 'pt-BR', 'general', 'ltr', 1, 1)", [BRIDGED, BRIDGED_SOURCE])
        .unwrap();
    (dir, Arc::new(core))
}

fn bridged_list(core: &Core) -> sources::BrowsePage {
    sources::popular(core, BRIDGED, BRIDGED_SOURCE, 1, None, None).unwrap()
}

fn set_filter(core: &Core, hide: bool) {
    library::set_setting(&core.db(), "content.adult", &json!(!hide)).unwrap();
}

fn stored_id(core: &Core, title: &str) -> Option<i64> {
    core.db().query_row("SELECT id FROM online_series WHERE title = ?1", [title], |r| r.get(0)).ok()
}

#[test]
fn with_the_filter_on_a_list_without_genres_shows_only_what_the_details_classify() {
    let fake = FakeSuwayomi::new(vec![fake_work("Segura", &[], &["Ação"]), fake_work("Marcada", &[], &["Ecchi"]), fake_work("Sem gêneros", &[], &[])], Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);

    let page = bridged_list(&core);
    assert_eq!(titles(&page), ["Segura"]);
    assert_eq!((page.hidden, page.unverified), (2, 0));
    assert_eq!(page.results[0].genres, ["Ação"], "a consulta já guarda os detalhes");
    assert!(sources::cover(&core, page.results[0].id).unwrap().is_some());
    for title in ["Marcada", "Sem gêneros"] {
        assert_eq!(sources::cover(&core, stored_id(&core, title).unwrap()).unwrap(), None, "{title}");
    }
    assert_eq!(fake.covers_requested(), 1, "a capa de obra escondida nem é pedida");

    set_filter(&core, false);
    let page = bridged_list(&core);
    assert_eq!(titles(&page), ["Segura", "Marcada", "Sem gêneros"]);
    assert_eq!((page.hidden, page.unverified), (0, 0));
    assert!(sources::cover(&core, stored_id(&core, "Marcada").unwrap()).unwrap().is_some());
}

#[test]
fn a_classified_series_is_not_asked_about_again() {
    let fake = FakeSuwayomi::new(vec![fake_work("Segura", &[], &["Ação"]), fake_work("Sem gêneros", &[], &[])], Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);
    bridged_list(&core);
    assert_eq!(fake.checks(), 2);
    assert_eq!(titles(&bridged_list(&core)), ["Segura"]);
    assert_eq!(fake.checks(), 2, "nem a que a fonte não classifica volta a ser consultada");
}

#[test]
fn a_list_that_brings_genres_needs_no_extra_call() {
    let fake = FakeSuwayomi::new(vec![fake_work("Com gêneros", &["Ação"], &["Ação"]), fake_work("Adulta", &["Hentai"], &["Hentai"])], Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);
    let page = bridged_list(&core);
    assert_eq!(titles(&page), ["Com gêneros"]);
    assert_eq!(page.hidden, 1);
    assert_eq!(fake.checks(), 0);
    assert_eq!(stored_id(&core, "Adulta"), None);
    assert_eq!(page.results[0].genres, ["Ação"]);
}

#[test]
fn a_series_that_could_not_be_checked_stays_hidden_and_is_tried_again() {
    let mut failing = fake_work("Falhou", &[], &["Ação"]);
    failing.failure = Some("HTTP error 500".into());
    let fake = FakeSuwayomi::new(vec![fake_work("Boa", &[], &["Ação"]), failing], Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);

    for hide in [true, false] {
        set_filter(&core, hide);
        let page = bridged_list(&core);
        assert_eq!(titles(&page), ["Boa"], "sem saber os gêneros, nem com o filtro desligado");
        assert_eq!((page.hidden, page.unverified), (0, 1));
    }
    assert_eq!(sources::cover(&core, stored_id(&core, "Falhou").unwrap()).unwrap().is_some(), true, "guardada e com o filtro desligado, como antes");

    set_filter(&core, true);
    assert_eq!(sources::cover(&core, stored_id(&core, "Falhou").unwrap()).unwrap(), None);
    fake.works.lock().unwrap()[1].failure = None;
    assert_eq!(titles(&bridged_list(&core)), ["Boa", "Falhou"]);
}

#[test]
fn a_source_whose_checks_all_fail_says_so_and_stops_trying() {
    let failing = |message: &str, count: usize| (0..count).map(|at| FakeWork { title: format!("Obra {at}"), failure: Some(message.to_string()), ..Default::default() }).collect::<Vec<_>>();

    let fake = FakeSuwayomi::new(failing("HTTP error 503", 30), Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);
    assert_eq!(code(sources::popular(&core, BRIDGED, BRIDGED_SOURCE, 1, None, None)), "source_error");
    assert!(fake.checks() < 10, "um site fora do ar não recebe uma consulta por obra: {}", fake.checks());

    let fake = FakeSuwayomi::new(failing("Cloudflare bypass currently disabled", 30), Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);
    assert_eq!(code(sources::popular(&core, BRIDGED, BRIDGED_SOURCE, 1, None, None)), "source_protected");
    assert!(fake.checks() < 10, "{}", fake.checks());
}

#[test]
fn a_few_failures_along_the_way_do_not_hide_the_rest_of_the_list() {
    let works: Vec<FakeWork> = (1..=40)
        .map(|at| {
            let mut work = fake_work(&format!("Obra {at}"), &[], &["Ação"]);
            work.failure = (at % 8 == 0).then(|| "HTTP error 500".to_string());
            work
        })
        .collect();
    let fake = FakeSuwayomi::new(works, Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);
    let page = bridged_list(&core);
    assert_eq!(page.results.len(), 35);
    assert_eq!((page.hidden, page.unverified), (0, 5));
    assert_eq!(fake.checks(), 40, "só falhas seguidas param as consultas");
}

#[test]
fn leaving_the_screen_stops_the_checks_and_keeps_what_was_answered() {
    let works = (0..24).map(|at| fake_work(&format!("Obra {at}"), &[], &["Ação"])).collect();
    let fake = FakeSuwayomi::new(works, Duration::from_millis(150));
    let (_dir, core) = core_with_bridge(&fake);
    let cancel = core.requests.begin("lista");
    let flag = core.clone();
    // Sai da tela assim que a primeira resposta estiver guardada.
    std::thread::spawn(move || {
        while flag.db().query_row("SELECT count(*) FROM online_series WHERE classification = 'known'", [], |r| r.get::<_, i64>(0)).unwrap() == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
        flag.requests.cancel("lista");
    });
    assert_eq!(code(sources::popular(&core, BRIDGED, BRIDGED_SOURCE, 1, None, Some(&cancel))), "extension_cancelled");
    core.requests.end("lista");
    let asked = fake.checks();
    assert!(asked < 24, "{asked}");
    let answered: Vec<String> = {
        let conn = core.db();
        let mut known = conn.prepare("SELECT remote_id FROM online_series WHERE classification = 'known'").unwrap();
        known.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert!(!answered.is_empty());

    assert_eq!(bridged_list(&core).results.len(), 24);
    for remote_id in answered {
        assert_eq!(fake.checks_of(&remote_id), 1, "só pergunta o que faltou: {remote_id}");
    }
}

// Rode com `--nocapture` para ver os tempos. A demora é simulada: num site de verdade depende dele e dos limites da extensão.
#[test]
fn checking_a_list_costs_time_only_on_the_first_visit() {
    const WORKS: u32 = 30;
    let delay = Duration::from_millis(150);
    let works = (0..WORKS).map(|at| fake_work(&format!("Obra {at}"), &[], &["Ação"])).collect();
    let fake = FakeSuwayomi::new(works, delay);
    let (_dir, core) = core_with_bridge(&fake);

    let started = Instant::now();
    assert_eq!(bridged_list(&core).results.len(), WORKS as usize);
    let first = started.elapsed();
    let started = Instant::now();
    assert_eq!(bridged_list(&core).results.len(), WORKS as usize);
    let second = started.elapsed();
    println!("{WORKS} obras, {delay:?} por consulta: primeira visita {first:?}, segunda {second:?}");

    assert_eq!(fake.checks(), WORKS as usize, "a segunda visita não consulta nada");
    assert!(first >= delay * (WORKS / 4), "no máximo 4 consultas ao mesmo tempo: {first:?}");
    assert!(first < delay * WORKS * 3 / 4, "as consultas correm em paralelo: {first:?}");
}

#[test]
fn a_series_that_sexualizes_minors_never_appears_with_the_filter_on_or_off() {
    let works = vec![fake_work("Comum", &[], &["Ação"]), fake_work("Obra Lolicon", &[], &["Ação"]), fake_work("Pelo gênero nos detalhes", &[], &["Loli"]), fake_work("Pelo gênero na lista", &["Shota"], &["Ação"])];
    let blocked = ["Obra Lolicon", "Pelo gênero nos detalhes", "Pelo gênero na lista"];
    let fake = FakeSuwayomi::new(works, Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);

    for hide in [false, true, false] {
        set_filter(&core, hide);
        let page = bridged_list(&core);
        assert_eq!(titles(&page), ["Comum"], "filtro ligado: {hide}");
        assert_eq!(page.hidden, 0, "desligar o filtro não traz estas de volta, então a tela não sugere isso");
        for title in blocked {
            assert_eq!(stored_id(&core, title), None, "{title} não fica no banco");
        }
    }
    let kept: i64 = core.db().query_row("SELECT count(*) FROM blocked_series", [], |r| r.get(0)).unwrap();
    assert_eq!(kept, 3);
    assert_eq!(fake.checks(), 2, "só a comum e a do gênero nos detalhes, e uma vez cada");
}

#[test]
fn a_native_source_is_checked_by_title_too() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    for hide in [true, false] {
        set_filter(&core, hide);
        let page = sources::search(&core, EXT, "livre", "Lolicon", 1, None, None).unwrap();
        assert!(page.results.is_empty(), "filtro ligado: {hide}");
        assert_eq!(page.hidden, 0);
    }
    assert_eq!(stored_id(&core, "Lolicon 1"), None);
}

#[test]
fn opening_a_series_that_turned_out_to_sexualize_minors_removes_it() {
    let fake = FakeSuwayomi::new(vec![fake_work("Obra", &[], &["Ação"])], Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);
    let id = bridged_list(&core).results[0].id;
    sources::set_favorite(&core.db(), id, true).unwrap();
    assert!(sources::cover(&core, id).unwrap().is_some());
    let cover = core.paths.covers.join(format!("online-{id}.jpg"));
    assert!(cover.exists());

    fake.works.lock().unwrap()[0].details = vec!["Loli".into()];
    assert_eq!(code(sources::open_series(&core, id, None)), "series_blocked");
    assert_eq!(code(sources::card(&core.db(), id)), "series_not_found");
    assert!(!cover.exists());
    assert_eq!(sources::cover(&core, id).unwrap(), None);
    assert!(bridged_list(&core).results.is_empty(), "e não volta pela lista");
}

#[test]
fn what_was_stored_before_is_removed_at_startup_without_touching_the_rest() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let conn = core.db();
    conn.execute_batch(
        "INSERT INTO online_series (id, extension_id, source_id, remote_id, title, genres, favorite, classification, created_at, updated_at) VALUES
            (1, 'e', 's', 'titulo', 'Obra Lolicon', '[]', 1, 'known', 1, 1),
            (2, 'e', 's', 'genero', 'Obra', '[\"Shota\"]', 1, 'known', 1, 1),
            (3, 'e', 's', 'comum', 'Comum', '[\"Ação\"]', 1, 'known', 1, 1);
         INSERT INTO online_chapters (id, series_id, remote_id, title, sort_order, file_path) VALUES (1, 1, 'cap', 'Cap', 0, 'capitulo.cbz');
         INSERT INTO chapter_downloads (id, chapter_id, status, created_at, updated_at) VALUES (1, 1, 'paused', 1, 1);",
    )
    .unwrap();
    let cover = core.paths.covers.join("online-1.jpg");
    std::fs::write(&cover, b"capa").unwrap();
    let partial = core.paths.data_dir.join("downloads/tmp/1");
    std::fs::create_dir_all(&partial).unwrap();
    std::fs::write(partial.join("0001.png"), b"pagina").unwrap();

    assert_eq!(sources::forget_blocked(&core, &conn).unwrap(), 2);
    let left: Vec<String> = sources::library_series(&conn).unwrap().into_iter().map(|series| series.title).collect();
    assert_eq!(left, ["Comum"]);
    assert!(!cover.exists() && !partial.exists(), "nem capa nem páginas soltas ficam no disco");
    let downloads: i64 = conn.query_row("SELECT count(*) FROM chapter_downloads", [], |r| r.get(0)).unwrap();
    assert_eq!(downloads, 0);
    assert_eq!(sources::forget_blocked(&core, &conn).unwrap(), 0);
}

#[test]
fn a_series_without_classification_stays_out_of_every_screen_with_the_filter_on() {
    let fake = FakeSuwayomi::new(vec![fake_work("Sem gêneros", &[], &[])], Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);
    let folder = library::create_collection(&core.db(), "Pasta").unwrap();
    library::set_setting(&core.db(), "plugins.online", &json!(true)).unwrap();

    set_filter(&core, false);
    let id = bridged_list(&core).results[0].id;
    assert!(sources::open_series(&core, id, None).is_ok(), "com o filtro desligado, abre");
    assert!(sources::cover(&core, id).unwrap().is_some());
    sources::set_favorite(&core.db(), id, true).unwrap();
    sources::set_following(&core.db(), id, true).unwrap();
    sources::set_series_collections(&mut core.db(), id, &[folder]).unwrap();

    set_filter(&core, true);
    assert!(sources::library_series(&core.db()).unwrap().is_empty());
    assert!(sources::list_following(&core.db()).unwrap().is_empty());
    assert_eq!(library::list_collections(&core.db()).unwrap()[0].comic_count, 0);
    assert_eq!(sources::cover(&core, id).unwrap(), None, "nem a capa que já estava em cache");
    assert_eq!(code(sources::open_series(&core, id, None)), "source_unrated");

    let adult: bool = core.db().query_row("SELECT adult FROM online_series WHERE id = ?1", [id], |r| r.get(0)).unwrap();
    assert!(!adult, "não é a mesma coisa que ser adulta");
    set_filter(&core, false);
    assert_eq!(sources::library_series(&core.db()).unwrap().len(), 1, "guardada, volta ao desligar");
}

#[test]
fn details_that_stop_bringing_genres_do_not_take_away_a_classification() {
    let fake = FakeSuwayomi::new(vec![fake_work("Obra", &[], &["Ação"])], Duration::ZERO);
    let (_dir, core) = core_with_bridge(&fake);
    let id = bridged_list(&core).results[0].id;
    fake.works.lock().unwrap()[0].details.clear();
    let view = sources::open_series(&core, id, None).unwrap();
    assert_eq!(view.series.genres, ["Ação"]);
    assert_eq!(titles(&bridged_list(&core)), ["Obra"]);
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

/// Servidor de páginas: `/n.png` responde a imagem `n`. Cada página tem conteúdo próprio.
fn pages_server() -> http::Server {
    serve(|seen| {
        let number = seen.path.trim_start_matches('/').trim_end_matches(".png").parse::<u8>().unwrap_or(0);
        Reply::ok(common::png(40, 60, number.wrapping_mul(13))).header("Content-Type", "image/png")
    })
}

/// Registra um capítulo cujo identificador remoto já diz à extensão quais páginas informar.
fn chapter(core: &Core, series: &str, remote: &str) -> i64 {
    let conn = core.db();
    conn.execute(
        "INSERT INTO online_series (extension_id, source_id, remote_id, title, created_at, updated_at) VALUES (?1, 'livre', ?2, ?2, 1, 1) ON CONFLICT DO NOTHING",
        rusqlite::params![EXT, series],
    )
    .unwrap();
    let series_id: i64 = conn.query_row("SELECT id FROM online_series WHERE remote_id = ?1", [series], |r| r.get(0)).unwrap();
    conn.execute("INSERT INTO online_chapters (series_id, remote_id, title, number, sort_order) VALUES (?1, ?2, 'Capítulo 1', '1', 0)", rusqlite::params![series_id, remote]).unwrap();
    conn.last_insert_rowid()
}

fn wait_until(core: &Core, id: i64, statuses: &[&str]) -> downloads::DownloadView {
    let give_up = Instant::now() + Duration::from_secs(30);
    loop {
        let view = downloads::get(&core.db(), id).unwrap();
        if statuses.contains(&view.status.as_str()) {
            return view;
        }
        assert!(Instant::now() < give_up, "download {id} parou em {view:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn entry_names(path: &str) -> Vec<String> {
    let file = std::fs::File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().unwrap().to_string()).collect()
}

fn downloaded(core: &Arc<Core>, chapter_id: i64) -> downloads::DownloadView {
    let ids = downloads::enqueue(core, &[chapter_id]).unwrap();
    wait_until(core, ids[0], &["completed", "failed", "cancelled", "paused"])
}

#[test]
fn a_chapter_becomes_a_valid_cbz_in_the_library() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "obra", &format!("{}|0,1,2", server.base));
    let view = downloaded(&core, chapter_id);
    assert_eq!((view.status.as_str(), view.pages_done, view.pages_total), ("completed", 3, 3), "{view:?}");
    assert!(view.bytes > 0);

    let comic_id = view.comic_id.expect("entrou na biblioteca");
    let info = library::file_info(&core.db(), comic_id).unwrap();
    assert_eq!(info.format, "cbz");
    assert!(info.path.contains("downloads"));
    assert_eq!(entry_names(&info.path), ["0001.png", "0002.png", "0003.png", "ComicInfo.xml"]);

    let comic = library::summary(&core.db(), comic_id).unwrap();
    assert_eq!(comic.series.as_deref(), Some("obra"));
    assert_eq!(comic.page_count, Some(3));

    let (bytes, mime) = core.pages.read_page(&info, 0).unwrap();
    assert_eq!(mime, "image/png");
    assert_eq!(bytes, common::png(40, 60, 0));

    let origin = sources::comic_origin(&core.db(), comic_id).unwrap().unwrap();
    assert_eq!((origin.series_title.as_str(), origin.chapter_title.as_str()), ("obra", "Capítulo 1"));
    assert!(!core.paths.data_dir.join("downloads/tmp").join(view.id.to_string()).exists(), "o temporário sai");

    let chapters = sources::chapters_of(&core.db(), view.series_id).unwrap();
    assert_eq!((chapters[0].state.as_str(), chapters[0].comic_id), ("offline", Some(comic_id)));
}

// ---------------------------------------------------------------------------
// Leitura online
// ---------------------------------------------------------------------------

#[test]
fn a_chapter_is_read_page_by_page_without_writing_anything() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "obra", &format!("{}|0,1,2", server.base));

    let session = sources::open_chapter(&core, chapter_id, None).unwrap();
    assert_eq!((session.page_count, session.start.page_index, session.comic_id), (3, 0, None));
    assert_eq!(session.series_title, "obra");
    assert!(server.seen().is_empty(), "abrir só pede a lista de páginas");

    let (bytes, mime) = sources::chapter_page(&core, chapter_id, 1).unwrap();
    assert_eq!((bytes, mime), (common::png(40, 60, 13), "image/png"));
    assert_eq!(server.seen().iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), ["/1.png"], "só a página pedida");
    assert_eq!(code(sources::chapter_page(&core, chapter_id, 3)), "page_not_found");

    assert!(!core.paths.data_dir.join("downloads").exists(), "ler não baixa nada");
    let comics: i64 = core.db().query_row("SELECT count(*) FROM comics", [], |r| r.get(0)).unwrap();
    assert_eq!(comics, 0, "ler não coloca nada na biblioteca");
}

#[test]
fn next_and_previous_follow_the_reading_order_whatever_the_order_on_screen() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let first = chapter(&core, "obra", &format!("{}|0", server.base));
    let second = chapter(&core, "obra", &format!("{}|0,1", server.base));
    let third = chapter(&core, "obra", &format!("{}|0,1,2", server.base));
    let conn = core.db();
    for (id, order) in [(first, 1), (second, 2), (third, 3)] {
        conn.execute("UPDATE online_chapters SET sort_order = ?2 WHERE id = ?1", [id, order]).unwrap();
    }
    conn.execute("UPDATE online_series SET chapter_sort = 'desc'", []).unwrap();
    drop(conn);

    let open = |id| sources::open_chapter(&core, id, None).unwrap();
    assert_eq!((open(first).previous_id, open(first).next_id), (None, Some(second)));
    assert_eq!((open(second).previous_id, open(second).next_id), (Some(first), Some(third)));
    assert_eq!((open(third).previous_id, open(third).next_id), (Some(second), None));
}

#[test]
fn online_reading_continues_where_it_stopped_and_a_finished_chapter_starts_over() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "obra", &format!("{}|0,1,2", server.base));
    let series_id: i64 = core.db().query_row("SELECT series_id FROM online_chapters WHERE id = ?1", [chapter_id], |r| r.get(0)).unwrap();

    sources::save_chapter_progress(&core.db(), chapter_id, 1, 3, 0.5).unwrap();
    let session = sources::open_chapter(&core, chapter_id, None).unwrap();
    assert_eq!((session.start.page_index, session.start.scroll_ratio), (1, 0.5));
    let listed = sources::chapters_of(&core.db(), series_id).unwrap();
    let progress = listed[0].progress.as_ref().unwrap();
    assert_eq!((progress.page_index, progress.page_count), (1, 3));

    sources::save_chapter_progress(&core.db(), chapter_id, 2, 3, 0.0).unwrap();
    assert_eq!(sources::open_chapter(&core, chapter_id, None).unwrap().start.page_index, 2, "sem conclusão, a última página aberta não recomeça");
    reading::complete(&mut core.db(), chapter_id).unwrap();
    assert_eq!(sources::open_chapter(&core, chapter_id, None).unwrap().start.page_index, 0, "concluído, reler começa do início");

    sources::reset_chapter_progress(&core.db(), chapter_id).unwrap();
    assert!(sources::chapters_of(&core.db(), series_id).unwrap()[0].progress.is_none());
    assert_eq!(code(sources::save_chapter_progress(&core.db(), chapter_id, 0, 0, 0.0)), "invalid");
}

#[test]
fn a_series_with_online_progress_is_not_forgotten() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let chapter_id = chapter(&core, "obra", "x|0");
    sources::save_chapter_progress(&core.db(), chapter_id, 0, 3, 0.0).unwrap();
    core.db().execute("UPDATE online_series SET updated_at = 1", []).unwrap();
    assert_eq!(sources::prune_stale(&core, &core.db()).unwrap(), 0);
}

#[test]
fn online_pages_follow_the_same_rules_as_downloads() {
    let (_dir, core, _repo) = core_with_fixture(&["exemplo.com"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "fora", &format!("{}|0", server.base));
    sources::open_chapter(&core, chapter_id, None).unwrap();
    assert!(sources::chapter_page(&core, chapter_id, 0).is_err(), "host fora da lista");
    assert!(server.seen().is_empty());

    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = serve(|seen| match seen.path.as_str() {
        "/0.png" => Reply::ok("<html>não é imagem</html>").header("Content-Type", "image/png"),
        _ => Reply::status(500),
    });
    let chapter_id = chapter(&core, "ruim", &format!("{}|0,1", server.base));
    sources::open_chapter(&core, chapter_id, None).unwrap();
    assert_eq!(code(sources::chapter_page(&core, chapter_id, 0)), "download_page");
    assert_eq!(code(sources::chapter_page(&core, chapter_id, 1)), "download_network");

    install::set_enabled(&core.db(), EXT, false).unwrap();
    assert!(sources::open_chapter(&core, chapter_id, None).is_err(), "extensão desabilitada não lê");
}

#[test]
fn a_page_list_that_expired_is_asked_again_after_a_failure() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let failing = Arc::new(AtomicBool::new(true));
    let flag = failing.clone();
    let server = serve(move |_| if flag.load(Ordering::Relaxed) { Reply::status(403) } else { Reply::ok(common::png(40, 60, 1)).header("Content-Type", "image/png") });
    let chapter_id = chapter(&core, "obra", &format!("{}|0", server.base));
    sources::open_chapter(&core, chapter_id, None).unwrap();
    assert!(sources::chapter_page(&core, chapter_id, 0).is_err());
    failing.store(false, Ordering::Relaxed);
    assert!(sources::chapter_page(&core, chapter_id, 0).is_ok());
}

#[test]
fn a_downloaded_chapter_opens_from_the_file_and_keeps_the_online_progress() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "obra", &format!("{}|0,1,2", server.base));
    sources::save_chapter_progress(&core.db(), chapter_id, 1, 3, 0.25).unwrap();
    let comic_id = downloaded(&core, chapter_id).comic_id.unwrap();

    install::remove(&core, EXT).unwrap();
    drop(server);
    let session = sources::open_chapter(&core, chapter_id, None).unwrap();
    assert_eq!((session.comic_id, session.page_count, session.start.page_index), (Some(comic_id), 3, 1));
    assert_eq!(sources::chapter_page(&core, chapter_id, 0).unwrap().1, "image/png");

    sources::save_chapter_progress(&core.db(), chapter_id, 2, 3, 0.0).unwrap();
    let page: i64 = core.db().query_row("SELECT page_index FROM reading_progress WHERE comic_id = ?1", [comic_id], |r| r.get(0)).unwrap();
    assert_eq!(page, 2, "o progresso de um capítulo baixado é o da biblioteca");
}

#[test]
fn the_reading_mode_is_remembered_per_series() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "obra", &format!("{}|0", server.base));
    let session = sources::open_chapter(&core, chapter_id, None).unwrap();
    assert_eq!(session.reader_mode, None);
    sources::set_reader_mode(&core.db(), session.series_id, "webtoon").unwrap();
    assert_eq!(sources::open_chapter(&core, chapter_id, None).unwrap().reader_mode.as_deref(), Some("webtoon"));
    assert_eq!(code(sources::set_reader_mode(&core.db(), session.series_id, "lado")), "bad_request");
}

#[test]
fn pages_follow_the_declared_order_whatever_the_list_order() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "obra", &format!("{}|2,0,1", server.base));
    let view = downloaded(&core, chapter_id);
    assert_eq!(view.status, "completed", "{view:?}");
    let info = library::file_info(&core.db(), view.comic_id.unwrap()).unwrap();
    for (position, number) in [(0usize, 0u8), (1, 1), (2, 2)] {
        assert_eq!(core.pages.read_page(&info, position).unwrap().0, common::png(40, 60, number.wrapping_mul(13)));
    }
}

#[test]
fn repeated_or_missing_positions_fail_the_chapter() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    for (index, order) in ["0,0,1", "0,2", "1,2", ""].iter().enumerate() {
        let chapter_id = chapter(&core, &format!("obra{index}"), &format!("{}|{order}", server.base));
        let view = downloaded(&core, chapter_id);
        assert_eq!(view.status, "failed", "{order}: {view:?}");
        assert!(view.comic_id.is_none());
    }
    assert!(server.seen().is_empty(), "nada foi baixado");
}

#[test]
fn an_invalid_page_fails_and_cleans_only_its_own_files() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = serve(|seen| match seen.path.as_str() {
        "/0.png" => Reply::ok(common::png(40, 60, 1)).header("Content-Type", "image/png"),
        "/1.png" => Reply::ok("<html>não é imagem</html>").header("Content-Type", "image/png"),
        _ => Reply::status(404),
    });
    let good_server = pages_server();
    let good = downloaded(&core, chapter(&core, "boa", &format!("{}|0,1", good_server.base)));
    assert_eq!(good.status, "completed");
    let good_file = library::file_info(&core.db(), good.comic_id.unwrap()).unwrap().path;

    let view = downloaded(&core, chapter(&core, "ruim", &format!("{}|0,1", server.base)));
    assert_eq!(view.status, "failed");
    assert!(view.error.as_deref().unwrap().contains("Página 2"), "{view:?}");
    assert!(view.comic_id.is_none());
    assert!(!core.paths.data_dir.join("downloads/tmp").join(view.id.to_string()).exists());
    assert!(std::path::Path::new(&good_file).exists(), "o concluído continua lá");
}

#[test]
fn a_page_with_a_bad_status_or_blocked_host_fails_the_download() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = serve(|_| Reply::status(500));
    let view = downloaded(&core, chapter(&core, "erro", &format!("{}|0", server.base)));
    assert_eq!(view.status, "failed");
    assert!(view.error.as_deref().unwrap().contains("500"));
    let view = downloaded(&core, chapter(&core, "local", "http://10.0.0.1|0"));
    assert_eq!(view.status, "failed");
    assert!(view.error.as_deref().unwrap().contains("não permitido"), "{view:?}");
    let redirected = serve(|_| Reply::redirect("http://169.254.169.254/latest"));
    let view = downloaded(&core, chapter(&core, "desvio", &format!("{}|0", redirected.base)));
    assert_eq!(view.status, "failed");
    assert!(view.error.as_deref().unwrap().contains("não permitido"), "{view:?}");
}

#[test]
fn downloading_again_does_not_duplicate_the_library_entry() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "obra", &format!("{}|0,1", server.base));
    let first = downloaded(&core, chapter_id);
    let comic_id = first.comic_id.unwrap();
    assert!(downloads::enqueue(&core, &[chapter_id]).unwrap().is_empty(), "já está na biblioteca");

    // O arquivo foi apagado por fora: baixar de novo recupera a mesma entrada, com o progresso.
    library::save_progress(&core.db(), comic_id, 1, 2, 0.0).unwrap();
    let path = library::file_info(&core.db(), comic_id).unwrap().path;
    std::fs::remove_file(&path).unwrap();
    let second = downloaded(&core, chapter_id);
    assert_eq!((second.status.as_str(), second.comic_id), ("completed", Some(comic_id)), "{second:?}");
    let comics: i64 = core.db().query_row("SELECT count(*) FROM comics", [], |r| r.get(0)).unwrap();
    assert_eq!(comics, 1);
    assert_eq!(library::summary(&core.db(), comic_id).unwrap().progress.unwrap().page_index, 1);

    // O mesmo conteúdo publicado com outro identificador aponta para a entrada existente, sem deixar cópia.
    let twin = chapter(&core, "gemea", &format!("{}|0,1", server.base));
    core.db().execute("UPDATE online_series SET title = 'obra' WHERE remote_id = 'gemea'", []).unwrap();
    let third = downloaded(&core, twin);
    assert_eq!((third.status.as_str(), third.comic_id), ("completed", Some(comic_id)));
    let cbz = walkdir::WalkDir::new(core.paths.data_dir.join("downloads")).into_iter().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "cbz")).count();
    assert_eq!(cbz, 1);
}

#[test]
fn a_download_can_be_cancelled_and_retried() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let slow = serve(|seen| {
        let number = seen.path.trim_start_matches('/').trim_end_matches(".png").parse::<u8>().unwrap_or(0);
        Reply::ok(common::png(40, 60, number)).header("Content-Type", "image/png").after(Duration::from_millis(200))
    });
    let chapter_id = chapter(&core, "obra", &format!("{}|0,1,2,3,4,5,6,7,8,9", slow.base));
    let id = downloads::enqueue(&core, &[chapter_id]).unwrap()[0];
    wait_until(&core, id, &["running"]);
    std::thread::sleep(Duration::from_millis(300));
    downloads::cancel(&core, id).unwrap();
    let view = wait_until(&core, id, &["cancelled"]);
    assert!(view.comic_id.is_none());
    assert!(view.finished_at.is_some());
    assert!(!core.paths.data_dir.join("downloads/tmp").join(id.to_string()).exists());

    downloads::retry(&core, id).unwrap();
    let view = wait_until(&core, id, &["completed", "failed"]);
    assert_eq!((view.status.as_str(), view.pages_done), ("completed", 10), "{view:?}");
    assert_eq!(downloads::clear_finished(&core.db()).unwrap(), 1);
    assert!(downloads::list(&core.db(), 10).unwrap().is_empty());
    assert!(view.comic_id.is_some_and(|comic| library::file_info(&core.db(), comic).is_ok()), "o histórico sai, o arquivo fica");
}

#[test]
fn a_paused_download_resumes_without_fetching_finished_pages_again() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let slow = serve(|seen| {
        let number = seen.path.trim_start_matches('/').trim_end_matches(".png").parse::<u8>().unwrap_or(0);
        Reply::ok(common::png(40, 60, number)).header("Content-Type", "image/png").after(Duration::from_millis(100))
    });
    let chapter_id = chapter(&core, "obra", &format!("{}|0,1,2,3,4,5", slow.base));
    let id = downloads::enqueue(&core, &[chapter_id]).unwrap()[0];
    let give_up = Instant::now() + Duration::from_secs(20);
    while downloads::get(&core.db(), id).unwrap().pages_done < 2 {
        assert!(Instant::now() < give_up);
        std::thread::sleep(Duration::from_millis(10));
    }
    downloads::pause(&core, id).unwrap();
    let paused = wait_until(&core, id, &["paused"]);
    assert!(paused.pages_done >= 2 && paused.pages_done < 6, "{paused:?}");
    let requested = slow.seen().iter().filter(|s| s.path == "/0.png").count();
    assert_eq!(requested, 1);

    downloads::resume(&core, id).unwrap();
    let view = wait_until(&core, id, &["completed", "failed"]);
    assert_eq!((view.status.as_str(), view.pages_done), ("completed", 6), "{view:?}");
    assert_eq!(slow.seen().iter().filter(|s| s.path == "/0.png").count(), 1, "a primeira página não foi pedida de novo");
}

#[test]
fn an_interrupted_download_continues_after_a_restart() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "obra", &format!("{}|0,1,2", server.base));
    // Como se o app tivesse fechado no meio: a primeira página já estava salva.
    let now = 1i64;
    core.db().execute("INSERT INTO chapter_downloads (chapter_id, status, pages_done, pages_total, created_at, updated_at) VALUES (?1, 'running', 1, 3, ?2, ?2)", [chapter_id, now]).unwrap();
    let id = core.db().last_insert_rowid();
    let temp = core.paths.data_dir.join("downloads/tmp").join(id.to_string());
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::write(temp.join("0001.png"), common::png(40, 60, 0)).unwrap();
    let leftover = core.paths.data_dir.join("downloads/tmp/999999");
    std::fs::create_dir_all(&leftover).unwrap();

    downloads::on_startup(&core).unwrap();
    let view = wait_until(&core, id, &["completed", "failed"]);
    assert_eq!(view.status, "completed", "{view:?}");
    assert!(!server.seen().iter().any(|s| s.path == "/0.png"), "a página salva não foi pedida");
    assert!(server.seen().iter().any(|s| s.path == "/1.png"));
    assert!(!leftover.exists(), "resto de outra operação sai na abertura");
}

#[test]
fn a_source_only_runs_one_download_at_a_time_and_honors_its_interval() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    core.db().execute("UPDATE extension_sources SET min_interval_ms = 100", []).unwrap();
    let server = pages_server();
    let first = chapter(&core, "a", &format!("{}|0,1,2", server.base));
    let second = chapter(&core, "b", &format!("{}|0,1,2", server.base));
    let started = Instant::now();
    let ids = downloads::enqueue(&core, &[first, second]).unwrap();
    assert_eq!(ids.len(), 2);
    let give_up = Instant::now() + Duration::from_secs(30);
    let mut seen_two_running = false;
    loop {
        let running = downloads::list(&core.db(), 10).unwrap().iter().filter(|d| d.status == "running").count();
        seen_two_running |= running > 1;
        if ids.iter().all(|id| matches!(downloads::get(&core.db(), *id).unwrap().status.as_str(), "completed" | "failed")) {
            break;
        }
        assert!(Instant::now() < give_up);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!seen_two_running);
    assert!(started.elapsed() >= Duration::from_millis(500), "intervalo entre pedidos, mesmo de capítulos diferentes: {:?}", started.elapsed());
}

#[test]
fn a_corrupted_archive_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("0001.png");
    std::fs::write(&page, common::png(20, 30, 1)).unwrap();
    let cbz = dir.path().join("a.cbz.part");
    downloads::write_cbz(&cbz, &[(1, page)], "<ComicInfo/>").unwrap();
    downloads::validate_cbz(&cbz, 1).unwrap();
    assert!(downloads::validate_cbz(&cbz, 2).is_err(), "página faltando");
    let bytes = std::fs::read(&cbz).unwrap();
    std::fs::write(&cbz, &bytes[..bytes.len() / 2]).unwrap();
    assert!(downloads::validate_cbz(&cbz, 1).is_err(), "arquivo cortado");
    std::fs::write(&cbz, b"nada a ver").unwrap();
    assert!(downloads::validate_cbz(&cbz, 1).is_err());
}

#[test]
fn the_same_chapter_generates_the_same_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("0001.png");
    std::fs::write(&page, common::png(20, 30, 1)).unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    downloads::write_cbz(&a, &[(1, page.clone())], "<ComicInfo/>").unwrap();
    std::thread::sleep(Duration::from_millis(2100));
    downloads::write_cbz(&b, &[(1, page)], "<ComicInfo/>").unwrap();
    assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
}

// ---------------------------------------------------------------------------
// Biblioteca
// ---------------------------------------------------------------------------

#[test]
fn a_downloaded_chapter_outlives_the_extension_the_repository_and_the_network() {
    let (_dir, core, repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let chapter_id = chapter(&core, "obra", &format!("{}|0,1", server.base));
    let view = downloaded(&core, chapter_id);
    let comic_id = view.comic_id.unwrap();
    library::save_progress(&core.db(), comic_id, 1, 2, 0.0).unwrap();
    let info = library::file_info(&core.db(), comic_id).unwrap();
    let requests = server.seen().len();

    install::remove(&core, EXT).unwrap();
    repository::remove(&core.db(), &repo.id).unwrap();

    // O servidor nem é consultado: a leitura vem do arquivo local.
    assert_eq!(core.pages.read_page(&info, 1).unwrap().0, common::png(40, 60, 13));
    assert_eq!(server.seen().len(), requests);
    assert!(std::path::Path::new(&info.path).exists());
    assert_eq!(library::summary(&core.db(), comic_id).unwrap().progress.unwrap().page_index, 1);

    let stored = sources::stored_series(&core.db(), view.series_id).unwrap();
    assert_eq!(stored.chapters[0].state, "offline");
    assert_eq!(stored.chapters[0].progress.as_ref().unwrap().page_index, 1);
    assert_eq!(stored.series.source_name, None);
    let reopened = sources::open_series(&core, view.series_id, None).unwrap();
    assert!(reopened.source_error.is_some());
    assert_eq!(reopened.chapters[0].state, "offline");
}

#[test]
fn removing_a_downloaded_comic_only_unlinks_the_chapter() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let view = downloaded(&core, chapter(&core, "obra", &format!("{}|0", server.base)));
    let comic_id = view.comic_id.unwrap();
    core.db().execute("DELETE FROM comics WHERE id = ?1", [comic_id]).unwrap();
    let chapters = sources::chapters_of(&core.db(), view.series_id).unwrap();
    assert_eq!(chapters[0].comic_id, None);
    assert_eq!(chapters[0].state, "downloaded", "o arquivo ainda está em disco");
    assert!(sources::comic_origin(&core.db(), comic_id).unwrap().is_none());
    assert_eq!(downloads::enqueue(&core, &[view.chapter_id]).unwrap().len(), 1, "pode voltar para a biblioteca");
}

#[test]
fn a_chapter_gone_from_the_source_stays_if_it_is_local() {
    let (_dir, core, _repo) = core_with_fixture(&["127.0.0.1"]);
    let server = pages_server();
    let id = found_series(&core, "obra");
    let remote = format!("{}|0", server.base);
    let (local_chapter, passing_chapter) = {
        let conn = core.db();
        conn.execute("INSERT INTO online_chapters (series_id, remote_id, title, sort_order) VALUES (?1, ?2, 'Antigo', 0)", rusqlite::params![id, remote]).unwrap();
        let local = conn.last_insert_rowid();
        conn.execute("INSERT INTO online_chapters (series_id, remote_id, title, sort_order) VALUES (?1, 'sumiu', 'Passageiro', 1)", [id]).unwrap();
        (local, conn.last_insert_rowid())
    };
    let view = downloaded(&core, local_chapter);
    assert_eq!(view.status, "completed");
    let opened = sources::open_series(&core, id, None).unwrap();
    assert!(opened.chapters.iter().any(|c| c.id == local_chapter), "o baixado continua");
    assert!(!opened.chapters.iter().any(|c| c.id == passing_chapter), "o que só existia na fonte sai");
    assert_eq!(opened.chapters.len(), 4);
}

// ---------------------------------------------------------------------------
// Demonstração
// ---------------------------------------------------------------------------

/// Publica o repositório com o mesmo script usado de verdade, só que em uma pasta descartável e com uma chave descartável.
fn publish_demonstration(out: &std::path::Path, key: &std::path::Path, base_url: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let status = std::process::Command::new("node")
        .arg(root.join("scripts/build-demo-repo.mjs"))
        .arg("--out")
        .arg(out)
        .args(["--base-url", base_url])
        .arg("--key")
        .arg(key)
        .env("CARGO_TARGET_DIR", wasm::target_dir())
        .status()
        .expect("node não abriu");
    assert!(status.success(), "a publicação da demonstração falhou");
}

#[test]
fn the_demonstration_goes_from_the_repository_to_reading_offline() {
    const DEMO: &str = "prancha.demonstracao.acervo";
    const SOURCE: &str = "acervo";

    let (published, keys) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root = published.path().to_path_buf();
    let online = Arc::new(AtomicBool::new(true));
    let switch = online.clone();
    let server = serve(move |seen| {
        if !switch.load(Ordering::Relaxed) {
            return Reply::status(503);
        }
        let file = root.join(seen.path.trim_start_matches('/'));
        let kind = if file.extension().is_some_and(|e| e == "png") { "image/png" } else { "text/plain; charset=utf-8" };
        match std::fs::read(&file) {
            Ok(bytes) => Reply::ok(bytes).header("Content-Type", kind),
            Err(_) => Reply::status(404),
        }
    });
    publish_demonstration(published.path(), &keys.path().join("demo.key"), &server.base);

    let (_dir, core) = core();
    let core = Arc::new(core);

    // Repositório: revisão, confiança e catálogo.
    let manifest = server.url("/prancha-repo.json");
    let preview = repository::preview(&core, &manifest).unwrap();
    assert_eq!((preview.name.as_str(), preview.extension_count), ("Demonstração do Prancha", 1));
    let repository = repository::add(&core, &manifest, &preview.fingerprint).unwrap();
    assert_eq!(repository.status, "ok");
    assert_eq!(catalog_ids(&core), [DEMO]);

    // Extensão: instalação assinada e registro da fonte.
    install::install(&core, &repository.id, DEMO).unwrap();
    let listed = sources::available_sources(&core.db()).unwrap();
    assert_eq!(listed.iter().map(|s| (s.source_id.as_str(), s.name.as_str())).collect::<Vec<_>>(), [(SOURCE, "Acervo de demonstração")]);

    // Pesquisa, listagem, filtro e paginação.
    let found = sources::search(&core, DEMO, SOURCE, "mares", 1, None, None).unwrap();
    assert_eq!(titles(&found), ["O Mapa das Marés"]);
    assert!(found.results[0].has_cover);
    let first = sources::popular(&core, DEMO, SOURCE, 1, None, None).unwrap();
    assert_eq!(titles(&first), ["Farol de Papel", "Quintal Estelar", "O Mapa das Marés", "Jardim de Engrenagens"]);
    assert!(first.has_more);
    let second = sources::popular(&core, DEMO, SOURCE, 2, None, None).unwrap();
    assert_eq!(titles(&second), ["Cidade Sem Relógios", "As Cartas de Lina"]);
    assert!(!second.has_more);
    assert_eq!(titles(&sources::popular(&core, DEMO, SOURCE, 1, Some("manga"), None).unwrap()), ["Quintal Estelar", "Jardim de Engrenagens"]);
    assert_eq!(titles(&sources::latest(&core, DEMO, SOURCE, 1, None, None).unwrap())[0], "Jardim de Engrenagens");
    assert!(sources::search(&core, DEMO, SOURCE, "não existe", 1, None, None).unwrap().results.is_empty());

    // Capa e detalhes.
    let series_id = found.results[0].id;
    assert_eq!(&sources::cover(&core, series_id).unwrap().unwrap()[..2], [0xff, 0xd8]);
    let series = sources::open_series(&core, series_id, None).unwrap();
    assert_eq!(series.source_error, None);
    assert_eq!(series.series.status.as_deref(), Some("completed"));
    assert_eq!(series.series.genres, ["Aventura", "Mistério"]);
    assert!(series.series.summary.as_deref().unwrap().contains("Duas irmãs"));
    let chapters: Vec<_> = series.chapters.iter().map(|c| (c.title.as_str(), c.state.as_str())).collect();
    assert_eq!(chapters, [("Lua nova", "available"), ("Lua cheia", "available")]);

    // Download, CBZ e biblioteca.
    let id = downloads::enqueue(&core, &[series.chapters[0].id]).unwrap()[0];
    let view = wait_until(&core, id, &["completed", "failed"]);
    assert_eq!((view.status.as_str(), view.pages_done, view.pages_total), ("completed", 5, 5), "{view:?}");
    let comic_id = view.comic_id.unwrap();
    let info = library::file_info(&core.db(), comic_id).unwrap();
    assert_eq!(entry_names(&info.path), ["0001.png", "0002.png", "0003.png", "0004.png", "0005.png", "ComicInfo.xml"]);
    let comic = library::summary(&core.db(), comic_id).unwrap();
    assert_eq!((comic.title.as_str(), comic.series.as_deref(), comic.page_count, comic.format.as_str()), ("Lua nova", Some("O Mapa das Marés"), Some(5), "cbz"));

    // Leitura sem rede: o servidor sai do ar e nenhum pedido é feito.
    online.store(false, Ordering::Relaxed);
    let requests = server.seen().len();
    let (bytes, _) = core.pages.read_page(&info, 4).unwrap();
    assert_eq!(bytes, std::fs::read(published.path().join("catalogo/o-mapa-das-mares/cap-01/05.png")).unwrap());
    library::save_progress(&core.db(), comic_id, 4, 5, 0.0).unwrap();
    assert_eq!(server.seen().len(), requests);

    // A fonte fora do ar não tira o que já é local.
    let offline = sources::open_series(&core, series_id, None).unwrap();
    assert!(offline.source_error.is_some());
    assert_eq!((offline.chapters[0].state.as_str(), offline.chapters[0].comic_id), ("offline", Some(comic_id)));
    assert_eq!(offline.chapters[0].progress.as_ref().unwrap().page_index, 4);
    assert_eq!(code(sources::popular(&core, DEMO, SOURCE, 1, None, None)), "source_failed");
}

#[test]
fn removing_a_repository_clears_only_its_catalog() {
    let (_dir, core) = core();
    let keep = added_repo(&core, "com.exemplo.fica", 1);
    let gone = added_repo(&core, "com.exemplo.sai", 2);
    repository::remove(&core.db(), &gone.id).unwrap();
    assert_eq!(catalog_ids(&core), ["com.exemplo.fica.fonte"]);
    assert_eq!(repository::list(&core.db()).unwrap().len(), 1);
    assert_eq!(code(repository::sync(&core, &gone.id)), "repo_not_found");
    assert_eq!(repository::sync(&core, &keep.id).unwrap().status, "ok");
}

// ---------------------------------------------------------------------------
// Estado de leitura por capítulo
// ---------------------------------------------------------------------------

use prancha_lib::extensions::runtime::Chapter;

fn remote(title: &str, order: u32) -> Chapter {
    Chapter { id: title.to_string(), title: title.to_string(), number: None, published_at: None, order }
}

/// Uma sincronização com a fonte: os capítulos na ordem em que a lista chegou, com a posição de leitura de cada um.
fn sync(core: &Core, series: i64, chapters: &[(&str, u32)]) -> Vec<i64> {
    let chapters: Vec<Chapter> = chapters.iter().map(|(title, order)| remote(title, *order)).collect();
    sources::store_chapters(&mut core.db(), series, &chapters).unwrap()
}

fn chapter_id(core: &Core, series: i64, remote: &str) -> i64 {
    core.db().query_row("SELECT id FROM online_chapters WHERE series_id = ?1 AND remote_id = ?2", rusqlite::params![series, remote], |r| r.get(0)).unwrap()
}

/// Obra encontrada na busca e sincronizada uma vez. Devolve a obra e os capítulos na ordem dada.
fn synced_series(core: &Core, query: &str, titles: &[&str]) -> (i64, Vec<i64>) {
    let series = found_series(core, query);
    let chapters: Vec<(&str, u32)> = titles.iter().enumerate().map(|(order, title)| (*title, order as u32)).collect();
    sync(core, series, &chapters);
    (series, titles.iter().map(|title| chapter_id(core, series, title)).collect())
}

fn info(core: &Core, id: i64) -> reading::ChapterReadInfo {
    reading::chapter_info(&core.db(), id).unwrap()
}

fn state(core: &Core, id: i64) -> ReadState {
    info(core, id).read_state
}

fn position(core: &Core, id: i64) -> Option<(i64, i64)> {
    use rusqlite::OptionalExtension;
    core.db().query_row("SELECT page_index, page_count FROM chapter_progress WHERE chapter_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional().unwrap()
}

fn column(core: &Core, id: i64, name: &str) -> Option<i64> {
    core.db().query_row(&format!("SELECT {name} FROM online_chapters WHERE id = ?1"), [id], |r| r.get(0)).unwrap()
}

/// (sessões abertas, motivos de encerramento em ordem) do capítulo.
fn sessions(core: &Core, id: i64) -> (i64, Vec<String>) {
    let conn = core.db();
    let open = conn.query_row("SELECT count(*) FROM reading_sessions WHERE chapter_id = ?1 AND ended_at IS NULL", [id], |r| r.get(0)).unwrap();
    let mut statement = conn.prepare("SELECT end_reason FROM reading_sessions WHERE chapter_id = ?1 AND ended_at IS NOT NULL ORDER BY id").unwrap();
    let reasons = statement.query_map([id], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    (open, reasons)
}

fn listed(core: &Core, series: i64, id: i64) -> sources::ChapterView {
    sources::chapters_of(&core.db(), series).unwrap().into_iter().find(|chapter| chapter.id == id).unwrap()
}

fn new_count(core: &Core, series: i64) -> i64 {
    sources::card(&core.db(), series).unwrap().new_count
}

#[test]
fn a_chapter_goes_from_unread_to_reading_to_in_progress_and_keeps_its_position() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let (series, ids) = synced_series(&core, "obra", &["Capítulo 1", "Capítulo 2"]);
    let id = ids[0];
    assert_eq!(state(&core, id), ReadState::Unread);
    assert_eq!(reading::acknowledge(&core.db(), id).unwrap().read_state, ReadState::Unread, "abrir não muda o estado");
    sources::save_chapter_progress(&core.db(), id, 0, 10, 0.0).unwrap();
    assert!(!reading::touch(&core.db(), id).unwrap(), "sem início, não há sessão para manter");
    assert_eq!(state(&core, id), ReadState::Unread);

    assert_eq!(reading::start(&mut core.db(), id).unwrap().read_state, ReadState::Reading);
    assert_eq!(listed(&core, series, id).read_state, ReadState::Reading);
    sources::save_chapter_progress(&core.db(), id, 4, 10, 0.2).unwrap();
    assert!(reading::touch(&core.db(), id).unwrap());
    let ended = reading::end(&core.db(), id).unwrap();
    assert_eq!((ended.read_state, ended.completed_at), (ReadState::InProgress, None));
    assert_eq!(listed(&core, series, id).read_state, ReadState::InProgress);
    assert_eq!(position(&core, id), Some((4, 10)));

    assert_eq!(reading::start(&mut core.db(), id).unwrap().read_state, ReadState::Reading, "reaberto, volta a ser lido");
    assert_eq!(position(&core, id), Some((4, 10)));
    assert_eq!(reading::end(&core.db(), id).unwrap().read_state, ReadState::InProgress);
    assert_eq!(position(&core, id), Some((4, 10)), "a posição atravessa as sessões");
    assert_eq!(sessions(&core, id), (0, vec!["left".to_string(), "left".to_string()]));
    assert_eq!(state(&core, ids[1]), ReadState::Unread, "o outro capítulo não muda");
}

#[test]
fn completing_is_idempotent_and_rereading_keeps_the_first_completion() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let (_series, ids) = synced_series(&core, "obra", &["Capítulo 1"]);
    let id = ids[0];
    reading::start(&mut core.db(), id).unwrap();
    let done = reading::complete(&mut core.db(), id).unwrap();
    assert_eq!(done.read_state, ReadState::Read);
    let at = done.completed_at.expect("data da conclusão");
    assert_eq!(sessions(&core, id), (0, vec!["completed".to_string()]));
    let last_read = column(&core, id, "last_read_at");

    std::thread::sleep(Duration::from_millis(5));
    let again = reading::complete(&mut core.db(), id).unwrap();
    assert_eq!((again.read_state, again.completed_at), (ReadState::Read, Some(at)));
    assert_eq!(column(&core, id, "last_read_at"), last_read, "repetir não grava nada");

    std::thread::sleep(Duration::from_millis(5));
    let rereading = reading::start(&mut core.db(), id).unwrap();
    assert_eq!((rereading.read_state, rereading.completed_at), (ReadState::Reading, Some(at)), "relendo aparece como lendo");
    let back = reading::end(&core.db(), id).unwrap();
    assert_eq!((back.read_state, back.completed_at), (ReadState::Read, Some(at)), "terminada a sessão, volta a lido");
    reading::start(&mut core.db(), id).unwrap();
    assert_eq!(reading::complete(&mut core.db(), id).unwrap().completed_at, Some(at), "concluir de novo não troca a data");
}

#[test]
fn marking_unread_keeps_the_position_and_does_not_bring_the_novelty_back() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let (series, ids) = synced_series(&core, "obra", &["Capítulo 1", "Capítulo 2"]);
    let id = ids[0];
    let acknowledged = column(&core, id, "acknowledged_at");
    assert!(acknowledged.is_some());
    reading::start(&mut core.db(), id).unwrap();
    sources::save_chapter_progress(&core.db(), id, 5, 10, 0.0).unwrap();
    reading::complete(&mut core.db(), id).unwrap();

    let unread = reading::set_read(&mut core.db(), &[id], false).unwrap();
    assert_eq!(unread.len(), 1);
    assert_eq!((unread[0].read_state, unread[0].completed_at, unread[0].is_new), (ReadState::Unread, None, false));
    assert_eq!(column(&core, id, "manual_unread"), Some(1));
    assert_eq!(column(&core, id, "acknowledged_at"), acknowledged, "a novidade não volta");
    assert_eq!(position(&core, id), Some((5, 10)), "a posição fica");
    assert_eq!(listed(&core, series, id).progress.map(|p| p.page_index), Some(5));
    assert_eq!(reading::set_read(&mut core.db(), &[id], false).unwrap()[0].read_state, ReadState::Unread, "repetir não muda nada");

    assert_eq!(reading::start(&mut core.db(), id).unwrap().read_state, ReadState::Reading);
    assert_eq!(column(&core, id, "manual_unread"), Some(0));
    assert_eq!(reading::end(&core.db(), id).unwrap().read_state, ReadState::InProgress);
    assert_eq!(position(&core, id), Some((5, 10)));

    assert_eq!(reading::set_read(&mut core.db(), &[ids[1]], false).unwrap()[0].read_state, ReadState::Unread, "nunca lido continua não lido");
}

#[test]
fn marking_read_acknowledges_the_novelty_and_ends_the_session() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let (series, ids) = synced_series(&core, "obra", &["Capítulo 1"]);
    sources::set_following(&core.db(), series, true).unwrap();
    let fresh = sync(&core, series, &[("Capítulo 1", 0), ("Capítulo 2", 1)]);
    assert_eq!(fresh.len(), 1);
    assert!(info(&core, fresh[0]).is_new);
    reading::start(&mut core.db(), ids[0]).unwrap();

    let marked = reading::set_read(&mut core.db(), &[ids[0], fresh[0]], true).unwrap();
    assert!(marked.iter().all(|chapter| chapter.read_state == ReadState::Read && !chapter.is_new && chapter.completed_at.is_some()), "{marked:?}");
    assert_eq!(sessions(&core, ids[0]), (0, vec!["manual".to_string()]));
    assert_eq!(new_count(&core, series), 0);
    assert_eq!(code(reading::set_read(&mut core.db(), &[ids[0], 999_999], true)), "chapter_not_found");
    assert!(info(&core, ids[0]).completed_at.is_some(), "a falha não desfaz o que estava");
}

#[test]
fn a_session_left_open_by_a_crash_becomes_in_progress_and_never_read() {
    let (dir, core, _repo) = core_with_fixture(&[]);
    let (series, ids) = synced_series(&core, "obra", &["Capítulo 1", "Capítulo 2"]);
    let id = ids[0];
    reading::start(&mut core.db(), id).unwrap();
    sources::save_chapter_progress(&core.db(), id, 9, 10, 0.0).unwrap();
    let last_activity: i64 = core.db().query_row("SELECT last_activity_at FROM reading_sessions WHERE chapter_id = ?1", [id], |r| r.get(0)).unwrap();
    // Queda: a sessão nunca recebe o fim. Na próxima abertura, o app a encerra.
    let reopened = Core::init(dir.path().to_path_buf(), Arc::new(|_, _| {})).unwrap();
    let (ended_at, reason): (i64, String) = reopened.db().query_row("SELECT ended_at, end_reason FROM reading_sessions WHERE chapter_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((ended_at, reason.as_str()), (last_activity, "recovered"));
    let recovered = reading::chapter_info(&reopened.db(), id).unwrap();
    assert_eq!((recovered.read_state, recovered.completed_at), (ReadState::InProgress, None), "mesmo na última página, não vira lido");
    assert_eq!(position(&core, id), Some((9, 10)));
    assert_eq!(reading::recover_sessions(&reopened.db()).unwrap(), 0, "nada mais aberto");
    drop(reopened);

    // Sem atividade há mais que o limite, a sessão não vale como "Lendo", mesmo aberta.
    let other = ids[1];
    reading::start(&mut core.db(), other).unwrap();
    core.db()
        .execute("UPDATE reading_sessions SET last_activity_at = ?2 WHERE chapter_id = ?1 AND ended_at IS NULL", [other, prancha_lib::db::now_ms() - reading::SESSION_STALE_MS - 1000])
        .unwrap();
    assert_eq!(state(&core, other), ReadState::InProgress);
    assert_eq!(listed(&core, series, other).read_state, ReadState::InProgress);
    assert_eq!(reading::recover_sessions(&core.db()).unwrap(), 1);
    assert_eq!(sessions(&core, other), (0, vec!["recovered".to_string()]));
    assert_eq!(state(&core, other), ReadState::InProgress);
}

#[test]
fn only_one_chapter_is_being_read_at_a_time() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let (series, ids) = synced_series(&core, "obra", &["Capítulo 1", "Capítulo 2"]);
    let (a, b) = (ids[0], ids[1]);
    reading::start(&mut core.db(), a).unwrap();
    sources::save_chapter_progress(&core.db(), a, 3, 10, 0.0).unwrap();
    assert_eq!(reading::start(&mut core.db(), b).unwrap().read_state, ReadState::Reading);
    assert_eq!(state(&core, a), ReadState::InProgress);
    assert_eq!(sessions(&core, a), (0, vec!["replaced".to_string()]));
    assert_eq!(sessions(&core, b).0, 1);
    reading::start(&mut core.db(), b).unwrap();
    assert_eq!(sessions(&core, b), (1, vec![]), "repetir o início não abre outra sessão");
    let open: i64 = core.db().query_row("SELECT count(*) FROM reading_sessions WHERE ended_at IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(open, 1);
    assert_eq!(position(&core, a), Some((3, 10)));
    let states: Vec<ReadState> = sources::chapters_of(&core.db(), series).unwrap().iter().map(|c| c.read_state).collect();
    assert_eq!(states, [ReadState::InProgress, ReadState::Reading]);
}

#[test]
fn marking_previous_chapters_follows_the_source_order_and_skips_ties() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let series = found_series(&core, "obra");
    // A lista chega embaralhada, e a ordem da fonte não é a do texto.
    sync(&core, series, &[("Vol. 2 Cap. 3", 50), ("Capítulo 10.5", 30), ("Especial", 25), ("Capítulo 9", 10), ("Capítulo 10 Extra", 30), ("Capítulo 10", 20)]);
    let id = |title: &str| chapter_id(&core, series, title);
    let (nine, ten, half, extra, special, volume) = (id("Capítulo 9"), id("Capítulo 10"), id("Capítulo 10.5"), id("Capítulo 10 Extra"), id("Especial"), id("Vol. 2 Cap. 3"));

    assert_eq!(reading::previous_ids(&core.db(), half).unwrap(), [nine, ten, special]);
    assert_eq!(reading::previous_ids(&core.db(), special).unwrap(), [nine, ten]);
    assert!(reading::previous_ids(&core.db(), nine).unwrap().is_empty());
    assert_eq!(code(reading::previous_ids(&core.db(), 999_999)), "chapter_not_found");

    let marked = reading::mark_previous_read(&mut core.db(), half).unwrap();
    assert_eq!(marked.iter().map(|c| c.id).collect::<Vec<_>>(), [nine, ten, special]);
    let read = |id: i64| state(&core, id) == ReadState::Read;
    assert!(read(nine) && read(ten) && read(special));
    assert!(!read(half), "o escolhido não é marcado");
    assert!(!read(extra), "na mesma posição, não é anterior");
    assert!(!read(volume), "o seguinte não é marcado");

    let dates = || -> Vec<Option<i64>> { [nine, ten, special].iter().map(|id| info(&core, *id).completed_at).collect() };
    let before = dates();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(reading::mark_previous_read(&mut core.db(), half).unwrap().len(), 3);
    assert_eq!(dates(), before, "repetir não muda nada");
    assert!(!read(half) && !read(extra) && !read(volume));

    // Sem ordem informada, ninguém é anterior a ninguém.
    let other = found_series(&core, "outra");
    sync(&core, other, &[("A", 0), ("B", 0), ("C", 0)]);
    let c = chapter_id(&core, other, "C");
    assert!(reading::previous_ids(&core.db(), c).unwrap().is_empty());
    assert!(reading::mark_previous_read(&mut core.db(), c).unwrap().is_empty());
    assert!(sources::chapters_of(&core.db(), other).unwrap().iter().all(|chapter| chapter.read_state == ReadState::Unread));
}

#[test]
fn only_chapters_that_arrive_after_the_first_sync_of_a_followed_series_are_new() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let series = found_series(&core, "obra");
    sources::set_following(&core.db(), series, true).unwrap();
    assert!(sync(&core, series, &[("Capítulo 1", 0), ("Capítulo 2", 1)]).is_empty(), "a primeira sincronização é a referência");
    assert!(sources::chapters_of(&core.db(), series).unwrap().iter().all(|chapter| !chapter.is_new && chapter.first_seen_at.is_some()));
    assert_eq!(new_count(&core, series), 0);

    let fresh = sync(&core, series, &[("Capítulo 1", 0), ("Capítulo 2", 1), ("Capítulo 3", 2)]);
    let third = chapter_id(&core, series, "Capítulo 3");
    assert_eq!(fresh, [third]);
    let flags: Vec<bool> = sources::chapters_of(&core.db(), series).unwrap().iter().map(|chapter| chapter.is_new).collect();
    assert_eq!(flags, [false, false, true]);
    assert_eq!(listed(&core, series, third).read_state, ReadState::Unread, "novo e não lido são coisas diferentes");
    assert_eq!(new_count(&core, series), 1);
    let library = sources::library_series(&core.db()).unwrap();
    assert_eq!(library.iter().map(|s| (s.id, s.new_count, s.following)).collect::<Vec<_>>(), [(series, 1, true)]);

    assert!(sync(&core, series, &[("Capítulo 1", 0), ("Capítulo 2", 1), ("Capítulo 3", 2)]).is_empty(), "o mesmo de novo não é novidade");
    assert_eq!(new_count(&core, series), 1, "a contagem continua");

    assert!(!reading::acknowledge(&core.db(), third).unwrap().is_new);
    assert_eq!(new_count(&core, series), 0);

    let fresh = sync(&core, series, &[("Capítulo 1", 0), ("Capítulo 2", 1), ("Capítulo 3", 2), ("Capítulo 4", 3), ("Capítulo 5", 4)]);
    assert_eq!(fresh.len(), 2);
    assert_eq!(new_count(&core, series), 2);
    reading::complete(&mut core.db(), fresh[0]).unwrap();
    assert_eq!(new_count(&core, series), 1);
    reading::set_read(&mut core.db(), &[fresh[1]], true).unwrap();
    assert_eq!(new_count(&core, series), 0);
    reading::set_read(&mut core.db(), &[third, fresh[0], fresh[1]], false).unwrap();
    assert_eq!(new_count(&core, series), 0, "não lido não traz a novidade de volta");
    assert_eq!(sources::library_series(&core.db()).unwrap()[0].new_count, 0);

    // Sem acompanhar, nada é novidade.
    let other = found_series(&core, "outra");
    assert!(sync(&core, other, &[("A", 0)]).is_empty());
    assert!(sync(&core, other, &[("A", 0), ("B", 1)]).is_empty());
    assert_eq!(new_count(&core, other), 0);
    assert!(sources::chapters_of(&core.db(), other).unwrap().iter().all(|chapter| !chapter.is_new));
}

#[test]
fn syncing_again_keeps_reading_state_and_only_drops_chapters_nobody_read() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let (series, ids) = synced_series(&core, "obra", &["Lendo", "Lido", "Marcado", "Intocado", "Fica"]);
    reading::start(&mut core.db(), ids[0]).unwrap();
    reading::end(&core.db(), ids[0]).unwrap();
    reading::complete(&mut core.db(), ids[1]).unwrap();
    reading::set_read(&mut core.db(), &[ids[2]], true).unwrap();
    let snapshot = |core: &Core| -> Vec<(ReadState, Option<i64>, Option<i64>, Option<i64>)> {
        ids[..3].iter().map(|id| (state(core, *id), column(core, *id, "completed_at"), column(core, *id, "acknowledged_at"), column(core, *id, "first_seen_at"))).collect()
    };
    let before = snapshot(&core);
    assert_eq!(before.iter().map(|s| s.0).collect::<Vec<_>>(), [ReadState::InProgress, ReadState::Read, ReadState::Read]);

    std::thread::sleep(Duration::from_millis(5));
    sync(&core, series, &[("Lendo", 0), ("Lido", 1), ("Marcado", 2), ("Intocado", 3), ("Fica", 4)]);
    assert_eq!(snapshot(&core), before, "sincronizar de novo não mexe no estado");

    sync(&core, series, &[("Fica", 4)]);
    let remaining: Vec<String> = sources::chapters_of(&core.db(), series).unwrap().into_iter().map(|c| c.remote_id).collect();
    assert_eq!(remaining, ["Lendo", "Lido", "Marcado", "Fica"], "o que tem histórico fica, o intocado sai");
    assert_eq!(snapshot(&core), before);
}

#[test]
fn uninstalling_the_extension_keeps_chapters_and_their_reading_state() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let (series, ids) = synced_series(&core, "obra", &["Capítulo 1", "Capítulo 2", "Capítulo 3"]);
    reading::complete(&mut core.db(), ids[0]).unwrap();
    reading::start(&mut core.db(), ids[1]).unwrap();
    sources::save_chapter_progress(&core.db(), ids[1], 2, 8, 0.0).unwrap();
    reading::end(&core.db(), ids[1]).unwrap();
    let completed = info(&core, ids[0]).completed_at;

    install::remove(&core, EXT).unwrap();
    let stored = sources::stored_series(&core.db(), series).unwrap();
    let states: Vec<ReadState> = stored.chapters.iter().map(|c| c.read_state).collect();
    assert_eq!(states, [ReadState::Read, ReadState::InProgress, ReadState::Unread]);
    assert_eq!(stored.chapters[0].completed_at, completed);
    assert_eq!(stored.chapters[1].progress.as_ref().map(|p| (p.page_index, p.page_count)), Some((2, 8)));
    let reopened = sources::open_series(&core, series, None).unwrap();
    assert!(reopened.source_error.is_some());
    assert_eq!(reopened.chapters.iter().map(|c| c.read_state).collect::<Vec<_>>(), states);
}

#[test]
fn favoriting_twice_keeps_one_series_and_unfollowing_keeps_the_reading() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let (series, ids) = synced_series(&core, "obra", &["Capítulo 1", "Capítulo 2"]);
    let total = |core: &Core| -> i64 { core.db().query_row("SELECT count(*) FROM online_series", [], |r| r.get(0)).unwrap() };
    let before = total(&core);
    sources::set_favorite(&core.db(), series, true).unwrap();
    sources::set_favorite(&core.db(), series, true).unwrap();
    assert_eq!(total(&core), before, "favoritar de novo não duplica");
    assert!(sources::card(&core.db(), series).unwrap().favorite);
    let favorite: i64 = core.db().query_row("SELECT favorite FROM online_series WHERE id = ?1", [series], |r| r.get(0)).unwrap();
    assert_eq!(favorite, 1);
    assert_eq!(sources::library_series(&core.db()).unwrap().iter().filter(|s| s.id == series).count(), 1);
    assert_eq!(found_series(&core, "obra"), series, "achar de novo na busca é a mesma obra");

    assert!(sources::set_following(&core.db(), series, true).unwrap().following);
    assert!(sources::card(&core.db(), series).unwrap().following);
    reading::complete(&mut core.db(), ids[0]).unwrap();
    reading::start(&mut core.db(), ids[1]).unwrap();
    reading::end(&core.db(), ids[1]).unwrap();
    assert!(!sources::set_following(&core.db(), series, false).unwrap().following);
    assert!(!sources::card(&core.db(), series).unwrap().following);
    let states: Vec<ReadState> = sources::chapters_of(&core.db(), series).unwrap().iter().map(|c| c.read_state).collect();
    assert_eq!(states, [ReadState::Read, ReadState::InProgress], "deixar de acompanhar não apaga a leitura");
}

#[test]
fn marking_a_whole_series_unread_keeps_every_position() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let (series, ids) = synced_series(&core, "obra", &["Capítulo 1", "Capítulo 2"]);
    reading::start(&mut core.db(), ids[0]).unwrap();
    sources::save_chapter_progress(&core.db(), ids[0], 3, 10, 0.4).unwrap();
    reading::end(&core.db(), ids[0]).unwrap();

    sources::set_series_read(&mut core.db(), series, true).unwrap();
    assert!(sources::chapters_of(&core.db(), series).unwrap().iter().all(|c| c.read_state == ReadState::Read));
    assert_eq!(position(&core, ids[0]), Some((3, 10)));
    sources::set_series_read(&mut core.db(), series, false).unwrap();
    assert!(sources::chapters_of(&core.db(), series).unwrap().iter().all(|c| c.read_state == ReadState::Unread && c.completed_at.is_none()));
    assert_eq!(position(&core, ids[0]), Some((3, 10)), "a posição fica");
}
