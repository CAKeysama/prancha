//! Conexão SQLite e migrações versionadas (`PRAGMA user_version`).

use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::error::AppResult;
use crate::natural::natural_cmp;

/// Migrações em ordem. A posição + 1 é a versão do esquema após aplicá-la.
/// Nunca altere uma migração publicada: acrescente uma nova.
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_initial.sql"), include_str!("../migrations/0002_guided_reading.sql")];

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
