-- Estado de leitura por capítulo, capítulos novos e sessões do leitor. Datas em milissegundos Unix.
--
-- O estado que dura fica no capítulo. A sessão ativa fica à parte, em reading_sessions, porque é temporária:
-- uma sessão que o app não encerrou (queda, fechamento forçado) é fechada na próxima abertura e o capítulo
-- volta a aparecer como "Em andamento", sem perder a posição, que continua em chapter_progress/reading_progress.
--
--   Não lido      completed_at IS NULL e (reading_started_at IS NULL ou manual_unread = 1), sem sessão ativa
--   Lendo         há sessão ativa (ended_at IS NULL e atividade recente)
--   Em andamento  reading_started_at IS NOT NULL, completed_at IS NULL, manual_unread = 0, sem sessão ativa
--   Lido          completed_at IS NOT NULL
--   Novo          acknowledged_at IS NULL (independente dos quatro estados)

ALTER TABLE online_chapters ADD COLUMN first_seen_at INTEGER;
ALTER TABLE online_chapters ADD COLUMN acknowledged_at INTEGER;
ALTER TABLE online_chapters ADD COLUMN reading_started_at INTEGER;
ALTER TABLE online_chapters ADD COLUMN last_read_at INTEGER;
ALTER TABLE online_chapters ADD COLUMN completed_at INTEGER;
ALTER TABLE online_chapters ADD COLUMN manual_unread INTEGER NOT NULL DEFAULT 0 CHECK (manual_unread IN (0,1));

-- O que já estava registrado não é novidade: só o que chegar daqui em diante pode ser.
UPDATE online_chapters
   SET first_seen_at = coalesce((SELECT o.created_at FROM online_series o WHERE o.id = online_chapters.series_id), 0);
UPDATE online_chapters SET acknowledged_at = first_seen_at;

-- O progresso que já existia vira estado. Abrir na versão anterior gravava a primeira página, e isso não é início de leitura.
UPDATE online_chapters
   SET completed_at = (SELECT q.updated_at FROM chapter_progress q WHERE q.chapter_id = online_chapters.id)
 WHERE comic_id IS NULL
   AND EXISTS (SELECT 1 FROM chapter_progress q WHERE q.chapter_id = online_chapters.id AND q.page_index >= q.page_count - 1);
UPDATE online_chapters
   SET completed_at = (SELECT c.updated_at FROM comics c WHERE c.id = online_chapters.comic_id)
 WHERE comic_id IS NOT NULL
   AND EXISTS (SELECT 1 FROM comics c WHERE c.id = online_chapters.comic_id AND c.status = 'completed');
UPDATE online_chapters
   SET reading_started_at = coalesce(
         (SELECT p.updated_at FROM reading_progress p WHERE p.comic_id = online_chapters.comic_id AND (p.page_index > 0 OR p.scroll_ratio > 0)),
         (SELECT q.updated_at FROM chapter_progress q WHERE q.chapter_id = online_chapters.id AND (q.page_index > 0 OR q.scroll_ratio > 0)),
         completed_at);
UPDATE online_chapters SET last_read_at = coalesce(completed_at, reading_started_at);

-- Capítulos por obra e ordem (anteriores, vizinhos, lista).
CREATE INDEX idx_online_chapters_order ON online_chapters(series_id, sort_order, id);
-- Contador de novos por obra.
CREATE INDEX idx_online_chapters_new ON online_chapters(series_id) WHERE acknowledged_at IS NULL;
-- Lidos e em andamento.
CREATE INDEX idx_online_chapters_completed ON online_chapters(series_id) WHERE completed_at IS NOT NULL;
CREATE INDEX idx_online_chapters_started ON online_chapters(series_id) WHERE reading_started_at IS NOT NULL AND completed_at IS NULL;

CREATE TABLE reading_sessions (
    id                INTEGER PRIMARY KEY,
    chapter_id        INTEGER NOT NULL REFERENCES online_chapters(id) ON DELETE CASCADE,
    started_at        INTEGER NOT NULL,
    last_activity_at  INTEGER NOT NULL,
    ended_at          INTEGER,
    end_reason        TEXT CHECK (end_reason IN ('left','completed','replaced','manual','recovered'))
);
-- No máximo uma sessão aberta por capítulo, e achar a aberta é barato.
CREATE UNIQUE INDEX idx_reading_sessions_open ON reading_sessions(chapter_id) WHERE ended_at IS NULL;
CREATE INDEX idx_reading_sessions_ended ON reading_sessions(ended_at) WHERE ended_at IS NOT NULL;

-- Obras favoritadas (as acompanhadas já têm índice desde a 0003).
CREATE INDEX idx_online_series_favorite ON online_series(favorite) WHERE favorite = 1;
