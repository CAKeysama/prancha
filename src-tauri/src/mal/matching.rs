//! Associação entre uma obra das fontes e uma obra do MyAnimeList. O título sozinho não decide: a nota junta a
//! semelhança com todos os nomes da obra no MAL (original, inglês, japonês, sinônimos), o tipo (romances ficam de
//! fora), o autor e se a quantidade de capítulos é possível. Só uma nota alta e bem à frente da segunda associa sem
//! perguntar; o resto vira uma lista para a pessoa escolher.

use std::collections::BTreeSet;

use unicode_normalization::UnicodeNormalization;

use super::api::Manga;

/// Nota mínima para associar sozinho.
pub const AUTO_SCORE: u8 = 88;
/// Distância mínima para a segunda colocada, para associar sozinho.
pub const AUTO_MARGIN: u8 = 12;
/// Abaixo disto, a candidata não aparece como sugestão.
pub const MIN_SCORE: u8 = 35;

/// O que o Prancha sabe da obra.
#[derive(Debug, Clone, Default)]
pub struct Local {
    pub title: String,
    pub author: Option<String>,
    /// O maior número de capítulo conhecido na fonte.
    pub max_chapter: Option<i64>,
}

/// Minúsculas, sem acentos e sem pontuação, com espaços simples.
pub fn normalize(text: &str) -> String {
    let lower: String = text.nfkd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).collect::<String>().to_lowercase();
    let cleaned: String = lower.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Tira o que vem entre parênteses ou colchetes ("(Official)", "[Colorido]"), se sobrar um título.
pub fn strip_brackets(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.trim().to_string();
    if normalize(&out).is_empty() { text.trim().to_string() } else { out }
}

fn bigrams(text: &str) -> Vec<(char, char)> {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    chars.windows(2).map(|w| (w[0], w[1])).collect()
}

/// Semelhança entre 0 e 1 (coeficiente de Dice sobre pares de letras). Iguais depois de normalizar valem 1.
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (normalize(&strip_brackets(a)), normalize(&strip_brackets(b)));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let (x, mut y) = (bigrams(&a), bigrams(&b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    let total = x.len() + y.len();
    let mut common = 0;
    for pair in x {
        if let Some(i) = y.iter().position(|p| *p == pair) {
            y.swap_remove(i);
            common += 1;
        }
    }
    (2 * common) as f64 / total as f64
}

/// Tipos do MAL que não são quadrinhos.
fn is_prose(media_type: Option<&str>) -> bool {
    matches!(media_type, Some("novel" | "light_novel"))
}

fn name_tokens(text: &str) -> BTreeSet<String> {
    normalize(text).split(' ').filter(|t| t.chars().count() >= 3).map(str::to_string).collect()
}

/// Nota de 0 a 100 de uma candidata.
pub fn score(local: &Local, manga: &Manga) -> u8 {
    if is_prose(manga.media_type.as_deref()) {
        return 0;
    }
    let best = manga.titles().iter().map(|t| similarity(&local.title, t)).fold(0.0, f64::max);
    let mut points = best * 100.0;
    if let Some(author) = local.author.as_deref().filter(|a| !a.trim().is_empty()) {
        let names = manga.author_names();
        if !names.is_empty() {
            let mine = name_tokens(author);
            let theirs: BTreeSet<String> = names.iter().flat_map(|n| name_tokens(n)).collect();
            points += if mine.iter().any(|t| theirs.contains(t)) { 8.0 } else { -12.0 };
        }
    }
    // Mais capítulos na fonte do que a obra tem no MAL: é outra obra (ou um one-shot com o mesmo nome).
    if let (Some(local_max), Some(total)) = (local.max_chapter, manga.num_chapters.filter(|n| *n > 0)) {
        if local_max > total + 3 {
            points -= 25.0;
        }
    }
    points.clamp(0.0, 100.0).round() as u8
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    pub manga: Manga,
    pub score: u8,
}

/// As candidatas com nota, da melhor para a pior, sem as que não chegam ao mínimo.
pub fn rank(local: &Local, results: Vec<Manga>) -> Vec<Scored> {
    let mut scored: Vec<Scored> = results.into_iter().map(|m| Scored { score: score(local, &m), manga: m }).filter(|s| s.score >= MIN_SCORE).collect();
    scored.sort_by(|a, b| b.score.cmp(&a.score).then(a.manga.id.cmp(&b.manga.id)));
    scored
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Uma candidata clara: associa sem perguntar.
    Confident,
    /// Mais de uma possível, ou nenhuma com nota alta: a pessoa escolhe.
    Ambiguous,
    NotFound,
}

pub fn decide(ranked: &[Scored]) -> Decision {
    match ranked {
        [] => Decision::NotFound,
        [first, rest @ ..] => {
            let second = rest.first().map_or(0, |s| s.score);
            if first.score >= AUTO_SCORE && first.score.saturating_sub(second) >= AUTO_MARGIN { Decision::Confident } else { Decision::Ambiguous }
        }
    }
}

/// O texto da pesquisa no MAL: o título sem o que está entre parênteses, com no máximo 64 caracteres.
/// O MAL recusa pesquisas com menos de 3 letras.
pub fn query_for(title: &str) -> Option<String> {
    let base = normalize(&strip_brackets(title));
    let query: String = base.chars().take(64).collect::<String>().trim().to_string();
    (query.chars().filter(|c| !c.is_whitespace()).count() >= 3).then_some(query)
}

/// O identificador do MAL que a própria fonte informou, quando a descrição traz o endereço da obra.
pub fn external_id(text: &str) -> Option<i64> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("myanimelist.net/manga/")? + "myanimelist.net/manga/".len();
    let digits: String = lower[start..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|id| *id > 0)
}

/// O número do capítulo, para o MAL, que conta capítulos inteiros. "10.5" conta como 10 lidos.
/// Primeiro o campo de número da fonte, depois o título. Sem número claro, `None`: melhor não sincronizar que errar.
pub fn chapter_number(number: Option<&str>, title: &str) -> Option<i64> {
    number.and_then(extract_number).or_else(|| extract_number(title)).map(|n| n.floor() as i64).filter(|n| *n > 0)
}

fn numbers_in(text: &str) -> Vec<(usize, f64)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < bytes.len() && (bytes[i] == b'.' || bytes[i] == b',') && bytes[i + 1].is_ascii_digit() {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if let Ok(n) = text[start..i].replace(',', ".").parse::<f64>() {
                out.push((start, n));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Como `normalize`, mas guarda o ponto ou a vírgula entre dígitos ("10.5").
fn normalize_number_text(text: &str) -> String {
    let lower: Vec<char> = text.nfkd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).collect::<String>().to_lowercase().chars().collect();
    let mut out = String::new();
    for (i, c) in lower.iter().enumerate() {
        let decimal = matches!(c, '.' | ',') && i > 0 && lower[i - 1].is_ascii_digit() && lower.get(i + 1).is_some_and(char::is_ascii_digit);
        out.push(if c.is_alphanumeric() || decimal { *c } else { ' ' });
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_number(text: &str) -> Option<f64> {
    let lower = normalize_number_text(text);
    let numbers = numbers_in(&lower);
    // Depois de uma palavra de capítulo: "Vol. 2 Cap. 13" é 13.
    for keyword in ["capitulo", "chapter", "cap", "ch", "episodio", "episode", "ep"] {
        for (pos, _) in lower.match_indices(keyword) {
            let before_ok = pos == 0 || lower.as_bytes()[pos - 1] == b' ';
            let after = pos + keyword.len();
            let rest = &lower[after..];
            if before_ok && rest.starts_with(|c: char| c == ' ' || c.is_ascii_digit()) {
                if let Some((_, n)) = numbers.iter().find(|(p, _)| *p >= after && lower[after..*p].trim().is_empty()) {
                    return Some(*n);
                }
            }
        }
    }
    // Sem palavra de capítulo, só vale se houver um número.
    match numbers.as_slice() {
        [(_, n)] => Some(*n),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::api::{AltTitles, Author, AuthorNode};
    use super::*;

    fn manga(id: i64, title: &str, en: &str, kind: &str, chapters: i64, author: &str) -> Manga {
        let (first, last) = author.split_once(' ').unwrap_or((author, ""));
        Manga {
            id,
            title: title.into(),
            alternative_titles: Some(AltTitles { synonyms: vec![], en: en.into(), ja: String::new() }),
            media_type: Some(kind.into()),
            num_chapters: Some(chapters),
            status: None,
            authors: if author.is_empty() { vec![] } else { vec![Author { node: AuthorNode { first_name: first.into(), last_name: last.into() } }] },
            start_date: None,
            my_list_status: None,
        }
    }

    #[test]
    fn normaliza_acentos_e_pontuacao() {
        assert_eq!(normalize("  Kimetsu no Yaiba: Édição!! "), "kimetsu no yaiba edicao");
        assert_eq!(strip_brackets("Solo Leveling (Official) [Colorido]"), "Solo Leveling");
        assert_eq!(strip_brackets("(Oneshot)"), "(Oneshot)");
        assert_eq!(similarity("Berserk", "BERSERK"), 1.0);
        assert!(similarity("One Piece", "One Punch-Man") < 0.6);
    }

    #[test]
    fn usa_o_titulo_em_ingles_e_os_alternativos() {
        let local = Local { title: "Demon Slayer".into(), ..Default::default() };
        let m = manga(1, "Kimetsu no Yaiba", "Demon Slayer: Kimetsu no Yaiba", "manga", 205, "");
        assert!(score(&local, &m) > 50);
        let mut syn = manga(2, "Shingeki no Kyojin", "", "manga", 141, "");
        syn.alternative_titles.as_mut().unwrap().synonyms.push("Attack on Titan".into());
        assert_eq!(score(&Local { title: "Attack on Titan".into(), ..Default::default() }, &syn), 100);
    }

    #[test]
    fn romance_com_o_mesmo_nome_fica_de_fora() {
        let local = Local { title: "Overlord".into(), ..Default::default() };
        let ranked = rank(&local, vec![manga(1, "Overlord", "", "light_novel", 0, ""), manga(2, "Overlord", "", "manga", 0, "")]);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].manga.id, 2);
        assert_eq!(decide(&ranked), Decision::Confident);
    }

    #[test]
    fn titulos_iguais_sem_desempate_ficam_para_a_pessoa() {
        let local = Local { title: "Monster".into(), ..Default::default() };
        let ranked = rank(&local, vec![manga(1, "Monster", "", "manga", 162, "Naoki Urasawa"), manga(2, "Monster", "", "one_shot", 1, "Outra Pessoa")]);
        assert_eq!(decide(&ranked), Decision::Ambiguous);
    }

    #[test]
    fn autor_e_capitulos_desempatam() {
        let local = Local { title: "Monster".into(), author: Some("Urasawa Naoki".into()), max_chapter: Some(162) };
        let ranked = rank(&local, vec![manga(1, "Monster", "", "manga", 162, "Naoki Urasawa"), manga(2, "Monster", "", "one_shot", 1, "Outra Pessoa")]);
        assert_eq!(ranked[0].manga.id, 1);
        assert_eq!(decide(&ranked), Decision::Confident);
    }

    #[test]
    fn parecido_mas_nao_igual_pergunta() {
        let local = Local { title: "Tower of God".into(), ..Default::default() };
        let ranked = rank(&local, vec![manga(1, "Tower of God: Urek Mazino", "", "manhwa", 0, "")]);
        assert_ne!(decide(&ranked), Decision::Confident);
        assert_eq!(decide(&[]), Decision::NotFound);
    }

    #[test]
    fn numero_do_capitulo() {
        assert_eq!(chapter_number(Some("12"), "Capítulo 12"), Some(12));
        assert_eq!(chapter_number(Some("10.5"), ""), Some(10));
        assert_eq!(chapter_number(None, "Vol. 2 Cap. 13 - O retorno"), Some(13));
        assert_eq!(chapter_number(None, "Chapter 7: 100 dias"), Some(7));
        assert_eq!(chapter_number(Some(""), "Ch.45"), Some(45));
        assert_eq!(chapter_number(None, "Especial de Natal"), None);
        assert_eq!(chapter_number(None, "Vol. 2 Extra 3"), None, "dois números sem palavra de capítulo: ambíguo");
        assert_eq!(chapter_number(Some("0"), "Prólogo"), None);
    }

    #[test]
    fn pesquisa_e_identificador_externo() {
        assert_eq!(query_for("Solo Leveling (Official)").as_deref(), Some("solo leveling"));
        assert_eq!(query_for("IT"), None);
        assert_eq!(external_id("Mais em https://myanimelist.net/manga/2/Berserk ."), Some(2));
        assert_eq!(external_id("sem link"), None);
    }
}
