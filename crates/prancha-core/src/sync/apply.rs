//! O espelho da nuvem e a aplicação dele no aparelho. Tudo aqui roda com `sync_state.applying` = 1: os gatilhos
//! ficam quietos e o que veio de outro aparelho não volta para a fila de envio.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::{Changed, chapter_key, series_key};
use crate::core::Core;
use crate::db::now_ms;
use crate::error::AppResult;

const READER_MODES: [&str; 4] = ["single", "double", "vertical", "webtoon"];

/// Roda `f` com os gatilhos quietos. O estado volta mesmo se `f` falhar.
pub(super) fn quietly<T>(conn: &Connection, f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    conn.execute("UPDATE sync_state SET applying = 1 WHERE id = 1", [])?;
    let result = f();
    conn.execute("UPDATE sync_state SET applying = 0 WHERE id = 1", [])?;
    result
}

pub(super) fn clear_mirror(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("DELETE FROM sync_remote_series; DELETE FROM sync_remote_chapters; DELETE FROM sync_remote_items;")?;
    Ok(())
}

fn text(value: &Value, field: &str) -> Option<String> {
    value[field].as_str().map(str::to_string)
}

/// Guarda no espelho uma mudança que a nuvem devolveu.
pub(super) fn remember(conn: &Connection, item: &Value, changed: &mut Changed) -> AppResult<()> {
    let data = item.to_string();
    match item["t"].as_str() {
        Some("series") => {
            let Some(key) = text(item, "key") else { return Ok(()) };
            conn.execute(
                "INSERT INTO sync_remote_series (series_key, data) VALUES (?1, ?2) ON CONFLICT (series_key) DO UPDATE SET data = excluded.data",
                params![key, data],
            )?;
            if item["removed"].as_bool() == Some(true) {
                // A nuvem apaga o estado dos capítulos e as pastas de uma obra removida.
                conn.execute("DELETE FROM sync_remote_chapters WHERE series_key = ?1", [&key])?;
                conn.execute("DELETE FROM sync_remote_items WHERE kind = 'series_collection' AND key LIKE ?1 || '/%'", [&key])?;
            }
            changed.series.insert(key);
        }
        Some("chapter") => {
            let (Some(series), Some(key)) = (text(item, "series"), text(item, "key")) else { return Ok(()) };
            conn.execute(
                "INSERT INTO sync_remote_chapters (series_key, chapter_key, data) VALUES (?1, ?2, ?3)
                 ON CONFLICT (series_key, chapter_key) DO UPDATE SET data = excluded.data",
                params![series, key, data],
            )?;
            changed.chapters.insert(series);
        }
        Some(kind @ ("repository" | "extension" | "collection" | "series_collection" | "mal_link")) => {
            let key = match kind {
                "repository" => text(item, "url"),
                "extension" | "collection" => text(item, "id"),
                "series_collection" => text(item, "series").zip(text(item, "collection")).map(|(s, c)| format!("{s}/{c}")),
                _ => text(item, "series"),
            };
            let Some(key) = key else { return Ok(()) };
            conn.execute(
                "INSERT INTO sync_remote_items (kind, key, data) VALUES (?1, ?2, ?3) ON CONFLICT (kind, key) DO UPDATE SET data = excluded.data",
                params![kind, key, data],
            )?;
            changed.items.insert((kind.to_string(), key));
        }
        _ => {}
    }
    Ok(())
}

/// As chaves portáteis das obras deste aparelho.
pub(super) fn local_keys(conn: &Connection) -> AppResult<HashMap<String, i64>> {
    let mut statement = conn.prepare("SELECT id, extension_id, source_id, remote_id FROM online_series")?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?;
    let mut keys = HashMap::new();
    for row in rows {
        let (id, extension, source, remote) = row?;
        keys.insert(series_key(&extension, &source, &remote), id);
    }
    Ok(keys)
}

pub(super) fn catalog_items(conn: &Connection) -> AppResult<Vec<(String, String, Value)>> {
    let mut statement = conn.prepare("SELECT kind, key, data FROM sync_remote_items WHERE kind IN ('repository','extension') ORDER BY kind DESC, key")?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    let mut items = Vec::new();
    for row in rows {
        let (kind, key, data) = row?;
        items.push((kind, key, serde_json::from_str(&data).unwrap_or(Value::Null)));
    }
    Ok(items)
}

/// Aplica no aparelho o que mudou no espelho. Devolve se algo mudou aqui.
pub(super) fn materialize(core: &Core, conn: &mut Connection, changed: &Changed) -> AppResult<bool> {
    let tx = conn.transaction()?;
    let applied = quietly(&tx, || {
        let mut keys = local_keys(&tx)?;
        let mut applied = false;
        let items = |kind: &str| -> AppResult<Vec<(String, Value)>> {
            let mut statement = tx.prepare("SELECT key, data FROM sync_remote_items WHERE kind = ?1 ORDER BY key")?;
            let rows = statement.query_map([kind], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (key, data) = row?;
                if changed.all || changed.items.contains(&(kind.to_string(), key.clone())) {
                    out.push((key, serde_json::from_str(&data).unwrap_or(Value::Null)));
                }
            }
            Ok(out)
        };

        for (_, data) in items("collection")? {
            applied |= apply_collection(&tx, &data)?;
        }

        let series: Vec<(String, Value)> = {
            let mut statement = tx.prepare("SELECT series_key, data FROM sync_remote_series")?;
            let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (key, data) = row?;
                if changed.all || changed.series.contains(&key) {
                    out.push((key, serde_json::from_str(&data).unwrap_or(Value::Null)));
                }
            }
            out
        };
        let mut created = Vec::new();
        for (key, data) in &series {
            let (changed_here, new) = apply_series(core, &tx, key, data, &mut keys)?;
            applied |= changed_here;
            if new {
                created.push(key.clone());
            }
        }
        if !created.is_empty() {
            crate::library::set_setting(&tx, "plugins.online", &Value::Bool(true))?;
        }

        let touched: Vec<String> = if changed.all {
            keys.keys().cloned().collect()
        } else {
            changed.series.iter().chain(changed.chapters.iter()).cloned().collect()
        };
        for key in &touched {
            if let Some(id) = keys.get(key) {
                applied |= apply_chapters(&tx, key, *id)?;
            }
        }

        let fresh = |key: &str, series: &str| created.iter().any(|c| c == series) || changed.all || changed.items.iter().any(|(_, k)| k == key);
        let mut statement = tx.prepare("SELECT kind, key, data FROM sync_remote_items WHERE kind IN ('series_collection','mal_link')")?;
        let links: Vec<(String, String, Value)> = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(kind, key, data)| (kind, key, serde_json::from_str(&data).unwrap_or(Value::Null)))
            .collect();
        drop(statement);
        for (kind, key, data) in links {
            let Some(series) = data["series"].as_str() else { continue };
            if !fresh(&key, series) {
                continue;
            }
            let Some(id) = keys.get(series) else { continue };
            applied |= if kind == "series_collection" { apply_membership(&tx, *id, &data)? } else { apply_mal_link(&tx, *id, &data)? };
        }
        Ok(applied)
    })?;
    tx.commit()?;
    Ok(applied)
}

/// A data mais recente de algo que a pessoa fez com a obra neste aparelho. Uma remoção mais antiga não vale.
pub(super) fn activity_at(conn: &Connection, id: i64) -> AppResult<i64> {
    Ok(conn
        .query_row(
            "SELECT max(
                coalesce(m.following_at, 0), coalesce(m.favorite_at, 0), coalesce(m.chapter_sort_at, 0), coalesce(m.reader_mode_at, 0),
                coalesce((SELECT max(cm.read_at) FROM sync_chapter_meta cm JOIN online_chapters c ON c.id = cm.chapter_id WHERE c.series_id = o.id), 0),
                coalesce((SELECT max(q.updated_at) FROM chapter_progress q JOIN online_chapters c ON c.id = q.chapter_id WHERE c.series_id = o.id), 0),
                coalesce((SELECT max(max(coalesce(c.last_read_at, 0), coalesce(c.reading_started_at, 0), coalesce(c.completed_at, 0))) FROM online_chapters c WHERE c.series_id = o.id), 0),
                coalesce((SELECT max(changed_at) FROM sync_member_meta WHERE series_id = o.id), 0),
                coalesce((SELECT max(added_at) FROM series_collections WHERE series_id = o.id), 0),
                CASE WHEN (o.favorite = 1 AND m.favorite_at IS NULL) OR (o.following = 1 AND m.following_at IS NULL) THEN o.created_at ELSE 0 END)
             FROM online_series o LEFT JOIN sync_series_meta m ON m.series_id = o.id WHERE o.id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

/// Devolve (mudou algo, a obra foi criada agora).
fn apply_series(core: &Core, conn: &Connection, key: &str, data: &Value, keys: &mut HashMap<String, i64>) -> AppResult<(bool, bool)> {
    let removed = data["removed"].as_bool() == Some(true);
    let mut created = false;
    let id = match keys.get(key).copied() {
        Some(id) if removed => {
            let removed_at = data["removed_at"].as_i64().unwrap_or(0);
            if removed_at <= activity_at(conn, id)? {
                return Ok((false, false));
            }
            crate::extensions::sources::forget_series_on(core, conn, id)?;
            let still: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM online_series WHERE id = ?1)", [id], |r| r.get(0))?;
            if !still {
                keys.remove(key);
            }
            conn.execute("UPDATE sync_remote_series SET series_id = NULL WHERE series_key = ?1", [key])?;
            return Ok((true, false));
        }
        None if removed => return Ok((false, false)),
        Some(id) => id,
        None => {
            let (Some(extension), Some(source), Some(item)) = (text(data, "extension_id"), text(data, "source_id"), text(data, "item_key")) else {
                return Ok((false, false));
            };
            if super::series_key(&extension, &source, &item) != key {
                return Ok((false, false));
            }
            let blocked: bool = conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM blocked_series WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3)",
                [&extension, &source, &item],
                |r| r.get(0),
            )?;
            if blocked {
                return Ok((false, false));
            }
            let title = text(data, "title").filter(|t| !t.trim().is_empty()).unwrap_or_else(|| item.clone());
            let now = now_ms();
            conn.execute(
                "INSERT INTO online_series (extension_id, source_id, remote_id, title, cover_url, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![extension, source, item, title, text(data, "cover_url"), now],
            )?;
            let id = conn.last_insert_rowid();
            keys.insert(key.to_string(), id);
            created = true;
            id
        }
    };
    conn.execute("UPDATE sync_remote_series SET series_id = ?2 WHERE series_key = ?1", params![key, id])?;

    type Local = (bool, bool, String, Option<String>, i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>);
    let (following, favorite, sort, mode, created_at, following_at, favorite_at, sort_at, mode_at): Local = conn.query_row(
        "SELECT o.following, o.favorite, o.chapter_sort, o.reader_mode, o.created_at, m.following_at, m.favorite_at, m.chapter_sort_at, m.reader_mode_at
         FROM online_series o LEFT JOIN sync_series_meta m ON m.series_id = o.id WHERE o.id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
    )?;
    // Sem data anotada, um valor diferente do padrão foi escolhido antes da sincronização existir.
    let local_at = |at: Option<i64>, customized: bool| at.unwrap_or(if customized { created_at } else { 0 });
    let mut applied = created;
    let mut set = |column: &str, meta: &str, value: &dyn rusqlite::ToSql, at: i64| -> AppResult<()> {
        conn.execute(&format!("UPDATE online_series SET {column} = ?2 WHERE id = ?1"), params![id, value])?;
        conn.execute(&format!("INSERT INTO sync_series_meta (series_id, {meta}) VALUES (?1, ?2) ON CONFLICT (series_id) DO UPDATE SET {meta} = excluded.{meta}"), params![id, at])?;
        applied = true;
        Ok(())
    };
    if let (Some(value), Some(at)) = (data["following"].as_bool(), data["following_at"].as_i64()) {
        if at > local_at(following_at, following) {
            set("following", "following_at", &value, at)?;
        }
    }
    if let (Some(value), Some(at)) = (data["favorite"].as_bool(), data["favorite_at"].as_i64()) {
        if at > local_at(favorite_at, favorite) {
            set("favorite", "favorite_at", &value, at)?;
        }
    }
    if let (Some(value), Some(at)) = (data["chapter_sort"].as_str().filter(|s| matches!(*s, "asc" | "desc")), data["chapter_sort_at"].as_i64()) {
        if at > local_at(sort_at, sort != "desc") {
            set("chapter_sort", "chapter_sort_at", &value, at)?;
        }
    }
    if let Some(at) = data["reader_mode_at"].as_i64() {
        let value = data["reader_mode"].as_str().filter(|m| READER_MODES.contains(m));
        if at > local_at(mode_at, mode.is_some()) {
            set("reader_mode", "reader_mode_at", &value, at)?;
        }
    }
    Ok((applied, created))
}

struct LocalChapter {
    id: i64,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    manual_unread: bool,
    acknowledged_at: Option<i64>,
    read_at: Option<i64>,
    position_at: Option<i64>,
}

/// Aplica o estado que a nuvem tem para os capítulos que este aparelho já listou. Os outros ficam no espelho.
fn apply_chapters(conn: &Connection, key: &str, series_id: i64) -> AppResult<bool> {
    let remote: Vec<(String, Value)> = {
        let mut statement = conn.prepare("SELECT chapter_key, data FROM sync_remote_chapters WHERE series_key = ?1")?;
        let rows = statement.query_map([key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (chapter, data) = row?;
            out.push((chapter, serde_json::from_str(&data).unwrap_or(Value::Null)));
        }
        out
    };
    if remote.is_empty() {
        return Ok(false);
    }
    let mut local: HashMap<String, LocalChapter> = HashMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT c.id, c.remote_id, c.reading_started_at, c.completed_at, c.manual_unread, c.acknowledged_at, m.read_at, q.updated_at
             FROM online_chapters c LEFT JOIN sync_chapter_meta m ON m.chapter_id = c.id LEFT JOIN chapter_progress q ON q.chapter_id = c.id
             WHERE c.series_id = ?1",
        )?;
        let rows = statement.query_map([series_id], |r| {
            Ok((
                r.get::<_, String>(1)?,
                LocalChapter {
                    id: r.get(0)?,
                    started_at: r.get(2)?,
                    completed_at: r.get(3)?,
                    manual_unread: r.get(4)?,
                    acknowledged_at: r.get(5)?,
                    read_at: r.get(6)?,
                    position_at: r.get(7)?,
                },
            ))
        })?;
        for row in rows {
            let (remote_id, chapter) = row?;
            local.insert(chapter_key(key, &remote_id), chapter);
        }
    }
    let mut applied = false;
    for (chapter, data) in remote {
        if let Some(local) = local.get(&chapter) {
            applied |= apply_chapter(conn, local, &data)?;
        }
    }
    Ok(applied)
}

fn earlier(local: Option<i64>, remote: Option<i64>) -> Option<i64> {
    match (local, remote) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn apply_chapter(conn: &Connection, local: &LocalChapter, data: &Value) -> AppResult<bool> {
    let mut applied = false;
    let started = earlier(local.started_at, data["started_at"].as_i64());
    let acknowledged = earlier(local.acknowledged_at, data["acknowledged_at"].as_i64());
    if started != local.started_at || acknowledged != local.acknowledged_at {
        conn.execute("UPDATE online_chapters SET reading_started_at = ?2, acknowledged_at = ?3 WHERE id = ?1", params![local.id, started, acknowledged])?;
        applied = true;
    }

    let local_read = local.completed_at.is_some() && !local.manual_unread;
    let local_read_at = local.read_at.or(if local_read { local.completed_at } else { None }).unwrap_or(0);
    if let (Some(read), Some(read_at)) = (data["read"].as_bool(), data["read_at"].as_i64()) {
        if read_at > local_read_at || (read && local_read && read_at == local_read_at) {
            if read {
                let remote_completed = data["completed_at"].as_i64().unwrap_or(read_at);
                let completed = if local_read { earlier(local.completed_at, Some(remote_completed)) } else { Some(remote_completed) };
                conn.execute(
                    "UPDATE online_chapters SET completed_at = ?2, manual_unread = 0, acknowledged_at = coalesce(acknowledged_at, ?2),
                        last_read_at = max(coalesce(last_read_at, 0), ?3) WHERE id = ?1",
                    params![local.id, completed, read_at],
                )?;
            } else {
                conn.execute("UPDATE online_chapters SET completed_at = NULL, manual_unread = 1 WHERE id = ?1", [local.id])?;
            }
            conn.execute(
                "INSERT INTO sync_chapter_meta (chapter_id, read_at) VALUES (?1, ?2) ON CONFLICT (chapter_id) DO UPDATE SET read_at = max(read_at, excluded.read_at)",
                params![local.id, read_at],
            )?;
            applied = true;
        }
    }

    if let (Some(index), Some(count), Some(at)) = (data["page_index"].as_i64(), data["page_count"].as_i64(), data["position_at"].as_i64()) {
        if count > 0 && at > local.position_at.unwrap_or(0) {
            let ratio = data["scroll_ratio"].as_f64().filter(|r| r.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0);
            conn.execute(
                "INSERT INTO chapter_progress (chapter_id, page_index, page_count, scroll_ratio, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (chapter_id) DO UPDATE SET page_index = ?2, page_count = ?3, scroll_ratio = ?4, updated_at = ?5",
                params![local.id, index.clamp(0, count - 1), count, ratio, at],
            )?;
            conn.execute("UPDATE online_chapters SET last_read_at = max(coalesce(last_read_at, 0), ?2) WHERE id = ?1", params![local.id, at])?;
            applied = true;
        }
    }
    Ok(applied)
}

/// Um nome livre para a pasta: outra pasta com o mesmo nome ganha um número.
fn free_name(conn: &Connection, name: &str, except: Option<i64>) -> AppResult<String> {
    for n in 1..100 {
        let candidate = if n == 1 { name.to_string() } else { format!("{name} ({n})") };
        let taken: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM collections WHERE name = ?1 COLLATE NOCASE AND id IS NOT ?2)",
            params![candidate, except],
            |r| r.get(0),
        )?;
        if !taken {
            return Ok(candidate);
        }
    }
    Ok(format!("{name} ({})", now_ms()))
}

fn apply_collection(conn: &Connection, data: &Value) -> AppResult<bool> {
    let (Some(sync_id), Some(at)) = (text(data, "id"), data["at"].as_i64()) else { return Ok(false) };
    let removed = data["removed"].as_bool() == Some(true);
    let name = text(data, "name").map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let local: Option<(i64, String, i64)> = conn
        .query_row(
            "SELECT c.id, c.name, coalesce(m.changed_at, c.created_at) FROM collections c LEFT JOIN sync_collection_meta m ON m.collection_id = c.id WHERE c.sync_id = ?1",
            [&sync_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let meta = |id: i64| conn.execute("INSERT INTO sync_collection_meta (collection_id, changed_at) VALUES (?1, ?2) ON CONFLICT (collection_id) DO UPDATE SET changed_at = excluded.changed_at", params![id, at]);
    match local {
        Some((id, current, local_at)) => {
            if at <= local_at {
                return Ok(false);
            }
            if removed {
                // Uma pasta com HQs deste aparelho fica: só as obras das fontes saem dela.
                let has_comics: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM comic_collections WHERE collection_id = ?1)", [id], |r| r.get(0))?;
                if has_comics {
                    conn.execute("DELETE FROM series_collections WHERE collection_id = ?1", [id])?;
                    meta(id)?;
                } else {
                    conn.execute("DELETE FROM collections WHERE id = ?1", [id])?;
                }
                return Ok(true);
            }
            if let Some(name) = name.filter(|n| *n != current) {
                let name = free_name(conn, &name, Some(id))?;
                conn.execute("UPDATE collections SET name = ?2, updated_at = ?3 WHERE id = ?1", params![id, name, now_ms()])?;
            }
            meta(id)?;
            Ok(true)
        }
        None => {
            let Some(name) = name.filter(|_| !removed) else { return Ok(false) };
            let gone: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM sync_collection_gone WHERE sync_id = ?1 AND removed_at >= ?2)", params![sync_id, at], |r| r.get(0))?;
            if gone {
                return Ok(false);
            }
            // Uma pasta com o mesmo nome que ainda não subiu é a mesma pasta: fica com o id da nuvem.
            let same: Option<i64> = conn
                .query_row(
                    "SELECT c.id FROM collections c WHERE c.name = ?1 COLLATE NOCASE
                       AND NOT EXISTS (SELECT 1 FROM sync_remote_items i WHERE i.kind = 'collection' AND i.key = c.sync_id)",
                    [&name],
                    |r| r.get(0),
                )
                .optional()?;
            let id = match same {
                Some(id) => {
                    conn.execute("UPDATE collections SET sync_id = ?2 WHERE id = ?1", params![id, sync_id])?;
                    id
                }
                None => {
                    let name = free_name(conn, &name, None)?;
                    let now = now_ms();
                    conn.execute("INSERT INTO collections (name, created_at, updated_at, sync_id) VALUES (?1, ?2, ?2, ?3)", params![name, now, sync_id])?;
                    conn.last_insert_rowid()
                }
            };
            meta(id)?;
            Ok(true)
        }
    }
}

fn apply_membership(conn: &Connection, series_id: i64, data: &Value) -> AppResult<bool> {
    let (Some(collection), Some(member), Some(at)) = (text(data, "collection"), data["member"].as_bool(), data["at"].as_i64()) else { return Ok(false) };
    let Some(collection_id) = conn.query_row("SELECT id FROM collections WHERE sync_id = ?1", [&collection], |r| r.get::<_, i64>(0)).optional()? else {
        return Ok(false);
    };
    let local_at: i64 = conn.query_row(
        "SELECT coalesce((SELECT changed_at FROM sync_member_meta WHERE series_id = ?1 AND collection_id = ?2),
                         (SELECT added_at FROM series_collections WHERE series_id = ?1 AND collection_id = ?2), 0)",
        params![series_id, collection_id],
        |r| r.get(0),
    )?;
    if at <= local_at {
        return Ok(false);
    }
    if member {
        conn.execute("INSERT OR IGNORE INTO series_collections (series_id, collection_id, added_at) VALUES (?1, ?2, ?3)", params![series_id, collection_id, at])?;
    } else {
        conn.execute("DELETE FROM series_collections WHERE series_id = ?1 AND collection_id = ?2", params![series_id, collection_id])?;
    }
    conn.execute(
        "INSERT INTO sync_member_meta (series_id, collection_id, member, changed_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (series_id, collection_id) DO UPDATE SET member = excluded.member, changed_at = excluded.changed_at",
        params![series_id, collection_id, member, at],
    )?;
    Ok(true)
}

fn apply_mal_link(conn: &Connection, series_id: i64, data: &Value) -> AppResult<bool> {
    let (Some(origin), Some(at)) = (data["origin"].as_str().filter(|o| matches!(*o, "manual" | "none")), data["at"].as_i64()) else { return Ok(false) };
    let mal_id = data["mal_id"].as_i64().filter(|id| *id > 0);
    if (origin == "manual") != mal_id.is_some() {
        return Ok(false);
    }
    let (extension, source, remote): (String, String, String) =
        conn.query_row("SELECT extension_id, source_id, remote_id FROM online_series WHERE id = ?1", [series_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let local_at: i64 = conn.query_row(
        "SELECT coalesce((SELECT changed_at FROM sync_mal_meta WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3),
                         (SELECT linked_at FROM mal_links WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3 AND origin IN ('manual','none')), 0)",
        [&extension, &source, &remote],
        |r| r.get(0),
    )?;
    if at <= local_at {
        return Ok(false);
    }
    conn.execute(
        "INSERT OR REPLACE INTO mal_links (extension_id, source_id, remote_id, mal_id, mal_title, origin, confidence, linked_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 100, ?7)",
        params![extension, source, remote, mal_id, text(data, "mal_title").filter(|_| mal_id.is_some()), origin, at],
    )?;
    conn.execute(
        "DELETE FROM mal_sync_queue WHERE extension_id = ?1 AND source_id = ?2 AND remote_id = ?3 AND mal_id IS NOT ?4",
        params![extension, source, remote, mal_id],
    )?;
    conn.execute("INSERT OR REPLACE INTO sync_mal_meta (extension_id, source_id, remote_id, changed_at) VALUES (?1, ?2, ?3, ?4)", params![extension, source, remote, at])?;
    Ok(true)
}

/// Depois de guardar a lista de capítulos de uma obra: aplica o estado que a nuvem tinha para eles.
pub fn chapters_listed(conn: &Connection, series_id: i64) -> AppResult<()> {
    let Some((extension, source, remote)) = conn
        .query_row("SELECT extension_id, source_id, remote_id FROM online_series WHERE id = ?1", [series_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
        .optional()?
    else {
        return Ok(());
    };
    let key = series_key(&extension, &source, &remote);
    let waiting: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM sync_remote_chapters WHERE series_key = ?1)", [&key], |r| r.get(0))?;
    if !waiting {
        return Ok(());
    }
    quietly(conn, || apply_chapters(conn, &key, series_id).map(|_| ()))
}

/// Obras que vieram da nuvem com estado de capítulos e ainda não têm a lista de capítulos neste aparelho, de
/// extensões do Prancha instaladas e ligadas.
pub(super) fn series_waiting_for_chapters(conn: &Connection, limit: usize) -> AppResult<Vec<i64>> {
    let mut statement = conn.prepare(
        "SELECT r.series_id FROM sync_remote_series r
           JOIN online_series o ON o.id = r.series_id
           JOIN installed_extensions e ON e.id = o.extension_id AND e.enabled = 1 AND coalesce(json_extract(e.entry, '$.runtime'), '') <> ?1
          WHERE EXISTS (SELECT 1 FROM sync_remote_chapters c WHERE c.series_key = r.series_key)
            AND NOT EXISTS (SELECT 1 FROM online_chapters c WHERE c.series_id = r.series_id)
          LIMIT ?2",
    )?;
    let rows = statement.query_map(params![crate::extensions::suwayomi::RUNTIME, limit as i64], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}
