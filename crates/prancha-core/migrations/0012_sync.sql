-- Conta do Prancha e sincronização da leitura online entre aparelhos. Datas em milissegundos Unix.
-- Ver src/account/, src/sync/ e docs/SINCRONIZACAO.md.
--
-- Nada aqui muda o que já existe. Os gatilhos só anotam quando a pessoa mudou algo que sobe para a nuvem
-- (`sync_*_meta`) e qual obra precisa ser conferida no próximo envio (`sync_dirty`). Quando a sincronização
-- aplica o que veio de outro aparelho, liga `sync_state.applying` e os gatilhos ficam quietos.

-- O aparelho e o andamento da sincronização. Uma linha só.
CREATE TABLE sync_state (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    device_id       TEXT    NOT NULL,                 -- uuid deste aparelho na conta
    applying        INTEGER NOT NULL DEFAULT 0 CHECK (applying IN (0,1)),
    cursor          INTEGER NOT NULL DEFAULT 0,       -- o último `seq` recebido da nuvem
    full_push       INTEGER NOT NULL DEFAULT 1 CHECK (full_push IN (0,1)),  -- o próximo envio confere tudo
    last_sync_at    INTEGER,
    last_error      TEXT
);
INSERT INTO sync_state (id, device_id)
SELECT 1, lower(substr(h, 1, 8) || '-' || substr(h, 9, 4) || '-4' || substr(h, 14, 3) || '-a' || substr(h, 18, 3) || '-' || substr(h, 21, 12))
  FROM (SELECT hex(randomblob(16)) AS h);

-- A conta conectada: no máximo uma. Os tokens ficam só cifrados pelo cofre do sistema, como os do MyAnimeList.
-- expired: a nuvem recusou a renovação; a sincronização espera a pessoa entrar de novo.
CREATE TABLE sync_account (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    user_id       TEXT    NOT NULL,
    email         TEXT,
    name          TEXT,
    provider      TEXT    NOT NULL,
    secret        BLOB    NOT NULL,
    expires_at    INTEGER NOT NULL,
    signed_in_at  INTEGER NOT NULL,
    state         TEXT    NOT NULL DEFAULT 'signed_in' CHECK (state IN ('signed_in','expired'))
);

-- A pasta tem um id que vale em todos os aparelhos.
ALTER TABLE collections ADD COLUMN sync_id TEXT;
UPDATE collections SET sync_id = lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-a' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)));
CREATE UNIQUE INDEX idx_collections_sync_id ON collections(sync_id);

-- Obras que mudaram desde o último envio (`kind` = 'series', `ref` = online_series.id), e as pastas e o catálogo de
-- extensões (`ref` = 0).
CREATE TABLE sync_dirty (
    kind  TEXT    NOT NULL CHECK (kind IN ('series','collections','catalog')),
    ref   INTEGER NOT NULL,
    PRIMARY KEY (kind, ref)
) WITHOUT ROWID;

-- Quando cada campo da obra mudou neste aparelho.
CREATE TABLE sync_series_meta (
    series_id        INTEGER PRIMARY KEY REFERENCES online_series(id) ON DELETE CASCADE,
    following_at     INTEGER,
    favorite_at      INTEGER,
    chapter_sort_at  INTEGER,
    reader_mode_at   INTEGER
);

-- Obras que saíram do banco deste aparelho (tiradas da biblioteca, bloqueadas ou esquecidas), para a saída subir.
-- Só sobe a de uma obra que a nuvem conhece.
CREATE TABLE sync_series_gone (
    extension_id  TEXT    NOT NULL,
    source_id     TEXT    NOT NULL,
    remote_id     TEXT    NOT NULL,
    removed_at    INTEGER NOT NULL,
    PRIMARY KEY (extension_id, source_id, remote_id)
) WITHOUT ROWID;

-- Quando o capítulo virou lido ou não lido neste aparelho (ou veio assim da nuvem).
CREATE TABLE sync_chapter_meta (
    chapter_id  INTEGER PRIMARY KEY REFERENCES online_chapters(id) ON DELETE CASCADE,
    read_at     INTEGER NOT NULL
);

-- Quando a pasta foi criada ou renomeada, e as pastas apagadas que ainda não subiram.
CREATE TABLE sync_collection_meta (
    collection_id  INTEGER PRIMARY KEY REFERENCES collections(id) ON DELETE CASCADE,
    changed_at     INTEGER NOT NULL
);
CREATE TABLE sync_collection_gone (
    sync_id     TEXT    PRIMARY KEY,
    removed_at  INTEGER NOT NULL
) WITHOUT ROWID;

-- Entrada e saída de obra em pasta. Sem chave estrangeira: a saída precisa ficar anotada até subir.
CREATE TABLE sync_member_meta (
    series_id      INTEGER NOT NULL,
    collection_id  INTEGER NOT NULL,
    member         INTEGER NOT NULL CHECK (member IN (0,1)),
    changed_at     INTEGER NOT NULL,
    PRIMARY KEY (series_id, collection_id)
) WITHOUT ROWID;

-- Associações com o MyAnimeList feitas à mão (ou desfeitas), pelo endereço da obra.
CREATE TABLE sync_mal_meta (
    extension_id  TEXT    NOT NULL,
    source_id     TEXT    NOT NULL,
    remote_id     TEXT    NOT NULL,
    changed_at    INTEGER NOT NULL,
    PRIMARY KEY (extension_id, source_id, remote_id)
) WITHOUT ROWID;

-- Repositórios e extensões: quando entraram, mudaram ou saíram deste aparelho.
CREATE TABLE sync_catalog_meta (
    kind        TEXT    NOT NULL CHECK (kind IN ('repository','extension')),
    key         TEXT    NOT NULL,                     -- endereço do repositório ou id da extensão
    removed     INTEGER NOT NULL DEFAULT 0 CHECK (removed IN (0,1)),
    changed_at  INTEGER NOT NULL,
    PRIMARY KEY (kind, key)
) WITHOUT ROWID;

-- O último estado que a nuvem informou. Fica guardado mesmo quando ainda não há onde aplicar (o capítulo ainda não
-- apareceu na lista deste aparelho), e é aplicado assim que der.
CREATE TABLE sync_remote_series (
    series_key  TEXT    PRIMARY KEY,
    data        TEXT    NOT NULL,                     -- JSON como a nuvem devolve
    series_id   INTEGER                               -- a obra local, quando já foi encontrada ou criada
) WITHOUT ROWID;
CREATE TABLE sync_remote_chapters (
    series_key   TEXT NOT NULL,
    chapter_key  TEXT NOT NULL,
    data         TEXT NOT NULL,
    PRIMARY KEY (series_key, chapter_key)
) WITHOUT ROWID;
CREATE TABLE sync_remote_items (
    kind  TEXT NOT NULL CHECK (kind IN ('repository','extension','collection','series_collection','mal_link')),
    key   TEXT NOT NULL,
    data  TEXT NOT NULL,
    PRIMARY KEY (kind, key)
) WITHOUT ROWID;

-- ---------------------------------------------------------------------------------------------------------------------
-- Gatilhos. Ficam quietos enquanto `sync_state.applying` = 1 (a sincronização aplicando o que veio da nuvem).
-- Sem `INSERT OR IGNORE`/`OR REPLACE`: dentro de um gatilho, o SQLite troca essa política pela do comando que
-- disparou o gatilho (um upsert, por exemplo), e a linha repetida vira erro. `ON CONFLICT` vale sempre.
-- ---------------------------------------------------------------------------------------------------------------------

CREATE TRIGGER sync_series_following AFTER UPDATE OF following ON online_series
WHEN old.following IS NOT new.following AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_series_meta (series_id, following_at) VALUES (new.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (series_id) DO UPDATE SET following_at = excluded.following_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('series', new.id) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_series_favorite AFTER UPDATE OF favorite ON online_series
WHEN old.favorite IS NOT new.favorite AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_series_meta (series_id, favorite_at) VALUES (new.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (series_id) DO UPDATE SET favorite_at = excluded.favorite_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('series', new.id) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_series_chapter_sort AFTER UPDATE OF chapter_sort ON online_series
WHEN old.chapter_sort IS NOT new.chapter_sort AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_series_meta (series_id, chapter_sort_at) VALUES (new.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (series_id) DO UPDATE SET chapter_sort_at = excluded.chapter_sort_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('series', new.id) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_series_reader_mode AFTER UPDATE OF reader_mode ON online_series
WHEN old.reader_mode IS NOT new.reader_mode AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_series_meta (series_id, reader_mode_at) VALUES (new.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (series_id) DO UPDATE SET reader_mode_at = excluded.reader_mode_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('series', new.id) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_series_delete AFTER DELETE ON online_series WHEN (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_series_gone (extension_id, source_id, remote_id, removed_at)
    VALUES (old.extension_id, old.source_id, old.remote_id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (extension_id, source_id, remote_id) DO UPDATE SET removed_at = excluded.removed_at;
END;

CREATE TRIGGER sync_chapter_read AFTER UPDATE OF completed_at, manual_unread ON online_chapters
WHEN ((old.completed_at IS NULL) <> (new.completed_at IS NULL) OR old.manual_unread <> new.manual_unread)
     AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_chapter_meta (chapter_id, read_at) VALUES (new.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (chapter_id) DO UPDATE SET read_at = excluded.read_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('series', new.series_id) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_chapter_started AFTER UPDATE OF reading_started_at, acknowledged_at ON online_chapters
WHEN (old.reading_started_at IS NOT new.reading_started_at OR old.acknowledged_at IS NOT new.acknowledged_at)
     AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_dirty (kind, ref) VALUES ('series', new.series_id) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_progress_insert AFTER INSERT ON chapter_progress WHEN (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_dirty (kind, ref) SELECT 'series', series_id FROM online_chapters WHERE id = new.chapter_id ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_progress_update AFTER UPDATE ON chapter_progress WHEN (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_dirty (kind, ref) SELECT 'series', series_id FROM online_chapters WHERE id = new.chapter_id ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_member_insert AFTER INSERT ON series_collections WHEN (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_member_meta (series_id, collection_id, member, changed_at)
    VALUES (new.series_id, new.collection_id, 1, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (series_id, collection_id) DO UPDATE SET member = 1, changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('series', new.series_id) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_member_delete AFTER DELETE ON series_collections WHEN (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_member_meta (series_id, collection_id, member, changed_at)
    VALUES (old.series_id, old.collection_id, 0, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (series_id, collection_id) DO UPDATE SET member = 0, changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('series', old.series_id) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_collection_insert AFTER INSERT ON collections
BEGIN
    UPDATE collections
       SET sync_id = lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-a' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)))
     WHERE id = new.id AND sync_id IS NULL;
    INSERT INTO sync_collection_meta (collection_id, changed_at)
    SELECT new.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) WHERE (SELECT applying FROM sync_state) = 0;
    INSERT INTO sync_dirty (kind, ref) SELECT 'collections', 0 WHERE (SELECT applying FROM sync_state) = 0 ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_collection_rename AFTER UPDATE OF name ON collections
WHEN old.name IS NOT new.name AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_collection_meta (collection_id, changed_at) VALUES (new.id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (collection_id) DO UPDATE SET changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('collections', 0) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_collection_delete AFTER DELETE ON collections
WHEN old.sync_id IS NOT NULL AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_collection_gone (sync_id, removed_at) VALUES (old.sync_id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)) ON CONFLICT (sync_id) DO UPDATE SET removed_at = excluded.removed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('collections', 0) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_mal_insert AFTER INSERT ON mal_links
WHEN new.origin IN ('manual','none') AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_mal_meta (extension_id, source_id, remote_id, changed_at)
    VALUES (new.extension_id, new.source_id, new.remote_id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (extension_id, source_id, remote_id) DO UPDATE SET changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref)
    SELECT 'series', id FROM online_series WHERE extension_id = new.extension_id AND source_id = new.source_id AND remote_id = new.remote_id
    ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_mal_delete AFTER DELETE ON mal_links
WHEN old.origin IN ('manual','none') AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_mal_meta (extension_id, source_id, remote_id, changed_at)
    VALUES (old.extension_id, old.source_id, old.remote_id, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (extension_id, source_id, remote_id) DO UPDATE SET changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref)
    SELECT 'series', id FROM online_series WHERE extension_id = old.extension_id AND source_id = old.source_id AND remote_id = old.remote_id
    ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_repository_insert AFTER INSERT ON source_repositories WHEN (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_catalog_meta (kind, key, removed, changed_at) VALUES ('repository', new.url, 0, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (kind, key) DO UPDATE SET removed = excluded.removed, changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('catalog', 0) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_repository_update AFTER UPDATE OF enabled, public_key ON source_repositories
WHEN (old.enabled IS NOT new.enabled OR old.public_key IS NOT new.public_key) AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_catalog_meta (kind, key, removed, changed_at) VALUES ('repository', new.url, 0, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (kind, key) DO UPDATE SET removed = excluded.removed, changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('catalog', 0) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_repository_delete AFTER DELETE ON source_repositories WHEN (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_catalog_meta (kind, key, removed, changed_at) VALUES ('repository', old.url, 1, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (kind, key) DO UPDATE SET removed = excluded.removed, changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('catalog', 0) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_extension_insert AFTER INSERT ON installed_extensions WHEN (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_catalog_meta (kind, key, removed, changed_at) VALUES ('extension', new.id, 0, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (kind, key) DO UPDATE SET removed = excluded.removed, changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('catalog', 0) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_extension_update AFTER UPDATE OF enabled, version ON installed_extensions
WHEN (old.enabled IS NOT new.enabled OR old.version IS NOT new.version) AND (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_catalog_meta (kind, key, removed, changed_at) VALUES ('extension', new.id, 0, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (kind, key) DO UPDATE SET removed = excluded.removed, changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('catalog', 0) ON CONFLICT DO NOTHING;
END;

CREATE TRIGGER sync_extension_delete AFTER DELETE ON installed_extensions WHEN (SELECT applying FROM sync_state) = 0
BEGIN
    INSERT INTO sync_catalog_meta (kind, key, removed, changed_at) VALUES ('extension', old.id, 1, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER))
    ON CONFLICT (kind, key) DO UPDATE SET removed = excluded.removed, changed_at = excluded.changed_at;
    INSERT INTO sync_dirty (kind, ref) VALUES ('catalog', 0) ON CONFLICT DO NOTHING;
END;
