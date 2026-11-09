-- Testes da migração da telemetria: permissões, RLS, validação, repetição, frequência, painel e exclusão.
-- Roda com supabase/tests/run.sh, que sobe um Postgres vazio, aplica o stub do Supabase e a migração.
\set ON_ERROR_STOP on
\set QUIET on

create function pg_temp.lote(batch uuid, install uuid, events jsonb, version text default '0.9.0') returns jsonb language sql as $$
  select jsonb_build_object('schema', 1, 'batchId', batch, 'installId', install,
    'app', jsonb_build_object('version', version, 'platform', 'windows', 'os', '10.0.22631', 'arch', 'x86_64', 'channel', 'nsis'),
    'events', events)
$$;
grant execute on function pg_temp.lote(uuid, uuid, jsonb, text) to anon, authenticated, service_role;

create function pg_temp.hoje() returns text language sql as $$ select ((now() at time zone 'utc')::date)::text $$;
grant execute on function pg_temp.hoje() to anon, authenticated, service_role;

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-00000000000a', 'admin@example.com'),
  ('00000000-0000-0000-0000-00000000000b', 'comum@example.com');
insert into telemetry.admins (user_id) values ('00000000-0000-0000-0000-00000000000a');

\echo '1. anon e authenticated não leem nem gravam as tabelas'
set role anon;
do $$ begin
  perform 1 from telemetry.install_days;
  raise exception 'anon leu telemetry.install_days';
exception when insufficient_privilege then null; end $$;
do $$ begin
  insert into telemetry.batches (batch_id, install_id, event_count) values (gen_random_uuid(), gen_random_uuid(), 0);
  raise exception 'anon gravou telemetry.batches';
exception when insufficient_privilege then null; end $$;
reset role;
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-00000000000a';
do $$ begin
  perform 1 from telemetry.errors;
  raise exception 'admin leu telemetry.errors direto';
exception when insufficient_privilege then null; end $$;
reset role;

\echo '2. só a service_role grava'
set role anon;
do $$ begin
  perform public.telemetry_ingest(pg_temp.lote(gen_random_uuid(), gen_random_uuid(), '[]'));
  raise exception 'anon chamou telemetry_ingest';
exception when insufficient_privilege then null; end $$;
do $$ begin
  perform public.telemetry_forget(gen_random_uuid());
  raise exception 'anon chamou telemetry_forget';
exception when insufficient_privilege then null; end $$;
reset role;
set role authenticated;
do $$ begin
  perform public.telemetry_ingest(pg_temp.lote(gen_random_uuid(), gen_random_uuid(), '[]'));
  raise exception 'authenticated chamou telemetry_ingest';
exception when insufficient_privilege then null; end $$;
reset role;

\echo '3. um lote válido entra'
set role service_role;
do $$
declare r jsonb; d text := pg_temp.hoje();
begin
  r := public.telemetry_ingest(pg_temp.lote('10000000-0000-0000-0000-000000000001', '20000000-0000-0000-0000-000000000001', jsonb_build_array(
    jsonb_build_object('type', 'session', 'day', d, 'launches', 2, 'uncleanExits', 1),
    jsonb_build_object('type', 'source', 'day', d, 'source', 'mangadex/pt-br', 'queries', 5, 'opens', 2, 'chapters', 1, 'errors', 1),
    jsonb_build_object('type', 'error', 'day', d, 'hour', 13, 'category', 'network', 'component', 'sources', 'code', 'timeout', 'httpStatus', 0, 'count', 3),
    jsonb_build_object('type', 'perf', 'day', d, 'metric', 'startup', 'count', 2, 'totalMs', 1800, 'maxMs', 1100)
  )));
  if (r ->> 'accepted')::int <> 4 then raise exception 'esperava 4 eventos, veio %', r; end if;
end $$;

\echo '4. o mesmo lote não conta duas vezes'
do $$
declare r jsonb; d text := pg_temp.hoje();
begin
  r := public.telemetry_ingest(pg_temp.lote('10000000-0000-0000-0000-000000000001', '20000000-0000-0000-0000-000000000001', jsonb_build_array(
    jsonb_build_object('type', 'session', 'day', d, 'launches', 2))));
  if not (r ->> 'duplicate')::boolean then raise exception 'lote repetido foi aceito: %', r; end if;
end $$;
reset role;
do $$ begin
  if (select launches from telemetry.install_days where install_id = '20000000-0000-0000-0000-000000000001') <> 2 then
    raise exception 'lote repetido somou de novo';
  end if;
end $$;

\echo '5. lotes inválidos são recusados inteiros'
set role service_role;
do $$
declare
  d text := pg_temp.hoje();
  bad jsonb[] := array[
    -- versão fora do padrão
    pg_temp.lote(gen_random_uuid(), gen_random_uuid(), '[]', '0.9.0-beta; drop table'),
    -- tipo desconhecido
    pg_temp.lote(gen_random_uuid(), gen_random_uuid(), jsonb_build_array(jsonb_build_object('type', 'title', 'day', d, 'title', 'One Piece'))),
    -- fonte com caminho
    pg_temp.lote(gen_random_uuid(), gen_random_uuid(), jsonb_build_array(jsonb_build_object('type', 'source', 'day', d, 'source', 'C:\Users\gustavo\x', 'queries', 1))),
    -- fonte com URL
    pg_temp.lote(gen_random_uuid(), gen_random_uuid(), jsonb_build_array(jsonb_build_object('type', 'source', 'day', d, 'source', 'https://x.com/?token=abc', 'queries', 1))),
    -- código de erro com texto livre
    pg_temp.lote(gen_random_uuid(), gen_random_uuid(), jsonb_build_array(jsonb_build_object('type', 'error', 'day', d, 'hour', 1, 'category', 'network', 'component', 'sources', 'code', 'Falha em gustavo@example.com', 'count', 1))),
    -- categoria fora da lista
    pg_temp.lote(gen_random_uuid(), gen_random_uuid(), jsonb_build_array(jsonb_build_object('type', 'error', 'day', d, 'hour', 1, 'category', 'senha', 'component', 'x', 'code', 'y', 'count', 1))),
    -- data antiga demais
    pg_temp.lote(gen_random_uuid(), gen_random_uuid(), jsonb_build_array(jsonb_build_object('type', 'session', 'day', '2020-01-01', 'launches', 1))),
    -- contagem negativa
    pg_temp.lote(gen_random_uuid(), gen_random_uuid(), jsonb_build_array(jsonb_build_object('type', 'perf', 'day', d, 'metric', 'startup', 'count', 1, 'totalMs', -5, 'maxMs', 1))),
    -- identificador que não é UUID
    jsonb_build_object('schema', 1, 'batchId', 'abc', 'installId', gen_random_uuid(), 'app', jsonb_build_object('version', '0.9.0', 'platform', 'windows'), 'events', '[]'::jsonb),
    -- schema desconhecido
    jsonb_build_object('schema', 2, 'batchId', gen_random_uuid(), 'installId', gen_random_uuid(), 'events', '[]'::jsonb)
  ];
  b jsonb;
  i int := 0;
begin
  foreach b in array bad loop
    i := i + 1;
    begin
      perform public.telemetry_ingest(b);
      raise exception 'lote inválido % foi aceito', i using errcode = 'XX000';
    exception
      when sqlstate 'XX000' then raise;
      when others then null;
    end;
  end loop;
  -- Mais de 200 eventos
  begin
    perform public.telemetry_ingest(pg_temp.lote(gen_random_uuid(), gen_random_uuid(),
      (select jsonb_agg(jsonb_build_object('type', 'session', 'day', d, 'launches', 1)) from generate_series(1, 201))));
    raise exception 'lote com 201 eventos foi aceito' using errcode = 'XX000';
  exception when sqlstate 'XX000' then raise; when others then null; end;
end $$;
reset role;
do $$ begin
  if (select count(*) from telemetry.batches) <> 1 then raise exception 'um lote inválido deixou rastro'; end if;
end $$;

\echo '6. limite de lotes por hora por instalação'
set role service_role;
do $$
declare install uuid := gen_random_uuid(); i int;
begin
  for i in 1..12 loop
    perform public.telemetry_ingest(pg_temp.lote(gen_random_uuid(), install, '[]'));
  end loop;
  begin
    perform public.telemetry_ingest(pg_temp.lote(gen_random_uuid(), install, '[]'));
    raise exception 'o 13º lote da hora foi aceito' using errcode = 'XX000';
  exception when sqlstate 'XX000' then raise; when sqlstate 'P0001' then null; end;
end $$;
reset role;

\echo '7. usuário comum não vê o painel'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-00000000000b';
do $$ begin
  if public.telemetry_is_admin() then raise exception 'usuário comum virou admin'; end if;
  perform public.telemetry_report(current_date - 30, current_date);
  raise exception 'usuário comum leu o relatório';
exception when insufficient_privilege then null; end $$;
reset role;
set role anon;
do $$ begin
  perform public.telemetry_report(current_date - 30, current_date);
  raise exception 'anon leu o relatório';
exception when insufficient_privilege then null; end $$;
reset role;

\echo '8. admin vê só números agregados'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-00000000000a';
do $$
declare r jsonb;
begin
  if not public.telemetry_is_admin() then raise exception 'admin não reconhecido'; end if;
  r := public.telemetry_report(current_date - 30, current_date + 1);
  if (r #>> '{totals,installs}')::int <> 1 then raise exception 'instalações: %', r -> 'totals'; end if;
  if (r #>> '{totals,launches}')::int <> 2 then raise exception 'aberturas: %', r -> 'totals'; end if;
  if (r #>> '{totals,errors}')::int <> 3 then raise exception 'erros: %', r -> 'totals'; end if;
  if r -> 'sources' -> 0 ->> 'source' <> 'mangadex/pt-br' then raise exception 'fontes: %', r -> 'sources'; end if;
  if r -> 'versions' -> 0 ->> 'version' <> '0.9.0' then raise exception 'versões: %', r -> 'versions'; end if;
  if r::text ilike '%install_id%' or r::text like '%20000000-0000-0000-0000-000000000001%' then
    raise exception 'o relatório expôs o identificador de uma instalação';
  end if;
  -- Filtro por categoria
  r := public.telemetry_report(current_date - 30, current_date + 1, null, 'crash');
  if (r #>> '{totals,errors}')::int <> 0 then raise exception 'filtro de categoria: %', r -> 'totals'; end if;
  -- Período inválido
  begin
    perform public.telemetry_report(current_date, current_date - 1);
    raise exception 'período invertido foi aceito' using errcode = 'XX000';
  exception when sqlstate 'XX000' then raise; when invalid_parameter_value then null; end;
end $$;
reset role;

\echo '9. apagar os dados de uma instalação'
set role service_role;
do $$
declare r jsonb;
begin
  r := public.telemetry_forget('20000000-0000-0000-0000-000000000001');
  if (r ->> 'removed')::int < 5 then raise exception 'removeu pouco: %', r; end if;
end $$;
reset role;
do $$ begin
  if exists (select 1 from telemetry.install_days where install_id = '20000000-0000-0000-0000-000000000001')
     or exists (select 1 from telemetry.source_usage where install_id = '20000000-0000-0000-0000-000000000001')
     or exists (select 1 from telemetry.errors where install_id = '20000000-0000-0000-0000-000000000001') then
    raise exception 'sobrou dado da instalação apagada';
  end if;
end $$;

\echo '10. retenção'
insert into telemetry.install_days (install_id, day, app_version, platform, channel) values (gen_random_uuid(), current_date - 200, '0.1.0', 'windows', 'nsis');
select telemetry.prune();
do $$ begin
  if exists (select 1 from telemetry.install_days where day < current_date - 180) then raise exception 'prune não apagou o antigo'; end if;
end $$;

\echo 'Tudo certo.'
