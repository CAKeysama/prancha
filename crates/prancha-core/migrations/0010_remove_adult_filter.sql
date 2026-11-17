-- O filtro +18 saiu do aplicativo: todas as obras aparecem. Saem a classificação por evidências (0009),
-- a marca antiga `adult` (0005) e as preferências do filtro. Ficam `classification`, que diz se os detalhes da obra
-- já foram conferidos, e `blocked_series`, o bloqueio de obras que indicam sexualização de menores.
DROP INDEX IF EXISTS idx_online_series_rating;
DROP INDEX IF EXISTS idx_online_series_rating_version;
DROP INDEX IF EXISTS idx_series_rating_history_series;

DROP TABLE IF EXISTS series_rating_evidence;
DROP TABLE IF EXISTS series_cover_analysis;
DROP TABLE IF EXISTS series_rating_overrides;
DROP TABLE IF EXISTS series_rating_history;

ALTER TABLE online_series DROP COLUMN rating_state;
ALTER TABLE online_series DROP COLUMN rating_auto;
ALTER TABLE online_series DROP COLUMN rating_score;
ALTER TABLE online_series DROP COLUMN rating_origin;
ALTER TABLE online_series DROP COLUMN rating_version;
ALTER TABLE online_series DROP COLUMN rating_fingerprint;
ALTER TABLE online_series DROP COLUMN rated_at;
ALTER TABLE online_series DROP COLUMN declared_rating;
ALTER TABLE online_series DROP COLUMN declared_adult_at;
ALTER TABLE online_series DROP COLUMN known_source_rating;
ALTER TABLE online_series DROP COLUMN known_runtime;
ALTER TABLE online_series DROP COLUMN adult;

DELETE FROM settings WHERE key IN ('content.adult', 'content.suspect');
