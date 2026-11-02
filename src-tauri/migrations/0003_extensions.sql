-- Extensões: repositórios, fontes, obras online e downloads. Datas em milissegundos Unix.

CREATE TABLE source_repositories (
    id                  TEXT    PRIMARY KEY,               -- identificador declarado no manifesto
    url                 TEXT    NOT NULL UNIQUE,           -- endereço do manifesto
    name                TEXT    NOT NULL,
    description         TEXT,
    website             TEXT,
    index_url           TEXT    NOT NULL,
    public_key          TEXT    NOT NULL,                  -- fixada na confirmação da pessoa
    key_fingerprint     TEXT    NOT NULL,
    pending_public_key  TEXT,                              -- chave nova anunciada, à espera de confirmação
    enabled             INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    status              TEXT    NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','ok','error','key_changed')),
    last_error          TEXT,
    last_sync_at        INTEGER,
    last_success_at     INTEGER,
    index_generated_at  INTEGER,                           -- último índice aceito: um mais antigo é recusado
    index_etag          TEXT,
    extension_count     INTEGER NOT NULL DEFAULT 0,
    added_at            INTEGER NOT NULL
);

-- Catálogo: a última versão válida do índice assinado.
CREATE TABLE repository_extensions (
    repository_id  TEXT NOT NULL REFERENCES source_repositories(id) ON DELETE CASCADE,
    extension_id   TEXT NOT NULL,
    name           TEXT NOT NULL,
    version        TEXT NOT NULL,
    entry          TEXT NOT NULL,                          -- JSON validado
    PRIMARY KEY (repository_id, extension_id)
);
CREATE INDEX idx_repository_extensions_extension ON repository_extensions(extension_id);

-- A procedência é gravada na instalação e sobrevive à remoção do repositório, por isso sem chave estrangeira.
CREATE TABLE installed_extensions (
    id               TEXT    PRIMARY KEY,
    repository_id    TEXT    NOT NULL,
    repository_name  TEXT    NOT NULL,
    public_key       TEXT    NOT NULL,
    key_fingerprint  TEXT    NOT NULL,
    name             TEXT    NOT NULL,
    version          TEXT    NOT NULL,
    entry            TEXT    NOT NULL,                     -- JSON assinado da versão instalada
    file_name        TEXT    NOT NULL,                     -- dentro da pasta de extensões
    sha256           TEXT    NOT NULL,
    enabled          INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    load_error       TEXT,
    installed_at     INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE TABLE extension_sources (
    extension_id     TEXT    NOT NULL REFERENCES installed_extensions(id) ON DELETE CASCADE,
    source_id        TEXT    NOT NULL,
    name             TEXT    NOT NULL,
    language         TEXT    NOT NULL,
    description      TEXT,
    kinds            TEXT    NOT NULL DEFAULT '[]',        -- JSON
    rating           TEXT    NOT NULL CHECK (rating IN ('general','teen','adult','unknown')),
    direction        TEXT    NOT NULL CHECK (direction IN ('ltr','rtl','vertical')),
    supports_latest  INTEGER NOT NULL CHECK (supports_latest IN (0,1)),
    supports_search  INTEGER NOT NULL CHECK (supports_search IN (0,1)),
    min_interval_ms  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (extension_id, source_id)
) WITHOUT ROWID;

-- Sem chave estrangeira para a extensão: desinstalar não apaga obras nem capítulos baixados.
CREATE TABLE online_series (
    id               INTEGER PRIMARY KEY,
    extension_id     TEXT    NOT NULL,
    source_id        TEXT    NOT NULL,
    remote_id        TEXT    NOT NULL,
    title            TEXT    NOT NULL,
    cover_url        TEXT,
    summary          TEXT,
    author           TEXT,
    genres           TEXT    NOT NULL DEFAULT '[]',        -- JSON
    status           TEXT,
    language         TEXT,
    following        INTEGER NOT NULL DEFAULT 0 CHECK (following IN (0,1)),
    chapter_sort     TEXT    NOT NULL DEFAULT 'desc' CHECK (chapter_sort IN ('asc','desc')),
    last_checked_at  INTEGER,
    last_update_at   INTEGER,                              -- data do capítulo mais novo que a fonte informou
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    UNIQUE (extension_id, source_id, remote_id)
);
CREATE INDEX idx_online_series_following ON online_series(following) WHERE following = 1;

CREATE TABLE online_chapters (
    id            INTEGER PRIMARY KEY,
    series_id     INTEGER NOT NULL REFERENCES online_series(id) ON DELETE CASCADE,
    remote_id     TEXT    NOT NULL,
    title         TEXT    NOT NULL,
    number        TEXT,
    published_at  INTEGER,
    sort_order    INTEGER NOT NULL,
    file_path     TEXT,                                    -- CBZ gerado pelo Prancha
    comic_id      INTEGER REFERENCES comics(id) ON DELETE SET NULL,
    UNIQUE (series_id, remote_id)
);
CREATE INDEX idx_online_chapters_comic ON online_chapters(comic_id) WHERE comic_id IS NOT NULL;

CREATE TABLE chapter_downloads (
    id           INTEGER PRIMARY KEY,
    chapter_id   INTEGER NOT NULL REFERENCES online_chapters(id) ON DELETE CASCADE,
    status       TEXT    NOT NULL CHECK (status IN ('queued','running','paused','completed','failed','cancelled')),
    pages_done   INTEGER NOT NULL DEFAULT 0,
    pages_total  INTEGER NOT NULL DEFAULT 0,
    bytes        INTEGER NOT NULL DEFAULT 0,
    error        TEXT,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    finished_at  INTEGER
);
-- Um capítulo tem no máximo um download em aberto.
CREATE UNIQUE INDEX idx_chapter_downloads_open ON chapter_downloads(chapter_id) WHERE status IN ('queued','running','paused');
CREATE INDEX idx_chapter_downloads_status ON chapter_downloads(status, id);

CREATE TABLE extension_storage (
    extension_id  TEXT NOT NULL REFERENCES installed_extensions(id) ON DELETE CASCADE,
    key           TEXT NOT NULL,
    value         BLOB NOT NULL,
    PRIMARY KEY (extension_id, key)
) WITHOUT ROWID;
