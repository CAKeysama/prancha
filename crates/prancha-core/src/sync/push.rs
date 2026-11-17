//! O que sobe para a nuvem: as obras marcadas pelos gatilhos (`sync_dirty`), as pastas, o catálogo e as remoções. Na
//! primeira vez (`full_push`), tudo o que está na biblioteca. A nuvem decide os conflitos pelas datas, então subir de
//! novo algo que ela já tem não muda nada.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value, json};

use super::{MAX_BATCH, MAX_BATCH_BYTES, Remote, apply, chapter_key, series_key};
use crate::account::CallError;
use crate::db::now_ms;
use crate::error::AppResult;

const MAX_TITLE: usize = 512;
const MAX_URL: usize = 2048;

/// O que sai num envio, na ordem em que a nuvem precisa: a obra antes dos capítulos dela, a pasta antes das obras.
#[derive(Default)]
struct Outgoing {
    repositories: Vec<Value>,
    extensions: Vec<Value>,
    collections: Vec<Value>,
    series: Vec<Value>,
    chapters: Vec<Value>,
    members: Vec<Value>,
    mal: Vec<Value>,
    /// Remoções de obras e pastas que podem ser esquecidas depois do envio.
    series_gone: Vec<(String, String, String)>,
    collections_gone: Vec<String>,
}

impl Outgoing {
    fn into_changes(self) -> Vec<Value> {
        [self.repositories, self.extensions, self.collections, self.series, self.chapters, self.members, self.mal].into_iter().flatten().collect()
    }
}

/// Os envios em lotes abaixo dos limites da nuvem, na mesma ordem.
pub(super) fn batches(changes: Vec<Value>) -> Vec<Vec<Value>> {
    let mut out: Vec<Vec<Value>> = Vec::new();
    let mut size = 0;
    for change in changes {
        let bytes = change.to_string().len() + 1;
        if out.last().is_none_or(|batch| batch.len() >= MAX_BATCH || size + bytes > MAX_BATCH_BYTES) {
            out.push(Vec::new());
            size = 0;
        }
        size += bytes;
        out.last_mut().expect("lote recém-criado").push(change);
    }
    out
}

pub(super) fn send(remote: &dyn Remote, conn: &mut Connection, device: &Value) -> Result<usize, CallError> {
    let full: bool = conn.query_row("SELECT full_push = 1 FROM sync_state WHERE id = 1", [], |r| r.get(0))?;
    // A fila sai do banco antes de ler os dados: uma mudança que chegar durante o envio volta para a fila.
    let taken: Vec<(String, i64)> = {
        let tx = conn.transaction()?;
        let taken = {
            let mut statement = tx.prepare("SELECT kind, ref FROM sync_dirty")?;
            let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        tx.execute("DELETE FROM sync_dirty", [])?;
        tx.commit()?;
        taken
    };
    let restore = |conn: &Connection| {
        for (kind, id) in &taken {
            let _ = conn.execute("INSERT OR IGNORE INTO sync_dirty (kind, ref) VALUES (?1, ?2)", params![kind, id]);
        }
    };
    let outgoing = match collect(conn, full, &taken) {
        Ok(outgoing) => outgoing,
        Err(e) => {
            restore(conn);
            return Err(e.into());
        }
    };
    let (series_gone, collections_gone) = (outgoing.series_gone.clone(), outgoing.collections_gone.clone());
    let changes = outgoing.into_changes();
    let count = changes.len();
    for batch in batches(changes) {
        if let Err(e) = remote.push(device, &batch) {
            restore(conn);
            return Err(e);
        }
    }
    let tx = conn.transaction()?;
    for (extension, source, remote_id) in &series_gone {
        tx.execute("DELETE FROM sync_series_gone WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3", [extension, source, remote_id])?;
    }
    for sync_id in &collections_gone {
        tx.execute("DELETE FROM sync_collection_gone WHERE sync_id = ?1", [sync_id])?;
    }
    tx.execute("UPDATE sync_state SET full_push = 0 WHERE id = 1", [])?;
    tx.commit()?;
    Ok(count)
}

/// O que a nuvem tem no espelho, por chave.
fn mirror(conn: &Connection, kind: &str) -> AppResult<HashMap<String, Value>> {
    let mut statement = conn.prepare("SELECT key, data FROM sync_remote_items WHERE kind = ?1")?;
    let rows = statement.query_map([kind], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (key, data) = row?;
        out.insert(key, serde_json::from_str(&data).unwrap_or(Value::Null));
    }
    Ok(out)
}

/// A obra está na nuvem e não foi removida.
fn present_remotely(conn: &Connection, key: &str) -> AppResult<bool> {
    let data: Option<String> = conn.query_row("SELECT data FROM sync_remote_series WHERE series_key = ?1", [key], |r| r.get(0)).optional()?;
    Ok(data.and_then(|d| serde_json::from_str::<Value>(&d).ok()).is_some_and(|d| d["removed"].as_bool() == Some(false)))
}

fn collect(conn: &Connection, full: bool, taken: &[(String, i64)]) -> AppResult<Outgoing> {
    let mut out = Outgoing::default();
    let dirty = |kind: &str| full || taken.iter().any(|(k, _)| k == kind);
    if dirty("catalog") {
        catalog(conn, &mut out)?;
    }
    if dirty("collections") {
        collections(conn, &mut out)?;
    }

    // Só extensões do Prancha: as obras do Mihon dependem do banco do Suwayomi de cada PC.
    let syncable: HashSet<String> = {
        let mut statement = conn.prepare("SELECT id FROM installed_extensions WHERE coalesce(json_extract(entry, '$.runtime'), '') <> ?1")?;
        let rows = statement.query_map([crate::extensions::suwayomi::RUNTIME], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let ids: Vec<i64> = if full {
        let mut statement = conn.prepare(&format!("SELECT o.id FROM online_series o WHERE {}", in_library("o")))?;
        let rows = statement.query_map([], |r| r.get(0))?;
        let mut ids: Vec<i64> = rows.collect::<Result<_, _>>()?;
        ids.extend(taken.iter().filter(|(k, _)| k == "series").map(|(_, id)| *id));
        ids.sort_unstable();
        ids.dedup();
        ids
    } else {
        taken.iter().filter(|(k, _)| k == "series").map(|(_, id)| *id).collect()
    };
    for id in ids {
        series(conn, id, &syncable, &mut out)?;
    }

    // Obras que saíram do banco: a remoção só sobe se a nuvem conhece a obra e ela não voltou a este aparelho.
    let gone: Vec<(String, String, String, i64)> = {
        let mut statement = conn.prepare(
            "SELECT g.extension_id, g.source_id, g.remote_id, g.removed_at FROM sync_series_gone g
              WHERE NOT EXISTS (SELECT 1 FROM online_series o WHERE o.extension_id = g.extension_id AND o.source_id = g.source_id AND o.remote_id = g.remote_id)",
        )?;
        let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let all_gone: Vec<(String, String, String)> = {
        let mut statement = conn.prepare("SELECT extension_id, source_id, remote_id FROM sync_series_gone")?;
        let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (extension, source, remote, removed_at) in gone {
        let key = series_key(&extension, &source, &remote);
        if present_remotely(conn, &key)? {
            out.series.push(json!({ "t": "series", "key": key, "extension_id": extension, "source_id": source, "item_key": remote, "removed": true, "removed_at": removed_at }));
        }
    }
    out.series_gone = all_gone;
    Ok(out)
}

/// A mesma regra de `sources::library_series`: o que mantém uma obra na biblioteca.
fn in_library(o: &str) -> String {
    format!(
        "({o}.favorite = 1 OR {o}.following = 1
          OR EXISTS (SELECT 1 FROM series_collections x WHERE x.series_id = {o}.id)
          OR EXISTS (SELECT 1 FROM online_chapters c JOIN chapter_progress q ON q.chapter_id = c.id WHERE c.series_id = {o}.id)
          OR EXISTS (SELECT 1 FROM online_chapters c WHERE c.series_id = {o}.id AND (c.reading_started_at IS NOT NULL OR c.completed_at IS NOT NULL)))"
    )
}

fn clip(text: Option<String>, limit: usize) -> Option<String> {
    text.map(|t| t.chars().take(limit).collect::<String>()).filter(|t| !t.trim().is_empty())
}

fn series(conn: &Connection, id: i64, syncable: &HashSet<String>, out: &mut Outgoing) -> AppResult<()> {
    type Row = (String, String, String, String, Option<String>, bool, bool, String, Option<String>, i64, i64, bool);
    type Meta = (Option<i64>, Option<i64>, Option<i64>, Option<i64>);
    let Some(((extension, source, remote, title, cover, following, favorite, sort, mode, created_at, updated_at, listed), (following_at, favorite_at, sort_at, mode_at))): Option<(Row, Meta)> = conn
        .query_row(
            &format!(
                "SELECT o.extension_id, o.source_id, o.remote_id, o.title, o.cover_url, o.following, o.favorite, o.chapter_sort, o.reader_mode, o.created_at, o.updated_at,
                        {}, m.following_at, m.favorite_at, m.chapter_sort_at, m.reader_mode_at
                 FROM online_series o LEFT JOIN sync_series_meta m ON m.series_id = o.id WHERE o.id = ?1",
                in_library("o")
            ),
            [id],
            |r| {
                Ok((
                    (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?),
                    (r.get(12)?, r.get(13)?, r.get(14)?, r.get(15)?),
                ))
            },
        )
        .optional()?
    else {
        return Ok(());
    };
    if !syncable.contains(&extension) {
        return Ok(());
    }
    let key = series_key(&extension, &source, &remote);
    if !listed {
        // Saiu da biblioteca por algo feito aqui: a saída sobe se a nuvem ainda tem a obra.
        if present_remotely(conn, &key)? {
            out.series.push(json!({ "t": "series", "key": key, "extension_id": extension, "source_id": source, "item_key": remote, "removed": true, "removed_at": now_ms() }));
        }
        return Ok(());
    }

    let mut change = Map::new();
    change.insert("t".into(), json!("series"));
    change.insert("key".into(), json!(key));
    change.insert("extension_id".into(), json!(extension));
    change.insert("source_id".into(), json!(source));
    change.insert("item_key".into(), json!(remote));
    if let Some(title) = clip(Some(title), MAX_TITLE) {
        change.insert("title".into(), json!(title));
        change.insert("cover_url".into(), json!(cover.filter(|c| c.len() <= MAX_URL)));
        change.insert("meta_at".into(), json!(updated_at));
    }
    // Sem data anotada, só sobe um valor diferente do padrão, com a data em que a obra entrou no aparelho.
    let mut field = |name: &str, value: Value, at: Option<i64>, customized: bool| {
        if let Some(at) = at.or(customized.then_some(created_at)) {
            change.insert(name.into(), value);
            change.insert(format!("{name}_at"), json!(at));
        }
    };
    field("following", json!(following), following_at, following);
    field("favorite", json!(favorite), favorite_at, favorite);
    field("chapter_sort", json!(sort), sort_at, sort != "desc");
    field("reader_mode", json!(mode), mode_at, mode.is_some());
    let activity = apply::activity_at(conn, id)?;
    change.insert("removed".into(), json!(false));
    change.insert("removed_at".into(), json!(if activity > 0 { activity } else { created_at }));
    out.series.push(Value::Object(change));

    chapters(conn, id, &key, out)?;
    members(conn, id, &key, out)?;
    mal_link(conn, &extension, &source, &remote, &key, out)?;
    Ok(())
}

fn chapters(conn: &Connection, series_id: i64, key: &str, out: &mut Outgoing) -> AppResult<()> {
    type Row = (String, Option<i64>, Option<i64>, bool, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<f64>, Option<i64>);
    let mut statement = conn.prepare(
        "SELECT c.remote_id, c.reading_started_at, c.completed_at, c.manual_unread, c.acknowledged_at, c.first_seen_at, m.read_at,
                q.page_index, q.page_count, q.scroll_ratio, q.updated_at
         FROM online_chapters c LEFT JOIN sync_chapter_meta m ON m.chapter_id = c.id LEFT JOIN chapter_progress q ON q.chapter_id = c.id
         WHERE c.series_id = ?1 AND (c.reading_started_at IS NOT NULL OR c.completed_at IS NOT NULL OR c.manual_unread = 1
                OR m.chapter_id IS NOT NULL OR q.chapter_id IS NOT NULL OR c.acknowledged_at > coalesce(c.first_seen_at, 0))",
    )?;
    let rows: Vec<Row> = statement
        .query_map([series_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?)))?
        .collect::<Result<_, _>>()?;
    for (remote, started, completed, manual_unread, acknowledged, first_seen, read_at, page, pages, ratio, position_at) in rows {
        let mut change = Map::new();
        change.insert("t".into(), json!("chapter"));
        change.insert("series".into(), json!(key));
        change.insert("key".into(), json!(chapter_key(key, &remote)));
        if let Some(started) = started {
            change.insert("started_at".into(), json!(started));
        }
        // A novidade vista: só quando a pessoa viu depois de o capítulo aparecer, não na primeira listagem.
        if let Some(acknowledged) = acknowledged.filter(|at| *at > first_seen.unwrap_or(0) || started.is_some() || completed.is_some()) {
            change.insert("acknowledged_at".into(), json!(acknowledged));
        }
        let read = completed.is_some() && !manual_unread;
        match (read, read_at.or(if read { completed } else { None })) {
            (true, Some(at)) => {
                change.insert("read".into(), json!(true));
                change.insert("read_at".into(), json!(at));
                change.insert("completed_at".into(), json!(completed));
            }
            (false, Some(at)) => {
                change.insert("read".into(), json!(false));
                change.insert("read_at".into(), json!(at));
            }
            _ => {}
        }
        if let (Some(page), Some(pages), Some(at)) = (page, pages, position_at) {
            change.insert("page_index".into(), json!(page.max(0)));
            change.insert("page_count".into(), json!(pages.max(0)));
            change.insert("scroll_ratio".into(), json!(ratio.filter(|r| r.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0)));
            change.insert("position_at".into(), json!(at));
        }
        if change.len() > 3 {
            out.chapters.push(Value::Object(change));
        }
    }
    Ok(())
}

fn members(conn: &Connection, series_id: i64, key: &str, out: &mut Outgoing) -> AppResult<()> {
    let mut statement = conn.prepare(
        "SELECT c.sync_id, 1, coalesce(m.changed_at, x.added_at) FROM series_collections x JOIN collections c ON c.id = x.collection_id
           LEFT JOIN sync_member_meta m ON m.series_id = x.series_id AND m.collection_id = x.collection_id
          WHERE x.series_id = ?1 AND c.sync_id IS NOT NULL
         UNION ALL
         SELECT c.sync_id, 0, m.changed_at FROM sync_member_meta m JOIN collections c ON c.id = m.collection_id
          WHERE m.series_id = ?1 AND m.member = 0 AND c.sync_id IS NOT NULL
            AND NOT EXISTS (SELECT 1 FROM series_collections x WHERE x.series_id = m.series_id AND x.collection_id = m.collection_id)",
    )?;
    let rows = statement.query_map([series_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, i64>(2)?)))?;
    for row in rows {
        let (collection, member, at) = row?;
        out.members.push(json!({ "t": "series_collection", "series": key, "collection": collection, "member": member, "at": at }));
    }
    Ok(())
}

fn mal_link(conn: &Connection, extension: &str, source: &str, remote: &str, key: &str, out: &mut Outgoing) -> AppResult<()> {
    let link: Option<(String, Option<i64>, Option<String>, i64)> = conn
        .query_row(
            "SELECT l.origin, l.mal_id, l.mal_title, coalesce(m.changed_at, l.linked_at) FROM mal_links l
               LEFT JOIN sync_mal_meta m ON m.extension_id = l.extension_id AND m.source_id = l.source_id AND m.remote_id = l.remote_id
              WHERE l.extension_id = ?1 AND l.source_id = ?2 AND l.remote_id = ?3 AND l.origin IN ('manual','none')",
            [extension, source, remote],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    if let Some((origin, mal_id, title, at)) = link {
        let manual = origin == "manual";
        out.mal.push(json!({ "t": "mal_link", "series": key, "origin": origin, "mal_id": mal_id.filter(|_| manual), "mal_title": clip(title, MAX_TITLE).filter(|_| manual), "at": at }));
    }
    Ok(())
}

fn collections(conn: &Connection, out: &mut Outgoing) -> AppResult<()> {
    let mut statement = conn.prepare(
        "SELECT c.sync_id, c.name, coalesce(m.changed_at, c.created_at) FROM collections c LEFT JOIN sync_collection_meta m ON m.collection_id = c.id WHERE c.sync_id IS NOT NULL",
    )?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
    for row in rows {
        let (id, name, at) = row?;
        if let Some(name) = clip(Some(name), 120) {
            out.collections.push(json!({ "t": "collection", "id": id, "name": name, "removed": false, "at": at }));
        }
    }
    // A nuvem pede o nome também na remoção: vem do espelho. Pasta que nunca subiu não precisa subir.
    let known = mirror(conn, "collection")?;
    let mut statement = conn.prepare("SELECT sync_id, removed_at FROM sync_collection_gone")?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (id, at) = row?;
        if let Some(remote) = known.get(&id).filter(|d| d["removed"].as_bool() == Some(false)) {
            out.collections.push(json!({ "t": "collection", "id": id, "name": remote["name"], "removed": true, "at": at }));
        }
        out.collections_gone.push(id);
    }
    Ok(())
}

fn catalog(conn: &Connection, out: &mut Outgoing) -> AppResult<()> {
    let mut statement = conn.prepare(
        "SELECT r.url, r.key_fingerprint, r.public_key, r.enabled, coalesce(m.changed_at, r.added_at) FROM source_repositories r
           LEFT JOIN sync_catalog_meta m ON m.kind = 'repository' AND m.key = r.url",
    )?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?, r.get::<_, i64>(4)?)))?;
    for row in rows {
        let (url, fingerprint, public_key, enabled, at) = row?;
        if url.len() <= MAX_URL && fingerprint.len() <= 128 && public_key.len() <= 1024 {
            out.repositories.push(json!({ "t": "repository", "url": url, "fingerprint": fingerprint, "public_key": public_key, "enabled": enabled, "removed": false, "at": at }));
        }
    }
    let mut statement = conn.prepare(
        "SELECT e.id, coalesce(json_extract(e.entry, '$.runtime'), ''), r.url, e.version, e.enabled, coalesce(m.changed_at, e.installed_at)
           FROM installed_extensions e JOIN source_repositories r ON r.id = e.repository_id
           LEFT JOIN sync_catalog_meta m ON m.kind = 'extension' AND m.key = e.id",
    )?;
    let rows = statement.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, bool>(4)?, r.get::<_, i64>(5)?))
    })?;
    for row in rows {
        let (id, runtime, url, version, enabled, at) = row?;
        let runtime = if runtime == crate::extensions::suwayomi::RUNTIME { "mihon" } else { "prancha" };
        if id.len() <= 256 && url.len() <= MAX_URL && !version.is_empty() && version.len() <= 64 {
            out.extensions.push(json!({ "t": "extension", "id": id, "runtime": runtime, "repository_url": url, "version": version, "enabled": enabled, "removed": false, "at": at }));
        }
    }
    // Saídas: os dados que a nuvem exige vêm do espelho.
    for (kind, list) in [("repository", &mut out.repositories), ("extension", &mut out.extensions)] {
        let known = mirror(conn, kind)?;
        let mut statement = conn.prepare("SELECT key, changed_at FROM sync_catalog_meta WHERE kind = ?1 AND removed = 1")?;
        let rows = statement.query_map([kind], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (key, at) = row?;
            let local: bool = if kind == "repository" {
                conn.query_row("SELECT EXISTS (SELECT 1 FROM source_repositories WHERE url = ?1)", [&key], |r| r.get(0))?
            } else {
                conn.query_row("SELECT EXISTS (SELECT 1 FROM installed_extensions WHERE id = ?1)", [&key], |r| r.get(0))?
            };
            let Some(remote) = known.get(&key).filter(|d| !local && d["removed"].as_bool() == Some(false)) else { continue };
            let mut change = remote.clone();
            change["removed"] = json!(true);
            change["at"] = json!(at);
            list.push(change);
        }
    }
    Ok(())
}
