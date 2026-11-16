-- O app Android manda `platform = android` e `channel = apk` (o APK do GitHub). Antes, o servidor recusava o lote
-- inteiro. A função é a mesma de 20261109140000_telemetry.sql, só com os dois valores novos; `create or replace`
-- mantém as permissões (só a service_role executa).

create or replace function public.telemetry_ingest(batch jsonb) returns jsonb
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
     or v_platform is null or v_platform not in ('windows', 'macos', 'linux', 'android')
     or v_channel not in ('nsis', 'msi', 'dev', 'unknown', 'apk')
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
