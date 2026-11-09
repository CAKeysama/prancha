-- Classificação +18 por evidências. O estado decide o filtro; `adult` e `classification` continuam como estavam
-- (`classification` ainda diz se os detalhes da obra já foram consultados).
-- confirmed: evidências fortes de conteúdo adulto. suspect: indícios sem confirmação.
-- none: nenhuma evidência encontrada, o que não quer dizer verificada. insufficient: não há o que avaliar.
ALTER TABLE online_series ADD COLUMN rating_state TEXT NOT NULL DEFAULT 'insufficient' CHECK (rating_state IN ('confirmed','suspect','none','insufficient'));
-- O que o classificador concluiu sozinho, mesmo quando a pessoa corrigiu.
ALTER TABLE online_series ADD COLUMN rating_auto TEXT NOT NULL DEFAULT 'insufficient' CHECK (rating_auto IN ('confirmed','suspect','none','insufficient'));
ALTER TABLE online_series ADD COLUMN rating_score INTEGER NOT NULL DEFAULT 0 CHECK (rating_score BETWEEN 0 AND 100);
-- repository: a fonte declarou. inferred: o Prancha deduziu dos metadados. manual: a pessoa corrigiu.
ALTER TABLE online_series ADD COLUMN rating_origin TEXT NOT NULL DEFAULT 'inferred' CHECK (rating_origin IN ('repository','inferred','manual'));
-- Versão do classificador da última análise. 0: nunca analisada, e a próxima abertura do app analisa.
ALTER TABLE online_series ADD COLUMN rating_version INTEGER NOT NULL DEFAULT 0;
-- Resumo dos metadados usados: igual ao anterior, a análise não se repete.
ALTER TABLE online_series ADD COLUMN rating_fingerprint TEXT;
ALTER TABLE online_series ADD COLUMN rated_at INTEGER;
-- A classificação que a própria obra declarou na última vez que declarou ('general','teen','adult','unknown').
ALTER TABLE online_series ADD COLUMN declared_rating TEXT CHECK (declared_rating IN ('general','teen','adult','unknown'));
-- Quando a fonte declarou a obra como +18 pela última vez. Se ela deixar de declarar, isso continua contando como histórico.
ALTER TABLE online_series ADD COLUMN declared_adult_at INTEGER;

-- Até a primeira análise, o mesmo que o filtro antigo decidia: nada que estava escondido aparece.
UPDATE online_series SET rating_state = CASE WHEN adult = 1 THEN 'suspect' WHEN classification = 'known' THEN 'none' ELSE 'insufficient' END;
UPDATE online_series SET rating_auto = rating_state;
-- Nas extensões nativas, obra marcada como adulta numa fonte livre só pode ter vindo da classificação da própria obra.
UPDATE online_series SET declared_rating = 'adult', declared_adult_at = updated_at
 WHERE adult = 1
   AND EXISTS (SELECT 1 FROM extension_sources s WHERE s.extension_id = online_series.extension_id AND s.source_id = online_series.source_id AND s.rating IN ('general','teen'))
   AND extension_id IN (SELECT id FROM installed_extensions WHERE CASE WHEN json_valid(entry) THEN coalesce(json_extract(entry, '$.runtime'), '') <> 'suwayomi' ELSE 0 END);

CREATE INDEX idx_online_series_rating ON online_series(rating_state);
CREATE INDEX idx_online_series_rating_version ON online_series(rating_version);

-- As evidências da última análise. Saem com a obra e são refeitas a cada análise.
CREATE TABLE series_rating_evidence (
    series_id    INTEGER NOT NULL REFERENCES online_series(id) ON DELETE CASCADE,
    position     INTEGER NOT NULL,
    kind         TEXT    NOT NULL,   -- rating, source_rating, tag, description, title, alt_title, cover, history
    origin       TEXT    NOT NULL,   -- de onde veio, em palavras
    value        TEXT    NOT NULL,   -- o que foi encontrado
    reliability  REAL    NOT NULL,   -- 0 a 1
    points       INTEGER NOT NULL,   -- quanto vale sozinha, antes da confiabilidade
    verdict      TEXT    NOT NULL,   -- adult, suggestive, safe, ignored, superseded, inconclusive
    observed_at  INTEGER NOT NULL,
    PRIMARY KEY (series_id, position)
) WITHOUT ROWID;

-- Análise visual da capa, separada das outras evidências. Nenhum analisador vem com o app: a tabela é onde um entraria.
CREATE TABLE series_cover_analysis (
    series_id    INTEGER PRIMARY KEY REFERENCES online_series(id) ON DELETE CASCADE,
    analyzer     TEXT    NOT NULL,
    likelihood   REAL    NOT NULL CHECK (likelihood BETWEEN 0 AND 1),
    confidence   REAL    NOT NULL CHECK (confidence BETWEEN 0 AND 1),
    analyzed_at  INTEGER NOT NULL
);

-- Correções da pessoa. Pelo endereço da obra, sem chave estrangeira: valem depois de a obra sair do banco e voltar.
CREATE TABLE series_rating_overrides (
    extension_id  TEXT    NOT NULL,
    source_id     TEXT    NOT NULL,
    remote_id     TEXT    NOT NULL,
    state         TEXT    NOT NULL CHECK (state IN ('confirmed','none')),
    reason        TEXT,
    set_at        INTEGER NOT NULL,
    PRIMARY KEY (extension_id, source_id, remote_id)
) WITHOUT ROWID;

-- Cada mudança de estado: as correções da pessoa e o que mudou sozinho quando os metadados mudaram.
CREATE TABLE series_rating_history (
    id              INTEGER PRIMARY KEY,
    extension_id    TEXT    NOT NULL,
    source_id       TEXT    NOT NULL,
    remote_id       TEXT    NOT NULL,
    previous_state  TEXT,
    new_state       TEXT    NOT NULL,
    origin          TEXT    NOT NULL CHECK (origin IN ('manual','automatic')),
    reason          TEXT,
    changed_at      INTEGER NOT NULL
);
CREATE INDEX idx_series_rating_history_series ON series_rating_history(extension_id, source_id, remote_id, changed_at);
