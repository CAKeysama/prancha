-- Prancha: esquema inicial do catálogo.
-- Datas são armazenadas como milissegundos Unix (INTEGER).

CREATE TABLE comics (
    id              INTEGER PRIMARY KEY,
    path            TEXT    NOT NULL UNIQUE,          -- caminho absoluto do arquivo ou da pasta
    file_name       TEXT    NOT NULL,
    format          TEXT    NOT NULL CHECK (format IN ('cbz','cbr','cb7','cbt','pdf','folder')),
    file_size       INTEGER NOT NULL DEFAULT 0,
    file_mtime      INTEGER,
    fingerprint     TEXT    NOT NULL,                  -- BLAKE3 do conteúdo (ver ARCHITECTURE.md)
    page_count      INTEGER,                           -- NULL enquanto desconhecido
    title           TEXT    NOT NULL,
    series          TEXT,
    issue_number    TEXT,
    volume          TEXT,
    year            INTEGER,
    writer          TEXT,
    artist          TEXT,
    publisher       TEXT,
    summary         TEXT,
    field_sources   TEXT    NOT NULL DEFAULT '{}',     -- JSON {campo: 'file' | 'filename' | 'user'}
    cover_kind      TEXT    NOT NULL DEFAULT 'pending'
                    CHECK (cover_kind IN ('pending','metadata','format','first_page','placeholder')),
    cover_file      TEXT,                              -- nome do arquivo dentro da pasta de capas
    cover_page      INTEGER,
    cover_error     TEXT,
    cover_params    INTEGER NOT NULL DEFAULT 0,
    cover_rev       INTEGER NOT NULL DEFAULT 0,
    favorite        INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0,1)),
    status          TEXT    NOT NULL DEFAULT 'unread' CHECK (status IN ('unread','reading','completed')),
    missing         INTEGER NOT NULL DEFAULT 0 CHECK (missing IN (0,1)),
    search_text     TEXT    NOT NULL DEFAULT '',
    added_at        INTEGER NOT NULL,
    last_opened_at  INTEGER,
    updated_at      INTEGER NOT NULL
);

CREATE INDEX idx_comics_fingerprint ON comics(fingerprint);
CREATE INDEX idx_comics_added_at    ON comics(added_at);
CREATE INDEX idx_comics_opened_at   ON comics(last_opened_at);
CREATE INDEX idx_comics_status      ON comics(status);
CREATE INDEX idx_comics_favorite    ON comics(favorite) WHERE favorite = 1;
CREATE INDEX idx_comics_cover_kind  ON comics(cover_kind);

CREATE TABLE reading_progress (
    comic_id      INTEGER PRIMARY KEY REFERENCES comics(id) ON DELETE CASCADE,
    page_index    INTEGER NOT NULL CHECK (page_index >= 0),
    page_count    INTEGER NOT NULL CHECK (page_count >= 0),
    scroll_ratio  REAL    NOT NULL DEFAULT 0 CHECK (scroll_ratio >= 0 AND scroll_ratio <= 1),
    updated_at    INTEGER NOT NULL
);

CREATE TABLE collections (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE comic_collections (
    comic_id      INTEGER NOT NULL REFERENCES comics(id) ON DELETE CASCADE,
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    added_at      INTEGER NOT NULL,
    PRIMARY KEY (comic_id, collection_id)
);
CREATE INDEX idx_comic_collections_collection ON comic_collections(collection_id);

CREATE TABLE tags (
    id    INTEGER PRIMARY KEY,
    name  TEXT NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE comic_tags (
    comic_id  INTEGER NOT NULL REFERENCES comics(id) ON DELETE CASCADE,
    tag_id    INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (comic_id, tag_id)
);
CREATE INDEX idx_comic_tags_tag ON comic_tags(tag_id);

CREATE TABLE settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL                                 -- JSON
);

CREATE TABLE import_jobs (
    id           INTEGER PRIMARY KEY,
    status       TEXT    NOT NULL CHECK (status IN ('running','finished','cancelled','interrupted')),
    total        INTEGER NOT NULL DEFAULT 0,
    processed    INTEGER NOT NULL DEFAULT 0,
    added        INTEGER NOT NULL DEFAULT 0,
    skipped      INTEGER NOT NULL DEFAULT 0,
    failed       INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    finished_at  INTEGER
);

CREATE TABLE import_job_items (
    id         INTEGER PRIMARY KEY,
    job_id     INTEGER NOT NULL REFERENCES import_jobs(id) ON DELETE CASCADE,
    path       TEXT    NOT NULL,
    status     TEXT    NOT NULL CHECK (status IN ('added','relinked','duplicate','skipped','failed')),
    message    TEXT,
    comic_id   INTEGER REFERENCES comics(id) ON DELETE SET NULL
);
CREATE INDEX idx_import_job_items_job ON import_job_items(job_id);
