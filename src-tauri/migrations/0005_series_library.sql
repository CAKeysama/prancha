-- Obras das fontes na biblioteca: favoritos, pastas e a marca que o filtro +18 usa.

ALTER TABLE online_series ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0,1));
-- Guardada na obra porque precisa valer mesmo depois de a extensão sair.
ALTER TABLE online_series ADD COLUMN adult INTEGER NOT NULL DEFAULT 0 CHECK (adult IN (0,1));

CREATE TABLE series_collections (
    series_id     INTEGER NOT NULL REFERENCES online_series(id) ON DELETE CASCADE,
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    added_at      INTEGER NOT NULL,
    PRIMARY KEY (series_id, collection_id)
) WITHOUT ROWID;
CREATE INDEX idx_series_collections_collection ON series_collections(collection_id);
