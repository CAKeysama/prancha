//! Reconhece, pelo título e pelos gêneros, obras que indicam sexualização de menores. Elas nunca aparecem,
//! com o filtro +18 ligado ou desligado. É uma lista de palavras: pega o que está escrito, não o que está na imagem.

use std::sync::LazyLock;

use unicode_normalization::UnicodeNormalization;

use crate::text::fold;

type Terms = LazyLock<Vec<String>>;

/// Rótulos que dizem tudo sozinhos, também colados a outra palavra ou separados por pontuação.
static LABELS: Terms = LazyLock::new(|| {
    compact_terms(&[
        "lolicon", "lolikon", "rorikon", "roricon", "shotacon", "shotakon", "toddlercon", "pedofil", "pedophil", "paedophil", "jailbait", "ロリコン", "ショタコン", "로리콘", "쇼타콘", "萝莉控", "蘿莉控", "正太控", "児童ポルノ", "儿童色情", "兒童色情",
    ])
});

/// No título, só como palavra inteira: "Lolita" e "LoliRock" ficam de fora.
static TITLE_WORDS: Terms = LazyLock::new(|| word_terms(&["loli", "lolis", "ninfeta", "ninfetas", "萝莉", "蘿莉"]));

/// Só nos gêneros: "Shota" também é nome de gente, e "幼女" está no título de obras comuns.
static GENRE_WORDS: Terms = LazyLock::new(|| word_terms(&["loli", "lolis", "shota", "shotas", "pedo", "underage", "minors", "ninfeta", "ninfetas", "ロリ", "ショタ", "로리", "쇼타", "萝莉", "蘿莉", "正太", "幼女"]));

/// Falam de uma criança ou adolescente e, sozinhas, não dizem nada: "Infantil" é gênero de obra para crianças.
static MINOR_WORDS: Terms = LazyLock::new(|| {
    word_terms(&[
        "crianca", "criancas", "infantil", "infantis", "menina", "meninas", "menino", "meninos", "menininha", "menininhas", "menininho", "menininhos", "garotinha", "garotinhas", "garotinho", "garotinhos", "novinha", "novinhas", "novinho", "novinhos", "colegial", "colegiais", "adolescente", "adolescentes",
        "menor de idade", "menores de idade", "child", "children", "kid", "kids", "little girl", "little girls", "little boy", "little boys", "young girl", "young girls", "young boy", "young boys", "preteen", "preteens", "pre teen", "underage", "schoolgirl", "schoolgirls", "schoolboy", "schoolboys", "toddler", "toddlers", "infant", "nina",
        "ninas", "nino", "ninos", "nena", "nenas", "colegiala", "colegialas",
    ])
});

/// Sem espaço entre as palavras nessas línguas, então vale qualquer trecho.
static MINOR_FRAGMENTS: Terms = LazyLock::new(|| compact_terms(&["幼女", "児童", "小学生", "中学生", "儿童", "兒童", "아동"]));

/// O contexto sexual explícito. "Ecchi", "Mature" e "Adulto" ficam de fora: são amplos, e "Young Adult" nem é adulto.
static SEXUAL_WORDS: Terms = LazyLock::new(|| {
    word_terms(&[
        "sexo", "sexual", "sexuais", "sex", "sexy", "hentai", "smut", "porno", "porn", "pornografia", "pornographic", "xxx", "nude", "nudes", "nudez", "estupro", "estuprada", "estuprado", "rape", "raped", "violacion", "transa", "transar", "foder", "fuck", "erotico", "erotica", "eroticos", "eroticas", "erotic", "lewd", "nsfw", "+ 18", "18 +",
    ])
});

static SEXUAL_FRAGMENTS: Terms = LazyLock::new(|| compact_terms(&["色情", "エロ", "ポルノ", "야동"]));

/// Unidades de idade depois de um número: "12 anos", "12 years", "12yo", "12歳".
static AGE_UNITS: Terms = LazyLock::new(|| ["ano", "anos", "year", "years", "yo", "歳", "才", "岁", "살"].iter().map(|unit| normalized(unit).trim().to_string()).collect());

/// Minúsculas, sem acentos e com as palavras separadas por um espaço, inclusive nas pontas.
pub(super) fn normalized(text: &str) -> String {
    let mut spaced = String::new();
    for c in fold(&text.nfkc().collect::<String>()).chars() {
        match c {
            '+' => spaced.push_str(" + "),
            c if c.is_alphanumeric() => spaced.push(c),
            _ => spaced.push(' '),
        }
    }
    let mut out = String::from(" ");
    for word in spaced.split_whitespace() {
        out.push_str(&without_lookalikes(word));
        out.push(' ');
    }
    out
}

/// "l0li" vira "loli". Número que abre a palavra continua número: "12yo" é uma idade.
fn without_lookalikes(word: &str) -> String {
    if !word.chars().next().is_some_and(char::is_alphabetic) {
        return word.to_string();
    }
    word.chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => 'i',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            other => other,
        })
        .collect()
}

fn word_terms(terms: &[&str]) -> Vec<String> {
    terms.iter().map(|term| normalized(term)).collect()
}

fn compact_terms(terms: &[&str]) -> Vec<String> {
    terms.iter().map(|term| normalized(term).replace(' ', "")).collect()
}

fn has_word(text: &str, terms: &[String]) -> bool {
    terms.iter().any(|term| text.contains(term.as_str()))
}

fn has_fragment(compact: &str, terms: &[String]) -> bool {
    terms.iter().any(|term| compact.contains(term.as_str()))
}

fn mentions_a_minor_age(title: &str) -> bool {
    let words: Vec<&str> = title.split_whitespace().collect();
    words.iter().enumerate().any(|(at, word)| {
        let (digits, unit) = word.split_at(word.find(|c: char| !c.is_ascii_digit()).unwrap_or(word.len()));
        let unit = if unit.is_empty() { words.get(at + 1).copied().unwrap_or("") } else { unit };
        digits.parse::<u32>().is_ok_and(|age| (1..18).contains(&age)) && AGE_UNITS.iter().any(|known| known == unit)
    })
}

/// O título ou os gêneros indicam sexualização de menores.
pub fn sexualizes_minors(title: &str, genres: &[String]) -> bool {
    let title = normalized(title);
    let genres = normalized(&genres.join(" | "));
    let all = format!("{title}{genres}");
    let compact = all.replace(' ', "");
    has_fragment(&compact, &LABELS)
        || has_word(&title, &TITLE_WORDS)
        || has_word(&genres, &GENRE_WORDS)
        || ((has_word(&all, &MINOR_WORDS) || has_fragment(&compact, &MINOR_FRAGMENTS) || mentions_a_minor_age(&title)) && (has_word(&all, &SEXUAL_WORDS) || has_fragment(&compact, &SEXUAL_FRAGMENTS)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_title(title: &str) -> bool {
        sexualizes_minors(title, &[])
    }

    fn in_genres(genres: &[&str]) -> bool {
        sexualizes_minors("Obra", &genres.iter().map(|genre| genre.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn labels_are_found_in_titles_and_genres_however_they_are_written() {
        for label in ["Lolicon", "LOLICON", "lolicon!", "loli-con", "l0licon", "Shotacon", "Pedofilia", "ＬＯＬＩＣＯＮ", "ロリコン", "ショタコン", "로리콘", "萝莉控", "児童ポルノ"] {
            assert!(in_title(&format!("Obra {label} 3")), "{label}");
            assert!(in_genres(&["Ação", label]), "{label}");
        }
    }

    #[test]
    fn short_labels_need_to_stand_alone() {
        for genre in ["Loli", "lolis", "Shota", "Underage", "ロリ", "ショタ", "로리", "쇼타", "幼女"] {
            assert!(in_genres(&[genre]), "{genre}");
        }
        assert!(in_title("Obra Loli 2") && in_title("l0li") && in_title("萝莉"));
    }

    #[test]
    fn a_word_about_a_minor_counts_only_next_to_an_explicit_one() {
        const MINORS: [&str; 9] = ["Criança", "Menina", "Novinha", "Colegial", "Menor de idade", "Child", "Little girl", "Schoolgirl", "Niña"];
        const EXPLICIT: [&str; 8] = ["Sexo", "Sex", "Hentai", "XXX", "Erótico", "NSFW", "+18", "18+"];
        for minor in MINORS {
            assert!(!in_title(&format!("Obra {minor}")), "{minor}");
            for explicit in EXPLICIT {
                assert!(in_title(&format!("{minor} {explicit}")), "{minor} {explicit}");
                assert!(sexualizes_minors(&format!("Obra {minor}"), &[explicit.to_string()]), "{minor} no título, {explicit} no gênero");
                assert!(sexualizes_minors(&format!("Obra {explicit}"), &[minor.to_string()]), "{explicit} no título, {minor} no gênero");
            }
        }
        assert!(sexualizes_minors("幼女", &["エロ".to_string()]));
    }

    #[test]
    fn an_age_under_18_counts_only_next_to_an_explicit_word() {
        for age in ["12 anos", "17 anos", "12 years", "12-year-old", "12yo", "12歳"] {
            assert!(!in_title(&format!("Obra de {age}")), "{age}");
            assert!(in_title(&format!("Hentai de {age}")), "{age}");
        }
        for age in ["18 anos", "30 anos", "0 anos", "12 capítulos"] {
            assert!(!in_title(&format!("Hentai de {age}")), "{age}");
        }
    }

    #[test]
    fn ordinary_works_are_not_caught() {
        for (title, genres) in [
            ("Turma da Mônica", &["Infantil", "Comédia"][..]),
            ("Kids on the Slope", &["Música", "Romance"]),
            ("Lolita", &["Drama"]),
            ("LoliRock", &["Fantasia"]),
            ("Shota no Sushi", &["Culinária"]),
            ("Meninos da Pelada", &["Esporte"]),
            ("A Menina do Outro Lado", &["Fantasia", "Young Adult"]),
            ("Sexta-feira", &["Terror"]),
            ("Essex County", &["Drama"]),
            ("12 anos de escravidão", &["Drama"]),
            ("幼女戦記", &["Fantasia"]),
            ("フロリダ", &["Ação"]),
            ("Obra Comum", &["Ecchi", "Mature"]),
            ("", &[]),
        ] {
            let genres: Vec<String> = genres.iter().map(|genre| genre.to_string()).collect();
            assert!(!sexualizes_minors(title, &genres), "{title} {genres:?}");
        }
    }
}
