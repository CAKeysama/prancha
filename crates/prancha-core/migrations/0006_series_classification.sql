-- O filtro +18 só deixa passar a obra que tem classificação. Nas extensões do Mihon, a marca da fonte não vale para cada obra,
-- e a lista nem sempre traz os gêneros: quem decide são os detalhes da obra.
-- pending: ninguém consultou ainda. unknown: consultou, e a fonte não informa gêneros. known: há classificação.
ALTER TABLE online_series ADD COLUMN classification TEXT NOT NULL DEFAULT 'pending' CHECK (classification IN ('pending','unknown','known'));

-- Extensões nativas declaram a classificação da fonte e de cada obra. Abrir a obra já consultou os detalhes.
UPDATE online_series SET classification = 'known'
 WHERE genres <> '[]'
    OR extension_id IN (SELECT id FROM installed_extensions WHERE CASE WHEN json_valid(entry) THEN coalesce(json_extract(entry, '$.runtime'), '') <> 'suwayomi' ELSE 0 END);
UPDATE online_series SET classification = 'unknown' WHERE classification = 'pending' AND last_checked_at IS NOT NULL;

-- Obras que indicam sexualização de menores: só o endereço, para nunca mais consultar nem mostrar.
CREATE TABLE blocked_series (
    extension_id  TEXT NOT NULL,
    source_id     TEXT NOT NULL,
    remote_id     TEXT NOT NULL,
    PRIMARY KEY (extension_id, source_id, remote_id)
) WITHOUT ROWID;
