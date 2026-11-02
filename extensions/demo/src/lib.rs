//! Extensão de demonstração: um acervo original, publicado pelo próprio projeto.
//! Faz o que uma fonte real faz: pede um catálogo por HTTP, pesquisa nele e informa capas e páginas.

use prancha_extension_sdk::host::{self, HttpError, HttpRequest, HttpResponse};
use prancha_extension_sdk::{
    Chapter, ComicDetails, ContentKind, ContentRating, Filter, Guest, Page, PublicationStatus, ReadingDirection, SearchPage, SearchResult, SourceError, SourceInfo, export_extension,
};
use serde::Deserialize;

const SOURCE: &str = "acervo";
const PAGE_SIZE: usize = 4;
/// `scripts/build-demo-repo.mjs` define o endereço de cada publicação.
const BASE_URL: &str = match option_env!("PRANCHA_DEMO_BASE_URL") {
    Some(url) => url,
    None => "https://raw.githubusercontent.com/CAKeysama/prancha/main/extensions/demo-repo",
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Work {
    id: String,
    title: String,
    author: String,
    synopsis: String,
    genres: Vec<String>,
    status: String,
    kind: String,
    popularity: u32,
    updated_at: i64,
    chapters: Vec<WorkChapter>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkChapter {
    id: String,
    title: String,
    number: String,
    published_at: i64,
    pages: u32,
}

fn fetch(url: String) -> Result<HttpResponse, SourceError> {
    let response = host::http_get(&HttpRequest { url, headers: vec![] }).map_err(|error| {
        SourceError::Network(match error {
            HttpError::Timeout => "O acervo demorou demais para responder.".into(),
            HttpError::Blocked(why) | HttpError::Network(why) => why,
            HttpError::TooLarge | HttpError::TooManyRequests => "O acervo não pôde ser consultado agora.".into(),
        })
    })?;
    if response.status == 200 { Ok(response) } else { Err(SourceError::Source(format!("O acervo respondeu {}.", response.status))) }
}

fn catalog() -> Result<Vec<Work>, SourceError> {
    let response = fetch(format!("{BASE_URL}/catalogo/obras.json"))?;
    serde_json::from_slice(&response.body).map_err(|e| SourceError::Data(format!("O catálogo veio em um formato inesperado: {e}")))
}

fn work_of(works: Vec<Work>, id: &str) -> Result<Work, SourceError> {
    works.into_iter().find(|work| work.id == id).ok_or_else(|| SourceError::Data("Esta obra não existe no acervo.".into()))
}

fn cover_url(work: &Work) -> String {
    format!("{BASE_URL}/catalogo/{}/capa.png", work.id)
}

fn status_of(work: &Work) -> Option<PublicationStatus> {
    match work.status.as_str() {
        "ongoing" => Some(PublicationStatus::Ongoing),
        "completed" => Some(PublicationStatus::Completed),
        "hiatus" => Some(PublicationStatus::Hiatus),
        "cancelled" => Some(PublicationStatus::Cancelled),
        _ => None,
    }
}

/// Minúsculas e sem acentos: quem digita "mares" encontra "Marés".
fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

fn page_of(works: Vec<Work>, page: u32, filters: &[Filter]) -> SearchPage {
    let wanted = filters.iter().find(|f| f.name == "kind").map(|f| f.value.as_str());
    let matching: Vec<Work> = works.into_iter().filter(|work| wanted.is_none_or(|kind| work.kind == kind)).collect();
    let skip = (page.max(1) as usize - 1) * PAGE_SIZE;
    let has_more = matching.len() > skip + PAGE_SIZE;
    let results = matching
        .iter()
        .skip(skip)
        .take(PAGE_SIZE)
        .map(|work| SearchResult {
            id: work.id.clone(),
            title: work.title.clone(),
            cover_url: Some(cover_url(work)),
            summary: Some(work.synopsis.clone()),
            author: Some(work.author.clone()),
            status: status_of(work),
            rating: Some(ContentRating::General),
        })
        .collect();
    SearchPage { results, has_more }
}

struct Extension;

impl Guest for Extension {
    fn list_sources() -> Vec<SourceInfo> {
        vec![SourceInfo {
            id: SOURCE.into(),
            name: "Acervo de demonstração".into(),
            language: "pt-BR".into(),
            description: Some("Obras curtas criadas pelo projeto Prancha para demonstrar as extensões.".into()),
            kinds: vec![ContentKind::Comic, ContentKind::Manga],
            rating: ContentRating::General,
            direction: ReadingDirection::LeftToRight,
            supports_latest: true,
            supports_search: true,
            min_interval_ms: 100,
        }]
    }

    fn popular(_source: String, page: u32, filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        let mut works = catalog()?;
        works.sort_by_key(|work| std::cmp::Reverse(work.popularity));
        Ok(page_of(works, page, &filters))
    }

    fn latest(_source: String, page: u32, filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        let mut works = catalog()?;
        works.sort_by_key(|work| std::cmp::Reverse(work.updated_at));
        Ok(page_of(works, page, &filters))
    }

    fn search(_source: String, query: String, page: u32, filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        let query = fold(&query);
        let mut works = catalog()?;
        works.retain(|work| fold(&work.title).contains(&query) || fold(&work.author).contains(&query) || work.genres.iter().any(|genre| fold(genre).contains(&query)));
        Ok(page_of(works, page, &filters))
    }

    fn details(_source: String, id: String) -> Result<ComicDetails, SourceError> {
        let work = work_of(catalog()?, &id)?;
        Ok(ComicDetails {
            cover_url: Some(cover_url(&work)),
            status: status_of(&work),
            id: work.id,
            title: work.title,
            synopsis: Some(work.synopsis),
            authors: vec![work.author],
            genres: work.genres,
            language: Some("pt-BR".into()),
            rating: Some(ContentRating::General),
        })
    }

    fn chapters(_source: String, id: String) -> Result<Vec<Chapter>, SourceError> {
        let work = work_of(catalog()?, &id)?;
        Ok(work
            .chapters
            .into_iter()
            .enumerate()
            .map(|(position, chapter)| Chapter {
                id: format!("{}/{}", work.id, chapter.id),
                title: chapter.title,
                number: Some(chapter.number),
                published_at: Some(chapter.published_at),
                order: position as u32,
            })
            .collect())
    }

    fn pages(_source: String, chapter_id: String) -> Result<Vec<Page>, SourceError> {
        let unknown = || SourceError::Data("Este capítulo não existe no acervo.".into());
        let (work_id, chapter) = chapter_id.split_once('/').ok_or_else(unknown)?;
        let work = work_of(catalog()?, work_id)?;
        let chapter = work.chapters.iter().find(|c| c.id == chapter).ok_or_else(unknown)?;
        Ok((0..chapter.pages).map(|index| Page { index, url: format!("{BASE_URL}/catalogo/{work_id}/{}/{:02}.png", chapter.id, index + 1), headers: vec![] }).collect())
    }
}

export_extension!(Extension);
