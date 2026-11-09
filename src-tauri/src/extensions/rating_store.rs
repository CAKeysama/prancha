//! Onde a classificação +18 fica guardada: estado, pontuação, evidências, correções da pessoa e histórico.
//! A análise é refeita só quando os metadados relevantes, a correção ou a versão do classificador mudam.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::rating::{self, CLASSIFIER_VERSION, CoverSignal, Declared, Metadata, Origin, Params, Runtime, State};
use crate::db::now_ms;
use crate::error::{AppError, AppResult};

const MAX_REASON_CHARS: usize = 500;
const HISTORY_SHOWN: usize = 20;
/// Por transação, na análise das obras antigas: a interface não espera o banco inteiro.
const BATCH: usize = 200;

struct Loaded {
    extension_id: String,
    source_id: String,
    remote_id: String,
    meta: Metadata,
    state: String,
    version: i64,
    fingerprint: Option<String>,
    manual: Option<State>,
}

fn load(conn: &Connection, id: i64) -> AppResult<Option<Loaded>> {
    Ok(conn
        .query_row(
            "SELECT o.extension_id, o.source_id, o.remote_id, o.title, o.summary, o.genres, o.declared_rating, o.declared_adult_at, o.rating_state, o.rating_version, o.rating_fingerprint, o.updated_at,
                    s.rating, s.name, e.name,
                    CASE WHEN e.id IS NULL THEN NULL WHEN json_valid(e.entry) AND json_extract(e.entry, '$.runtime') = 'suwayomi' THEN 'bridged' ELSE 'native' END,
                    m.state, c.analyzer, c.likelihood, c.confidence, c.analyzed_at
             FROM online_series o
             LEFT JOIN extension_sources s ON s.extension_id = o.extension_id AND s.source_id = o.source_id
             LEFT JOIN installed_extensions e ON e.id = o.extension_id
             LEFT JOIN series_rating_overrides m ON m.extension_id = o.extension_id AND m.source_id = o.source_id AND m.remote_id = o.remote_id
             LEFT JOIN series_cover_analysis c ON c.series_id = o.id
             WHERE o.id = ?1",
            [id],
            |r| {
                let summary: Option<String> = r.get(4)?;
                let genres: Vec<String> = serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default();
                let source_name: Option<String> = r.get(13)?;
                let extension_name: Option<String> = r.get(14)?;
                let origin = match (source_name, extension_name) {
                    (Some(source), Some(extension)) if source != extension => format!("{source} ({extension})"),
                    (Some(source), _) => source,
                    (None, Some(extension)) => extension,
                    (None, None) => "fonte não instalada".to_string(),
                };
                let cover = match r.get::<_, Option<String>>(17)? {
                    Some(analyzer) => Some(CoverSignal { analyzer, likelihood: r.get(18)?, confidence: r.get(19)?, analyzed_at: r.get(20)? }),
                    None => None,
                };
                Ok(Loaded {
                    extension_id: r.get(0)?,
                    source_id: r.get(1)?,
                    remote_id: r.get(2)?,
                    meta: Metadata {
                        origin,
                        runtime: match r.get::<_, Option<String>>(15)?.as_deref() {
                            Some("bridged") => Some(Runtime::Bridged),
                            Some(_) => Some(Runtime::Native),
                            None => None,
                        },
                        title: r.get(3)?,
                        alt_titles: rating::alt_titles_from(summary.as_deref()),
                        description: summary,
                        genres: rating::normalize_genres(&genres),
                        series_rating: r.get::<_, Option<String>>(6)?.as_deref().and_then(Declared::parse),
                        source_rating: r.get::<_, Option<String>>(12)?.as_deref().and_then(Declared::parse),
                        declared_adult_at: r.get(7)?,
                        cover,
                        observed_at: r.get(11)?,
                    },
                    state: r.get(8)?,
                    version: r.get(9)?,
                    fingerprint: r.get(10)?,
                    manual: r.get::<_, Option<String>>(16)?.as_deref().and_then(State::parse),
                })
            },
        )
        .optional()?)
}

fn log_change(conn: &Connection, loaded: &Loaded, previous: Option<&str>, new: &str, origin: &str, reason: Option<&str>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO series_rating_history (extension_id, source_id, remote_id, previous_state, new_state, origin, reason, changed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![loaded.extension_id, loaded.source_id, loaded.remote_id, previous, new, origin, reason, now_ms()],
    )?;
    Ok(())
}

/// Analisa a obra se algo que importa mudou desde a última vez. Devolve o estado com que ela ficou.
pub fn analyze(conn: &Connection, id: i64) -> AppResult<Option<State>> {
    let Some(loaded) = load(conn, id)? else { return Ok(None) };
    let fingerprint = rating::fingerprint(&loaded.meta, loaded.manual);
    if loaded.version == CLASSIFIER_VERSION && loaded.fingerprint.as_deref() == Some(fingerprint.as_str()) {
        return Ok(State::parse(&loaded.state));
    }
    let outcome = rating::classify(&loaded.meta, &Params::DEFAULT);
    let state = loaded.manual.unwrap_or(outcome.state);
    let origin = if loaded.manual.is_some() { Origin::Manual } else { outcome.origin };
    conn.execute(
        "UPDATE online_series SET rating_state = ?2, rating_auto = ?3, rating_score = ?4, rating_origin = ?5, rating_version = ?6, rating_fingerprint = ?7, rated_at = ?8 WHERE id = ?1",
        params![id, state.as_str(), outcome.state.as_str(), outcome.score, origin.as_str(), CLASSIFIER_VERSION, fingerprint, now_ms()],
    )?;
    conn.execute("DELETE FROM series_rating_evidence WHERE series_id = ?1", [id])?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO series_rating_evidence (series_id, position, kind, origin, value, reliability, points, verdict, observed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for (position, evidence) in outcome.evidence.iter().enumerate() {
        insert.execute(params![id, position as i64, evidence.kind.as_str(), evidence.origin, evidence.value, evidence.reliability, evidence.points.round() as i64, evidence.verdict.as_str(), evidence.observed_at])?;
    }
    // A primeira análise só substitui o palpite da migração: não é uma mudança que a pessoa precise ver.
    if loaded.version > 0 && loaded.manual.is_none() && loaded.state != state.as_str() {
        log_change(conn, &loaded, Some(&loaded.state), state.as_str(), "automatic", Some("Os metadados da obra mudaram"))?;
    }
    Ok(Some(state))
}

/// Analisa até um lote das obras guardadas com uma versão anterior do classificador, ou nunca analisadas.
/// Devolve quantas analisou: zero quando não falta nenhuma. Quem chama solta o banco entre um lote e outro.
pub fn analyze_outdated_batch(conn: &mut Connection) -> AppResult<usize> {
    let ids: Vec<i64> = {
        let mut statement = conn.prepare("SELECT id FROM online_series WHERE rating_version <> ?1 ORDER BY id LIMIT ?2")?;
        let rows = statement.query_map(params![CLASSIFIER_VERSION, BATCH as i64], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let tx = conn.transaction()?;
    for id in &ids {
        analyze(&tx, *id)?;
    }
    tx.commit()?;
    Ok(ids.len())
}

/// A pessoa corrige a classificação de uma obra: `Some` fixa o estado, `None` volta ao automático.
/// A correção vale só para esta obra, pelo endereço dela, e continua valendo quando os metadados mudam.
pub fn set_manual(conn: &mut Connection, id: i64, state: Option<State>, reason: Option<&str>) -> AppResult<RatingDetails> {
    if state.is_some_and(|state| !matches!(state, State::Confirmed | State::None)) {
        return Err(AppError::user("bad_request", "Só dá para marcar como +18 ou como sem indicação de +18."));
    }
    let reason = reason.map(|text| text.trim().chars().take(MAX_REASON_CHARS).collect::<String>()).filter(|text| !text.is_empty());
    let tx = conn.transaction()?;
    let loaded = load(&tx, id)?.ok_or_else(|| AppError::user("series_not_found", "Esta obra não está mais registrada."))?;
    match state {
        Some(state) => tx.execute(
            "INSERT INTO series_rating_overrides (extension_id, source_id, remote_id, state, reason, set_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (extension_id, source_id, remote_id) DO UPDATE SET state = excluded.state, reason = excluded.reason, set_at = excluded.set_at",
            params![loaded.extension_id, loaded.source_id, loaded.remote_id, state.as_str(), reason, now_ms()],
        )?,
        None => tx.execute("DELETE FROM series_rating_overrides WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3", [&loaded.extension_id, &loaded.source_id, &loaded.remote_id])?,
    };
    let after = analyze(&tx, id)?.map_or("insufficient", State::as_str);
    if state.is_some() || loaded.manual.is_some() {
        log_change(&tx, &loaded, Some(&loaded.state), after, "manual", reason.as_deref().or(if state.is_none() { Some("Voltou à classificação automática") } else { None }))?;
    }
    tx.commit()?;
    details(conn, id)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredEvidence {
    pub kind: String,
    pub origin: String,
    pub value: String,
    pub reliability: f64,
    pub points: i64,
    pub verdict: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualRating {
    pub state: String,
    pub reason: Option<String>,
    pub set_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RatingChange {
    pub previous_state: Option<String>,
    pub new_state: String,
    pub origin: String,
    pub reason: Option<String>,
    pub changed_at: i64,
}

/// O que a tela de detalhes da classificação mostra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RatingDetails {
    pub state: String,
    /// O que o classificador concluiu, mesmo com uma correção valendo.
    pub automatic_state: String,
    pub score: i64,
    pub origin: String,
    pub rated_at: Option<i64>,
    pub classifier_version: i64,
    pub evidence: Vec<StoredEvidence>,
    pub manual: Option<ManualRating>,
    pub history: Vec<RatingChange>,
}

pub fn details(conn: &Connection, id: i64) -> AppResult<RatingDetails> {
    let row: Option<(String, String, String, String, i64, String, i64, Option<i64>)> = conn
        .query_row(
            "SELECT extension_id, source_id, remote_id, rating_state, rating_score, rating_auto, rating_version, rated_at FROM online_series WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
        )
        .optional()?;
    let (extension_id, source_id, remote_id, state, score, automatic_state, version, rated_at) = row.ok_or_else(|| AppError::user("series_not_found", "Esta obra não está mais registrada."))?;
    let origin: String = conn.query_row("SELECT rating_origin FROM online_series WHERE id = ?1", [id], |r| r.get(0))?;
    let evidence = {
        let mut statement = conn.prepare("SELECT kind, origin, value, reliability, points, verdict, observed_at FROM series_rating_evidence WHERE series_id = ?1 ORDER BY position")?;
        let rows = statement.query_map([id], |r| Ok(StoredEvidence { kind: r.get(0)?, origin: r.get(1)?, value: r.get(2)?, reliability: r.get(3)?, points: r.get(4)?, verdict: r.get(5)?, observed_at: r.get(6)? }))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let manual = conn
        .query_row(
            "SELECT state, reason, set_at FROM series_rating_overrides WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3",
            [&extension_id, &source_id, &remote_id],
            |r| Ok(ManualRating { state: r.get(0)?, reason: r.get(1)?, set_at: r.get(2)? }),
        )
        .optional()?;
    let history = {
        let mut statement = conn.prepare(
            "SELECT previous_state, new_state, origin, reason, changed_at FROM series_rating_history WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3 ORDER BY changed_at DESC, id DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(params![extension_id, source_id, remote_id, HISTORY_SHOWN as i64], |r| {
            Ok(RatingChange { previous_state: r.get(0)?, new_state: r.get(1)?, origin: r.get(2)?, reason: r.get(3)?, changed_at: r.get(4)? })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    Ok(RatingDetails { state, automatic_state, score, origin, rated_at, classifier_version: version, evidence, manual, history })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::extensions::sources;
    use crate::library;

    fn database() -> Connection {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO installed_extensions (id, repository_id, repository_name, public_key, key_fingerprint, name, version, entry, file_name, sha256, installed_at, updated_at)
                VALUES ('ponte', 'r', 'R', 'k', 'f', 'Mihon', '1.0.0', '{\"runtime\":\"suwayomi\"}', 'p', 'h', 1, 1),
                       ('nativa', 'r', 'R', 'k', 'f', 'Nativa', '1.0.0', '{\"runtime\":\"wasm-component\"}', 'n', 'h', 1, 1);
             INSERT INTO extension_sources (extension_id, source_id, name, language, rating, direction, supports_latest, supports_search)
                VALUES ('ponte', 's', 'Fonte Mihon', 'pt-BR', 'general', 'ltr', 1, 1),
                       ('nativa', 's', 'Fonte Nativa', 'pt-BR', 'general', 'ltr', 1, 1);
             INSERT INTO settings (key, value) VALUES ('plugins.online', 'true');
             INSERT INTO collections (id, name, created_at, updated_at) VALUES (1, 'Lista', 1, 1);",
        )
        .unwrap();
        conn
    }

    /// Uma obra fictícia, acompanhada e numa coleção, para aparecer em todas as telas que listam obras.
    fn series(conn: &Connection, extension: &str, remote: &str, genres: &[&str], summary: Option<&str>, declared: Option<&str>) -> i64 {
        conn.execute(
            "INSERT INTO online_series (extension_id, source_id, remote_id, title, genres, summary, declared_rating, following, classification, created_at, updated_at) VALUES (?1, 's', ?2, ?2, ?3, ?4, ?5, 1, 'known', 1, 1)",
            params![extension, remote, serde_json::to_string(genres).unwrap(), summary, declared],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute("INSERT INTO series_collections (series_id, collection_id, added_at) VALUES (?1, 1, 1)", [id]).unwrap();
        analyze(conn, id).unwrap();
        id
    }

    fn state(conn: &Connection, id: i64) -> String {
        conn.query_row("SELECT rating_state FROM online_series WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    }

    fn set(conn: &Connection, key: &str, value: &str) {
        library::set_setting(conn, key, &serde_json::Value::String(value.into())).unwrap();
    }

    fn set_bool(conn: &Connection, key: &str, value: bool) {
        library::set_setting(conn, key, &serde_json::Value::Bool(value)).unwrap();
    }

    #[test]
    fn analysis_is_stored_with_its_evidence_and_not_repeated() {
        let conn = database();
        let id = series(&conn, "ponte", "obra", &["Romance", "Hentai"], None, None);
        let found = details(&conn, id).unwrap();
        assert_eq!(found.state, "confirmed");
        assert_eq!(found.origin, "inferred");
        assert_eq!(found.classifier_version, CLASSIFIER_VERSION);
        assert!(found.score >= 80);
        assert!(found.evidence.iter().any(|e| e.kind == "tag" && e.value == "Hentai" && e.verdict == "adult" && e.origin.contains("Fonte Mihon")));
        // Sem mudança nos metadados, nada é refeito.
        conn.execute("UPDATE online_series SET rated_at = -1 WHERE id = ?1", [id]).unwrap();
        analyze(&conn, id).unwrap();
        let rated_at: i64 = conn.query_row("SELECT rated_at FROM online_series WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(rated_at, -1);
    }

    #[test]
    fn s11_a_manual_correction_survives_metadata_updates_and_the_series_leaving_the_database() {
        let mut conn = database();
        let id = series(&conn, "ponte", "obra", &["Ação"], None, None);
        assert_eq!(state(&conn, id), "none");
        let after = set_manual(&mut conn, id, Some(State::Confirmed), Some("  vi o capítulo 3  ")).unwrap();
        assert_eq!(after.state, "confirmed");
        assert_eq!(after.automatic_state, "none");
        assert_eq!(after.origin, "manual");
        assert_eq!(after.manual.as_ref().unwrap().reason.as_deref(), Some("vi o capítulo 3"));
        assert_eq!(after.history.len(), 1);
        assert_eq!((after.history[0].previous_state.as_deref(), after.history[0].new_state.as_str(), after.history[0].origin.as_str()), (Some("none"), "confirmed", "manual"));

        // Os metadados mudam: a correção continua.
        conn.execute("UPDATE online_series SET genres = '[\"Ação\",\"Comédia\"]' WHERE id = ?1", [id]).unwrap();
        analyze(&conn, id).unwrap();
        assert_eq!(state(&conn, id), "confirmed");

        // A obra sai do banco (limpeza de obras antigas) e volta numa busca: a correção vale pelo endereço.
        conn.execute("DELETE FROM online_series WHERE id = ?1", [id]).unwrap();
        let again = series(&conn, "ponte", "obra", &["Ação"], None, None);
        assert_eq!(state(&conn, again), "confirmed");
        // Só esta obra: outra com os mesmos metadados não é afetada.
        let other = series(&conn, "ponte", "outra", &["Ação"], None, None);
        assert_eq!(state(&conn, other), "none");

        let back = set_manual(&mut conn, again, None, None).unwrap();
        assert_eq!(back.state, "none");
        assert!(back.manual.is_none());
        assert_eq!(back.history[0].reason.as_deref(), Some("Voltou à classificação automática"));
        assert!(set_manual(&mut conn, again, Some(State::Suspect), None).is_err(), "só +18 ou sem indicação");
    }

    #[test]
    fn s12_the_state_follows_the_metadata_and_the_change_is_recorded() {
        let conn = database();
        let id = series(&conn, "ponte", "obra", &["Ação"], None, None);
        assert_eq!(state(&conn, id), "none");
        conn.execute("UPDATE online_series SET genres = '[\"Ação\",\"Hentai\"]' WHERE id = ?1", [id]).unwrap();
        analyze(&conn, id).unwrap();
        assert_eq!(state(&conn, id), "confirmed");
        let history = details(&conn, id).unwrap().history;
        assert_eq!(history.len(), 1);
        assert_eq!((history[0].previous_state.as_deref(), history[0].new_state.as_str(), history[0].origin.as_str()), (Some("none"), "confirmed", "automatic"));
        // Uma declaração +18 da fonte fica no histórico: se a fonte parar de informar, a obra não vira livre sozinha.
        let native = series(&conn, "nativa", "declarada", &[], None, Some("adult"));
        conn.execute("UPDATE online_series SET declared_adult_at = 5, declared_rating = NULL WHERE id = ?1", [native]).unwrap();
        analyze(&conn, native).unwrap();
        assert_eq!(state(&conn, native), "suspect");
    }

    #[test]
    fn outdated_series_are_analyzed_in_batches() {
        let mut conn = database();
        let ids: Vec<i64> = (0..5).map(|i| series(&conn, "ponte", &format!("o{i}"), &["Ação"], None, None)).collect();
        conn.execute("UPDATE online_series SET rating_version = 0, rating_state = 'insufficient'", []).unwrap();
        assert_eq!(analyze_outdated_batch(&mut conn).unwrap(), 5);
        assert_eq!(analyze_outdated_batch(&mut conn).unwrap(), 0);
        for id in ids {
            assert_eq!(state(&conn, id), "none");
        }
    }

    /// Todas as telas usam a mesma regra: Acompanhando (Explorar), biblioteca, coleções e a abertura da obra.
    #[test]
    fn every_screen_follows_the_same_preferences() {
        let conn = database();
        let none = series(&conn, "ponte", "livre", &["Ação"], None, None);
        let suspect = series(&conn, "ponte", "suspeita", &["Mature"], None, None);
        let insufficient = series(&conn, "ponte", "sem-info", &[], None, None);
        let confirmed = series(&conn, "ponte", "adulta", &["Hentai"], None, None);
        assert_eq!([state(&conn, none), state(&conn, suspect), state(&conn, insufficient), state(&conn, confirmed)], ["none", "suspect", "insufficient", "confirmed"]);

        let shown = |conn: &Connection| {
            let mut following: Vec<i64> = sources::list_following(conn).unwrap().into_iter().map(|card| card.id).collect();
            let mut library: Vec<i64> = sources::library_series(conn).unwrap().into_iter().map(|item| item.id).collect();
            following.sort();
            library.sort();
            assert_eq!(following, library, "Explorar e biblioteca concordam");
            let collection = library::list_collections(conn).unwrap()[0].comic_count;
            assert_eq!(collection as usize, library.len(), "a contagem da coleção concorda");
            for id in [none, suspect, insufficient, confirmed] {
                assert_eq!(sources::filtered(conn, id).unwrap().is_none(), library.contains(&id), "abrir a obra concorda ({id})");
            }
            library
        };

        // Padrão: filtro ligado, suspeitas escondidas.
        assert_eq!(shown(&conn), vec![none]);
        set(&conn, "content.suspect", "mark");
        assert_eq!(shown(&conn), vec![none, suspect, insufficient]);
        set(&conn, "content.suspect", "show");
        assert_eq!(shown(&conn), vec![none, suspect, insufficient]);
        // A confirmada depende só da outra chave.
        set(&conn, "content.suspect", "hide");
        set_bool(&conn, "content.adult", true);
        assert_eq!(shown(&conn), vec![none, suspect, insufficient, confirmed]);

        // O motivo da obra escondida diz o que fazer.
        set_bool(&conn, "content.adult", false);
        assert_eq!(sources::filtered(&conn, confirmed).unwrap().unwrap().code(), "source_rating");
        assert_eq!(sources::filtered(&conn, insufficient).unwrap().unwrap().code(), "source_unrated");

        // Uma correção manual muda o que aparece em todas as telas.
        let mut conn = conn;
        set_manual(&mut conn, suspect, Some(State::None), None).unwrap();
        set_manual(&mut conn, none, Some(State::Confirmed), Some("conferido")).unwrap();
        assert_eq!(shown(&conn), vec![suspect]);
        let card = sources::card(&conn, suspect).unwrap();
        assert_eq!((card.rating_state.as_str(), card.rating_origin.as_str()), ("none", "manual"));
    }
}
