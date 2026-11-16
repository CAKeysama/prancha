//! Repositórios do Mihon no Android. As extensões dele são APKs do Android, então lá elas rodam dentro do próprio app,
//! carregadas pelo código Kotlin do Prancha (`MihonPlugin`), com a mesma API que o Mihon oferece a elas. No Windows
//! quem faz esse papel é o Suwayomi (`suwayomi.rs`); este módulo tem as mesmas funções, e no Android ocupa o lugar dele.
//!
//! Como no Mihon, o código das extensões roda sem isolamento, dentro do processo do app: a interface avisa antes de
//! instalar. O pacote só é aceito com o certificado que o repositório anuncia, e com o nome que o índice deu.
//!
//! Os identificadores que o resto do Prancha guarda levam a fonte na frente (`<fonte>:<endereço>`), porque no Mihon
//! obras, capítulos e imagens são endereços dentro de uma fonte, e as funções daqui recebem só o identificador.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use super::net::{Access, Cancel, Limits, NetError, Request, Response};
use super::runtime::{Chapter, ComicDetails, ContentRating, Page, PublicationStatus, ReadingDirection, SearchPage, SearchResult, SourceInfo};
use crate::core::Core;
use crate::error::{AppError, AppResult};

/// O mesmo runtime das entradas do índice no Windows: o repositório é o mesmo, só quem roda as extensões muda.
pub const RUNTIME: &str = "suwayomi";

/// As chamadas passam pelo site da fonte, que pode ser lento.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_APK_BYTES: u64 = 64 * 1024 * 1024;
const APK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_MESSAGE_CHARS: usize = 300;

/// O lado Kotlin: um comando com argumentos em JSON e a resposta em JSON, ou a mensagem do erro.
pub trait Host: Send + Sync {
    fn call(&self, command: &str, args: Value) -> Result<Value, String>;
}

static HOST: OnceLock<Arc<dyn Host>> = OnceLock::new();

/// O app Android registra o plugin ao iniciar.
pub fn register(host: Arc<dyn Host>) {
    let _ = HOST.set(host);
}

/// Nada a guardar: o lado Kotlin mantém as extensões carregadas. Existe para o `Core` ser o mesmo nas duas plataformas.
#[derive(Default)]
pub struct Suwayomi;

fn failed(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::user(code, message)
}

/// O erro vem do site da fonte, passando pela extensão, às vezes com a classe da exceção na frente.
fn source_error(message: &str) -> AppError {
    if message.to_lowercase().contains("cloudflare") {
        return failed("source_protected", "Este site pede uma verificação anti-robô, que só você pode passar.");
    }
    let line = message.lines().next().unwrap_or(message).trim();
    let sentence = match line.split_once(": ") {
        Some((class, rest)) if class.contains('.') && !class.contains(' ') => rest,
        _ => line,
    };
    failed("source_error", format!("A fonte respondeu com erro: {}", sentence.trim().chars().take(MAX_MESSAGE_CHARS).collect::<String>()))
}

/// Roda a chamada fora da thread de quem pediu, para um cancelamento responder na hora. A chamada que já saiu
/// termina sozinha, e a resposta dela é descartada.
fn call(command: &str, args: Value, cancel: Option<&Cancel>) -> AppResult<Value> {
    let host = HOST.get().cloned().ok_or_else(|| failed("component_missing", "As extensões do Mihon não estão disponíveis nesta versão do app."))?;
    let (sender, receiver) = mpsc::channel();
    let name = command.to_string();
    std::thread::spawn(move || {
        let _ = sender.send(host.call(&name, args));
    });
    let started = Instant::now();
    loop {
        if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
            return Err(failed("extension_cancelled", "Operação cancelada."));
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(message)) => return Err(source_error(&message)),
            Err(mpsc::RecvTimeoutError::Timeout) if started.elapsed() < CALL_TIMEOUT => {}
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(failed("source_network", "A fonte demorou demais para responder.")),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(failed("source_error", "A extensão parou sem responder.")),
        }
    }
}

fn reply<T: for<'de> Deserialize<'de>>(value: Value) -> AppResult<T> {
    serde_json::from_value(value).map_err(|e| failed("source_data", format!("Resposta inesperada da extensão: {e}.")))
}

/// `<fonte>:<endereço>`. As fontes do Mihon têm números como identificador, então o primeiro `:` separa.
fn encode(source_id: &str, payload: &str) -> String {
    format!("{source_id}:{payload}")
}

fn decode(id: &str) -> AppResult<(&str, &str)> {
    id.split_once(':').filter(|(source, _)| !source.is_empty() && source.bytes().all(|b| b.is_ascii_digit())).ok_or_else(|| failed("source_data", "Esta obra veio de outra instalação do Prancha."))
}

// ---------------------------------------------------------------------------
// Extensões
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Installed {
    content_warning: String,
    sources: Vec<SourceRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceRow {
    id: String,
    name: String,
    lang: String,
    supports_latest: bool,
}

/// Como no Windows: o que não vier marcado como seguro conta como adulto.
fn to_sources(installed: Installed) -> Vec<SourceInfo> {
    let rating = if installed.content_warning == "SAFE" { ContentRating::General } else { ContentRating::Adult };
    installed
        .sources
        .into_iter()
        .map(|row| SourceInfo {
            id: row.id,
            name: row.name,
            language: row.lang,
            description: None,
            kinds: Vec::new(),
            rating,
            direction: ReadingDirection::LeftToRight,
            supports_latest: row.supports_latest,
            supports_search: true,
            min_interval_ms: 0,
        })
        .collect()
}

fn download_folder(core: &Core) -> PathBuf {
    core.paths.data_dir.join("cache").join("mihon")
}

/// Baixa o APK que o índice anuncia e entrega ao lado Kotlin, que confere o certificado e o nome do pacote antes de
/// guardar. O arquivo baixado sai daqui de qualquer jeito.
pub fn install_extension(core: &Core, _repository_url: &str, fingerprint: &str, extension_id: &str, package_url: &str, updating: bool) -> AppResult<Vec<SourceInfo>> {
    let folder = download_folder(core);
    std::fs::create_dir_all(&folder)?;
    let response = core
        .net
        .get(package_url, Access::Public, Limits { max_bytes: MAX_APK_BYTES, timeout: APK_TIMEOUT }, Request::default())
        .map_err(|e| failed("extension_download", format!("Não foi possível baixar a extensão: {e}")))?;
    if !response.is_success() {
        return Err(failed("extension_download", format!("Não foi possível baixar a extensão (resposta {}).", response.status)));
    }
    let file = folder.join(format!("{extension_id}.apk"));
    std::fs::write(&file, &response.body)?;
    let installed = call("install", json!({ "apk": file.to_string_lossy(), "fingerprint": fingerprint, "packageName": extension_id, "updating": updating }), None);
    let _ = std::fs::remove_file(&file);
    let installed: Installed = reply(installed.map_err(|e| match e.code() {
        "source_error" => failed("extension_install", e.to_string().replacen("A fonte respondeu com erro: ", "Não foi possível instalar a extensão: ", 1)),
        _ => e,
    })?)?;
    Ok(to_sources(installed))
}

/// Nada fica rodando à parte para parar: as extensões estão dentro do app.
pub fn stop(_core: &Core) {}

pub fn uninstall_extension(_core: &Core, extension_id: &str) {
    if let Err(error) = call("uninstall", json!({ "packageName": extension_id }), None) {
        log::warn!("mihon: a extensão {extension_id} ficou no app: {error}");
    }
}

/// A tela do site para a pessoa passar pela verificação. Os cookies ficam no navegador do sistema, que as extensões
/// usam, então não há o que entregar depois. Diz se o site liberou.
pub fn verify(address: &str) -> AppResult<bool> {
    let host = HOST.get().cloned().ok_or_else(|| failed("verification_failed", "A verificação não está disponível nesta versão do app."))?;
    let reply = host.call("verify", json!({ "url": address })).map_err(|e| failed("verification_failed", format!("Não foi possível abrir o site: {e}")))?;
    Ok(reply["cleared"].as_bool().unwrap_or(false))
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
    url: String,
    title: String,
    thumbnail_url: Option<String>,
    #[serde(default)]
    genre: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaDetails {
    title: String,
    author: Option<String>,
    artist: Option<String>,
    description: Option<String>,
    /// `SManga.status` do Mihon.
    status: i32,
    #[serde(default)]
    genre: Vec<String>,
    thumbnail_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChapterRow {
    url: String,
    name: String,
    chapter_number: f64,
    date_upload: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageRow {
    index: u32,
    url: String,
    image_url: Option<String>,
}

fn cover(source_id: &str, url: Option<String>) -> Option<String> {
    url.filter(|url| !url.is_empty()).map(|url| encode(source_id, &format!("capa {url}")))
}

pub enum Browse<'a> {
    Popular,
    Latest,
    Search(&'a str),
}

/// A página e os gêneros que a lista trouxe, por obra. `SearchResult` vem do contrato das extensões e não tem esse campo.
pub fn browse(_core: &Core, source_id: &str, kind: Browse, page: u32, cancel: Option<&Cancel>) -> AppResult<(SearchPage, HashMap<String, Vec<String>>)> {
    let (kind, query) = match kind {
        Browse::Popular => ("popular", None),
        Browse::Latest => ("latest", None),
        Browse::Search(text) => ("search", Some(text)),
    };
    let found: MangaPage = reply(call("browse", json!({ "source": source_id, "kind": kind, "page": page.max(1), "query": query }), cancel)?)?;
    let genres = found.mangas.iter().map(|manga| (encode(source_id, &manga.url), manga.genre.clone())).collect();
    let results = found
        .mangas
        .into_iter()
        .map(|m| SearchResult { rating: None, id: encode(source_id, &m.url), title: m.title, cover_url: cover(source_id, m.thumbnail_url), summary: None, author: None, status: None })
        .collect();
    Ok((SearchPage { results, has_more: found.has_next_page }, genres))
}

/// Os números de `SManga` no Mihon: 1 em andamento, 2 e 4 concluída, 5 cancelada, 6 em pausa.
fn status(code: i32) -> Option<PublicationStatus> {
    match code {
        1 => Some(PublicationStatus::Ongoing),
        2 | 4 => Some(PublicationStatus::Completed),
        5 => Some(PublicationStatus::Cancelled),
        6 => Some(PublicationStatus::Hiatus),
        _ => None,
    }
}

/// Autor e artista vêm como texto livre, às vezes com vários nomes separados por vírgula.
fn to_details(remote_id: &str, source_id: &str, manga: MangaDetails) -> ComicDetails {
    let mut authors: Vec<String> = manga.author.into_iter().chain(manga.artist).flat_map(|names| names.split(',').map(|name| name.trim().to_string()).collect::<Vec<_>>()).filter(|name| !name.is_empty()).collect();
    authors.dedup();
    ComicDetails {
        rating: None,
        id: remote_id.to_string(),
        title: manga.title,
        synopsis: manga.description,
        cover_url: cover(source_id, manga.thumbnail_url),
        authors,
        genres: manga.genre,
        status: status(manga.status),
        language: None,
    }
}

pub fn details(_core: &Core, remote_id: &str, cancel: Option<&Cancel>) -> AppResult<ComicDetails> {
    let (source_id, url) = decode(remote_id)?;
    let manga: MangaDetails = reply(call("details", json!({ "source": source_id, "url": url }), cancel)?)?;
    Ok(to_details(remote_id, source_id, manga))
}

/// O Mihon lista do mais novo para o mais antigo; a ordem do Prancha, como a do Suwayomi, começa no mais antigo.
fn to_chapters(source_id: &str, rows: Vec<ChapterRow>) -> Vec<Chapter> {
    let total = rows.len() as u32;
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| Chapter {
            id: encode(source_id, &row.url),
            title: row.name,
            // O Mihon usa -1 quando a fonte não numera, e 0 quando não informa a data.
            number: (row.chapter_number >= 0.0).then(|| row.chapter_number.to_string()),
            published_at: (row.date_upload > 0).then_some(row.date_upload),
            order: total - index as u32,
        })
        .collect()
}

pub fn chapters(_core: &Core, remote_id: &str, cancel: Option<&Cancel>) -> AppResult<Vec<Chapter>> {
    let (source_id, url) = decode(remote_id)?;
    let rows: Vec<ChapterRow> = reply(call("chapters", json!({ "source": source_id, "url": url }), cancel)?["items"].take())?;
    Ok(to_chapters(source_id, rows))
}

/// A página guarda o que a extensão precisa para buscar a imagem depois, mesmo num download com o app reiniciado.
fn to_pages(source_id: &str, rows: Vec<PageRow>) -> Vec<Page> {
    rows.into_iter()
        .enumerate()
        .map(|(position, row)| {
            let page = json!({ "index": row.index, "url": row.url, "imageUrl": row.image_url });
            Page { index: position as u32, url: encode(source_id, &format!("pagina {page}")), headers: Vec::new() }
        })
        .collect()
}

pub fn pages(_core: &Core, chapter_id: &str, cancel: Option<&Cancel>) -> AppResult<Vec<Page>> {
    let (source_id, url) = decode(chapter_id)?;
    let rows: Vec<PageRow> = reply(call("pages", json!({ "source": source_id, "url": url }), cancel)?["items"].take())?;
    Ok(to_pages(source_id, rows))
}

/// O endereço no site da fonte: do capítulo, da obra ou, sem eles, o da própria fonte.
pub fn site_address(_core: &Core, source_id: &str, series: Option<&str>, chapter: Option<&str>) -> AppResult<String> {
    let url = |id: Option<&str>| -> AppResult<Option<String>> { id.map(|id| decode(id).map(|(_, url)| url.to_string())).transpose() };
    let reply = call("address", json!({ "source": source_id, "manga": url(series)?, "chapter": url(chapter)? }), None)?;
    reply["url"].as_str().filter(|address| !address.is_empty()).map(str::to_string).ok_or_else(|| failed("source_data", "Esta fonte não informa o endereço do site."))
}

static IMAGE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Capas e páginas passam pela extensão, que sabe os cabeçalhos que o site pede. O lado Kotlin grava a imagem num
/// arquivo, e o corpo vem de lá, sem passar a imagem inteira pela ponte.
pub fn image(core: &Core, path: &str, limits: Limits, cancel: Option<&Cancel>) -> Result<Response, NetError> {
    let (source_id, payload) = decode(path).map_err(|e| NetError::Blocked(e.to_string()))?;
    let args = if let Some(url) = payload.strip_prefix("capa ") {
        json!({ "source": source_id, "cover": url })
    } else if let Some(page) = payload.strip_prefix("pagina ") {
        json!({ "source": source_id, "page": serde_json::from_str::<Value>(page).map_err(|_| NetError::Blocked("página inválida".into()))? })
    } else {
        return Err(NetError::Blocked("caminho inesperado".into()));
    };
    let folder = download_folder(core);
    std::fs::create_dir_all(&folder).map_err(|e| NetError::Network(e.to_string()))?;
    let file = folder.join(format!("imagem-{}-{}", std::process::id(), IMAGE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let mut args = args;
    args["file"] = json!(file.to_string_lossy());
    args["maxBytes"] = json!(limits.max_bytes);
    let outcome = call("image", args, cancel);
    let body = std::fs::read(&file);
    let _ = std::fs::remove_file(&file);
    let reply = outcome.map_err(|e| match e.code() {
        "extension_cancelled" => NetError::Cancelled,
        "source_network" => NetError::Timeout,
        _ => NetError::Network(e.to_string()),
    })?;
    if reply["tooLarge"].as_bool() == Some(true) {
        return Err(NetError::TooLarge);
    }
    let status = reply["status"].as_u64().unwrap_or(0) as u16;
    Ok(Response { status, url: reply["url"].as_str().unwrap_or_default().to_string(), content_type: reply["contentType"].as_str().map(str::to_string), etag: None, body: body.unwrap_or_default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_carry_the_source_in_front() {
        let id = encode("9124545289113350724", "/manga/a:b?c=1");
        assert_eq!(decode(&id).unwrap(), ("9124545289113350724", "/manga/a:b?c=1"));
        assert_eq!(decode("7").unwrap_err().code(), "source_data");
        assert_eq!(decode("abc:/x").unwrap_err().code(), "source_data", "id do Suwayomi ou de outra fonte");
    }

    #[test]
    fn chapters_count_from_the_oldest_and_keep_unknown_numbers_and_dates_empty() {
        let row = |url: &str, number: f64, date: i64| ChapterRow { url: url.into(), name: url.into(), chapter_number: number, date_upload: date };
        let chapters = to_chapters("1", vec![row("/c3", 3.0, 1_700_000_000_000), row("/extra", -1.0, 0), row("/c1", 1.0, 1)]);
        assert_eq!(chapters.iter().map(|c| c.order).collect::<Vec<_>>(), [3, 2, 1]);
        assert_eq!((chapters[0].id.as_str(), chapters[0].number.as_deref(), chapters[0].published_at), ("1:/c3", Some("3"), Some(1_700_000_000_000)));
        assert_eq!((chapters[1].number.as_deref(), chapters[1].published_at), (None, None));
    }

    #[test]
    fn details_split_authors_and_map_the_mihon_status() {
        let manga = MangaDetails { title: "T".into(), author: Some("Ana, Bia".into()), artist: Some("Bia".into()), description: None, status: 6, genre: vec!["Ação".into()], thumbnail_url: Some("https://site/capa.jpg".into()) };
        let details = to_details("1:/obra", "1", manga);
        assert_eq!(details.authors, ["Ana", "Bia"]);
        assert!(matches!(details.status, Some(PublicationStatus::Hiatus)));
        assert_eq!(details.cover_url.as_deref(), Some("1:capa https://site/capa.jpg"));
        assert!(details.rating.is_none(), "como no Windows, quem classifica a obra é `rating`, pelos gêneros");
        assert!(status(0).is_none() && status(3).is_none());
    }

    #[test]
    fn pages_keep_what_the_extension_needs_to_fetch_the_image_later() {
        let pages = to_pages("5", vec![PageRow { index: 0, url: "/p/1".into(), image_url: None }, PageRow { index: 1, url: String::new(), image_url: Some("https://img/2.jpg".into()) }]);
        assert_eq!(pages[1].index, 1);
        let (source, payload) = decode(&pages[1].url).unwrap();
        assert_eq!(source, "5");
        let page: Value = serde_json::from_str(payload.strip_prefix("pagina ").unwrap()).unwrap();
        assert_eq!(page, json!({ "index": 1, "url": "", "imageUrl": "https://img/2.jpg" }));
    }

    #[test]
    fn only_extensions_marked_safe_count_as_general() {
        let installed = |warning: &str| Installed { content_warning: warning.into(), sources: vec![SourceRow { id: "1".into(), name: "F".into(), lang: "pt-BR".into(), supports_latest: false }] };
        assert!(matches!(to_sources(installed("SAFE"))[0].rating, ContentRating::General));
        for other in ["MIXED", "NSFW", ""] {
            assert!(matches!(to_sources(installed(other))[0].rating, ContentRating::Adult), "{other}");
        }
    }

    #[test]
    fn an_error_from_the_source_reaches_the_person_without_the_exception_class() {
        assert_eq!(source_error("java.io.IOException: HTTP error 503\n\tat x").to_string(), "A fonte respondeu com erro: HTTP error 503");
        assert_eq!(source_error("HTTP error 503").to_string(), "A fonte respondeu com erro: HTTP error 503");
        assert_eq!(source_error("Cloudflare: verificação anti-robô pedida por site.com").code(), "source_protected");
        assert!(source_error(&"x".repeat(MAX_MESSAGE_CHARS + 50)).to_string().chars().count() < MAX_MESSAGE_CHARS + 40);
    }
}
