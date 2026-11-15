-- Integração com o MyAnimeList. Datas em milissegundos Unix. Ver src-tauri/src/mal/ e docs/MYANIMELIST.md.

-- A conta conectada: no máximo uma. Os tokens ficam só cifrados pelo sistema (DPAPI no Windows), nunca em texto.
-- expired: o MAL recusou a renovação; as operações esperam a pessoa conectar de novo.
CREATE TABLE mal_account (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    user_id       INTEGER,
    user_name     TEXT    NOT NULL,
    secret        BLOB    NOT NULL,
    expires_at    INTEGER NOT NULL,
    connected_at  INTEGER NOT NULL,
    state         TEXT    NOT NULL DEFAULT 'connected' CHECK (state IN ('connected','expired')),
    last_sync_at  INTEGER
);

-- Associação de cada obra das fontes a uma obra do MAL. Pelo endereço da obra, sem chave estrangeira:
-- vale depois de a obra sair da biblioteca e voltar, e não depende da conta conectada.
-- mal_id NULL com origin 'none': a pessoa disse que a obra não está no MAL, e nada é procurado de novo.
CREATE TABLE mal_links (
    extension_id     TEXT    NOT NULL,
    source_id        TEXT    NOT NULL,
    remote_id        TEXT    NOT NULL,
    mal_id           INTEGER,
    mal_title        TEXT,
    media_type       TEXT,
    num_chapters     INTEGER,
    origin           TEXT    NOT NULL CHECK (origin IN ('auto','manual','none')),
    confidence       INTEGER NOT NULL DEFAULT 0,
    -- O último número de capítulos lidos confirmado no MAL, para não repetir a mesma atualização.
    synced_chapters  INTEGER,
    linked_at        INTEGER NOT NULL,
    PRIMARY KEY (extension_id, source_id, remote_id),
    CHECK ((origin = 'none') = (mal_id IS NULL))
) WITHOUT ROWID;
CREATE INDEX idx_mal_links_mal ON mal_links(mal_id) WHERE mal_id IS NOT NULL;

-- A última procura automática, para não repetir a mesma pesquisa a cada abertura da obra.
CREATE TABLE mal_lookups (
    extension_id  TEXT    NOT NULL,
    source_id     TEXT    NOT NULL,
    remote_id     TEXT    NOT NULL,
    checked_at    INTEGER NOT NULL,
    candidates    TEXT    NOT NULL DEFAULT '[]',   -- JSON
    PRIMARY KEY (extension_id, source_id, remote_id)
) WITHOUT ROWID;

-- Fila de sincronização. Uma linha por obra do MAL: concluir outro capítulo só aumenta o alvo, nunca duplica.
-- pending: espera a vez (next_attempt_at). failed: desistiu depois das tentativas, ou o MAL recusou; aparece à pessoa.
CREATE TABLE mal_sync_queue (
    id               INTEGER PRIMARY KEY,
    mal_id           INTEGER NOT NULL UNIQUE,
    mal_title        TEXT,
    extension_id     TEXT    NOT NULL,
    source_id        TEXT    NOT NULL,
    remote_id        TEXT    NOT NULL,
    chapters         INTEGER NOT NULL CHECK (chapters > 0),
    status           TEXT    NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','failed')),
    attempts         INTEGER NOT NULL DEFAULT 0,
    next_attempt_at  INTEGER NOT NULL,
    last_error       TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);
CREATE INDEX idx_mal_sync_queue_due ON mal_sync_queue(status, next_attempt_at);
