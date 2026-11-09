-- Central de notificações: um histórico que fica, não uma sequência de avisos que somem.
-- A notificação só aponta para obras, capítulos e HQs. Apagar uma delas não apaga nada do resto, e
-- apagar o destino deixa a referência vazia: a ação avisa que o destino não existe mais.

CREATE TABLE notifications (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    category    TEXT NOT NULL CHECK (category IN ('chapters','updates','system','downloads','library')),
    kind        TEXT NOT NULL,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL DEFAULT '',
    series_id   INTEGER REFERENCES online_series(id) ON DELETE SET NULL,
    chapter_id  INTEGER REFERENCES online_chapters(id) ON DELETE SET NULL,
    comic_id    INTEGER REFERENCES comics(id) ON DELETE SET NULL,
    -- Ação principal, por tipo: o destino sai das referências acima, nunca de uma rota guardada.
    action      TEXT,
    -- Dados do evento (versão, quantidades). Nunca uma rota ou um endereço.
    payload     TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(payload)),
    -- Eventos do mesmo grupo, num intervalo curto, somam na mesma notificação enquanto ela não for lida.
    group_key   TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    read_at     INTEGER
);
CREATE INDEX idx_notifications_recent ON notifications(created_at DESC, id DESC);
CREATE INDEX idx_notifications_category ON notifications(category, created_at DESC, id DESC);
CREATE INDEX idx_notifications_unread ON notifications(created_at DESC, id DESC) WHERE read_at IS NULL;
CREATE INDEX idx_notifications_series ON notifications(series_id) WHERE series_id IS NOT NULL;
CREATE INDEX idx_notifications_group ON notifications(group_key, created_at) WHERE group_key IS NOT NULL AND read_at IS NULL;

-- Capítulos que a notificação anuncia. Sai junto com a notificação ou com o capítulo.
CREATE TABLE notification_chapters (
    notification_id INTEGER NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
    chapter_id      INTEGER NOT NULL REFERENCES online_chapters(id) ON DELETE CASCADE,
    PRIMARY KEY (notification_id, chapter_id)
) WITHOUT ROWID;
CREATE INDEX idx_notification_chapters_chapter ON notification_chapters(chapter_id);

-- Cada evento já anunciado, pela chave de deduplicação (tipo + referências estáveis). Fica mesmo
-- depois de a notificação ser removida ou marcada como não lida: o mesmo evento não volta.
CREATE TABLE notification_events (
    dedup_key       TEXT PRIMARY KEY,
    notification_id INTEGER REFERENCES notifications(id) ON DELETE SET NULL,
    created_at      INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX idx_notification_events_created ON notification_events(created_at);
