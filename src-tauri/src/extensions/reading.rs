//! Estado de leitura de cada capítulo de uma obra: não lido, lendo, em andamento e lido, além da novidade.
//!
//! O estado que dura fica em `online_chapters` (`reading_started_at`, `last_read_at`, `completed_at`, `manual_unread`,
//! `first_seen_at`, `acknowledged_at`). A sessão do leitor fica em `reading_sessions` e é temporária: "Lendo" só vale
//! enquanto há uma sessão aberta com atividade recente. Quem decide o que é início efetivo e o que é chegar ao fim é o
//! leitor (`src/domain/reading-lifecycle.ts`); aqui as transições são gravadas de forma idempotente e transacional.
//! A posição continua em `chapter_progress` (ou `reading_progress`, quando o capítulo está na biblioteca) e nunca é
//! apagada por uma mudança de estado. Ver docs/adr/0010-progresso-de-leitura.md.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::db::now_ms;
use crate::error::{AppError, AppResult};

/// Sem atividade há mais que isto, a sessão não vale mais como "Lendo", mesmo que ninguém a tenha fechado.
/// O leitor avisa que continua aberto a cada minuto.
pub const SESSION_STALE_MS: i64 = 3 * 60 * 1000;

/// Sessões encerradas guardadas como histórico. As mais velhas saem na abertura do app.
const SESSION_HISTORY_MS: i64 = 180 * 24 * 60 * 60 * 1000;

/// Os quatro estados de um capítulo. A novidade é outra coisa e não entra aqui.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadState {
    Unread,
    Reading,
    InProgress,
    Read,
}

impl ReadState {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadState::Unread => "unread",
            ReadState::Reading => "reading",
            ReadState::InProgress => "in_progress",
            ReadState::Read => "read",
        }
    }
}

/// Hierarquia: sessão ativa > concluído > marcado como não lido > iniciado > não lido.
/// Reler um capítulo concluído mostra "Lendo" durante a sessão e volta a "Lido" quando ela termina.
pub fn derive_state(active_session: bool, started: bool, completed: bool, manual_unread: bool) -> ReadState {
    if active_session {
        ReadState::Reading
    } else if completed {
        ReadState::Read
    } else if manual_unread || !started {
        ReadState::Unread
    } else {
        ReadState::InProgress
    }
}

/// Trecho de SQL que diz se o capítulo `c` tem uma sessão ativa. `?now` é o parâmetro com o instante atual.
pub fn active_session_sql(now_param: &str) -> String {
    format!("EXISTS (SELECT 1 FROM reading_sessions s WHERE s.chapter_id = c.id AND s.ended_at IS NULL AND s.last_activity_at > {now_param} - {SESSION_STALE_MS})")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterReadInfo {
    pub id: i64,
    pub series_id: i64,
    pub read_state: ReadState,
    pub is_new: bool,
    pub completed_at: Option<i64>,
}

pub fn chapter_info(conn: &Connection, id: i64) -> AppResult<ChapterReadInfo> {
    let now = now_ms();
    conn.query_row(
        &format!(
            "SELECT c.id, c.series_id, {active}, c.reading_started_at IS NOT NULL, c.completed_at, c.manual_unread, c.acknowledged_at IS NULL
             FROM online_chapters c WHERE c.id = ?1",
            active = active_session_sql("?2")
        ),
        params![id, now],
        |r| {
            let completed_at: Option<i64> = r.get(4)?;
            Ok(ChapterReadInfo {
                id: r.get(0)?,
                series_id: r.get(1)?,
                read_state: derive_state(r.get(2)?, r.get(3)?, completed_at.is_some(), r.get::<_, i64>(5)? != 0),
                is_new: r.get(6)?,
                completed_at,
            })
        },
    )
    .optional()?
    .ok_or_else(|| AppError::user("chapter_not_found", "Este capítulo não está mais registrado."))
}

fn exists(conn: &Connection, id: i64) -> AppResult<()> {
    let found: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM online_chapters WHERE id = ?1)", [id], |r| r.get(0))?;
    if found { Ok(()) } else { Err(AppError::user("chapter_not_found", "Este capítulo não está mais registrado.")) }
}

fn end_sessions_of(conn: &Connection, chapter_id: i64, now: i64, reason: &str) -> AppResult<usize> {
    Ok(conn.execute("UPDATE reading_sessions SET ended_at = ?2, end_reason = ?3 WHERE chapter_id = ?1 AND ended_at IS NULL", params![chapter_id, now, reason])?)
}

/// O capítulo foi aberto de fato no leitor (não só pré-carregado): a novidade deixa de aparecer. Não muda o estado de leitura.
pub fn acknowledge(conn: &Connection, id: i64) -> AppResult<ChapterReadInfo> {
    exists(conn, id)?;
    conn.execute("UPDATE online_chapters SET acknowledged_at = ?2 WHERE id = ?1 AND acknowledged_at IS NULL", params![id, now_ms()])?;
    chapter_info(conn, id)
}

/// Primeira interação que avança a leitura numa sessão nova. Repetir não muda nada além da atividade.
/// Só um capítulo é lido de cada vez: outra sessão aberta é encerrada.
pub fn start(conn: &mut Connection, id: i64) -> AppResult<ChapterReadInfo> {
    exists(conn, id)?;
    let now = now_ms();
    let tx = conn.transaction()?;
    tx.execute("UPDATE reading_sessions SET ended_at = ?2, end_reason = 'replaced' WHERE chapter_id <> ?1 AND ended_at IS NULL", params![id, now])?;
    let touched = tx.execute("UPDATE reading_sessions SET last_activity_at = ?2 WHERE chapter_id = ?1 AND ended_at IS NULL", params![id, now])?;
    if touched == 0 {
        tx.execute("INSERT INTO reading_sessions (chapter_id, started_at, last_activity_at) VALUES (?1, ?2, ?2)", params![id, now])?;
    }
    // Depois de "Marcar como não lido", uma leitura nova começa do zero no estado, mas a posição continua onde estava.
    tx.execute(
        "UPDATE online_chapters SET
            reading_started_at = CASE WHEN reading_started_at IS NULL OR manual_unread = 1 THEN ?2 ELSE reading_started_at END,
            manual_unread = 0,
            last_read_at = ?2,
            acknowledged_at = coalesce(acknowledged_at, ?2)
         WHERE id = ?1",
        params![id, now],
    )?;
    tx.commit()?;
    chapter_info(conn, id)
}

/// O leitor continua aberto neste capítulo. Não abre sessão: sem início efetivo, não há o que manter.
pub fn touch(conn: &Connection, id: i64) -> AppResult<bool> {
    let now = now_ms();
    let touched = conn.execute("UPDATE reading_sessions SET last_activity_at = ?2 WHERE chapter_id = ?1 AND ended_at IS NULL", params![id, now])?;
    if touched > 0 {
        conn.execute("UPDATE online_chapters SET last_read_at = ?2 WHERE id = ?1", params![id, now])?;
    }
    Ok(touched > 0)
}

/// A pessoa saiu do capítulo (voltou, trocou de capítulo, fechou a janela). Iniciado e não concluído, fica "Em andamento".
pub fn end(conn: &Connection, id: i64) -> AppResult<ChapterReadInfo> {
    exists(conn, id)?;
    end_sessions_of(conn, id, now_ms(), "left")?;
    chapter_info(conn, id)
}

/// Chegou ao fim de verdade. Idempotente: a data da primeira conclusão fica, e repetir não grava nada.
pub fn complete(conn: &mut Connection, id: i64) -> AppResult<ChapterReadInfo> {
    exists(conn, id)?;
    let now = now_ms();
    let tx = conn.transaction()?;
    end_sessions_of(&tx, id, now, "completed")?;
    tx.execute(
        "UPDATE online_chapters SET
            completed_at = coalesce(completed_at, ?2),
            reading_started_at = coalesce(reading_started_at, ?2),
            last_read_at = ?2,
            acknowledged_at = coalesce(acknowledged_at, ?2),
            manual_unread = 0
         WHERE id = ?1 AND (completed_at IS NULL OR acknowledged_at IS NULL OR manual_unread = 1)",
        params![id, now],
    )?;
    mark_comics(&tx, &[id], true)?;
    tx.commit()?;
    chapter_info(conn, id)
}

/// O capítulo baixado também é uma HQ da biblioteca: o estado dela acompanha, sem mexer na posição salva.
fn mark_comics(conn: &Connection, ids: &[i64], read: bool) -> AppResult<()> {
    let now = now_ms();
    let mut statement = conn.prepare("SELECT comic_id FROM online_chapters WHERE id = ?1 AND comic_id IS NOT NULL")?;
    for id in ids {
        let comic: Option<i64> = statement.query_row([id], |r| r.get(0)).optional()?;
        if let Some(comic) = comic {
            if read {
                conn.execute("UPDATE comics SET status = 'completed', updated_at = ?2 WHERE id = ?1 AND status <> 'completed'", params![comic, now])?;
            } else {
                conn.execute("UPDATE comics SET status = 'unread', updated_at = ?2 WHERE id = ?1 AND status <> 'unread'", params![comic, now])?;
            }
        }
    }
    Ok(())
}

/// Marcação manual. Lido reconhece a novidade. Não lido não traz a novidade de volta e não apaga a posição.
/// Tudo numa transação: repetir o clique não deixa estado pela metade.
pub fn set_read(conn: &mut Connection, ids: &[i64], read: bool) -> AppResult<Vec<ChapterReadInfo>> {
    let now = now_ms();
    let tx = conn.transaction()?;
    for id in ids {
        exists(&tx, *id)?;
        end_sessions_of(&tx, *id, now, "manual")?;
        if read {
            tx.execute(
                "UPDATE online_chapters SET completed_at = coalesce(completed_at, ?2), acknowledged_at = coalesce(acknowledged_at, ?2), manual_unread = 0 WHERE id = ?1",
                params![id, now],
            )?;
        } else {
            tx.execute("UPDATE online_chapters SET completed_at = NULL, manual_unread = 1 WHERE id = ?1", params![id])?;
        }
    }
    mark_comics(&tx, ids, read)?;
    tx.commit()?;
    ids.iter().map(|id| chapter_info(conn, *id)).collect()
}

/// Os capítulos que vêm antes deste na ordem de leitura que a fonte informou (`sort_order`).
/// Capítulos com a mesma posição do escolhido não são "anteriores": sem como saber, nenhum deles é marcado.
/// O título e o número não entram na conta, porque "10 Extra", "Especial" ou "Vol. 2 Cap. 3" não se ordenam por texto.
pub fn previous_ids(conn: &Connection, id: i64) -> AppResult<Vec<i64>> {
    let (series_id, order): (i64, i64) = conn
        .query_row("SELECT series_id, sort_order FROM online_chapters WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| AppError::user("chapter_not_found", "Este capítulo não está mais registrado."))?;
    let mut statement = conn.prepare("SELECT id FROM online_chapters WHERE series_id = ?1 AND sort_order < ?2 ORDER BY sort_order, id")?;
    let ids = statement.query_map(params![series_id, order], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(ids)
}

/// Marca como lidos todos os anteriores ao capítulo escolhido, numa transação. O escolhido e os seguintes ficam como estão.
pub fn mark_previous_read(conn: &mut Connection, id: i64) -> AppResult<Vec<ChapterReadInfo>> {
    let ids = previous_ids(conn, id)?;
    set_read(conn, &ids, true)
}

/// Na abertura do app nenhuma sessão pode estar ativa: as que ficaram abertas (queda, fechamento forçado) terminam
/// na última atividade registrada. O capítulo fica "Em andamento", com a posição salva, e nunca vira "Lido" por isso.
pub fn recover_sessions(conn: &Connection) -> AppResult<usize> {
    let recovered = conn.execute("UPDATE reading_sessions SET ended_at = last_activity_at, end_reason = 'recovered' WHERE ended_at IS NULL", [])?;
    conn.execute("DELETE FROM reading_sessions WHERE ended_at IS NOT NULL AND ended_at < ?1", [now_ms() - SESSION_HISTORY_MS])?;
    Ok(recovered)
}

/// Quantos capítulos novos cada obra tem. Uma consulta só para a lista inteira.
pub fn new_counts(conn: &Connection) -> AppResult<std::collections::HashMap<i64, i64>> {
    let mut statement = conn.prepare("SELECT series_id, count(*) FROM online_chapters WHERE acknowledged_at IS NULL GROUP BY series_id")?;
    let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_follows_the_hierarchy() {
        assert_eq!(derive_state(false, false, false, false), ReadState::Unread);
        assert_eq!(derive_state(true, true, false, false), ReadState::Reading);
        assert_eq!(derive_state(false, true, false, false), ReadState::InProgress);
        assert_eq!(derive_state(false, true, true, false), ReadState::Read);
        assert_eq!(derive_state(false, true, false, true), ReadState::Unread, "marcado como não lido prevalece sobre o início antigo");
        assert_eq!(derive_state(true, true, true, false), ReadState::Reading, "relendo, a sessão aparece");
        assert_eq!(derive_state(false, false, true, false), ReadState::Read, "marcado à mão sem ter lido");
    }
}
