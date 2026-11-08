//! Conexão SQLite e migrações versionadas (`PRAGMA user_version`).

use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::error::AppResult;
use crate::natural::natural_cmp;

/// Migrações em ordem. A posição + 1 é a versão do esquema após aplicá-la.
/// Nunca altere uma migração publicada: acrescente uma nova.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_guided_reading.sql"),
    include_str!("../migrations/0003_extensions.sql"),
    include_str!("../migrations/0004_online_reading.sql"),
    include_str!("../migrations/0005_series_library.sql"),
    include_str!("../migrations/0006_series_classification.sql"),
];

pub fn schema_version() -> i64 {
    MIGRATIONS.len() as i64
}

fn configure(conn: &Connection) -> AppResult<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.create_collation("NATSORT", natural_cmp)?;
    Ok(())
}

/// Abre (ou cria) o banco, ativa WAL e aplica migrações pendentes em transação.
pub fn open(path: &Path) -> AppResult<Connection> {
    let mut conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    configure(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Banco em memória (testes).
pub fn open_in_memory() -> AppResult<Connection> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current > schema_version() {
        return Err(crate::error::AppError::user(
            "schema_newer",
            "Este banco de dados foi criado por uma versão mais nova do aplicativo. Atualize o aplicativo para continuar.",
        ));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let conn = open(&path).unwrap();
            let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
            assert_eq!(v, schema_version());
        }
        let conn = open(&path).unwrap();
        let tables: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='comics'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, 1);
        let fk: i64 = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
    }

    /// Banco como a versão 0.2.0 deixava, com o uso de uma pessoa.
    fn database_from_0_2_0() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch(MIGRATIONS[1]).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute_batch(
            "INSERT INTO comics (id, path, file_name, format, fingerprint, title, added_at, updated_at) VALUES (7, 'C:/hq.cbz', 'hq.cbz', 'cbz', 'abc', 'Minha HQ', 1, 1);
             INSERT INTO reading_progress (comic_id, page_index, page_count, updated_at) VALUES (7, 3, 10, 1);
             INSERT INTO collections (id, name, created_at, updated_at) VALUES (1, 'Favoritas', 1, 1);
             INSERT INTO comic_collections (comic_id, collection_id, added_at) VALUES (7, 1, 1);
             INSERT INTO settings (key, value) VALUES ('appearance.theme', '\"light\"');",
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        conn
    }

    fn extension_rows(conn: &Connection) {
        conn.execute_batch(
            "INSERT INTO source_repositories (id, url, name, index_url, public_key, key_fingerprint, added_at) VALUES ('r', 'https://r/prancha-repo.json', 'R', 'https://r/i', 'k', 'f', 1);
             INSERT INTO repository_extensions (repository_id, extension_id, name, version, entry) VALUES ('r', 'e', 'E', '1.0.0', '{}');
             INSERT INTO installed_extensions (id, repository_id, repository_name, public_key, key_fingerprint, name, version, entry, file_name, sha256, installed_at, updated_at)
                VALUES ('e', 'r', 'R', 'k', 'f', 'E', '1.0.0', '{}', 'e.wasm', 'h', 1, 1);
             INSERT INTO extension_sources (extension_id, source_id, name, language, rating, direction, supports_latest, supports_search) VALUES ('e', 's', 'S', 'pt-BR', 'general', 'ltr', 1, 1);
             INSERT INTO extension_storage (extension_id, key, value) VALUES ('e', 'chave', x'00');
             INSERT INTO online_series (id, extension_id, source_id, remote_id, title, created_at, updated_at) VALUES (1, 'e', 's', 'obra', 'Obra', 1, 1);
             INSERT INTO online_chapters (id, series_id, remote_id, title, sort_order, comic_id) VALUES (1, 1, 'cap', 'Cap', 0, 7);
             INSERT INTO chapter_downloads (chapter_id, status, created_at, updated_at) VALUES (1, 'completed', 1, 1);",
        )
        .unwrap();
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn upgrading_a_used_database_keeps_library_progress_and_settings() {
        let conn = database_from_0_2_0();
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(version, schema_version());
        let title: String = conn.query_row("SELECT title FROM comics WHERE id = 7", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "Minha HQ");
        let page: i64 = conn.query_row("SELECT page_index FROM reading_progress WHERE comic_id = 7", [], |r| r.get(0)).unwrap();
        assert_eq!(page, 3);
        assert_eq!(count(&conn, "comic_collections"), 1);
        let theme: String = conn.query_row("SELECT value FROM settings WHERE key = 'appearance.theme'", [], |r| r.get(0)).unwrap();
        assert_eq!(theme, "\"light\"");
        extension_rows(&conn);
    }

    /// Quem já usava fontes na 0.3.0 não perde o acesso quando elas viram plugin. Quem nunca usou começa sem ele.
    #[test]
    fn the_online_plugin_stays_on_only_for_who_already_used_sources() {
        let setting = |conn: &Connection| conn.query_row("SELECT value FROM settings WHERE key = 'plugins.online'", [], |r| r.get::<_, String>(0)).ok();
        assert_eq!(setting(&database_from_0_2_0()), None);

        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        for sql in &MIGRATIONS[..3] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn.execute_batch("INSERT INTO source_repositories (id, url, name, index_url, public_key, key_fingerprint, added_at) VALUES ('r', 'https://r/prancha-repo.json', 'R', 'https://r/i', 'k', 'f', 1);").unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(setting(&conn).as_deref(), Some("true"));
    }

    /// Só a obra de extensão do Mihon que nunca trouxe gêneros perde a confiança que tinha.
    #[test]
    fn series_stored_before_the_classification_get_the_state_they_earned() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        let install = |id: &str, entry: &str| {
            conn.execute("INSERT INTO installed_extensions (id, repository_id, repository_name, public_key, key_fingerprint, name, version, entry, file_name, sha256, installed_at, updated_at) VALUES (?1, 'r', 'R', 'k', 'f', 'E', '1.0.0', ?2, 'e', 'h', 1, 1)", [id, entry]).unwrap();
        };
        install("ponte", r#"{"runtime":"suwayomi"}"#);
        install("nativa", r#"{"runtime":"wasm-component"}"#);
        install("quebrada", "nao é json");
        let series = |remote: &str, extension: &str, genres: &str, checked: Option<i64>| {
            conn.execute("INSERT INTO online_series (extension_id, source_id, remote_id, title, genres, last_checked_at, created_at, updated_at) VALUES (?1, 's', ?2, ?2, ?3, ?4, 1, 1)", rusqlite::params![extension, remote, genres, checked]).unwrap();
        };
        series("com-generos", "ponte", r#"["Ação"]"#, None);
        series("nunca-aberta", "ponte", "[]", None);
        series("aberta-sem-generos", "ponte", "[]", Some(1));
        series("nativa", "nativa", "[]", None);
        series("sem-extensao", "removida", "[]", None);
        series("entrada-quebrada", "quebrada", "[]", None);
        migrate(&mut conn).unwrap();
        let state = |remote: &str| conn.query_row("SELECT classification FROM online_series WHERE remote_id = ?1", [remote], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(state("com-generos"), "known");
        assert_eq!(state("nunca-aberta"), "pending");
        assert_eq!(state("aberta-sem-generos"), "unknown");
        assert_eq!(state("nativa"), "known");
        assert_eq!(state("sem-extensao"), "pending", "sem saber de onde veio, não vale como segura");
        assert_eq!(state("entrada-quebrada"), "pending", "uma entrada ilegível não impede o app de abrir nem vale como segura");
    }

    #[test]
    fn removing_a_chapter_takes_its_online_progress() {
        let conn = database_from_0_2_0();
        extension_rows(&conn);
        conn.execute("INSERT INTO chapter_progress (chapter_id, page_index, page_count, updated_at) VALUES (1, 2, 5, 1)", []).unwrap();
        conn.execute("DELETE FROM online_chapters WHERE id = 1", []).unwrap();
        assert_eq!(count(&conn, "chapter_progress"), 0);
    }

    #[test]
    fn removing_a_repository_keeps_installed_extensions() {
        let conn = database_from_0_2_0();
        extension_rows(&conn);
        conn.execute("DELETE FROM source_repositories WHERE id = 'r'", []).unwrap();
        assert_eq!(count(&conn, "repository_extensions"), 0);
        assert_eq!(count(&conn, "installed_extensions"), 1);
        assert_eq!(count(&conn, "extension_sources"), 1);
    }

    #[test]
    fn uninstalling_keeps_series_chapters_and_the_library() {
        let conn = database_from_0_2_0();
        extension_rows(&conn);
        conn.execute("DELETE FROM installed_extensions WHERE id = 'e'", []).unwrap();
        assert_eq!(count(&conn, "extension_sources"), 0);
        assert_eq!(count(&conn, "extension_storage"), 0, "os dados privados saem com a extensão");
        assert_eq!(count(&conn, "online_series"), 1);
        assert_eq!(count(&conn, "online_chapters"), 1);
        assert_eq!(count(&conn, "chapter_downloads"), 1);
        assert_eq!(count(&conn, "comics"), 1);
    }

    #[test]
    fn removing_a_comic_only_unlinks_the_chapter() {
        let conn = database_from_0_2_0();
        extension_rows(&conn);
        conn.execute("DELETE FROM comics WHERE id = 7", []).unwrap();
        let linked: Option<i64> = conn.query_row("SELECT comic_id FROM online_chapters WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(linked, None);
        assert_eq!(count(&conn, "online_chapters"), 1);
    }

    #[test]
    fn series_are_unique_per_source_and_a_chapter_has_one_open_download() {
        let conn = database_from_0_2_0();
        extension_rows(&conn);
        let repeated = "INSERT INTO online_series (extension_id, source_id, remote_id, title, created_at, updated_at) VALUES ('e', 's', 'obra', 'Outra', 1, 1)";
        assert!(conn.execute(repeated, []).is_err());
        let other_source = "INSERT INTO online_series (extension_id, source_id, remote_id, title, created_at, updated_at) VALUES ('e', 's2', 'obra', 'Outra', 1, 1)";
        assert!(conn.execute(other_source, []).is_ok());
        let open = "INSERT INTO chapter_downloads (chapter_id, status, created_at, updated_at) VALUES (1, 'queued', 1, 1)";
        assert!(conn.execute(open, []).is_ok());
        assert!(conn.execute(open, []).is_err());
    }

    #[test]
    fn a_rating_outside_the_list_is_refused() {
        let conn = database_from_0_2_0();
        extension_rows(&conn);
        let bad = "INSERT INTO extension_sources (extension_id, source_id, name, language, rating, direction, supports_latest, supports_search) VALUES ('e', 'x', 'X', 'pt-BR', 'liberado', 'ltr', 1, 1)";
        assert!(conn.execute(bad, []).is_err());
    }

    #[test]
    fn natural_collation_is_registered() {
        let conn = open_in_memory().unwrap();
        let mut stmt = conn
            .prepare("SELECT v FROM (SELECT 'Vol 10' v UNION SELECT 'Vol 2' UNION SELECT 'Vol 1') ORDER BY v COLLATE NATSORT")
            .unwrap();
        let rows: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(rows, vec!["Vol 1", "Vol 2", "Vol 10"]);
    }
}
