-- Leitura online: progresso de quem lê sem baixar e o modo de leitura escolhido para cada obra.

CREATE TABLE chapter_progress (
    chapter_id   INTEGER PRIMARY KEY REFERENCES online_chapters(id) ON DELETE CASCADE,
    page_index   INTEGER NOT NULL,
    page_count   INTEGER NOT NULL,
    scroll_ratio REAL NOT NULL DEFAULT 0,
    updated_at   INTEGER NOT NULL
);

ALTER TABLE online_series ADD COLUMN reader_mode TEXT;

-- As fontes viraram um plugin opcional. Quem já usava continua com ele instalado.
INSERT OR IGNORE INTO settings (key, value)
SELECT 'plugins.online', 'true'
WHERE EXISTS (SELECT 1 FROM source_repositories) OR EXISTS (SELECT 1 FROM installed_extensions);
