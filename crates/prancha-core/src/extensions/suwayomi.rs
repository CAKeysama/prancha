//! Repositórios do Mihon rodam no Suwayomi-Server, um programa de terceiros que o Prancha baixa na primeira instalação
//! e controla como processo local. As extensões dele são código Java sem isolamento: a interface avisa antes de instalar.
//! Ele aceita pedidos de qualquer site aberto no navegador, então escuta só em 127.0.0.1, com porta e senha novas a cada início.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::net::TcpListener;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::net::{self, Cancel, Limits, NetError, Response};
use super::runtime::{Chapter, ComicDetails, ContentRating, Page, PublicationStatus, ReadingDirection, SearchPage, SearchResult, SourceInfo};
use super::signing::sha256_hex;
use crate::core::Core;
use crate::error::{AppError, AppResult};

pub const RUNTIME: &str = "suwayomi";
pub const EVENT_PROGRESS: &str = "component:progress";

pub const VERSION: &str = "v2.4.2366";
const PACKAGE_URL: &str = "https://github.com/Suwayomi/Suwayomi-Server/releases/download/v2.4.2366/Suwayomi-Server-v2.4.2366-windows-x64.zip";
const PACKAGE_SHA256: &str = "b955f374a8d94b8d1c2750ae28fa7eb2066b479957561b5ba9cf8ac993c8fc13";
const PACKAGE_BYTES: u64 = 343_828_201;
const JAR: &str = "bin/Suwayomi-Server.jar";
const JAVA: &str = "jre/bin/javaw.exe";
const USER: &str = "prancha";

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
const START_TIMEOUT: Duration = Duration::from_secs(120);
const START_STEP: Duration = Duration::from_millis(250);
/// As chamadas passam pelo site da fonte, que pode ser lento.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_REPLY_BYTES: u64 = 32 * 1024 * 1024;
const MAX_MESSAGE_CHARS: usize = 300;

/// Sem processo até o primeiro uso, e nada aqui impede o app de abrir.
#[derive(Default)]
pub struct Suwayomi {
    installing: Mutex<()>,
    server: Mutex<Option<Server>>,
}

struct Server {
    /// Só falta nos testes, que falam com um servidor já no ar.
    child: Option<Child>,
    /// Fechado pelo Windows quando o Prancha termina, mesmo em uma falha, e leva o processo junto.
    #[cfg(windows)]
    _job: Option<OwnedHandle>,
    client: Client,
}

impl Server {
    fn is_running(&mut self) -> bool {
        self.child.as_mut().is_none_or(|child| matches!(child.try_wait(), Ok(None)))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Clone)]
struct Client {
    http: reqwest::Client,
    base: String,
    password: String,
}

fn failed(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::user(code, message)
}

// ---------------------------------------------------------------------------
// Componente
// ---------------------------------------------------------------------------

fn root(core: &Core) -> PathBuf {
    core.paths.data_dir.join("suwayomi")
}

fn is_complete(dir: &Path) -> bool {
    dir.join(JAR).is_file() && dir.join(JAVA).is_file()
}

fn report(core: &Core, stage: &str, percent: u64) {
    (core.emit)(EVENT_PROGRESS, json!({ "stage": stage, "percent": percent }));
}

/// Sem um provedor de TLS, o reqwest entra em pânico ao montar o cliente, mesmo para falar só com 127.0.0.1.
fn http() -> reqwest::ClientBuilder {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy()
}

/// Grava o pacote em disco, sem guardar na memória, e só aceita o conteúdo esperado.
pub fn fetch_package(url: &str, file: &Path, sha256: &str, size: u64, progress: impl Fn(u64) + Send + 'static) -> AppResult<()> {
    let client = http()
        .user_agent(concat!("Prancha/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(STALL_TIMEOUT)
        .read_timeout(STALL_TIMEOUT)
        .build()
        .map_err(|e| failed("component_download", e.to_string()))?;
    let (request, path) = (client.get(url), file.to_path_buf());
    let work = async move {
        let network = |e: reqwest::Error| NetError::Network(e.to_string());
        let mut response = request.send().await.map_err(network)?;
        if !response.status().is_success() {
            return Err(NetError::Network(format!("resposta {}", response.status())));
        }
        let mut writer = File::create(&path).map_err(|e| NetError::Network(e.to_string()))?;
        let (mut hasher, mut done) = (Sha256::new(), 0u64);
        while let Some(chunk) = response.chunk().await.map_err(network)? {
            done += chunk.len() as u64;
            if done > size {
                return Err(NetError::TooLarge);
            }
            writer.write_all(&chunk).map_err(|e| NetError::Network(e.to_string()))?;
            hasher.update(&chunk);
            progress(done);
        }
        writer.flush().map_err(|e| NetError::Network(e.to_string()))?;
        Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>())
    };
    let digest = net::wait(work, None, Instant::now() + DOWNLOAD_TIMEOUT).map_err(|error| {
        log::warn!("componente: {error}");
        failed("component_download", "Não foi possível baixar o componente destas extensões. Confira a conexão e tente de novo.")
    })?;
    if digest != sha256 {
        return Err(failed("component_hash", "O componente baixado não é o esperado e foi descartado."));
    }
    Ok(())
}

/// Extrai só o Java e o servidor: o pacote também traz um aplicativo Electron, que o Prancha não usa.
pub fn unpack(package: &Path, target: &Path) -> AppResult<()> {
    let broken = |why: String| failed("component_package", format!("O componente baixado não pôde ser aberto: {why}."));
    let mut archive = zip::ZipArchive::new(File::open(package)?).map_err(|e| broken(e.to_string()))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| broken(e.to_string()))?;
        let Some(path) = entry.enclosed_name() else { continue };
        // Por fora de tudo há uma pasta com o nome da versão.
        let inside: PathBuf = path.components().skip(1).collect();
        if entry.is_dir() || !(inside.starts_with("jre") || inside == Path::new(JAR)) {
            continue;
        }
        let out = target.join(&inside);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::io::copy(&mut entry, &mut File::create(&out)?)?;
    }
    if is_complete(target) { Ok(()) } else { Err(broken("faltam arquivos".into())) }
}

/// Baixa e extrai uma única vez. A pasta final só aparece com tudo no lugar.
fn ensure_component(core: &Core) -> AppResult<()> {
    let _only_one = core.suwayomi.installing.lock().unwrap_or_else(|e| e.into_inner());
    let dir = root(core).join(VERSION);
    if is_complete(&dir) {
        return Ok(());
    }
    std::fs::create_dir_all(root(core))?;
    let package = root(core).join("pacote.zip.part");
    let staging = root(core).join(format!("{VERSION}.part"));
    let _ = std::fs::remove_dir_all(&staging);
    let emit = core.emit.clone();
    let last = Mutex::new(u64::MAX);
    let progress = move |done: u64| {
        let percent = done * 100 / PACKAGE_BYTES;
        let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
        if *last != percent {
            *last = percent;
            emit(EVENT_PROGRESS, json!({ "stage": "download", "percent": percent }));
        }
    };
    let outcome = fetch_package(PACKAGE_URL, &package, PACKAGE_SHA256, PACKAGE_BYTES, progress)
        .and_then(|()| {
            report(core, "unpack", 0);
            unpack(&package, &staging)
        })
        .and_then(|()| Ok(std::fs::rename(&staging, &dir)?));
    let _ = std::fs::remove_file(&package);
    let _ = std::fs::remove_dir_all(&staging);
    outcome
}

// ---------------------------------------------------------------------------
// Processo
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn kill_with_app(child: &Child) -> Option<OwnedHandle> {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    // SAFETY: os ponteiros valem durante as chamadas, e o identificador do job passa a ser só do `OwnedHandle`.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }
        let job = OwnedHandle::from_raw_handle(job);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = std::mem::size_of_val(&limits) as u32;
        let set = SetInformationJobObject(job.as_raw_handle(), JobObjectExtendedLimitInformation, (&raw const limits).cast(), size);
        (set != 0 && AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) != 0).then_some(job)
    }
}

fn start(core: &Core, dir: &Path) -> AppResult<Server> {
    report(core, "start", 0);
    let data = root(core).join("dados");
    // O servidor guarda ali cada página e capa que passa por ele, e nunca apaga. O capítulo baixado fica na biblioteca do Prancha.
    let temporary = data.join("tmp");
    let _ = std::fs::remove_dir_all(&temporary);
    std::fs::create_dir_all(&temporary)?;

    let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| failed("component_start", e.to_string()))?;
    let password = sha256_hex(&secret);

    let setting = |name: &str, value: &str| format!("-Dsuwayomi.tachidesk.config.server.{name}={value}");
    let mut command = Command::new(dir.join(JAVA));
    command
        .current_dir(dir)
        .args(["-Xmx512m", "-Djava.awt.headless=true", "--add-exports=java.desktop/sun.awt=ALL-UNNAMED"])
        .arg(format!("-Djava.io.tmpdir={}", temporary.display()))
        .arg(setting("rootDir", &data.display().to_string()))
        .arg(setting("ip", "127.0.0.1"))
        .arg(setting("port", &port.to_string()))
        .arg(setting("authMode", "BASIC_AUTH"))
        .arg(setting("authUsername", USER))
        .arg(setting("authPassword", &password))
        .arg(setting("webUIEnabled", "false"))
        .arg(setting("initialOpenInBrowserEnabled", "false"))
        .arg(setting("systemTrayEnabled", "false"))
        // Sem navegador embutido nem serviço para passar por verificações anti-robô: fontes que dependem disso falham com um erro.
        .arg(setting("kcefEnabled", "false"))
        .arg(setting("flareSolverrEnabled", "false"))
        .args(["-jar", JAR])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    // Antes do processo: um processo solto não seria encerrado com o Prancha.
    let http = http().redirect(reqwest::redirect::Policy::none()).build().map_err(|e| failed("component_start", e.to_string()))?;
    let child = command.spawn()?;
    let mut server = Server {
        #[cfg(windows)]
        _job: kill_with_app(&child),
        child: Some(child),
        client: Client { http, base: format!("http://127.0.0.1:{port}"), password },
    };
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if server.client.graphql::<Value>("aboutServer", "{ aboutServer { version } }", json!({}), None).is_ok() {
            return Ok(server);
        }
        if !server.is_running() || Instant::now() >= deadline {
            return Err(failed("component_start", "O componente destas extensões não iniciou. Tente de novo."));
        }
        std::thread::sleep(START_STEP);
    }
}

/// O servidor em execução, iniciado na primeira chamada. Só a instalação baixa o componente.
fn client(core: &Core) -> AppResult<Client> {
    let mut server = core.suwayomi.server.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(running) = server.as_mut() {
        if running.is_running() {
            return Ok(running.client.clone());
        }
    }
    *server = None;
    let dir = root(core).join(VERSION);
    if !is_complete(&dir) {
        return Err(failed("component_missing", "O componente destas extensões não está instalado. Remova a extensão e instale de novo."));
    }
    let started = start(core, &dir)?;
    let client = started.client.clone();
    *server = Some(started);
    Ok(client)
}

/// Para os testes: usa um servidor já no ar, sem iniciar o Java.
pub fn attach(core: &Core, base: &str) -> AppResult<()> {
    let http = http().redirect(reqwest::redirect::Policy::none()).build().map_err(|e| failed("component_start", e.to_string()))?;
    let client = Client { http, base: base.to_string(), password: String::new() };
    *core.suwayomi.server.lock().unwrap_or_else(|e| e.into_inner()) = Some(Server {
        child: None,
        #[cfg(windows)]
        _job: None,
        client,
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// Chamadas
// ---------------------------------------------------------------------------

fn call_error(error: NetError) -> AppError {
    match error {
        NetError::Cancelled => failed("extension_cancelled", "Operação cancelada."),
        NetError::Timeout => failed("source_network", "A fonte demorou demais para responder."),
        other => {
            log::warn!("suwayomi: {other}");
            failed("source_network", "Não foi possível falar com o componente destas extensões.")
        }
    }
}

impl Client {
    fn graphql<T: DeserializeOwned>(&self, field: &str, query: &str, variables: Value, cancel: Option<&Cancel>) -> AppResult<T> {
        let body = serde_json::to_vec(&json!({ "query": query, "variables": variables })).map_err(|e| failed("source_data", e.to_string()))?;
        let request = self.http.post(format!("{}/api/graphql", self.base)).basic_auth(USER, Some(&self.password)).header(CONTENT_TYPE, "application/json").body(body);
        let response = net::send_local(request, Limits { max_bytes: MAX_REPLY_BYTES, timeout: CALL_TIMEOUT }, cancel).map_err(call_error)?;
        read_reply(&response.body, field)
    }

    fn image(&self, path: &str, limits: Limits, cancel: Option<&Cancel>) -> Result<Response, NetError> {
        if !path.starts_with("/api/v1/") {
            return Err(NetError::Blocked("caminho inesperado".into()));
        }
        net::send_local(self.http.get(format!("{}{path}", self.base)).basic_auth(USER, Some(&self.password)), limits, cancel)
    }
}

fn read_reply<T: DeserializeOwned>(body: &[u8], field: &str) -> AppResult<T> {
    let mut reply: Value = serde_json::from_slice(body).map_err(|e| failed("source_data", format!("Resposta inválida do componente: {e}.")))?;
    if let Some(message) = reply["errors"][0]["message"].as_str() {
        return Err(source_error(message));
    }
    serde_json::from_value(reply["data"][field].take()).map_err(|e| failed("source_data", format!("Resposta inesperada do componente: {e}.")))
}

/// O erro vem do site da fonte, passando pelo servidor, com a pilha do Java junto. Para a pessoa, só a frase interessa.
fn source_error(message: &str) -> AppError {
    // O servidor só fala nisso quando o site pediu a verificação, e o resolvedor automático dele fica desligado de propósito.
    if message.to_lowercase().contains("cloudflare") {
        return failed("source_protected", "Este site pede uma verificação anti-robô, que só você pode passar.".to_string());
    }
    let line = message.lines().next().unwrap_or(message);
    let line = line.split_once(" java.").map_or(line, |(sentence, _)| sentence);
    let sentence = line.strip_prefix("Exception while fetching data").and_then(|rest| rest.split_once(") : ")).map_or(line, |(_, sentence)| sentence);
    failed("source_error", format!("A fonte respondeu com erro: {}", sentence.trim().chars().take(MAX_MESSAGE_CHARS).collect::<String>()))
}

// ---------------------------------------------------------------------------
// Extensões
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreReply {
    extension_store: StoreRow,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreRow {
    signing_key: Option<String>,
}

#[derive(Deserialize)]
struct ExtensionReply {
    extension: Option<ExtensionRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionRow {
    is_installed: bool,
    source: SourceNodes,
}

#[derive(Deserialize)]
struct SourceNodes {
    nodes: Vec<SourceRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceRow {
    id: String,
    name: String,
    lang: String,
    content_warning: String,
    supports_latest: bool,
}

/// O que não vier marcado como seguro conta como adulto: só aparece com o filtro +18 desligado.
fn to_source(row: SourceRow) -> SourceInfo {
    SourceInfo {
        id: row.id,
        name: row.name,
        language: row.lang,
        description: None,
        kinds: Vec::new(),
        rating: if row.content_warning == "SAFE" { ContentRating::General } else { ContentRating::Adult },
        direction: ReadingDirection::LeftToRight,
        supports_latest: row.supports_latest,
        supports_search: true,
        min_interval_ms: 0,
    }
}

/// Instala ou atualiza, confere se o repositório lido pelo servidor é o que a pessoa aprovou e devolve as fontes.
pub fn install_extension(core: &Core, repository_url: &str, fingerprint: &str, extension_id: &str, updating: bool) -> AppResult<Vec<SourceInfo>> {
    ensure_component(core)?;
    let client = client(core)?;
    let store: StoreReply = client.graphql("addExtensionStore", "mutation($u:String!){addExtensionStore(input:{indexUrl:$u}){extensionStore{signingKey}}}", json!({ "u": repository_url }), None)?;
    if store.extension_store.signing_key.as_deref() != Some(fingerprint) {
        return Err(failed("extension_provenance", "A chave deste repositório mudou. Atualize o repositório e revise a chave nova antes de instalar."));
    }
    client.graphql::<Value>("fetchExtensions", "mutation{fetchExtensions(input:{}){extensions{pkgName}}}", json!({}), None)?;
    let reply: ExtensionReply = client.graphql(
        "updateExtension",
        "mutation($p:String!,$i:Boolean,$u:Boolean){updateExtension(input:{id:$p,patch:{install:$i,update:$u}}){extension{isInstalled source{nodes{id name lang contentWarning supportsLatest}}}}}",
        json!({ "p": extension_id, "i": !updating, "u": updating }),
        None,
    )?;
    let extension = reply.extension.filter(|e| e.is_installed).ok_or_else(|| failed("extension_install", "O componente não conseguiu instalar esta extensão."))?;
    Ok(extension.source.nodes.into_iter().map(to_source).collect())
}

/// Tira o Java da memória. Os arquivos ficam, e o próximo uso inicia de novo.
pub fn stop(core: &Core) {
    *core.suwayomi.server.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// O navegador que o servidor diz ser ao falar com os sites (`NetworkHelper.kt` da v2.4.2366), e muda junto com `VERSION`.
/// A janela de verificação precisa dizer o mesmo: a liberação de um site vale só para o navegador que passou por ela.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub struct SiteCookie {
    pub domain: String,
    /// No formato de `Set-Cookie`, que é como o servidor guarda.
    pub text: String,
}

/// Entrega ao servidor o que um site deu à janela de verificação. Ele só lê esse arquivo ao iniciar,
/// então fica parado enquanto o arquivo muda, e o próximo uso o inicia de novo.
pub fn hand_over_cookies(core: &Core, cookies: &[SiteCookie]) -> AppResult<()> {
    let mut server = core.suwayomi.server.lock().unwrap_or_else(|e| e.into_inner());
    *server = None;
    let folder = root(core).join("dados").join("settings");
    std::fs::create_dir_all(&folder)?;
    let path = folder.join("cookie_store.xml");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let fresh = folder.join("cookie_store.novo");
    std::fs::write(&fresh, with_cookies(&existing, cookies))?;
    std::fs::rename(&fresh, &path)?;
    Ok(())
}

fn xml_escaped(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// O arquivo tem uma entrada por linha, com `<domínio>.size` e `<domínio>.<n>`.
/// O que chegou entra no lugar do que o domínio tinha, e os outros domínios ficam como estão.
fn with_cookies(existing: &str, cookies: &[SiteCookie]) -> String {
    let replaced = |line: &str| {
        let key = line.strip_prefix("<entry key=\"").and_then(|rest| rest.split_once('"')).map(|(key, _)| key);
        key.and_then(|key| key.rsplit_once('.')).is_some_and(|(domain, _)| cookies.iter().any(|cookie| xml_escaped(&cookie.domain) == domain))
    };
    let mut file = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE properties SYSTEM \"http://java.sun.com/dtd/properties.dtd\">\n<properties>\n");
    for line in existing.lines().map(str::trim).filter(|line| line.starts_with("<entry ") && !replaced(line)) {
        file.push_str(line);
        file.push('\n');
    }
    let mut domains: Vec<&str> = cookies.iter().map(|cookie| cookie.domain.as_str()).collect();
    domains.sort_unstable();
    domains.dedup();
    for domain in domains {
        let texts: Vec<&str> = cookies.iter().filter(|cookie| cookie.domain == domain).map(|cookie| cookie.text.as_str()).collect();
        let key = xml_escaped(domain);
        file.push_str(&format!("<entry key=\"{key}.size\">{}</entry>\n", texts.len()));
        for (index, text) in texts.iter().enumerate() {
            file.push_str(&format!("<entry key=\"{key}.{index}\">{}</entry>\n", xml_escaped(text)));
        }
    }
    file.push_str("</properties>\n");
    file
}

/// Se não der para falar com o servidor, a extensão sai do Prancha mesmo assim: nenhuma fonte dela continua disponível.
pub fn uninstall_extension(core: &Core, extension_id: &str) {
    let outcome = client(core).and_then(|client| client.graphql::<Value>("updateExtension", "mutation($p:String!){updateExtension(input:{id:$p,patch:{uninstall:true}}){extension{pkgName}}}", json!({ "p": extension_id }), None));
    if let Err(error) = outcome {
        log::warn!("suwayomi: a extensão {extension_id} ficou no componente: {error}");
    }
}

// ---------------------------------------------------------------------------
// Fontes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaPage {
    mangas: Vec<MangaRow>,
    has_next_page: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaRow {
    id: i64,
    title: String,
    thumbnail_url: Option<String>,
    #[serde(default)]
    genre: Vec<String>,
}

#[derive(Deserialize)]
struct MangaReply {
    manga: MangaDetails,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaDetails {
    id: i64,
    title: String,
    author: Option<String>,
    artist: Option<String>,
    description: Option<String>,
    status: Option<String>,
    #[serde(default)]
    genre: Vec<String>,
    thumbnail_url: Option<String>,
}

#[derive(Deserialize)]
struct ChaptersReply {
    chapters: Vec<ChapterRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChapterRow {
    id: i64,
    name: String,
    chapter_number: f64,
    source_order: i64,
    upload_date: String,
}

#[derive(Deserialize)]
struct PagesReply {
    pages: Vec<String>,
}

/// O Mihon não classifica obra por obra, e uma fonte marcada como segura ainda traz obras adultas.
/// A ponte não inventa uma classificação para a obra: quem lê os gêneros, a descrição e o título é `rating`.
fn to_search_page(page: MangaPage) -> SearchPage {
    SearchPage {
        results: page.mangas.into_iter().map(|m| SearchResult { rating: None, id: m.id.to_string(), title: m.title, cover_url: m.thumbnail_url, summary: None, author: None, status: None }).collect(),
        has_more: page.has_next_page,
    }
}

fn to_details(manga: MangaDetails) -> ComicDetails {
    let status = match manga.status.as_deref() {
        Some("ONGOING") => Some(PublicationStatus::Ongoing),
        Some("COMPLETED" | "PUBLISHING_FINISHED") => Some(PublicationStatus::Completed),
        Some("ON_HIATUS") => Some(PublicationStatus::Hiatus),
        Some("CANCELLED") => Some(PublicationStatus::Cancelled),
        _ => None,
    };
    let mut authors: Vec<String> = manga.author.into_iter().chain(manga.artist).collect();
    authors.dedup();
    ComicDetails { rating: None, id: manga.id.to_string(), title: manga.title, synopsis: manga.description, cover_url: manga.thumbnail_url, authors, genres: manga.genre, status, language: None }
}

fn to_chapter(chapter: ChapterRow) -> Chapter {
    Chapter {
        id: chapter.id.to_string(),
        title: chapter.name,
        // O servidor usa -1 quando a fonte não numera, e 0 quando não informa a data.
        number: (chapter.chapter_number >= 0.0).then(|| chapter.chapter_number.to_string()),
        published_at: chapter.upload_date.parse::<i64>().ok().filter(|date| *date > 0),
        order: chapter.source_order.max(0) as u32,
    }
}

fn to_pages(paths: Vec<String>) -> Vec<Page> {
    paths.into_iter().enumerate().map(|(index, url)| Page { index: index as u32, url, headers: Vec::new() }).collect()
}

fn number(remote_id: &str) -> AppResult<i64> {
    remote_id.parse().map_err(|_| failed("source_data", "Esta obra veio de outra instalação do componente."))
}

pub enum Browse<'a> {
    Popular,
    Latest,
    Search(&'a str),
}

/// A página e os gêneros que a lista trouxe, por obra. `SearchResult` vem do contrato das extensões e não tem esse campo.
pub fn browse(core: &Core, source_id: &str, kind: Browse, page: u32, cancel: Option<&Cancel>) -> AppResult<(SearchPage, HashMap<String, Vec<String>>)> {
    let (kind, query) = match kind {
        Browse::Popular => ("POPULAR", None),
        Browse::Latest => ("LATEST", None),
        Browse::Search(text) => ("SEARCH", Some(text)),
    };
    let found: MangaPage = client(core)?.graphql(
        "fetchSourceManga",
        "mutation($s:LongString!,$t:FetchSourceMangaType!,$p:Int!,$q:String){fetchSourceManga(input:{source:$s,type:$t,page:$p,query:$q}){hasNextPage mangas{id title thumbnailUrl genre}}}",
        json!({ "s": source_id, "t": kind, "p": page.max(1), "q": query }),
        cancel,
    )?;
    let genres = found.mangas.iter().map(|manga| (manga.id.to_string(), manga.genre.clone())).collect();
    Ok((to_search_page(found), genres))
}

pub fn details(core: &Core, remote_id: &str, cancel: Option<&Cancel>) -> AppResult<ComicDetails> {
    let reply: MangaReply = client(core)?.graphql("fetchManga", "mutation($i:Int!){fetchManga(input:{id:$i}){manga{id title author artist description status genre thumbnailUrl}}}", json!({ "i": number(remote_id)? }), cancel)?;
    Ok(to_details(reply.manga))
}

pub fn chapters(core: &Core, remote_id: &str, cancel: Option<&Cancel>) -> AppResult<Vec<Chapter>> {
    let reply: ChaptersReply = client(core)?.graphql("fetchChapters", "mutation($i:Int!){fetchChapters(input:{mangaId:$i}){chapters{id name chapterNumber sourceOrder uploadDate}}}", json!({ "i": number(remote_id)? }), cancel)?;
    Ok(reply.chapters.into_iter().map(to_chapter).collect())
}

pub fn pages(core: &Core, chapter_id: &str, cancel: Option<&Cancel>) -> AppResult<Vec<Page>> {
    let reply: PagesReply = client(core)?.graphql("fetchChapterPages", "mutation($i:Int!){fetchChapterPages(input:{chapterId:$i}){pages}}", json!({ "i": number(chapter_id)? }), cancel)?;
    Ok(to_pages(reply.pages))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageAddress {
    real_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceAddress {
    home_url: Option<String>,
    base_url: Option<String>,
}

/// O endereço no site da fonte: do capítulo, da obra ou, sem eles, o da própria fonte.
pub fn site_address(core: &Core, source_id: &str, series: Option<&str>, chapter: Option<&str>) -> AppResult<String> {
    let client = client(core)?;
    let page = |name: &str, query: &str, id: Option<&str>| -> AppResult<Option<String>> {
        let Some(id) = id else { return Ok(None) };
        Ok(client.graphql::<PageAddress>(name, query, json!({ "i": number(id)? }), None)?.real_url)
    };
    if let Some(address) = page("chapter", "query($i:Int!){chapter(id:$i){realUrl}}", chapter)?.filter(|address| !address.is_empty()) {
        return Ok(address);
    }
    if let Some(address) = page("manga", "query($i:Int!){manga(id:$i){realUrl}}", series)?.filter(|address| !address.is_empty()) {
        return Ok(address);
    }
    let source: SourceAddress = client.graphql("source", "query($s:LongString!){source(id:$s){homeUrl baseUrl}}", json!({ "s": source_id }), None)?;
    source.home_url.or(source.base_url).filter(|address| !address.is_empty()).ok_or_else(|| failed("source_data", "Esta fonte não informa o endereço do site."))
}

/// Capas e páginas chegam pelo servidor local, com um caminho no lugar do endereço.
pub fn image(core: &Core, path: &str, limits: Limits, cancel: Option<&Cancel>) -> Result<Response, NetError> {
    client(core).map_err(|e| NetError::Network(e.to_string()))?.image(path, limits, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Respostas copiadas do Suwayomi v2.4.2366 rodando a extensão do Webtoons.com.
    #[test]
    fn real_replies_become_prancha_types() {
        let page: MangaPage = read_reply(br#"{"data":{"fetchSourceManga":{"hasNextPage":true,"mangas":[{"id":1,"title":"Hero Killer","thumbnailUrl":"/api/v1/manga/1/thumbnail"}]}}}"#, "fetchSourceManga").unwrap();
        let page = to_search_page(page);
        assert!(page.has_more);
        assert_eq!((page.results[0].id.as_str(), page.results[0].cover_url.as_deref()), ("1", Some("/api/v1/manga/1/thumbnail")));

        let reply: ChaptersReply = read_reply(br#"{"data":{"fetchChapters":{"chapters":[{"id":7,"name":"Episode 1 (ch. 1)","chapterNumber":1,"sourceOrder":1,"uploadDate":"1620684028000"},{"id":8,"name":"Extra","chapterNumber":-1,"sourceOrder":2,"uploadDate":"0"},{"id":9,"name":"Meio","chapterNumber":2.5,"sourceOrder":3,"uploadDate":"1"}]}}}"#, "fetchChapters").unwrap();
        let chapters: Vec<Chapter> = reply.chapters.into_iter().map(to_chapter).collect();
        assert_eq!((chapters[0].id.as_str(), chapters[0].number.as_deref(), chapters[0].published_at, chapters[0].order), ("7", Some("1"), Some(1_620_684_028_000), 1));
        assert_eq!((chapters[1].number.as_deref(), chapters[1].published_at), (None, None));
        assert_eq!(chapters[2].number.as_deref(), Some("2.5"));

        let reply: PagesReply = read_reply(br#"{"data":{"fetchChapterPages":{"pages":["/api/v1/manga/1/chapter/1/page/0","/api/v1/manga/1/chapter/1/page/1"]}}}"#, "fetchChapterPages").unwrap();
        let pages = to_pages(reply.pages);
        assert_eq!((pages[1].index, pages[1].url.as_str()), (1, "/api/v1/manga/1/chapter/1/page/1"));
    }

    #[test]
    fn details_fill_what_the_screen_shows() {
        let reply: MangaReply = read_reply(br#"{"data":{"fetchManga":{"manga":{"id":1,"title":"T","author":"A","artist":"A","description":"D","status":"ON_HIATUS","genre":["Action"],"thumbnailUrl":null}}}}"#, "fetchManga").unwrap();
        let details = to_details(reply.manga);
        assert_eq!(details.authors, ["A"]);
        assert!(matches!(details.status, Some(PublicationStatus::Hiatus)));
        let reply: MangaReply = read_reply(br#"{"data":{"fetchManga":{"manga":{"id":1,"title":"T","author":null,"artist":null,"description":null,"status":"UNKNOWN","thumbnailUrl":null}}}}"#, "fetchManga").unwrap();
        assert!(to_details(reply.manga).status.is_none());
    }

    #[test]
    fn the_bridge_does_not_rate_a_series_by_itself() {
        let reply: MangaReply = read_reply(br#"{"data":{"fetchManga":{"manga":{"id":1,"title":"T","author":null,"artist":null,"description":null,"status":"ONGOING","genre":["Ecchi","Young Adult"],"thumbnailUrl":null}}}}"#, "fetchManga").unwrap();
        let details = to_details(reply.manga);
        assert!(details.rating.is_none());
        assert_eq!(details.genres, ["Ecchi", "Young Adult"]);
    }

    #[test]
    fn only_sources_marked_safe_count_as_general() {
        let source = |warning: &str| to_source(SourceRow { id: "1".into(), name: "F".into(), lang: "en".into(), content_warning: warning.into(), supports_latest: true });
        assert!(matches!(source("SAFE").rating, ContentRating::General));
        for other in ["MIXED", "NSFW", ""] {
            assert!(matches!(source(other).rating, ContentRating::Adult), "{other}");
        }
    }

    #[test]
    fn an_error_from_the_source_reaches_the_person_shortened() {
        let body = json!({ "errors": [{ "message": "x".repeat(MAX_MESSAGE_CHARS + 50) }], "data": null }).to_string();
        let error = read_reply::<Value>(body.as_bytes(), "fetchManga").unwrap_err();
        assert_eq!(error.code(), "source_error");
        assert!(error.to_string().chars().count() < MAX_MESSAGE_CHARS + 40, "{error}");
    }

    #[test]
    fn only_the_sentence_of_an_error_reaches_the_person() {
        for message in [
            "Exception while fetching data (/fetchSourceManga) : HTTP error 503\njava.io.IOException: HTTP error 503\n\tat keiyoushi.a.t.a(Unknown Source)",
            "Exception while fetching data (/fetchSourceManga) : HTTP error 503 java.io.IOException: HTTP error 503 at keiyoushi.a.t.a(Unknown Source)",
            "HTTP error 503",
        ] {
            assert_eq!(source_error(message).to_string(), "A fonte respondeu com erro: HTTP error 503", "{message}");
        }
    }

    #[test]
    fn a_site_behind_an_anti_robot_check_says_so_in_plain_words() {
        let error = source_error("Exception while fetching data (/fetchSourceManga) : Cloudflare bypass currently disabled\njava.io.IOException: Cloudflare bypass currently disabled\n\tat keiyoushi.a.t.a(Unknown Source)");
        assert_eq!(error.code(), "source_protected");
        assert!(error.to_string().contains("verificação anti-robô"), "{error}");
        assert!(!error.to_string().contains("java"), "{error}");
    }

    #[test]
    fn cookies_from_a_site_replace_only_what_that_site_had() {
        let cookie = |domain: &str, text: &str| SiteCookie { domain: domain.into(), text: text.into() };
        let existing = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE properties SYSTEM \"http://java.sun.com/dtd/properties.dtd\">\n<properties>\n<entry key=\"site.com.size\">1</entry>\n<entry key=\"site.com.0\">velho=1; path=/</entry>\n<entry key=\"www.site.com.size\">1</entry>\n<entry key=\"www.site.com.0\">outro=1; path=/</entry>\n<entry key=\"site.com.br.size\">1</entry>\n<entry key=\"site.com.br.0\">br=1; path=/</entry>\n</properties>\n";
        let file = with_cookies(existing, &[cookie("site.com", "cf_clearance=a&b; Path=/; Secure"), cookie("site.com", "sessao=2")]);
        assert!(!file.contains("velho=1"), "{file}");
        assert!(file.contains("<entry key=\"site.com.size\">2</entry>"), "{file}");
        assert!(file.contains("<entry key=\"site.com.0\">cf_clearance=a&amp;b; Path=/; Secure</entry>"), "{file}");
        assert!(file.contains("<entry key=\"site.com.1\">sessao=2</entry>"), "{file}");
        assert!(file.contains("www.site.com.0\">outro=1") && file.contains("site.com.br.0\">br=1"), "outros domínios ficam: {file}");
        assert!(file.starts_with("<?xml") && file.ends_with("</properties>\n"), "{file}");

        // Sem arquivo ainda, o servidor nunca falou com site nenhum.
        let fresh = with_cookies("", &[cookie("site.com", "a=1")]);
        assert!(fresh.starts_with("<?xml") && fresh.contains("<entry key=\"site.com.0\">a=1</entry>") && fresh.ends_with("</properties>\n"), "{fresh}");
    }

    fn package(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pacote.zip");
        let mut writer = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, body) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap();
        (dir, path)
    }

    #[test]
    fn only_java_and_the_server_are_unpacked() {
        let (dir, path) = package(&[("v/jre/bin/javaw.exe", b"java"), ("v/jre/lib/x", b"lib"), ("v/bin/Suwayomi-Server.jar", b"jar"), ("v/electron/electron.exe", b"e"), ("v/Suwayomi-Launcher.jar", b"l"), ("../fora.txt", b"f")]);
        let target = dir.path().join("alvo");
        unpack(&path, &target).unwrap();
        assert!(is_complete(&target));
        assert!(target.join("jre/lib/x").is_file());
        assert!(!target.join("electron").exists());
        assert!(!target.join("Suwayomi-Launcher.jar").exists());
        assert!(!dir.path().join("fora.txt").exists());
    }

    #[test]
    fn a_package_without_java_or_server_is_refused() {
        let (dir, path) = package(&[("v/bin/Suwayomi-Server.jar", b"jar")]);
        assert_eq!(unpack(&path, &dir.path().join("alvo")).unwrap_err().code(), "component_package");
    }
}
