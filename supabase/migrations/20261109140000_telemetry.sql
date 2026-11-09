-- Telemetria anônima e voluntária do Prancha. Ver docs/TELEMETRY.md.
--
-- Tudo fica no schema `telemetry`, que a API do Supabase não expõe e que anon e authenticated não leem.
-- Só três portas entram nele, todas funções com `security definer`:
--   telemetry_ingest(lote)    grava um lote validado. Só a service_role, usada pela função da Vercel.
--   telemetry_forget(id)      apaga tudo de uma instalação. Só a service_role.
--   telemetry_report(...)     devolve números agregados. Só usuários autenticados que estão em telemetry.admins.
-- Nenhuma tabela guarda IP, conta, e-mail, títulos de obras ou texto livre.

create schema if not exists telemetry;
revoke all on schema telemetry from public;
revoke all on schema telemetry from anon, authenticated;
grant usage on schema telemetry to service_role;

-- Quem pode ver o painel. A pessoa precisa existir no Supabase Auth e ser posta aqui à mão.
create table telemetry.admins (
  user_id uuid primary key references auth.users (id) on delete cascade,
  added_at timestamptz not null default now()
);

-- Um lote por envio: impede repetição (o mesmo batch_id não entra duas vezes) e mede a frequência por instalação.
create table telemetry.batches (
  batch_id uuid primary key,
  install_id uuid not null,
  received_at timestamptz not null default now(),
  event_count integer not null check (event_count between 0 and 200)
);
create index batches_install_recent on telemetry.batches (install_id, received_at desc);
create index batches_received on telemetry.batches (received_at);

-- Uma linha por instalação e por dia em que o app abriu: versão, plataforma e canal daquele dia.
create table telemetry.install_days (
  install_id uuid not null,
  day date not null,
  app_version text not null,
  platform text not null,
  os_version text,
  arch text,
  channel text not null,
  launches integer not null default 0 check (launches >= 0),
  unclean_exits integer not null default 0 check (unclean_exits >= 0),
  primary key (install_id, day)
);
create index install_days_day on telemetry.install_days (day);
create index install_days_version on telemetry.install_days (app_version, day);

-- Uso de fontes, somado por instalação, dia e fonte. Nenhum título, busca ou obra.
create table telemetry.source_usage (
  install_id uuid not null,
  day date not null,
  source text not null,
  queries integer not null default 0 check (queries >= 0),
  opens integer not null default 0 check (opens >= 0),
  chapters integer not null default 0 check (chapters >= 0),
  errors integer not null default 0 check (errors >= 0),
  primary key (install_id, day, source)
);
create index source_usage_day on telemetry.source_usage (day, source);

-- Falhas, somadas por hora. Só identificadores técnicos de uma lista fechada, nenhuma mensagem.
create table telemetry.errors (
  install_id uuid not null,
  day date not null,
  hour smallint not null check (hour between 0 and 23),
  app_version text not null,
  platform text not null,
  category text not null,
  component text not null,
  code text not null,
  http_status smallint not null default 0 check (http_status between 0 and 599),
  count integer not null default 0 check (count >= 0),
  primary key (install_id, day, hour, app_version, category, component, code, http_status)
);
create index errors_day on telemetry.errors (day);
create index errors_version on telemetry.errors (app_version, day);

-- Desempenho, somado por instalação, dia, versão e métrica.
create table telemetry.performance (
  install_id uuid not null,
  day date not null,
  app_version text not null,
  platform text not null,
  metric text not null,
  count integer not null default 0 check (count >= 0),
  total_ms bigint not null default 0 check (total_ms >= 0),
  max_ms integer not null default 0 check (max_ms >= 0),
  primary key (install_id, day, app_version, metric)
);
create index performance_day on telemetry.performance (day, metric);

-- RLS ligado e sem nenhuma política: mesmo se alguém der acesso ao schema por engano, nada é lido ou gravado
-- por anon ou authenticated. As funções abaixo rodam como dono das tabelas e passam por cima disso de propósito.
alter table telemetry.admins enable row level security;
alter table telemetry.batches enable row level security;
alter table telemetry.install_days enable row level security;
alter table telemetry.source_usage enable row level security;
alter table telemetry.errors enable row level security;
alter table telemetry.performance enable row level security;
revoke all on all tables in schema telemetry from public, anon, authenticated;

-- ---------------------------------------------------------------------------------------------------------------------
-- Limites. Ficam aqui e na função da Vercel (site/api/_telemetry.mjs), que confere antes de chegar ao banco.
-- ---------------------------------------------------------------------------------------------------------------------

create function telemetry.limits() returns jsonb
language sql immutable
set search_path = ''
as $$
  select jsonb_build_object(
    'max_events', 200,
    'batches_per_hour', 12,
    'batches_per_minute_global', 600,
    'max_count', 100000,
    'max_ms', 3600000,
    'retention_days', 180,
    'batch_retention_days', 30
  )
$$;

create function telemetry.is_token(value text, max_len integer) returns boolean
language sql immutable
set search_path = ''
as $$
  select value is not null and length(value) between 1 and max_len and value ~ '^[a-z0-9][a-z0-9._:-]*$'
$$;

-- ---------------------------------------------------------------------------------------------------------------------
-- Gravação
-- ---------------------------------------------------------------------------------------------------------------------

create function public.telemetry_ingest(batch jsonb) returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  lim jsonb := telemetry.limits();
  v_batch uuid;
  v_install uuid;
  v_version text;
  v_platform text;
  v_os text;
  v_arch text;
  v_channel text;
  v_events jsonb;
  v_event jsonb;
  v_type text;
  v_day date;
  v_count integer;
  v_today date := (now() at time zone 'utc')::date;
  v_max_count integer := (lim ->> 'max_count')::integer;
  v_max_ms integer := (lim ->> 'max_ms')::integer;
begin
  if jsonb_typeof(batch) is distinct from 'object' or (batch ->> 'schema') is distinct from '1' then
    raise exception 'telemetry: schema' using errcode = '22023';
  end if;

  begin
    v_batch := (batch ->> 'batchId')::uuid;
    v_install := (batch ->> 'installId')::uuid;
  exception when others then
    raise exception 'telemetry: id' using errcode = '22023';
  end;
  if v_batch is null or v_install is null then
    raise exception 'telemetry: id' using errcode = '22023';
  end if;

  v_version := batch #>> '{app,version}';
  v_platform := batch #>> '{app,platform}';
  v_os := batch #>> '{app,os}';
  v_arch := batch #>> '{app,arch}';
  v_channel := coalesce(batch #>> '{app,channel}', 'unknown');
  if v_version is null or v_version !~ '^[0-9]{1,4}\.[0-9]{1,4}\.[0-9]{1,4}$'
     or v_platform is null or v_platform not in ('windows', 'macos', 'linux')
     or v_channel not in ('nsis', 'msi', 'dev', 'unknown')
     or (v_os is not null and v_os !~ '^[0-9][0-9.]{0,23}$')
     or (v_arch is not null and v_arch not in ('x86_64', 'aarch64', 'x86', 'arm')) then
    raise exception 'telemetry: app' using errcode = '22023';
  end if;

  v_events := batch -> 'events';
  if jsonb_typeof(v_events) is distinct from 'array' or jsonb_array_length(v_events) > (lim ->> 'max_events')::integer then
    raise exception 'telemetry: events' using errcode = '22023';
  end if;

  -- Repetido: responde como aceito, para o app não tentar de novo, e não grava nada.
  if exists (select 1 from telemetry.batches where batch_id = v_batch) then
    return jsonb_build_object('accepted', 0, 'duplicate', true);
  end if;

  if (select count(*) from telemetry.batches where install_id = v_install and received_at > now() - interval '1 hour')
     >= (lim ->> 'batches_per_hour')::integer then
    raise exception 'telemetry: rate' using errcode = 'P0001', hint = 'rate_install';
  end if;
  if (select count(*) from telemetry.batches where received_at > now() - interval '1 minute')
     >= (lim ->> 'batches_per_minute_global')::integer then
    raise exception 'telemetry: rate' using errcode = 'P0001', hint = 'rate_global';
  end if;

  -- Tudo é conferido antes de gravar qualquer coisa: um evento inválido recusa o lote inteiro.
  for v_event in select value from jsonb_array_elements(v_events) loop
    v_type := v_event ->> 'type';
    begin
      v_day := (v_event ->> 'day')::date;
    exception when others then
      raise exception 'telemetry: day' using errcode = '22023';
    end;
    if v_day is null or v_day > v_today + 1 or v_day < v_today - 7 then
      raise exception 'telemetry: day' using errcode = '22023';
    end if;
    if v_type = 'session' then
      if coalesce((v_event ->> 'launches')::integer, -1) not between 0 and 1000
         or coalesce((v_event ->> 'uncleanExits')::integer, 0) not between 0 and 1000 then
        raise exception 'telemetry: session' using errcode = '22023';
      end if;
    elsif v_type = 'source' then
      if coalesce(v_event ->> 'source', '') !~ '^[a-z0-9][a-z0-9._-]{0,63}/[a-z0-9][a-z0-9._-]{0,63}$'
         or coalesce((v_event ->> 'queries')::integer, 0) not between 0 and v_max_count
         or coalesce((v_event ->> 'opens')::integer, 0) not between 0 and v_max_count
         or coalesce((v_event ->> 'chapters')::integer, 0) not between 0 and v_max_count
         or coalesce((v_event ->> 'errors')::integer, 0) not between 0 and v_max_count then
        raise exception 'telemetry: source' using errcode = '22023';
      end if;
    elsif v_type = 'error' then
      if (v_event ->> 'category') not in ('network', 'source', 'extension', 'reader', 'library', 'import', 'database', 'update', 'interface', 'crash', 'other')
         or not telemetry.is_token(v_event ->> 'component', 40)
         or not telemetry.is_token(v_event ->> 'code', 64)
         or coalesce((v_event ->> 'hour')::integer, -1) not between 0 and 23
         or coalesce((v_event ->> 'httpStatus')::integer, 0) not between 0 and 599
         or coalesce((v_event ->> 'count')::integer, 0) not between 1 and v_max_count then
        raise exception 'telemetry: error' using errcode = '22023';
      end if;
    elsif v_type = 'perf' then
      if (v_event ->> 'metric') not in ('startup', 'library_load', 'explore_results', 'series_open', 'chapter_open', 'reader_first_page')
         or coalesce((v_event ->> 'count')::integer, 0) not between 1 and v_max_count
         or coalesce((v_event ->> 'totalMs')::bigint, -1) not between 0 and v_max_ms::bigint * v_max_count
         or coalesce((v_event ->> 'maxMs')::integer, -1) not between 0 and v_max_ms then
        raise exception 'telemetry: perf' using errcode = '22023';
      end if;
    else
      raise exception 'telemetry: type' using errcode = '22023';
    end if;
  end loop;

  insert into telemetry.batches (batch_id, install_id, event_count) values (v_batch, v_install, jsonb_array_length(v_events));
  v_count := 0;

  for v_event in select value from jsonb_array_elements(v_events) loop
    v_type := v_event ->> 'type';
    v_day := (v_event ->> 'day')::date;
    -- Todo lote marca a instalação como ativa no dia, com a versão daquele momento.
    insert into telemetry.install_days (install_id, day, app_version, platform, os_version, arch, channel)
    values (v_install, v_day, v_version, v_platform, v_os, v_arch, v_channel)
    on conflict (install_id, day) do update
      set app_version = excluded.app_version, platform = excluded.platform, os_version = excluded.os_version,
          arch = excluded.arch, channel = excluded.channel;

    if v_type = 'session' then
      update telemetry.install_days
        set launches = least(launches + (v_event ->> 'launches')::integer, v_max_count),
            unclean_exits = least(unclean_exits + coalesce((v_event ->> 'uncleanExits')::integer, 0), v_max_count)
        where install_id = v_install and day = v_day;
    elsif v_type = 'source' then
      insert into telemetry.source_usage as s (install_id, day, source, queries, opens, chapters, errors)
      values (v_install, v_day, v_event ->> 'source', coalesce((v_event ->> 'queries')::integer, 0),
              coalesce((v_event ->> 'opens')::integer, 0), coalesce((v_event ->> 'chapters')::integer, 0),
              coalesce((v_event ->> 'errors')::integer, 0))
      on conflict (install_id, day, source) do update
        set queries = least(s.queries + excluded.queries, v_max_count),
            opens = least(s.opens + excluded.opens, v_max_count),
            chapters = least(s.chapters + excluded.chapters, v_max_count),
            errors = least(s.errors + excluded.errors, v_max_count);
    elsif v_type = 'error' then
      insert into telemetry.errors as e (install_id, day, hour, app_version, platform, category, component, code, http_status, count)
      values (v_install, v_day, (v_event ->> 'hour')::smallint, v_version, v_platform, v_event ->> 'category',
              v_event ->> 'component', v_event ->> 'code', coalesce((v_event ->> 'httpStatus')::smallint, 0),
              (v_event ->> 'count')::integer)
      on conflict (install_id, day, hour, app_version, category, component, code, http_status) do update
        set count = least(e.count + excluded.count, v_max_count);
    elsif v_type = 'perf' then
      insert into telemetry.performance as p (install_id, day, app_version, platform, metric, count, total_ms, max_ms)
      values (v_install, v_day, v_version, v_platform, v_event ->> 'metric', (v_event ->> 'count')::integer,
              (v_event ->> 'totalMs')::bigint, (v_event ->> 'maxMs')::integer)
      on conflict (install_id, day, app_version, metric) do update
        set count = least(p.count + excluded.count, v_max_count),
            total_ms = p.total_ms + excluded.total_ms,
            max_ms = greatest(p.max_ms, excluded.max_ms);
    end if;
    v_count := v_count + 1;
  end loop;

  -- A limpeza também roda aqui de vez em quando, para quem não tem o pg_cron ligado.
  if random() < 0.01 then
    perform telemetry.prune();
  end if;

  return jsonb_build_object('accepted', v_count, 'duplicate', false);
end;
$$;

-- Apaga tudo de uma instalação. O app pede isso quando a pessoa escolhe "Apagar os dados enviados".
create function public.telemetry_forget(install uuid) returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  removed integer := 0;
  n integer;
begin
  delete from telemetry.install_days where install_id = install; get diagnostics n = row_count; removed := removed + n;
  delete from telemetry.source_usage where install_id = install; get diagnostics n = row_count; removed := removed + n;
  delete from telemetry.errors where install_id = install; get diagnostics n = row_count; removed := removed + n;
  delete from telemetry.performance where install_id = install; get diagnostics n = row_count; removed := removed + n;
  delete from telemetry.batches where install_id = install; get diagnostics n = row_count; removed := removed + n;
  return jsonb_build_object('removed', removed);
end;
$$;

-- Retenção: 180 dias para os dados, 30 para os lotes (que só servem para repetição e frequência).
create function telemetry.prune() returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  lim jsonb := telemetry.limits();
  cutoff date := (now() at time zone 'utc')::date - (lim ->> 'retention_days')::integer;
begin
  delete from telemetry.install_days where day < cutoff;
  delete from telemetry.source_usage where day < cutoff;
  delete from telemetry.errors where day < cutoff;
  delete from telemetry.performance where day < cutoff;
  delete from telemetry.batches where received_at < now() - make_interval(days => (lim ->> 'batch_retention_days')::integer);
end;
$$;

-- ---------------------------------------------------------------------------------------------------------------------
-- Painel
-- ---------------------------------------------------------------------------------------------------------------------

create function public.telemetry_is_admin() returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select auth.uid() is not null and exists (select 1 from telemetry.admins where user_id = auth.uid())
$$;

-- Tudo que o painel mostra, já agregado. Nenhuma linha devolve install_id.
-- p_version e p_category filtram as falhas e as séries; o período vale para tudo.
create function public.telemetry_report(p_from date, p_to date, p_version text default null, p_category text default null)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  result jsonb;
begin
  if not public.telemetry_is_admin() then
    raise exception 'telemetry: forbidden' using errcode = '42501';
  end if;
  if p_from is null or p_to is null or p_to < p_from or p_to - p_from > 366 then
    raise exception 'telemetry: period' using errcode = '22023';
  end if;

  with
  days as (
    select * from telemetry.install_days
    where day between p_from and p_to and (p_version is null or app_version = p_version)
  ),
  -- A versão de cada instalação é a do último dia em que ela apareceu no período.
  latest as (
    select distinct on (install_id) install_id, app_version, platform, channel
    from days order by install_id, day desc
  ),
  errs as (
    select * from telemetry.errors
    where day between p_from and p_to
      and (p_version is null or app_version = p_version)
      and (p_category is null or category = p_category)
  ),
  src as (
    select * from telemetry.source_usage s
    where s.day between p_from and p_to
      and (p_version is null or exists (
        select 1 from days d where d.install_id = s.install_id and d.day = s.day))
  ),
  perf as (
    select * from telemetry.performance
    where day between p_from and p_to and (p_version is null or app_version = p_version)
  ),
  totals as (
    select
      (select count(distinct install_id) from days) as installs,
      (select coalesce(sum(launches), 0) from days) as launches,
      (select coalesce(sum(unclean_exits), 0) from days) as unclean_exits,
      (select coalesce(sum(count), 0) from errs) as errors,
      (select count(distinct install_id) from errs) as installs_with_errors,
      (select coalesce(sum(event_count), 0) from telemetry.batches
         where received_at >= p_from and received_at < p_to + 1) as events,
      (select count(*) from telemetry.batches where received_at >= p_from and received_at < p_to + 1) as batches
  )
  select jsonb_build_object(
    'period', jsonb_build_object('from', p_from, 'to', p_to),
    'totals', (select to_jsonb(totals) from totals),
    'versions', coalesce((
      select jsonb_agg(v order by v.installs desc, v.version desc) from (
        select app_version as version, count(*) as installs from latest group by app_version
      ) v), '[]'::jsonb),
    'allVersions', coalesce((
      select jsonb_agg(distinct app_version) from telemetry.install_days where day between p_from and p_to), '[]'::jsonb),
    'platforms', coalesce((
      select jsonb_agg(p order by p.installs desc) from (
        select platform, count(*) as installs from latest group by platform
      ) p), '[]'::jsonb),
    'channels', coalesce((
      select jsonb_agg(c order by c.installs desc) from (
        select channel, count(*) as installs from latest group by channel
      ) c), '[]'::jsonb),
    'adoption', coalesce((
      select jsonb_agg(a order by a.day, a.version) from (
        select day, app_version as version, count(distinct install_id) as installs from days group by day, app_version
      ) a), '[]'::jsonb),
    'daily', coalesce((
      select jsonb_agg(d order by d.day) from (
        select g.day::date as day,
               (select count(distinct install_id) from days where days.day = g.day) as installs,
               (select coalesce(sum(launches), 0) from days where days.day = g.day) as launches,
               (select coalesce(sum(unclean_exits), 0) from days where days.day = g.day) as unclean_exits,
               (select coalesce(sum(count), 0) from errs where errs.day = g.day) as errors
        from generate_series(p_from, p_to, interval '1 day') g(day)
      ) d), '[]'::jsonb),
    'sources', coalesce((
      select jsonb_agg(s order by s.queries + s.opens desc, s.source) from (
        select source, sum(queries) as queries, sum(opens) as opens, sum(chapters) as chapters, sum(errors) as errors,
               count(distinct install_id) as installs, count(distinct day) as active_days
        from src group by source order by sum(queries) + sum(opens) desc limit 50
      ) s), '[]'::jsonb),
    'errorsByVersion', coalesce((
      select jsonb_agg(e order by e.errors desc) from (
        select app_version as version, sum(count) as errors from errs group by app_version
      ) e), '[]'::jsonb),
    'errorsByPlatform', coalesce((
      select jsonb_agg(e order by e.errors desc) from (
        select platform, sum(count) as errors from errs group by platform
      ) e), '[]'::jsonb),
    'errorsByComponent', coalesce((
      select jsonb_agg(e order by e.errors desc) from (
        select component, sum(count) as errors from errs group by component order by 2 desc limit 15
      ) e), '[]'::jsonb),
    'errorsByCategory', coalesce((
      select jsonb_agg(e order by e.errors desc) from (
        select category, sum(count) as errors from errs group by category
      ) e), '[]'::jsonb),
    'topErrors', coalesce((
      select jsonb_agg(e order by e.errors desc) from (
        select category, component, code, http_status, sum(count) as errors, count(distinct install_id) as installs,
               min(app_version) as first_version, max(app_version) as last_version, max(day) as last_day
        from errs group by category, component, code, http_status order by sum(count) desc limit 20
      ) e), '[]'::jsonb),
    'performance', coalesce((
      select jsonb_agg(p order by p.metric) from (
        select metric, sum(count) as samples, round(sum(total_ms)::numeric / nullif(sum(count), 0)) as avg_ms, max(max_ms) as max_ms
        from perf group by metric
      ) p), '[]'::jsonb)
  ) into result;

  return result;
end;
$$;

-- Quem pode chamar cada função. Por padrão o Postgres deixa qualquer um executar funções novas.
revoke all on function public.telemetry_ingest(jsonb) from public, anon, authenticated;
revoke all on function public.telemetry_forget(uuid) from public, anon, authenticated;
revoke all on function public.telemetry_report(date, date, text, text) from public, anon;
revoke all on function public.telemetry_is_admin() from public, anon;
revoke all on function telemetry.prune() from public, anon, authenticated;
revoke all on function telemetry.limits() from public, anon, authenticated;
revoke all on function telemetry.is_token(text, integer) from public, anon, authenticated;
grant execute on function public.telemetry_ingest(jsonb) to service_role;
grant execute on function public.telemetry_forget(uuid) to service_role;
grant execute on function public.telemetry_report(date, date, text, text) to authenticated;
grant execute on function public.telemetry_is_admin() to authenticated;

-- Limpeza diária pelo pg_cron, quando a extensão existe (no Supabase existe; no Postgres dos testes, não).
do $$
begin
  if exists (select 1 from pg_available_extensions where name = 'pg_cron') then
    create extension if not exists pg_cron;
    perform cron.schedule('telemetry-prune', '17 3 * * *', 'select telemetry.prune()');
  end if;
end;
$$;
