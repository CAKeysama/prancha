//! Leitura do `ComicInfo.xml` (padrão ComicRack), presente em muitos CBZ/CBR.

use crate::text::clean_opt;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ComicInfo {
    pub title: Option<String>,
    pub series: Option<String>,
    pub number: Option<String>,
    pub volume: Option<String>,
    pub year: Option<i64>,
    pub writer: Option<String>,
    pub artist: Option<String>,
    pub publisher: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    /// Índice (na ordem das páginas) marcado como `FrontCover`.
    pub front_cover: Option<usize>,
}

fn split_list(s: &str) -> Vec<String> {
    s.split([',', ';']).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
}

pub fn parse(xml: &[u8]) -> Option<ComicInfo> {
    let text = String::from_utf8_lossy(xml);
    let text = text.trim_start_matches('\u{feff}');
    let doc = roxmltree::Document::parse(text).ok()?;
    let root = doc.root_element();
    if !root.tag_name().name().eq_ignore_ascii_case("ComicInfo") {
        return None;
    }
    let field = |name: &str| -> Option<String> {
        clean_opt(
            root.children()
                .find(|n| n.is_element() && n.tag_name().name() == name)
                .and_then(|n| n.text())
                .map(str::to_string),
        )
    };

    let mut artists: Vec<String> = Vec::new();
    for key in ["Penciller", "Inker", "Colorist", "CoverArtist"] {
        if let Some(v) = field(key) {
            for a in split_list(&v) {
                if !artists.iter().any(|x| x.eq_ignore_ascii_case(&a)) {
                    artists.push(a);
                }
            }
        }
    }

    let mut tags: Vec<String> = Vec::new();
    for key in ["Tags", "Genre"] {
        if let Some(v) = field(key) {
            for t in split_list(&v) {
                if !tags.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
                    tags.push(t);
                }
            }
        }
    }

    let front_cover = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Pages")
        .and_then(|pages| {
            pages.children().filter(|n| n.is_element()).find_map(|p| {
                let is_cover = p.attribute("Type").is_some_and(|t| t.eq_ignore_ascii_case("FrontCover"));
                if is_cover { p.attribute("Image").and_then(|i| i.trim().parse::<usize>().ok()) } else { None }
            })
        });

    Some(ComicInfo {
        title: field("Title"),
        series: field("Series"),
        number: field("Number"),
        volume: field("Volume"),
        year: field("Year").and_then(|y| y.parse::<i64>().ok()).filter(|y| *y > 0),
        writer: field("Writer"),
        artist: if artists.is_empty() { None } else { Some(artists.join(", ")) },
        publisher: field("Publisher"),
        summary: field("Summary"),
        tags,
        front_cover,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_fields() {
        let xml = r#"<?xml version="1.0"?>
<ComicInfo xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Title>O Retorno</Title>
  <Series>Prancha Teste</Series>
  <Number>3</Number>
  <Year>2021</Year>
  <Writer>Ana Autora</Writer>
  <Penciller>Beto Desenhista</Penciller>
  <Inker>Beto Desenhista</Inker>
  <Colorist>Cida Cor</Colorist>
  <Tags>aventura, ficção</Tags>
  <Genre>Ficção</Genre>
  <Pages>
    <Page Image="0" />
    <Page Image="1" Type="FrontCover" />
  </Pages>
</ComicInfo>"#.as_bytes();
        let info = parse(xml).unwrap();
        assert_eq!(info.title.as_deref(), Some("O Retorno"));
        assert_eq!(info.series.as_deref(), Some("Prancha Teste"));
        assert_eq!(info.number.as_deref(), Some("3"));
        assert_eq!(info.year, Some(2021));
        assert_eq!(info.writer.as_deref(), Some("Ana Autora"));
        assert_eq!(info.artist.as_deref(), Some("Beto Desenhista, Cida Cor"));
        assert_eq!(info.tags, vec!["aventura", "ficção"]);
        assert_eq!(info.front_cover, Some(1));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"not xml").is_none());
        assert!(parse(b"<Other/>").is_none());
    }
}
