#!/usr/bin/env bash
# Sobe um Postgres vazio numa pasta temporária, aplica o stub do Supabase e as migrações e roda os testes.
# Uso: bash supabase/tests/run.sh   (precisa do Postgres 15 ou mais novo instalado: initdb, pg_ctl, psql)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
bin="${PG_BIN:-$(ls -d /usr/lib/postgresql/*/bin 2>/dev/null | sort -V | tail -1)}"
[ -x "$bin/initdb" ] || bin="$(dirname "$(command -v initdb)")"
tmp="$(mktemp -d)"
port="${PG_PORT:-54329}"
cleanup() { "$bin/pg_ctl" -D "$tmp/data" -m immediate stop >/dev/null 2>&1 || true; rm -rf "$tmp"; }
trap cleanup EXIT
run_as=()
if [ "$(id -u)" = 0 ]; then
  chown -R postgres "$tmp" 2>/dev/null || true
  run_as=(runuser -u postgres --)
fi
"${run_as[@]}" "$bin/initdb" -D "$tmp/data" -U postgres --auth=trust >/dev/null
"${run_as[@]}" "$bin/pg_ctl" -D "$tmp/data" -o "-p $port -k $tmp -c listen_addresses=''" -w start >/dev/null
psql=(psql -X -q -v ON_ERROR_STOP=1 -h "$tmp" -p "$port" -U postgres -d postgres)
"${psql[@]}" -f "$here/supabase-stub.sql"
for migration in "$here"/../migrations/*.sql; do
  "${psql[@]}" -f "$migration"
done
"${psql[@]}" -f "$here/telemetry.test.sql"
