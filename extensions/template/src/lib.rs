//! Ponto de partida para uma extensão do Prancha: uma fonte sem obras.
//! Troque o conteúdo de cada função pelas chamadas ao site que você tem permissão de consultar.

use prancha_extension_sdk::{
    Chapter, ComicDetails, ContentKind, ContentRating, Filter, Guest, Page, ReadingDirection, SearchPage, SourceError, SourceInfo, export_extension,
};

struct Extension;

const SOURCE: &str = "exemplo";

fn nothing_found() -> SourceError {
    SourceError::Data("Esta obra não existe nesta fonte.".into())
}

impl Guest for Extension {
    fn list_sources() -> Vec<SourceInfo> {
        vec![SourceInfo {
            id: SOURCE.into(),
            name: "Fonte de exemplo".into(),
            language: "pt-BR".into(),
            description: Some("Modelo para novas extensões.".into()),
            kinds: vec![ContentKind::Comic],
            rating: ContentRating::General,
            direction: ReadingDirection::LeftToRight,
            supports_latest: true,
            supports_search: true,
            min_interval_ms: 250,
        }]
    }

    fn popular(_source: String, _page: u32, _filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        Ok(SearchPage { results: vec![], has_more: false })
    }

    fn latest(_source: String, _page: u32, _filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        Ok(SearchPage { results: vec![], has_more: false })
    }

    fn search(_source: String, _query: String, _page: u32, _filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        Ok(SearchPage { results: vec![], has_more: false })
    }

    fn details(_source: String, _id: String) -> Result<ComicDetails, SourceError> {
        Err(nothing_found())
    }

    fn chapters(_source: String, _id: String) -> Result<Vec<Chapter>, SourceError> {
        Err(nothing_found())
    }

    fn pages(_source: String, _chapter_id: String) -> Result<Vec<Page>, SourceError> {
        Err(nothing_found())
    }
}

export_extension!(Extension);
