-- Leitura Guiada: análise automática por página e correções do usuário, guardadas à parte.

CREATE TABLE guided_pages (
    comic_id          INTEGER NOT NULL REFERENCES comics(id) ON DELETE CASCADE,
    page_index        INTEGER NOT NULL,
    fingerprint       TEXT NOT NULL,
    detector_version  INTEGER NOT NULL,
    width             INTEGER NOT NULL,
    height            INTEGER NOT NULL,
    panels            TEXT NOT NULL,
    analyzed_at       INTEGER NOT NULL,
    PRIMARY KEY (comic_id, page_index)
) WITHOUT ROWID;

CREATE TABLE guided_corrections (
    comic_id     INTEGER NOT NULL REFERENCES comics(id) ON DELETE CASCADE,
    page_index   INTEGER NOT NULL,
    fingerprint  TEXT NOT NULL,
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (comic_id, page_index)
) WITHOUT ROWID;

-- source: 'auto' (substituível por uma nova análise) ou 'user' (só muda pela mão do usuário).
CREATE TABLE guided_regions (
    comic_id       INTEGER NOT NULL REFERENCES comics(id) ON DELETE CASCADE,
    page_index     INTEGER NOT NULL,
    source         TEXT NOT NULL CHECK (source IN ('auto', 'user')),
    region_id      TEXT NOT NULL,
    x              REAL NOT NULL,
    y              REAL NOT NULL,
    w              REAL NOT NULL,
    h              REAL NOT NULL,
    kind           TEXT NOT NULL,
    reading_order  INTEGER NOT NULL,
    confidence     REAL,
    review         TEXT NOT NULL,
    -- Contorno do balão em frações da própria região (JSON); vazio em regiões sem forma conhecida.
    outline        TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (comic_id, page_index, source, region_id)
) WITHOUT ROWID;
