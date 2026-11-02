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
use prancha_lib::extensions::{downloads, install, sources};
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
        Installed { id: id.to_string(), file, sha256, grant }
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

// Único teste com a internet de verdade: prova TLS e DNS reais. Rode com `cargo test --test extensions -- --ignored`.
#[test]
#[ignore = "depende da internet"]
fn a_public_https_address_is_really_fetched() {
    let (_dir, core) = core();
    let error = repository::preview(&core, "https://raw.githubusercontent.com/keiyoushi/extensions/repo/repo.json").unwrap_err();
    assert_eq!((error.code(), error.to_string().as_str()), ("repo_mihon", MIHON_MESSAGE));
}

#[test]
fn mihon_repositories_get_the_exact_message() {
    let (_dir, core) = core();
    let mihon_repo = json!({ "index_v2": "https://exemplo.com/index.pb", "meta": { "name": "Exemplo", "website": "https://exemplo.com", "signingKeyFingerprint": "abc" } });
    let mihon_index = json!([{ "name": "X", "pkg": "eu.kanade.tachiyomi.extension.all.x", "apk": "x.apk", "lang": "all", "code": 1, "version": "1.4.1", "nsfw": 0, "sources": [] }]);

    let check = |address: &str| {
        let error = repository::preview(&core, address).unwrap_err();
        assert_eq!((error.code(), error.to_string().as_str()), ("repo_mihon", MIHON_MESSAGE), "{address}");
    };
    // Pelo nome do arquivo, sem consultar a rede.
    check("https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json");
    check("https://exemplo.com/apk/tachiyomi-all.x-v1.4.1.apk");

    let repo = TestRepo::new("mihon", 1);
    repo.put("/repo.json", mihon_repo.to_string());
    check(&repo.server.url("/repo.json"));
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
    assert_eq!(ids, [("livre", true), ("sem-classificacao", false)], "a adulta não é registrada e a sem classificação fica desabilitada");
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
    assert_eq!(code(sources::search(&core, EXT, "adulta", "x", 1, None, None)), "source_not_found");
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
fn a_series_marked_adult_in_the_details_is_refused_and_forgotten() {
    let (_dir, core, _repo) = core_with_fixture(&[]);
    let id = found_series(&core, "adulta");
    core.db().execute("UPDATE online_series SET remote_id = 'adulta' WHERE id = ?1", [id]).unwrap();
    assert_eq!(code(sources::open_series(&core, id, None)), "source_rating");
    assert_eq!(code(sources::card(&core.db(), id)), "series_not_found");
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
