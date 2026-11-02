//! Extensão só para os testes: a busca escolhe o que ela faz, inclusive tentar o que não deveria conseguir.

use prancha_extension_sdk::host::{self, HttpError, HttpRequest};
use prancha_extension_sdk::{
    Chapter, ComicDetails, ContentKind, ContentRating, Filter, Guest, Page, PublicationStatus, ReadingDirection, SearchPage, SearchResult, SourceError, SourceInfo, export_extension,
};

struct Extension;

fn source(id: &str, rating: ContentRating, supports_search: bool) -> SourceInfo {
    SourceInfo {
        id: id.into(),
        name: format!("Fonte {id}"),
        language: "pt-BR".into(),
        description: None,
        kinds: vec![ContentKind::Comic],
        rating,
        direction: ReadingDirection::LeftToRight,
        supports_latest: true,
        supports_search,
        min_interval_ms: 0,
    }
}

fn says(title: impl Into<String>) -> Result<SearchPage, SourceError> {
    Ok(SearchPage { results: vec![result(title)], has_more: false })
}

fn result(title: impl Into<String>) -> SearchResult {
    let title = title.into();
    SearchResult { id: title.clone(), title, cover_url: None, summary: None, author: None, status: None, rating: None }
}

fn get(url: &str) -> Result<host::HttpResponse, HttpError> {
    host::http_get(&HttpRequest { url: url.into(), headers: vec![] })
}

fn recurse(depth: u64) -> u64 {
    if depth == u64::MAX { 0 } else { 1 + std::hint::black_box(recurse(depth + 1)) }
}

fn page_of(page: u32, label: &str) -> SearchPage {
    if page > 2 {
        return SearchPage { results: vec![], has_more: false };
    }
    SearchPage { results: (0..3).map(|i| result(format!("{label} {page}.{i}"))).collect(), has_more: page < 2 }
}

impl Guest for Extension {
    fn list_sources() -> Vec<SourceInfo> {
        vec![source("livre", ContentRating::General, true), source("sem-classificacao", ContentRating::Unknown, true), source("adulta", ContentRating::Adult, true)]
    }

    fn popular(_source: String, page: u32, _filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        Ok(page_of(page, "Popular"))
    }

    fn latest(_source: String, page: u32, _filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        Ok(page_of(page, "Recente"))
    }

    fn search(_source: String, query: String, page: u32, filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        let (command, argument) = query.split_once(':').unwrap_or((&query, ""));
        match command {
            "laco" => loop {
                std::hint::black_box(0);
            },
            "memoria" => {
                let mut blocks = Vec::new();
                loop {
                    blocks.push(vec![1u8; 1024 * 1024]);
                    std::hint::black_box(&blocks);
                }
            }
            "pilha" => says(recurse(0).to_string()),
            "panico" => panic!("falha de propósito"),
            "arquivo" => match std::fs::read(argument) {
                Ok(bytes) => says(format!("leu {} bytes", bytes.len())),
                Err(_) => says("sem acesso"),
            },
            "ambiente" => match std::env::var("PATH") {
                Ok(value) => says(format!("leu {value}")),
                Err(_) => says("sem acesso"),
            },
            "rede" => match get(argument) {
                Ok(response) => says(format!("{} {} bytes", response.status, response.body.len())),
                Err(error) => Err(SourceError::Network(format!("{error:?}"))),
            },
            "muitas" => {
                let mut done = 0;
                loop {
                    match get(argument) {
                        Ok(_) => done += 1,
                        Err(error) => return says(format!("{done} e depois {error:?}")),
                    }
                }
            }
            "guardar" => {
                let (key, value) = argument.split_once('=').unwrap_or((argument, ""));
                host::storage_set(key, value.as_bytes()).map_err(SourceError::Data)?;
                says("guardou")
            }
            "encher" => {
                let block = vec![0u8; 200 * 1024];
                let mut done = 0;
                loop {
                    match host::storage_set(&format!("bloco{done}"), &block) {
                        Ok(()) => done += 1,
                        Err(_) => return says(format!("{done} blocos")),
                    }
                }
            }
            "ler" => match host::storage_get(argument) {
                Some(value) => says(String::from_utf8_lossy(&value).to_string()),
                None => says("nada"),
            },
            "apagar" => {
                host::storage_delete(argument);
                says("apagou")
            }
            "cabecalho" => match host::http_get(&HttpRequest { url: argument.into(), headers: vec![prancha_extension_sdk::Header { name: "cookie".into(), value: "x".into() }] }) {
                Ok(_) => says("enviou"),
                Err(error) => Err(SourceError::Network(format!("{error:?}"))),
            },
            "erro-rede" => Err(SourceError::Network(String::new())),
            "erro-fonte" => Err(SourceError::Source("A fonte está em manutenção.".into())),
            "erro-login" => Err(SourceError::Authentication(String::new())),
            "erro-dados" => Err(SourceError::Data(String::new())),
            "erro-nao-suportado" => Err(SourceError::Unsupported(String::new())),
            "vazio" => Ok(SearchPage { results: vec![], has_more: false }),
            "filtros" => says(filters.iter().map(|f| format!("{}={}", f.name, f.value)).collect::<Vec<_>>().join(",")),
            "classificados" => Ok(SearchPage {
                results: vec![
                    SearchResult { rating: Some(ContentRating::Adult), ..result("Adulta") },
                    SearchResult { rating: Some(ContentRating::Teen), status: Some(PublicationStatus::Ongoing), ..result("Juvenil") },
                    SearchResult { title: "   ".into(), ..result("Sem título") },
                    SearchResult { cover_url: Some("http://exemplo.com/capa.png".into()), ..result("Capa insegura") },
                ],
                has_more: false,
            }),
            _ => says(format!("{query} {page}")),
        }
    }

    fn details(_source: String, id: String) -> Result<ComicDetails, SourceError> {
        if id == "adulta" {
            return Ok(ComicDetails { id: id.clone(), title: id, synopsis: None, cover_url: None, authors: vec![], genres: vec![], status: None, language: None, rating: Some(ContentRating::Adult) });
        }
        Ok(ComicDetails {
            id: id.clone(),
            title: format!("Obra {id}"),
            synopsis: Some("Sinopse".into()),
            cover_url: None,
            authors: vec!["Autoria".into()],
            genres: vec!["Aventura".into()],
            status: Some(PublicationStatus::Completed),
            language: Some("pt-BR".into()),
            rating: None,
        })
    }

    fn chapters(_source: String, id: String) -> Result<Vec<Chapter>, SourceError> {
        Ok((1..=3).map(|n| Chapter { id: format!("{id}-{n}"), title: format!("Capítulo {n}"), number: Some(n.to_string()), published_at: Some(1_700_000_000_000 + i64::from(n)), order: n }).collect())
    }

    /// `base|2,0,1`: as páginas na ordem informada, com a posição declarada em cada uma.
    fn pages(_source: String, chapter_id: String) -> Result<Vec<Page>, SourceError> {
        if let Some((base, order)) = chapter_id.split_once('|') {
            let indexes = order.split(',').filter_map(|n| n.parse::<u32>().ok());
            return Ok(indexes.map(|index| Page { index, url: format!("{base}/{index}.png"), headers: vec![] }).collect());
        }
        Ok((0..2).map(|index| Page { index, url: format!("https://exemplo.com/{chapter_id}/{index}.png"), headers: vec![] }).collect())
    }
}

export_extension!(Extension);
