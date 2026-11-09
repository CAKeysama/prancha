//! Classificação +18 por evidências. Junta o que a fonte declara, as categorias, a descrição, os títulos,
//! a capa (quando houver análise) e o histórico, cada um com a sua confiabilidade, e chega a um de quatro estados.
//!
//! Nenhuma evidência isolada fraca bloqueia uma obra, e a falta de metadados não libera nenhuma: sem nada para
//! avaliar, a obra fica em `Insufficient`, e não em `None`. Os pesos estão todos em [`Params`].
//! Esta parte não fala com o banco: recebe os metadados já normalizados e devolve o resultado.

use std::collections::HashSet;
use std::sync::LazyLock;

use serde::Serialize;

use super::minors::normalized;

/// Muda quando as regras ou os pesos mudam: as obras analisadas com uma versão anterior são analisadas de novo.
pub const CLASSIFIER_VERSION: i64 = 1;

// ---------------------------------------------------------------------------
// Parâmetros
// ---------------------------------------------------------------------------

/// Pesos e limites. Os pontos são o que a evidência vale sozinha, de 0 a 100; a confiabilidade, de 0 a 1, diz quanto
/// se pode acreditar na origem. A pontuação final é a probabilidade combinada das famílias de evidência, não uma soma.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub explicit_rating: f64,
    pub explicit_tag: f64,
    pub mature_tag: f64,
    pub suggestive_tag: f64,
    pub explicit_description: f64,
    pub suggestive_description: f64,
    pub adult_cover: f64,
    pub explicit_title_word: f64,
    /// Cada palavra ambígua tem o seu valor, entre 5 e este máximo.
    pub ambiguous_title_word_max: f64,
    pub declared_adult_history: f64,
    /// Fonte do Mihon marcada como não segura: pode ser só adulta ou mista, e não dá para saber qual.
    pub bridged_adult_source: f64,

    pub series_rating_reliability: f64,
    pub native_source_rating_reliability: f64,
    pub bridged_source_rating_reliability: f64,
    pub native_tag_reliability: f64,
    pub bridged_tag_reliability: f64,
    pub description_reliability: f64,
    pub title_reliability: f64,
    pub alt_title_reliability: f64,
    pub history_reliability: f64,
    pub safe_tag_reliability: f64,

    /// Quanto uma declaração de obra livre, confiável, reduz o que foi só deduzido de texto e imagem.
    pub safe_dampening: f64,
    pub confirmed_from: u8,
    pub suspect_from: u8,
    /// Uma evidência de uma família conta como forte a partir deste valor, já com a confiabilidade.
    pub strong_contribution: f64,
    /// Com uma declaração de obra livre contradizendo, a pontuação não passa disto: fica suspeita, nunca confirmada.
    pub contradiction_cap: u8,
}

impl Params {
    pub const DEFAULT: Params = Params {
        explicit_rating: 100.0,
        explicit_tag: 90.0,
        mature_tag: 65.0,
        suggestive_tag: 50.0,
        explicit_description: 50.0,
        suggestive_description: 25.0,
        adult_cover: 30.0,
        explicit_title_word: 30.0,
        ambiguous_title_word_max: 15.0,
        declared_adult_history: 65.0,
        bridged_adult_source: 65.0,

        series_rating_reliability: 0.95,
        native_source_rating_reliability: 0.9,
        bridged_source_rating_reliability: 0.7,
        native_tag_reliability: 0.95,
        bridged_tag_reliability: 0.9,
        description_reliability: 0.8,
        title_reliability: 1.0,
        alt_title_reliability: 0.8,
        history_reliability: 0.8,
        safe_tag_reliability: 0.7,

        safe_dampening: 0.5,
        confirmed_from: 80,
        suspect_from: 40,
        strong_contribution: 40.0,
        contradiction_cap: 79,
    };
}

impl Default for Params {
    fn default() -> Self {
        Params::DEFAULT
    }
}

// ---------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Confirmed,
    Suspect,
    None,
    Insufficient,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Confirmed => "confirmed",
            State::Suspect => "suspect",
            State::None => "none",
            State::Insufficient => "insufficient",
        }
    }

    pub fn parse(text: &str) -> Option<State> {
        Some(match text {
            "confirmed" => State::Confirmed,
            "suspect" => State::Suspect,
            "none" => State::None,
            "insufficient" => State::Insufficient,
            _ => return None,
        })
    }
}

/// Quem decidiu o estado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// A fonte declarou a classificação.
    Repository,
    /// O Prancha deduziu dos metadados.
    Inferred,
    /// A pessoa corrigiu.
    Manual,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Repository => "repository",
            Origin::Inferred => "inferred",
            Origin::Manual => "manual",
        }
    }
}

/// Uma classificação declarada, como no contrato das extensões.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Declared {
    General,
    Teen,
    Adult,
    Unknown,
}

impl Declared {
    pub fn parse(text: &str) -> Option<Declared> {
        Some(match text {
            "general" => Declared::General,
            "teen" => Declared::Teen,
            "adult" => Declared::Adult,
            "unknown" => Declared::Unknown,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Declared::General => "general",
            Declared::Teen => "teen",
            Declared::Adult => "adult",
            Declared::Unknown => "unknown",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Declared::General => "livre",
            Declared::Teen => "adolescente",
            Declared::Adult => "+18",
            Declared::Unknown => "não informada",
        }
    }
}

/// Extensão nativa (o contrato do Prancha, com classificação por fonte e por obra) ou do Mihon, pelo Suwayomi
/// (só a marca da fonte, que não vale para cada obra, e os gêneros).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Native,
    Bridged,
}

/// O resultado de um analisador de capa. Nenhum vem com o app: isto é o que um entregaria.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverSignal {
    /// Probabilidade de indício de conteúdo adulto, de 0 a 1.
    pub likelihood: f64,
    /// Quanto o próprio analisador confia na resposta, de 0 a 1.
    pub confidence: f64,
    pub analyzer: String,
    pub analyzed_at: i64,
}

/// Os metadados de uma obra num formato único, seja qual for a fonte. Campos ausentes ficam vazios ou `None`,
/// e ausência nunca vale como classificação livre.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    /// De onde vieram, em palavras: o nome da fonte e da extensão.
    pub origin: String,
    pub runtime: Option<Runtime>,
    pub title: String,
    pub alt_titles: Vec<String>,
    pub description: Option<String>,
    pub genres: Vec<String>,
    /// A classificação que a obra declarou.
    pub series_rating: Option<Declared>,
    /// A classificação da fonte. `None` quando a fonte não está mais instalada.
    pub source_rating: Option<Declared>,
    /// Quando a fonte declarou a obra como +18 pela última vez.
    pub declared_adult_at: Option<i64>,
    pub cover: Option<CoverSignal>,
    /// Quando os metadados foram obtidos.
    pub observed_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Rating,
    SourceRating,
    Tag,
    Description,
    Title,
    AltTitle,
    Cover,
    History,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Rating => "rating",
            Kind::SourceRating => "source_rating",
            Kind::Tag => "tag",
            Kind::Description => "description",
            Kind::Title => "title",
            Kind::AltTitle => "alt_title",
            Kind::Cover => "cover",
            Kind::History => "history",
        }
    }

    /// Evidências da mesma família repetem a mesma informação: só a mais forte conta.
    fn family(self) -> Family {
        match self {
            Kind::Rating | Kind::SourceRating => Family::Declared,
            Kind::Tag => Family::Tags,
            Kind::Description => Family::Description,
            Kind::Title | Kind::AltTitle => Family::Titles,
            Kind::Cover => Family::Cover,
            Kind::History => Family::History,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Family {
    Declared,
    Tags,
    Description,
    Titles,
    Cover,
    History,
}

impl Family {
    /// Texto e imagem: o que é deduzido, e não declarado.
    fn inferred(self) -> bool {
        matches!(self, Family::Description | Family::Titles | Family::Cover)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Indica conteúdo adulto de forma explícita.
    Adult,
    /// Indica, mas não confirma.
    Suggestive,
    /// Indica obra livre.
    Safe,
    /// Parece, mas não é: "Young Adult", uma negação na descrição.
    Ignored,
    /// Valeu antes, e algo mais novo e confiável contradiz.
    Superseded,
    /// A análise não chegou a uma resposta.
    Inconclusive,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Adult => "adult",
            Verdict::Suggestive => "suggestive",
            Verdict::Safe => "safe",
            Verdict::Ignored => "ignored",
            Verdict::Superseded => "superseded",
            Verdict::Inconclusive => "inconclusive",
        }
    }

    pub fn parse(text: &str) -> Verdict {
        match text {
            "adult" => Verdict::Adult,
            "suggestive" => Verdict::Suggestive,
            "safe" => Verdict::Safe,
            "superseded" => Verdict::Superseded,
            "inconclusive" => Verdict::Inconclusive,
            _ => Verdict::Ignored,
        }
    }

    fn counts(self) -> bool {
        matches!(self, Verdict::Adult | Verdict::Suggestive)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub kind: Kind,
    pub origin: String,
    pub value: String,
    pub reliability: f64,
    pub points: f64,
    pub verdict: Verdict,
    pub observed_at: i64,
}

impl Evidence {
    fn contribution(&self) -> f64 {
        if self.verdict.counts() { (self.points * self.reliability).clamp(0.0, 100.0) } else { 0.0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub state: State,
    pub score: u8,
    pub origin: Origin,
    pub evidence: Vec<Evidence>,
}

// ---------------------------------------------------------------------------
// Vocabulário
// ---------------------------------------------------------------------------

type Terms = LazyLock<Vec<String>>;

fn terms(list: &[&str]) -> Vec<String> {
    list.iter().map(|term| normalized(term)).collect()
}

fn find<'a>(text: &str, list: &'a [String]) -> Option<&'a str> {
    list.iter().find(|term| text.contains(term.as_str())).map(|term| term.trim())
}

/// Categorias que dizem, sozinhas, que a obra é adulta. Em várias línguas, comparadas como palavra inteira.
static EXPLICIT_TAGS: Terms = LazyLock::new(|| {
    terms(&[
        "hentai", "adult", "adults", "adulto", "adulta", "adultos", "adultas", "+18", "18+", "r18", "r-18", "r 18", "nsfw", "porn", "porno", "pornô", "pornografia", "pornographic", "pornográfico", "xxx", "erotica", "erótica", "erotico", "erótico", "erotic", "eroge", "explicit", "explícito", "explícita",
        "sexual content", "conteúdo sexual", "contenido sexual", "conteúdo adulto", "contenido adulto", "adult content", "adult only", "adults only", "só para adultos", "成人", "18禁", "エロ", "성인", "色情", "成年コミック",
    ])
});

/// Indicam conteúdo maduro, sem confirmar que seja sexual ou explícito.
static MATURE_TAGS: Terms = LazyLock::new(|| terms(&["mature", "maduro", "madura", "smut", "lemon", "sexual violence", "violência sexual", "violencia sexual", "erotismo", "nudity", "nudez", "desnudez"]));

/// Sugestivas: "fan service" não é conteúdo adulto, mas merece cuidado.
static SUGGESTIVE_TAGS: Terms = LazyLock::new(|| terms(&["ecchi", "fanservice", "fan service", "suggestive", "sugestivo", "sugestiva", "+16", "16+", "lewd"]));

/// Parecem adultas e não são: comparadas antes de tudo.
static NOT_ADULT_TAGS: Terms = LazyLock::new(|| terms(&["young adult", "adulto jovem", "adultos jovens", "jovem adulto", "jovens adultos", "joven adulto", "new adult", "adult life", "vida adulta", "adulting", "ya"]));

/// Declaram obra livre. Só servem para notar uma contradição e para pesar menos o que foi deduzido.
static SAFE_TAGS: Terms = LazyLock::new(|| terms(&["all ages", "livre", "para todos", "todas as idades", "para todas las edades", "kids", "kodomo", "infantil", "family", "família", "familia", "children"]));

static EXPLICIT_PHRASES: Terms = LazyLock::new(|| {
    terms(&[
        "conteúdo adulto", "conteúdo sexual", "cenas de sexo", "sexo explícito", "cenas explícitas", "conteúdo explícito", "somente para adultos", "apenas para adultos", "só para adultos", "maiores de 18", "proibido para menores",
        "adult content", "sexual content", "explicit sex", "sex scenes", "explicit scenes", "explicit content", "adults only", "only for adults", "for adults only", "mature audiences only",
        "contenido adulto", "contenido sexual", "escenas de sexo", "sexo explícito", "solo para adultos", "mayores de 18",
        "hentai", "smut", "porn", "porno", "pornô", "pornographic", "pornográfico", "xxx", "nsfw", "r18", "r-18", "+18", "18+", "erotic", "erótico", "erótica", "erotica",
    ])
});

static SUGGESTIVE_PHRASES: Terms = LazyLock::new(|| terms(&["ecchi", "fanservice", "fan service", "lewd", "sexy", "nudity", "nudez", "desnudo", "desnuda", "seduction", "sedução", "seducción"]));

/// Antes de uma expressão, desfazem o que ela diz: "sem conteúdo adulto", "no explicit content".
static NEGATIONS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    ["sem", "nao", "não", "nenhum", "nenhuma", "livre", "no", "not", "without", "free", "zero", "sin", "ningun", "ningún", "ninguna", "nada"].iter().map(|word| normalized(word).trim().to_string()).collect()
});

static EXPLICIT_TITLE_WORDS: Terms = LazyLock::new(|| terms(&["hentai", "porn", "porno", "pornô", "xxx", "nsfw", "r18", "+18", "18+", "sex", "sexo", "smut", "erotic", "erótico", "erótica", "erotica", "milf"]));

/// Palavras ambíguas no título e quanto valem. "Hot", "Kiss" e "Harém" estão em muitas obras comuns.
static AMBIGUOUS_TITLE_WORDS: LazyLock<Vec<(String, f64)>> = LazyLock::new(|| {
    [("sexy", 15.0), ("lewd", 15.0), ("ecchi", 15.0), ("safada", 15.0), ("safado", 15.0), ("naughty", 10.0), ("succubus", 10.0), ("súcubo", 10.0), ("tarado", 10.0), ("seduction", 8.0), ("sedução", 8.0), ("harem", 5.0), ("harém", 5.0), ("hot", 5.0)]
        .iter()
        .map(|(word, points)| (normalized(word), *points))
        .collect()
});

/// Rótulos que, no começo de uma linha da descrição, apresentam títulos alternativos.
static ALT_TITLE_LABELS: Terms = LazyLock::new(|| {
    terms(&["alternative title", "alternative titles", "alternative name", "alternative names", "alternative", "alt title", "alt titles", "alt name", "alt names", "associated names", "also known as", "aka", "título alternativo", "títulos alternativos", "nome alternativo", "nomes alternativos", "outros nomes", "otros nombres", "nombres alternativos", "título alternativo", "別名", "다른 이름"])
});

// ---------------------------------------------------------------------------
// Normalização
// ---------------------------------------------------------------------------

/// Gêneros como as fontes mandam, inclusive vários numa linha só ("Ação, Adulto"), sem repetição por caixa ou acento.
pub fn normalize_genres(genres: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for genre in genres.iter().flat_map(|genre| genre.split([',', ';', '|', '/', '·'])) {
        let genre = genre.trim();
        if genre.is_empty() {
            continue;
        }
        if seen.insert(normalized(genre)) {
            out.push(genre.to_string());
        }
    }
    out
}

/// Títulos alternativos que a descrição apresenta. Nenhuma fonte manda em campo próprio, e muitas põem na descrição.
pub fn alt_titles_from(description: Option<&str>) -> Vec<String> {
    let Some(description) = description else { return Vec::new() };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for line in description.lines() {
        let Some((label, rest)) = line.split_once([':', '：']) else { continue };
        let label = normalized(label);
        if !ALT_TITLE_LABELS.iter().any(|known| *known == label) {
            continue;
        }
        for title in rest.split([',', ';', '/', '|', '、']) {
            let title = title.trim();
            if !title.is_empty() && title.chars().count() <= 200 && seen.insert(normalized(title)) {
                out.push(title.to_string());
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Coleta de evidências
// ---------------------------------------------------------------------------

struct Collector<'a> {
    meta: &'a Metadata,
    out: Vec<Evidence>,
}

impl Collector<'_> {
    fn push(&mut self, kind: Kind, origin: &str, value: impl Into<String>, reliability: f64, points: f64, verdict: Verdict) {
        self.out.push(Evidence { kind, origin: origin.to_string(), value: value.into(), reliability, points, verdict, observed_at: self.meta.observed_at });
    }
}

fn negated(text: &str, term: &str) -> bool {
    let Some(at) = text.find(term) else { return false };
    let before: Vec<&str> = text[..at].split_whitespace().collect();
    before.iter().rev().take(3).any(|word| NEGATIONS.contains(*word))
}

fn ratings(c: &mut Collector, p: &Params) {
    let meta = c.meta;
    let origin = meta.origin.clone();
    match meta.series_rating {
        Some(Declared::Adult) => c.push(Kind::Rating, &origin, "A obra se declara +18", p.series_rating_reliability, p.explicit_rating, Verdict::Adult),
        Some(rating @ (Declared::General | Declared::Teen)) => c.push(Kind::Rating, &origin, format!("A obra se declara {}", rating.label()), p.series_rating_reliability, 0.0, Verdict::Safe),
        Some(Declared::Unknown) | None => {}
    }
    match (meta.source_rating, meta.runtime) {
        (Some(Declared::Adult), Some(Runtime::Native)) => c.push(Kind::SourceRating, &origin, "A fonte se declara +18", p.native_source_rating_reliability, p.explicit_tag, Verdict::Adult),
        // O Suwayomi junta fontes só adultas e mistas na mesma marca.
        (Some(Declared::Adult), _) => c.push(Kind::SourceRating, &origin, "A fonte não se declara segura", p.bridged_source_rating_reliability, p.bridged_adult_source, Verdict::Suggestive),
        (Some(rating @ (Declared::General | Declared::Teen)), Some(Runtime::Native)) => {
            c.push(Kind::SourceRating, &origin, format!("A fonte se declara {}", rating.label()), p.native_source_rating_reliability * 0.6, 0.0, Verdict::Safe)
        }
        // Fonte do Mihon marcada como segura ainda traz obras adultas: registra, mas não pesa.
        (Some(Declared::General | Declared::Teen), _) => c.push(Kind::SourceRating, &origin, "A fonte se declara segura, o que não vale para cada obra", 0.0, 0.0, Verdict::Ignored),
        _ => {}
    }
}

fn tags(c: &mut Collector, p: &Params) {
    let meta = c.meta;
    let reliability = if meta.runtime == Some(Runtime::Native) { p.native_tag_reliability } else { p.bridged_tag_reliability };
    let origin = format!("Categorias de {}", meta.origin);
    for genre in &meta.genres {
        let text = normalized(genre);
        if find(&text, &NOT_ADULT_TAGS).is_some() {
            c.push(Kind::Tag, &origin, genre.clone(), reliability, 0.0, Verdict::Ignored);
        } else if find(&text, &EXPLICIT_TAGS).is_some() {
            c.push(Kind::Tag, &origin, genre.clone(), reliability, p.explicit_tag, Verdict::Adult);
        } else if find(&text, &MATURE_TAGS).is_some() {
            c.push(Kind::Tag, &origin, genre.clone(), reliability, p.mature_tag, Verdict::Suggestive);
        } else if find(&text, &SUGGESTIVE_TAGS).is_some() {
            c.push(Kind::Tag, &origin, genre.clone(), reliability, p.suggestive_tag, Verdict::Suggestive);
        } else if find(&text, &SAFE_TAGS).is_some() {
            c.push(Kind::Tag, &origin, genre.clone(), p.safe_tag_reliability, 0.0, Verdict::Safe);
        }
    }
}

fn description(c: &mut Collector, p: &Params) {
    let Some(description) = c.meta.description.as_deref() else { return };
    let text = normalized(description);
    let origin = format!("Descrição em {}", c.meta.origin);
    let mut found = false;
    for term in EXPLICIT_PHRASES.iter().filter(|term| text.contains(term.as_str())) {
        let verdict = if negated(&text, term) { Verdict::Ignored } else { Verdict::Adult };
        found |= verdict == Verdict::Adult;
        c.push(Kind::Description, &origin, format!("“{}”", term.trim()), p.description_reliability, p.explicit_description, verdict);
    }
    if found {
        return;
    }
    if let Some(term) = SUGGESTIVE_PHRASES.iter().find(|term| text.contains(term.as_str()) && !negated(&text, term)) {
        c.push(Kind::Description, &origin, format!("“{}”", term.trim()), p.description_reliability, p.suggestive_description, Verdict::Suggestive);
    }
}

fn title(c: &mut Collector, p: &Params, kind: Kind, title: &str, reliability: f64) {
    let text = normalized(title);
    let origin = if kind == Kind::Title { "Título".to_string() } else { "Título alternativo".to_string() };
    if let Some(word) = find(&text, &EXPLICIT_TITLE_WORDS) {
        c.push(kind, &origin, format!("“{word}” em {title}"), reliability, p.explicit_title_word, Verdict::Suggestive);
        return;
    }
    let ambiguous = AMBIGUOUS_TITLE_WORDS.iter().filter(|(word, _)| text.contains(word.as_str())).max_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((word, points)) = ambiguous {
        c.push(kind, &origin, format!("“{}” em {title}", word.trim()), reliability, points.min(p.ambiguous_title_word_max), Verdict::Suggestive);
    }
}

fn cover(c: &mut Collector, p: &Params) {
    let Some(signal) = c.meta.cover.clone() else { return };
    let origin = format!("Análise da capa ({})", signal.analyzer);
    let likelihood = signal.likelihood.clamp(0.0, 1.0);
    let confidence = signal.confidence.clamp(0.0, 1.0);
    // A capa é promocional e não representa a obra inteira: no máximo um indício.
    let (verdict, points) = if likelihood >= 0.5 && confidence >= 0.5 { (Verdict::Suggestive, p.adult_cover * likelihood) } else { (Verdict::Inconclusive, 0.0) };
    c.out.push(Evidence { kind: Kind::Cover, origin, value: format!("{:.0}% de indício", likelihood * 100.0), reliability: confidence, points, verdict, observed_at: signal.analyzed_at });
}

fn history(c: &mut Collector, p: &Params) {
    let Some(at) = c.meta.declared_adult_at else { return };
    if c.meta.series_rating == Some(Declared::Adult) {
        return;
    }
    // A fonte mudou de ideia e declarou a obra livre: o que vale é o mais novo.
    let superseded = matches!(c.meta.series_rating, Some(Declared::General | Declared::Teen));
    let verdict = if superseded { Verdict::Superseded } else { Verdict::Suggestive };
    c.out.push(Evidence {
        kind: Kind::History,
        origin: c.meta.origin.clone(),
        value: "A fonte já declarou esta obra como +18".into(),
        reliability: p.history_reliability,
        points: if superseded { 0.0 } else { p.declared_adult_history },
        verdict,
        observed_at: at,
    });
}

pub fn collect(meta: &Metadata, p: &Params) -> Vec<Evidence> {
    let mut c = Collector { meta, out: Vec::new() };
    ratings(&mut c, p);
    tags(&mut c, p);
    description(&mut c, p);
    title(&mut c, p, Kind::Title, &meta.title, p.title_reliability);
    for alt in &meta.alt_titles {
        title(&mut c, p, Kind::AltTitle, alt, p.alt_title_reliability);
    }
    cover(&mut c, p);
    history(&mut c, p);
    c.out
}

// ---------------------------------------------------------------------------
// Pontuação
// ---------------------------------------------------------------------------

/// Há o bastante para dizer "sem indicação": uma classificação declarada que vale para a obra, ou categorias.
/// Uma descrição sozinha serve para achar indícios, mas não para liberar a obra.
/// Sem saber de onde a obra veio (extensão removida), ou de uma fonte que não informa a própria classificação,
/// só a declaração da própria obra libera: categorias sozinhas não bastam.
fn sufficient(meta: &Metadata) -> bool {
    if matches!(meta.series_rating, Some(Declared::General | Declared::Teen)) {
        return true;
    }
    let source_known = match meta.runtime {
        Some(Runtime::Native) => matches!(meta.source_rating, Some(Declared::General | Declared::Teen)),
        // Fonte do Mihon: a marca não vale para a obra, mas a fonte precisa estar instalada.
        Some(Runtime::Bridged) => meta.source_rating.is_some(),
        None => false,
    };
    source_known && (meta.runtime == Some(Runtime::Native) || !meta.genres.is_empty())
}

pub fn classify(meta: &Metadata, p: &Params) -> Outcome {
    let evidence = collect(meta, p);
    score(meta, evidence, p)
}

fn score(meta: &Metadata, evidence: Vec<Evidence>, p: &Params) -> Outcome {
    // Quão confiável é a declaração mais forte de obra livre. A da fonte inteira não conta: vale para a obra
    // que não declarar a sua, mas não desmente o que a própria obra mostra.
    let safe = evidence.iter().filter(|e| e.verdict == Verdict::Safe && e.kind != Kind::SourceRating).map(|e| e.reliability).fold(0.0, f64::max);
    let declared_safe = evidence.iter().any(|e| e.verdict == Verdict::Safe && e.kind == Kind::Rating);

    // A mais forte de cada família; o que é só deduzido pesa menos diante de uma declaração de obra livre.
    let mut best: Vec<(Family, f64, &Evidence)> = Vec::new();
    for e in &evidence {
        let family = e.kind.family();
        let mut value = e.contribution();
        if family.inferred() {
            value *= 1.0 - safe * p.safe_dampening;
        }
        if value <= 0.0 {
            continue;
        }
        match best.iter_mut().find(|(f, _, _)| *f == family) {
            Some(slot) if slot.1 >= value => {}
            Some(slot) => *slot = (family, value, e),
            None => best.push((family, value, e)),
        }
    }

    let combined = 100.0 * (1.0 - best.iter().fold(1.0, |rest, (_, value, _)| rest * (1.0 - value / 100.0)));
    let explicit = best.iter().any(|(_, value, e)| e.verdict == Verdict::Adult && *value >= p.confirmed_from as f64 * 0.95);
    let strong_families = best.iter().filter(|(_, value, _)| *value >= p.strong_contribution).count();
    let series_says_adult = meta.series_rating == Some(Declared::Adult);

    let mut score = combined.round().clamp(0.0, 100.0) as u8;
    // A obra se declara livre e algo forte diz o contrário: fica suspeita até alguém conferir.
    let contradicted = declared_safe && !series_says_adult && best.iter().any(|(family, _, _)| !matches!(family, Family::Declared));
    if contradicted && score > p.contradiction_cap {
        score = p.contradiction_cap;
    }

    let state = if score >= p.confirmed_from && (explicit || strong_families >= 2) {
        State::Confirmed
    } else if score >= p.suspect_from {
        State::Suspect
    } else if sufficient(meta) {
        State::None
    } else {
        State::Insufficient
    };
    // Abaixo da confirmação por falta de uma evidência explícita: a pontuação acompanha o estado.
    let score = if state == State::Suspect { score.min(p.confirmed_from - 1) } else { score };

    let top = best.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|(family, _, _)| *family);
    let origin = match (state, top) {
        (State::Confirmed | State::Suspect, Some(Family::Declared)) => Origin::Repository,
        (State::None, _) if declared_safe => Origin::Repository,
        _ => Origin::Inferred,
    };
    Outcome { state, score, origin, evidence }
}

/// Um resumo do que a análise usa, para não repetir a análise quando nada mudou.
pub fn fingerprint(meta: &Metadata, manual: Option<State>) -> String {
    let mut hasher = blake3::Hasher::new();
    let mut put = |text: &str| {
        hasher.update(text.as_bytes());
        hasher.update(&[0]);
    };
    put(&CLASSIFIER_VERSION.to_string());
    put(&meta.origin);
    put(match meta.runtime {
        Some(Runtime::Native) => "native",
        Some(Runtime::Bridged) => "bridged",
        None => "-",
    });
    put(&meta.title);
    for alt in &meta.alt_titles {
        put(alt);
    }
    put("|");
    put(meta.description.as_deref().unwrap_or("-"));
    for genre in &meta.genres {
        put(genre);
    }
    put("|");
    put(meta.series_rating.map_or("-", Declared::as_str));
    put(meta.source_rating.map_or("-", Declared::as_str));
    put(&meta.declared_adult_at.is_some().to_string());
    if let Some(cover) = &meta.cover {
        put(&format!("{}:{:.3}:{:.3}:{}", cover.analyzer, cover.likelihood, cover.confidence, cover.analyzed_at));
    }
    put(manual.map_or("-", State::as_str));
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(title: &str) -> Metadata {
        Metadata { origin: "Fonte Teste".into(), runtime: Some(Runtime::Bridged), source_rating: Some(Declared::General), title: title.into(), observed_at: 1, ..Default::default() }
    }

    fn with_genres(mut m: Metadata, genres: &[&str]) -> Metadata {
        m.genres = genres.iter().map(|g| g.to_string()).collect();
        m
    }

    fn run(m: &Metadata) -> Outcome {
        classify(m, &Params::DEFAULT)
    }

    // --- Cenários pedidos -------------------------------------------------

    #[test]
    fn s01_explicit_rating_confirms_and_comes_from_the_repository() {
        let mut m = meta("Obra Qualquer");
        m.runtime = Some(Runtime::Native);
        m.source_rating = Some(Declared::General);
        m.series_rating = Some(Declared::Adult);
        let out = run(&m);
        assert_eq!(out.state, State::Confirmed);
        assert!(out.score >= 95, "{}", out.score);
        assert_eq!(out.origin, Origin::Repository);
    }

    #[test]
    fn s02_hentai_tag_confirms() {
        let out = run(&with_genres(meta("Obra"), &["Comédia", "Hentai"]));
        assert_eq!(out.state, State::Confirmed);
        assert_eq!(out.origin, Origin::Inferred);
    }

    #[test]
    fn s03_mature_tag_alone_is_suspect_not_confirmed() {
        let out = run(&with_genres(meta("Obra"), &["Drama", "Mature"]));
        assert_eq!(out.state, State::Suspect);
        assert!((40..80).contains(&out.score), "{}", out.score);
    }

    #[test]
    fn s04_an_ambiguous_title_word_does_not_block() {
        for title in ["Sexy Commando Gaiden", "Hot Summer Days", "Harem in the Labyrinth", "A Kiss and a Succubus"] {
            let out = run(&with_genres(meta(title), &["Comédia"]));
            assert_eq!(out.state, State::None, "{title}: {out:?}");
            assert!(out.score < 40);
        }
        // Até uma palavra explícita no título, sozinha, é só indício: há comédias com "Hentai" no nome.
        let out = run(&with_genres(meta("Hentai Prince and the Stony Cat"), &["Romance", "Comédia"]));
        assert_eq!(out.state, State::None, "{out:?}");
    }

    #[test]
    fn s05_without_categories_the_description_still_counts() {
        let mut m = meta("Noites em Claro");
        m.description = Some("Uma história de romance com cenas de sexo explícito e conteúdo adulto.".into());
        let out = run(&m);
        assert_eq!(out.state, State::Suspect, "{out:?}");
        // Sem categorias e sem nada suspeito na descrição, não dá para liberar.
        let mut m = meta("Aventuras no Mar");
        m.description = Some("Um grupo de piratas procura um tesouro.".into());
        assert_eq!(run(&m).state, State::Insufficient);
    }

    #[test]
    fn s06_without_description_the_categories_decide() {
        assert_eq!(run(&with_genres(meta("Obra"), &["Ação", "Aventura"])).state, State::None);
        assert_eq!(run(&with_genres(meta("Obra"), &["Ação", "Adult"])).state, State::Confirmed);
    }

    #[test]
    fn s07_no_suggestive_title_and_no_categories_is_insufficient() {
        let out = run(&meta("Crônicas da Cidade"));
        assert_eq!(out.state, State::Insufficient);
        assert_eq!(out.score, 0);
    }

    #[test]
    fn s08_contradictory_signals_stay_suspect() {
        // A obra se declara livre, e a categoria diz adulto.
        let mut m = with_genres(meta("Obra"), &["Adult"]);
        m.runtime = Some(Runtime::Native);
        m.series_rating = Some(Declared::General);
        let out = run(&m);
        assert_eq!(out.state, State::Suspect, "{out:?}");
        assert!(out.score <= 79);
        // Categorias que se contradizem.
        let out = run(&with_genres(meta("Obra"), &["All Ages", "Adult"]));
        assert_eq!(out.state, State::Confirmed, "uma categoria livre não apaga uma explícita: {out:?}");
        let out = run(&with_genres(meta("Obra"), &["All Ages", "Ecchi"]));
        assert_eq!(out.state, State::Suspect, "{out:?}");
    }

    #[test]
    fn s09_metadata_from_different_repositories_reach_the_same_answer() {
        // Nativa: classificação na obra, sem gêneros na lista.
        let mut native = meta("Obra");
        native.runtime = Some(Runtime::Native);
        native.source_rating = Some(Declared::General);
        native.series_rating = Some(Declared::Adult);
        // Mihon: só gêneros, em outra língua e caixa.
        let bridged = with_genres(meta("Obra"), &["ADULTO", "romance"]);
        assert_eq!(run(&native).state, State::Confirmed);
        assert_eq!(run(&bridged).state, State::Confirmed);
        // Nativa livre sem gêneros: a classificação da fonte vale como informação.
        let mut free = meta("Obra");
        free.runtime = Some(Runtime::Native);
        free.source_rating = Some(Declared::General);
        assert_eq!(run(&free).state, State::None);
        // Mihon com a fonte marcada como segura e nada mais: a marca não vale para a obra.
        let mut safe_source = meta("Obra");
        safe_source.source_rating = Some(Declared::General);
        assert_eq!(run(&safe_source).state, State::Insufficient);
        // Fonte nativa adulta: tudo nela é adulto.
        let mut adult_source = meta("Obra");
        adult_source.runtime = Some(Runtime::Native);
        adult_source.source_rating = Some(Declared::Adult);
        assert_eq!(run(&adult_source).state, State::Confirmed);
        // Fonte do Mihon não segura: pode ser mista, fica suspeita.
        let mut mixed = with_genres(meta("Obra"), &["Ação"]);
        mixed.source_rating = Some(Declared::Adult);
        assert_eq!(run(&mixed).state, State::Suspect);
    }

    #[test]
    fn s10_alternative_titles_are_read_from_the_description() {
        let description = "Uma obra.\nAlternative Titles: 成人の恋, Otona no Koi; Grown-up Love\nAutor: Fulano";
        assert_eq!(alt_titles_from(Some(description)), vec!["成人の恋", "Otona no Koi", "Grown-up Love"]);
        let mut m = with_genres(meta("Otona no Koi"), &["Romance"]);
        m.alt_titles = vec!["Hentai no Koi".into(), "Sexy Love".into()];
        let out = run(&m);
        // Uma família só: os dois títulos dizem a mesma coisa.
        assert_eq!(out.evidence.iter().filter(|e| e.kind == Kind::AltTitle).count(), 2);
        assert_eq!(out.state, State::None, "{out:?}");
        m.genres.push("Smut".into());
        assert_eq!(run(&m).state, State::Suspect);
    }

    #[test]
    fn s13_nothing_at_all_is_insufficient_never_free() {
        let out = run(&Metadata::default());
        assert_eq!(out.state, State::Insufficient);
        let mut m = meta("");
        m.source_rating = Some(Declared::Unknown);
        m.series_rating = Some(Declared::Unknown);
        assert_eq!(run(&m).state, State::Insufficient);
    }

    #[test]
    fn an_unknown_origin_never_frees_a_series_on_categories_alone() {
        // Fonte nativa que não informa a própria classificação.
        let mut m = with_genres(meta("Obra"), &["Romance"]);
        m.runtime = Some(Runtime::Native);
        m.source_rating = Some(Declared::Unknown);
        assert_eq!(run(&m).state, State::Insufficient);
        m.series_rating = Some(Declared::General);
        assert_eq!(run(&m).state, State::None, "a obra declarou");
        // Extensão removida: não se sabe mais de onde veio.
        let mut gone = with_genres(meta("Obra"), &["Romance"]);
        gone.runtime = None;
        gone.source_rating = None;
        assert_eq!(run(&gone).state, State::Insufficient);
        gone.declared_adult_at = Some(1);
        assert_eq!(run(&gone).state, State::Suspect, "o histórico continua valendo");
    }

    #[test]
    fn s14_inconclusive_cover_keeps_the_state_and_a_suggestive_one_never_confirms_alone() {
        let signal = |likelihood, confidence| CoverSignal { likelihood, confidence, analyzer: "teste".into(), analyzed_at: 5 };
        let mut m = with_genres(meta("Obra"), &["Mature"]);
        m.cover = Some(signal(0.5, 0.3));
        let out = run(&m);
        assert_eq!(out.state, State::Suspect);
        assert!(out.evidence.iter().any(|e| e.kind == Kind::Cover && e.verdict == Verdict::Inconclusive));
        let mut alone = with_genres(meta("Obra"), &["Ação"]);
        alone.cover = Some(signal(1.0, 1.0));
        assert_eq!(run(&alone).state, State::None, "a capa sozinha não classifica");
        m.cover = Some(signal(1.0, 1.0));
        assert_eq!(run(&m).state, State::Suspect, "capa e Mature: indícios, sem confirmação");
    }

    // --- Normalização e regras --------------------------------------------

    #[test]
    fn young_adult_and_similar_are_not_adult() {
        for genre in ["Young Adult", "young-adult", "Adulto Jovem", "New Adult", "Vida Adulta"] {
            let out = run(&with_genres(meta("Obra"), &[genre]));
            assert_eq!(out.state, State::None, "{genre}: {out:?}");
            assert!(out.evidence.iter().any(|e| e.verdict == Verdict::Ignored), "{genre}");
        }
    }

    #[test]
    fn tags_in_other_languages_and_cases_are_recognized() {
        for genre in ["ADULT", "adulto", "Adulta", "Conteúdo +18", "18+", "R-18", "r18", "Pornográfico", "成人", "Erótico", "NSFW"] {
            assert_eq!(run(&with_genres(meta("Obra"), &[genre])).state, State::Confirmed, "{genre}");
        }
        for genre in ["Ecchi", "Smut", "Madura", "Lemon"] {
            assert_eq!(run(&with_genres(meta("Obra"), &[genre])).state, State::Suspect, "{genre}");
        }
    }

    #[test]
    fn genres_are_split_and_deduplicated() {
        let genres = normalize_genres(&["Ação, Adulto".into(), "ação".into(), " ".into(), "Romance/Drama".into()]);
        assert_eq!(genres, vec!["Ação", "Adulto", "Romance", "Drama"]);
    }

    #[test]
    fn repeated_evidence_does_not_add_up() {
        let one = run(&with_genres(meta("Obra"), &["Mature"]));
        let many = run(&with_genres(meta("Obra"), &["Mature", "Madura", "Smut", "Lemon"]));
        assert_eq!(one.score, many.score);
        assert_eq!(many.state, State::Suspect);
    }

    #[test]
    fn independent_evidence_combines_without_an_unbounded_sum() {
        let mut m = with_genres(meta("Obra"), &["Mature"]);
        m.description = Some("Contém cenas de sexo e conteúdo adulto.".into());
        let out = run(&m);
        // Duas famílias fortes, mas nenhuma explícita e a soma ainda abaixo de 80: suspeita alta.
        assert_eq!(out.state, State::Suspect, "{out:?}");
        assert!(out.score > 70);
        // Com o histórico de uma declaração +18, são três fontes independentes: confirma.
        m.declared_adult_at = Some(1);
        assert_eq!(run(&m).state, State::Confirmed);
    }

    #[test]
    fn a_negated_phrase_is_ignored() {
        let mut m = with_genres(meta("Obra"), &["Romance"]);
        m.description = Some("Um romance leve, sem conteúdo adulto, para toda a família.".into());
        let out = run(&m);
        assert_eq!(out.state, State::None, "{out:?}");
        assert!(out.evidence.iter().any(|e| e.kind == Kind::Description && e.verdict == Verdict::Ignored));
    }

    #[test]
    fn an_explicit_safe_rating_outweighs_inferences() {
        let mut m = meta("Sexy Nights");
        m.runtime = Some(Runtime::Native);
        m.series_rating = Some(Declared::Teen);
        m.description = Some("Fan service e muita comédia.".into());
        let out = run(&m);
        assert_eq!(out.state, State::None, "{out:?}");
        assert_eq!(out.origin, Origin::Repository);
    }

    #[test]
    fn history_keeps_a_once_declared_work_suspect_until_the_source_says_otherwise() {
        let mut m = with_genres(meta("Obra"), &["Romance"]);
        m.runtime = Some(Runtime::Native);
        m.declared_adult_at = Some(10);
        assert_eq!(run(&m).state, State::Suspect, "a fonte deixou de informar: não vira livre sozinha");
        m.series_rating = Some(Declared::General);
        let out = run(&m);
        assert_eq!(out.state, State::None);
        assert!(out.evidence.iter().any(|e| e.kind == Kind::History && e.verdict == Verdict::Superseded));
    }

    #[test]
    fn scores_are_bounded() {
        let mut m = with_genres(meta("Hentai XXX"), &["Hentai", "Adult", "Smut", "Mature"]);
        m.runtime = Some(Runtime::Native);
        m.series_rating = Some(Declared::Adult);
        m.source_rating = Some(Declared::Adult);
        m.description = Some("hentai, porn, nsfw, conteúdo adulto".into());
        m.declared_adult_at = Some(1);
        let out = run(&m);
        assert_eq!(out.state, State::Confirmed);
        assert!(out.score <= 100);
    }

    #[test]
    fn parameters_change_the_outcome_without_touching_the_rules() {
        let m = with_genres(meta("Obra"), &["Mature"]);
        let strict = Params { mature_tag: 95.0, ..Params::DEFAULT };
        assert_eq!(classify(&m, &strict).state, State::Suspect, "sem evidência explícita nem duas fortes, não confirma");
        let lenient = Params { suspect_from: 60, ..Params::DEFAULT };
        assert_eq!(classify(&m, &lenient).state, State::None);
    }

    #[test]
    fn the_fingerprint_follows_relevant_metadata_only() {
        let m = with_genres(meta("Obra"), &["Ação"]);
        let mut later = m.clone();
        later.observed_at = 99;
        assert_eq!(fingerprint(&m, None), fingerprint(&later, None));
        later.genres.push("Adult".into());
        assert_ne!(fingerprint(&m, None), fingerprint(&later, None));
        assert_ne!(fingerprint(&m, None), fingerprint(&m, Some(State::None)));
    }

    // --- Métricas ---------------------------------------------------------

    /// Uma base fictícia, com a resposta certa ao lado. Precisão e recall medem os dois tipos de erro:
    /// o objetivo não é bloquear mais, é errar menos.
    #[test]
    fn precision_and_recall_on_the_controlled_set() {
        struct Case {
            meta: Metadata,
            adult: bool,
        }
        let native = |title: &str, rating: Option<Declared>, genres: &[&str]| {
            let mut m = with_genres(meta(title), genres);
            m.runtime = Some(Runtime::Native);
            m.source_rating = Some(Declared::General);
            m.series_rating = rating;
            m
        };
        let described = |title: &str, genres: &[&str], description: &str| {
            let mut m = with_genres(meta(title), genres);
            m.description = Some(description.into());
            m
        };
        let cases = vec![
            Case { meta: native("A", Some(Declared::Adult), &[]), adult: true },
            Case { meta: with_genres(meta("B"), &["Hentai"]), adult: true },
            Case { meta: with_genres(meta("C"), &["Adulto", "Romance"]), adult: true },
            Case { meta: with_genres(meta("D"), &["Erotica"]), adult: true },
            Case { meta: described("E", &["Romance"], "Contém cenas de sexo explícito. Conteúdo adulto."), adult: true },
            Case { meta: described("F", &["Mature"], "Somente para adultos."), adult: true },
            Case { meta: with_genres(meta("G"), &["+18", "Drama"]), adult: true },
            Case { meta: native("H", Some(Declared::General), &["Ação"]), adult: false },
            Case { meta: with_genres(meta("Sexy Commando"), &["Comédia"]), adult: false },
            Case { meta: with_genres(meta("Young Love"), &["Young Adult", "Romance"]), adult: false },
            Case { meta: with_genres(meta("Hentai Prince"), &["Romance", "Escolar"]), adult: false },
            Case { meta: described("I", &["Aventura"], "Sem conteúdo adulto, para toda a família."), adult: false },
            Case { meta: with_genres(meta("Hot Dog Heroes"), &["Culinária"]), adult: false },
            Case { meta: native("J", None, &["Fantasia"]), adult: false },
        ];
        // Duas leituras: só a confirmação como positiva, e também a suspeita (que a preferência padrão esconde).
        let measure = |positive: &dyn Fn(State) -> bool| {
            let (mut tp, mut fp, mut fn_) = (0, 0, 0);
            for case in &cases {
                match (positive(run(&case.meta).state), case.adult) {
                    (true, true) => tp += 1,
                    (true, false) => fp += 1,
                    (false, true) => fn_ += 1,
                    (false, false) => {}
                }
            }
            (tp as f64 / (tp + fp).max(1) as f64, tp as f64 / (tp + fn_).max(1) as f64, fp, fn_)
        };
        let (precision, recall, fp, fn_) = measure(&|state| state == State::Confirmed);
        println!("só confirmadas: precisão {precision:.2}, recall {recall:.2}, falsos positivos {fp}, falsos negativos {fn_}");
        assert_eq!(fp, 0, "nenhuma obra comum confirmada como +18");
        assert!(recall >= 0.7, "recall {recall}");
        let (precision, recall, fp, fn_) = measure(&|state| matches!(state, State::Confirmed | State::Suspect));
        println!("confirmadas e suspeitas: precisão {precision:.2}, recall {recall:.2}, falsos positivos {fp}, falsos negativos {fn_}");
        assert_eq!(fp, 0, "nenhuma obra comum nem como suspeita");
        assert_eq!(fn_, 0, "com a preferência padrão, nenhuma obra adulta passa");
    }
}
