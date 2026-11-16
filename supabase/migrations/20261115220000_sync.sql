-- Sincronização da leitura online entre aparelhos do Prancha. Ver docs/SINCRONIZACAO.md.
--
-- Só sobem identificadores e estado: repositórios, extensões, obras das fontes, estado e posição de cada capítulo,
-- pastas e associações manuais com o MyAnimeList. Nenhum arquivo, imagem, HQ local ou dado de extensão.
--
-- Tudo fica no schema `sync`, que a API do Supabase não expõe e que anon e authenticated não leem.
-- O app entra só por funções `security definer` que enxergam apenas as linhas de auth.uid():
--   sync_push(aparelho, mudanças)   aplica as mudanças com as regras de conflito e devolve o novo cursor.
--   sync_pull(desde, limite)         devolve o que mudou depois do cursor, de todas as tabelas, em ordem.
--   sync_devices() / sync_remove_device(id)   os aparelhos da conta.
--   sync_export()                    tudo o que a conta tem, para a pessoa baixar.
--   sync_forget_me()                 apaga os dados e a conta.
--
-- Datas de estado são milissegundos Unix, como no SQLite do app. Uma data no futuro vira a hora do servidor,
-- para um relógio adiantado não ganhar todas as disputas.
--
-- `seq` vem de uma sequência só e muda a cada escrita que altera a linha. Cada envio trava a conta
-- (pg_advisory_xact_lock), então, para uma mesma conta, os `seq` ficam visíveis na ordem em que foram dados
-- e um `sync_pull` nunca pula uma mudança que ainda estava para ser gravada.
--
-- Nenhuma tabela daqui liga install_id da telemetria a user_id.

create schema if not exists sync;
revoke all on schema sync from public;
revoke all on schema sync from anon, authenticated;

create sequence sync.seq;

-- Uma linha por conta: limite de frequência e o piso do cursor depois da limpeza de marcas de remoção.
create table sync.accounts (
  user_id uuid primary key references auth.users (id) on delete cascade,
  created_at timestamptz not null default now(),
  -- Remoções mais antigas que isto já foram apagadas: quem puxa a partir de um cursor menor recomeça do zero.
  floor_seq bigint not null default 0,
  window_start timestamptz not null default now(),
  window_calls integer not null default 0
);

create table sync.devices (
  user_id uuid not null references auth.users (id) on delete cascade,
  id uuid not null,
  name text not null check (length(name) between 1 and 80),
  platform text not null check (platform in ('windows', 'android', 'linux', 'macos')),
  app_version text not null check (length(app_version) between 1 and 32),
  created_at timestamptz not null default now(),
  last_seen_at timestamptz not null default now(),
  primary key (user_id, id)
);

create table sync.repositories (
  user_id uuid not null references auth.users (id) on delete cascade,
  url text not null check (length(url) between 1 and 2048),
  key_fingerprint text not null check (length(key_fingerprint) between 1 and 128),
  public_key text not null check (length(public_key) between 1 and 1024),
  enabled boolean not null default true,
  removed boolean not null default false,
  changed_at bigint not null,
  seq bigint not null,
  updated_at timestamptz not null default now(),
  primary key (user_id, url)
);

create table sync.extensions (
  user_id uuid not null references auth.users (id) on delete cascade,
  extension_id text not null check (length(extension_id) between 1 and 256),
  runtime text not null check (runtime in ('prancha', 'mihon')),
  repository_url text not null check (length(repository_url) between 1 and 2048),
  version text not null check (length(version) between 1 and 64),
  enabled boolean not null default true,
  removed boolean not null default false,
  changed_at bigint not null,
  seq bigint not null,
  updated_at timestamptz not null default now(),
  primary key (user_id, extension_id)
);

-- series_key = 16 bytes do BLAKE3 de (extension_id, source_id, item_key), em hexadecimal.
-- item_key é a chave portátil da obra: o id da extensão do Prancha, ou o endereço da obra no Mihon.
-- Cada campo que a pessoa muda tem a própria data: mudanças em campos diferentes não se apagam.
create table sync.series (
  user_id uuid not null references auth.users (id) on delete cascade,
  series_key text not null check (series_key ~ '^[0-9a-f]{32}$'),
  extension_id text not null check (length(extension_id) between 1 and 256),
  source_id text not null check (length(source_id) between 1 and 256),
  item_key text not null check (length(item_key) between 1 and 1024),
  title text check (length(title) <= 512),
  cover_url text check (length(cover_url) <= 2048),
  meta_at bigint,
  following boolean not null default false,
  following_at bigint,
  favorite boolean not null default false,
  favorite_at bigint,
  chapter_sort text not null default 'desc' check (chapter_sort in ('asc', 'desc')),
  chapter_sort_at bigint,
  reader_mode text check (length(reader_mode) <= 32),
  reader_mode_at bigint,
  removed boolean not null default false,
  removed_at bigint,
  seq bigint not null,
  updated_at timestamptz not null default now(),
  primary key (user_id, series_key)
);

-- chapter_key = 16 bytes do BLAKE3 da chave portátil do capítulo. Nenhum título nem endereço de capítulo sobe.
--
--   read / read_at      o último "lido" ou "não lido" (concluir, marcar como lido, marcar como não lido); o mais
--                       recente vence. Dois "lido" seguidos ficam com a conclusão mais antiga em completed_at.
--   started_at          o início de leitura mais antigo.
--   acknowledged_at     a primeira vez que a novidade foi vista, em qualquer aparelho.
--   page_* / position_* a posição mais recente; empate exato decide pelo id do aparelho.
create table sync.chapters (
  user_id uuid not null,
  series_key text not null,
  chapter_key text not null check (chapter_key ~ '^[0-9a-f]{32}$'),
  started_at bigint,
  read boolean,
  read_at bigint,
  completed_at bigint,
  acknowledged_at bigint,
  page_index integer check (page_index >= 0),
  page_count integer check (page_count >= 0),
  scroll_ratio real check (scroll_ratio >= 0 and scroll_ratio <= 1),
  position_at bigint,
  position_device uuid,
  seq bigint not null,
  updated_at timestamptz not null default now(),
  primary key (user_id, series_key, chapter_key),
  foreign key (user_id, series_key) references sync.series (user_id, series_key) on delete cascade
);

create table sync.collections (
  user_id uuid not null references auth.users (id) on delete cascade,
  collection_id uuid not null,
  name text not null check (length(name) between 1 and 120),
  removed boolean not null default false,
  changed_at bigint not null,
  seq bigint not null,
  updated_at timestamptz not null default now(),
  primary key (user_id, collection_id)
);

create table sync.series_collections (
  user_id uuid not null,
  series_key text not null,
  collection_id uuid not null,
  member boolean not null,
  changed_at bigint not null,
  seq bigint not null,
  updated_at timestamptz not null default now(),
  primary key (user_id, series_key, collection_id),
  foreign key (user_id, series_key) references sync.series (user_id, series_key) on delete cascade,
  foreign key (user_id, collection_id) references sync.collections (user_id, collection_id) on delete cascade
);

-- Só as associações que a pessoa fez à mão: a obra é esta no MAL (manual) ou não está no MAL (none).
create table sync.mal_links (
  user_id uuid not null,
  series_key text not null,
  mal_id integer check (mal_id > 0),
  mal_title text check (length(mal_title) <= 512),
  origin text not null check (origin in ('manual', 'none')),
  changed_at bigint not null,
  seq bigint not null,
  updated_at timestamptz not null default now(),
  primary key (user_id, series_key),
  foreign key (user_id, series_key) references sync.series (user_id, series_key) on delete cascade,
  check ((origin = 'none') = (mal_id is null))
);

create index repositories_seq on sync.repositories (user_id, seq);
create index extensions_seq on sync.extensions (user_id, seq);
create index series_seq on sync.series (user_id, seq);
create index chapters_seq on sync.chapters (user_id, seq);
create index collections_seq on sync.collections (user_id, seq);
create index series_collections_seq on sync.series_collections (user_id, seq);
create index mal_links_seq on sync.mal_links (user_id, seq);
create index series_removed on sync.series (removed_at) where removed;
create index repositories_removed on sync.repositories (changed_at) where removed;
create index extensions_removed on sync.extensions (changed_at) where removed;
create index collections_removed on sync.collections (changed_at) where removed;

-- RLS ligada em tudo. Ninguém fora das funções tem acesso ao schema; as políticas são a segunda barreira,
-- caso alguém dê acesso por engano: cada um só veria as próprias linhas.
do $$
declare
  t text;
begin
  foreach t in array array['accounts', 'devices', 'repositories', 'extensions', 'series', 'chapters', 'collections',
                           'series_collections', 'mal_links'] loop
    execute format('alter table sync.%I enable row level security', t);
    execute format('create policy own_rows on sync.%I for all to authenticated using (user_id = auth.uid()) with check (user_id = auth.uid())', t);
  end loop;
end;
$$;
revoke all on all tables in schema sync from public, anon, authenticated;
revoke all on sequence sync.seq from public, anon, authenticated;

-- ---------------------------------------------------------------------------------------------------------------------
-- seq: muda só quando a linha muda de fato, para o pull não devolver o que já se sabe.
-- ---------------------------------------------------------------------------------------------------------------------

create function sync.stamp() returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if tg_op = 'UPDATE' and (to_jsonb(new) - 'seq' - 'updated_at') = (to_jsonb(old) - 'seq' - 'updated_at') then
    new.seq := old.seq;
    new.updated_at := old.updated_at;
  else
    new.seq := nextval('sync.seq');
    new.updated_at := now();
  end if;
  return new;
end;
$$;

do $$
declare
  t text;
begin
  foreach t in array array['repositories', 'extensions', 'series', 'chapters', 'collections', 'series_collections',
                           'mal_links'] loop
    execute format('create trigger stamp before insert or update on sync.%I for each row execute function sync.stamp()', t);
  end loop;
end;
$$;

-- ---------------------------------------------------------------------------------------------------------------------
-- Limites e validação
-- ---------------------------------------------------------------------------------------------------------------------

create function sync.limits() returns jsonb
language sql immutable
set search_path = ''
as $$
  select jsonb_build_object(
    'max_changes', 500,
    'max_bytes', 262144,
    'calls_per_minute', 120,
    'max_devices', 20,
    'max_series', 5000,
    'max_chapters', 50000,
    'max_repositories', 200,
    'max_extensions', 1000,
    'max_collections', 500,
    'pull_limit', 1000,
    'tombstone_days', 90
  )
$$;

create function sync.now_ms() returns bigint
language sql stable
set search_path = ''
as $$ select (extract(epoch from clock_timestamp()) * 1000)::bigint $$;

-- Uma data do app: nula se ausente, recusada se impossível, a hora do servidor se estiver no futuro.
create function sync.ts(value jsonb) returns bigint
language plpgsql stable
set search_path = ''
as $$
declare
  v bigint;
begin
  if value is null or jsonb_typeof(value) = 'null' then
    return null;
  end if;
  if jsonb_typeof(value) <> 'number' then
    raise exception 'data inválida' using errcode = '22023';
  end if;
  v := (value #>> '{}')::numeric::bigint;
  if v < 1000000000000 then
    raise exception 'data inválida' using errcode = '22023';
  end if;
  return least(v, sync.now_ms());
end;
$$;

create function sync.text(value jsonb, max_len integer, required boolean default true) returns text
language plpgsql immutable
set search_path = ''
as $$
begin
  if value is null or jsonb_typeof(value) = 'null' then
    if required then
      raise exception 'campo obrigatório ausente' using errcode = '22023';
    end if;
    return null;
  end if;
  if jsonb_typeof(value) <> 'string' or length(value #>> '{}') = 0 or length(value #>> '{}') > max_len then
    raise exception 'texto inválido' using errcode = '22023';
  end if;
  return value #>> '{}';
end;
$$;

create function sync.bool(value jsonb) returns boolean
language plpgsql immutable
set search_path = ''
as $$
begin
  if value is null or jsonb_typeof(value) = 'null' then
    return null;
  end if;
  if jsonb_typeof(value) <> 'boolean' then
    raise exception 'booleano inválido' using errcode = '22023';
  end if;
  return (value #>> '{}')::boolean;
end;
$$;

create function sync.key(value jsonb) returns text
language plpgsql immutable
set search_path = ''
as $$
begin
  if value is null or jsonb_typeof(value) <> 'string' or (value #>> '{}') !~ '^[0-9a-f]{32}$' then
    raise exception 'chave inválida' using errcode = '22023';
  end if;
  return value #>> '{}';
end;
$$;

-- Toda chamada passa por aqui: exige login, cria a conta na primeira vez e conta a frequência.
create function sync.enter() returns uuid
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  lim jsonb := sync.limits();
  calls integer;
begin
  if uid is null then
    raise exception 'entre na sua conta para sincronizar' using errcode = '42501';
  end if;
  insert into sync.accounts (user_id) values (uid) on conflict (user_id) do nothing;
  update sync.accounts
     set window_calls = case when window_start < now() - interval '1 minute' then 1 else window_calls + 1 end,
         window_start = case when window_start < now() - interval '1 minute' then now() else window_start end
   where user_id = uid
  returning window_calls into calls;
  if calls > (lim ->> 'calls_per_minute')::integer then
    raise exception 'muitas sincronizações seguidas, tente daqui a pouco' using errcode = '54000';
  end if;
  return uid;
end;
$$;

-- ---------------------------------------------------------------------------------------------------------------------
-- Envio
-- ---------------------------------------------------------------------------------------------------------------------

-- p_device: {"id", "name", "platform", "version"}
-- p_changes: lista de {"t": tipo, ...}, aplicada em ordem (a obra antes dos capítulos dela). Uma mudança que não
-- pode entrar (obra desconhecida, remoção mais nova) é pulada e contada em "skipped"; uma inválida recusa o envio todo.
create function public.sync_push(p_device jsonb, p_changes jsonb) returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := sync.enter();
  lim jsonb := sync.limits();
  v_device uuid;
  c jsonb;
  t text;
  applied integer := 0;
  skipped integer := 0;
  new_rows integer := 0;
  n integer;
  v_series text;
begin
  perform pg_advisory_xact_lock(hashtextextended('sync:' || uid::text, 0));

  if p_changes is null or jsonb_typeof(p_changes) <> 'array' then
    raise exception 'mudanças inválidas' using errcode = '22023';
  end if;
  if jsonb_array_length(p_changes) > (lim ->> 'max_changes')::integer
     or octet_length(p_changes::text) > (lim ->> 'max_bytes')::integer then
    raise exception 'envio grande demais' using errcode = '54000';
  end if;

  -- O aparelho
  if p_device is null or jsonb_typeof(p_device) <> 'object' then
    raise exception 'aparelho inválido' using errcode = '22023';
  end if;
  begin
    v_device := (p_device ->> 'id')::uuid;
  exception when others then
    raise exception 'aparelho inválido' using errcode = '22023';
  end;
  if v_device is null then
    raise exception 'aparelho inválido' using errcode = '22023';
  end if;
  insert into sync.devices as d (user_id, id, name, platform, app_version)
  values (uid, v_device, sync.text(p_device -> 'name', 80), sync.text(p_device -> 'platform', 16), sync.text(p_device -> 'version', 32))
  on conflict (user_id, id) do update
    set name = excluded.name, platform = excluded.platform, app_version = excluded.app_version, last_seen_at = now();
  if (select count(*) from sync.devices where user_id = uid) > (lim ->> 'max_devices')::integer then
    raise exception 'aparelhos demais nesta conta; remova um em Configurações' using errcode = '54000';
  end if;

  for c in select value from jsonb_array_elements(p_changes) loop
    if jsonb_typeof(c) <> 'object' then
      raise exception 'mudança inválida' using errcode = '22023';
    end if;
    t := c ->> 't';

    if t = 'repository' then
      if sync.ts(c -> 'at') is null then
        raise exception 'data ausente' using errcode = '22023';
      end if;
      insert into sync.repositories as r (user_id, url, key_fingerprint, public_key, enabled, removed, changed_at, seq)
      values (uid, sync.text(c -> 'url', 2048), sync.text(c -> 'fingerprint', 128), sync.text(c -> 'public_key', 1024),
              coalesce(sync.bool(c -> 'enabled'), true), coalesce(sync.bool(c -> 'removed'), false), sync.ts(c -> 'at'), 0)
      on conflict (user_id, url) do update
        set key_fingerprint = excluded.key_fingerprint, public_key = excluded.public_key, enabled = excluded.enabled,
            removed = excluded.removed, changed_at = excluded.changed_at
        where excluded.changed_at > r.changed_at;

    elsif t = 'extension' then
      if coalesce(c ->> 'runtime', '') not in ('prancha', 'mihon') then
        raise exception 'runtime inválido' using errcode = '22023';
      end if;
      if sync.ts(c -> 'at') is null then
        raise exception 'data ausente' using errcode = '22023';
      end if;
      insert into sync.extensions as e (user_id, extension_id, runtime, repository_url, version, enabled, removed, changed_at, seq)
      values (uid, sync.text(c -> 'id', 256), c ->> 'runtime', sync.text(c -> 'repository_url', 2048), sync.text(c -> 'version', 64),
              coalesce(sync.bool(c -> 'enabled'), true), coalesce(sync.bool(c -> 'removed'), false), sync.ts(c -> 'at'), 0)
      on conflict (user_id, extension_id) do update
        set runtime = excluded.runtime, repository_url = excluded.repository_url, version = excluded.version,
            enabled = excluded.enabled, removed = excluded.removed, changed_at = excluded.changed_at
        where excluded.changed_at > e.changed_at;

    elsif t = 'series' then
      if c ? 'chapter_sort' and coalesce(c ->> 'chapter_sort', '') not in ('asc', 'desc') then
        raise exception 'ordem inválida' using errcode = '22023';
      end if;
      insert into sync.series as s (user_id, series_key, extension_id, source_id, item_key, title, cover_url, meta_at,
                                    following, following_at, favorite, favorite_at, chapter_sort, chapter_sort_at,
                                    reader_mode, reader_mode_at, removed, removed_at, seq)
      values (uid, sync.key(c -> 'key'), sync.text(c -> 'extension_id', 256), sync.text(c -> 'source_id', 256),
              sync.text(c -> 'item_key', 1024),
              sync.text(c -> 'title', 512, false), sync.text(c -> 'cover_url', 2048, false), sync.ts(c -> 'meta_at'),
              coalesce(sync.bool(c -> 'following'), false), sync.ts(c -> 'following_at'),
              coalesce(sync.bool(c -> 'favorite'), false), sync.ts(c -> 'favorite_at'),
              coalesce(c ->> 'chapter_sort', 'desc'), sync.ts(c -> 'chapter_sort_at'),
              sync.text(c -> 'reader_mode', 32, false), sync.ts(c -> 'reader_mode_at'),
              coalesce(sync.bool(c -> 'removed'), false), sync.ts(c -> 'removed_at'), 0)
      on conflict (user_id, series_key) do update
        set title = case when excluded.meta_at > coalesce(s.meta_at, 0) then excluded.title else s.title end,
            cover_url = case when excluded.meta_at > coalesce(s.meta_at, 0) then excluded.cover_url else s.cover_url end,
            meta_at = greatest(s.meta_at, excluded.meta_at),
            following = case when excluded.following_at > coalesce(s.following_at, 0) then excluded.following else s.following end,
            following_at = greatest(s.following_at, excluded.following_at),
            favorite = case when excluded.favorite_at > coalesce(s.favorite_at, 0) then excluded.favorite else s.favorite end,
            favorite_at = greatest(s.favorite_at, excluded.favorite_at),
            chapter_sort = case when excluded.chapter_sort_at > coalesce(s.chapter_sort_at, 0) then excluded.chapter_sort else s.chapter_sort end,
            chapter_sort_at = greatest(s.chapter_sort_at, excluded.chapter_sort_at),
            reader_mode = case when excluded.reader_mode_at > coalesce(s.reader_mode_at, 0) then excluded.reader_mode else s.reader_mode end,
            reader_mode_at = greatest(s.reader_mode_at, excluded.reader_mode_at),
            removed = case when excluded.removed_at > coalesce(s.removed_at, 0) then excluded.removed else s.removed end,
            removed_at = greatest(s.removed_at, excluded.removed_at);
      -- Removida por último: o estado dos capítulos e as pastas saem junto, como no app.
      if exists (select 1 from sync.series x where x.user_id = uid and x.series_key = c ->> 'key' and x.removed) then
        delete from sync.chapters where user_id = uid and series_key = c ->> 'key';
        delete from sync.series_collections where user_id = uid and series_key = c ->> 'key';
      end if;

    elsif t = 'chapter' then
      v_series := sync.key(c -> 'series');
      -- Obra desconhecida ou removida: o capítulo não entra. Para voltar, a obra precisa ser adicionada de novo
      -- (uma remoção mais nova que a última adição vence).
      if not exists (select 1 from sync.series x where x.user_id = uid and x.series_key = v_series and not x.removed) then
        skipped := skipped + 1;
        continue;
      end if;
      if (c ? 'read') and (sync.bool(c -> 'read') is null or sync.ts(c -> 'read_at') is null) then
        raise exception 'estado de leitura sem data' using errcode = '22023';
      end if;
      if (c ? 'page_index') and sync.ts(c -> 'position_at') is null then
        raise exception 'posição sem data' using errcode = '22023';
      end if;
      insert into sync.chapters as ch (user_id, series_key, chapter_key, started_at, read, read_at, completed_at,
                                       acknowledged_at, page_index, page_count, scroll_ratio, position_at, position_device, seq)
      values (uid, v_series, sync.key(c -> 'key'), sync.ts(c -> 'started_at'),
              sync.bool(c -> 'read'), sync.ts(c -> 'read_at'),
              case when sync.bool(c -> 'read') then coalesce(sync.ts(c -> 'completed_at'), sync.ts(c -> 'read_at')) end,
              sync.ts(c -> 'acknowledged_at'),
              case when c ? 'page_index' then (c ->> 'page_index')::integer end,
              case when c ? 'page_index' then (c ->> 'page_count')::integer end,
              case when c ? 'page_index' then least(greatest(coalesce((c ->> 'scroll_ratio')::real, 0), 0), 1) end,
              case when c ? 'page_index' then sync.ts(c -> 'position_at') end,
              case when c ? 'page_index' then v_device end,
              0)
      on conflict (user_id, series_key, chapter_key) do update
        set started_at = least(ch.started_at, excluded.started_at),
            acknowledged_at = least(ch.acknowledged_at, excluded.acknowledged_at),
            -- Lido duas vezes: fica a conclusão mais antiga. Fora isso, o último lido/não lido vence.
            completed_at = case
              when excluded.read_at is null then ch.completed_at
              when ch.read is true and excluded.read is true then least(ch.completed_at, excluded.completed_at)
              when ch.read_at is null or excluded.read_at > ch.read_at then excluded.completed_at
              else ch.completed_at end,
            read = case when excluded.read_at > coalesce(ch.read_at, 0) then excluded.read else ch.read end,
            read_at = greatest(ch.read_at, excluded.read_at),
            page_index = case when sync.newer(excluded.position_at, excluded.position_device, ch.position_at, ch.position_device)
                              then excluded.page_index else ch.page_index end,
            page_count = case when sync.newer(excluded.position_at, excluded.position_device, ch.position_at, ch.position_device)
                              then excluded.page_count else ch.page_count end,
            scroll_ratio = case when sync.newer(excluded.position_at, excluded.position_device, ch.position_at, ch.position_device)
                                then excluded.scroll_ratio else ch.scroll_ratio end,
            position_device = case when sync.newer(excluded.position_at, excluded.position_device, ch.position_at, ch.position_device)
                                   then excluded.position_device else ch.position_device end,
            position_at = case when sync.newer(excluded.position_at, excluded.position_device, ch.position_at, ch.position_device)
                               then excluded.position_at else ch.position_at end
      returning (xmax = 0)::integer into n;
      new_rows := new_rows + n;

    elsif t = 'collection' then
      if sync.ts(c -> 'at') is null then
        raise exception 'data ausente' using errcode = '22023';
      end if;
      insert into sync.collections as k (user_id, collection_id, name, removed, changed_at, seq)
      values (uid, (c ->> 'id')::uuid, sync.text(c -> 'name', 120), coalesce(sync.bool(c -> 'removed'), false), sync.ts(c -> 'at'), 0)
      on conflict (user_id, collection_id) do update
        set name = excluded.name, removed = excluded.removed, changed_at = excluded.changed_at
        where excluded.changed_at > k.changed_at;

    elsif t = 'series_collection' then
      v_series := sync.key(c -> 'series');
      if sync.ts(c -> 'at') is null or sync.bool(c -> 'member') is null then
        raise exception 'pasta sem data' using errcode = '22023';
      end if;
      if not exists (select 1 from sync.series where user_id = uid and series_key = v_series and not removed)
         or not exists (select 1 from sync.collections where user_id = uid and collection_id = (c ->> 'collection')::uuid) then
        skipped := skipped + 1;
        continue;
      end if;
      insert into sync.series_collections as m (user_id, series_key, collection_id, member, changed_at, seq)
      values (uid, v_series, (c ->> 'collection')::uuid, sync.bool(c -> 'member'), sync.ts(c -> 'at'), 0)
      on conflict (user_id, series_key, collection_id) do update
        set member = excluded.member, changed_at = excluded.changed_at
        where excluded.changed_at > m.changed_at;

    elsif t = 'mal_link' then
      v_series := sync.key(c -> 'series');
      if sync.ts(c -> 'at') is null or coalesce(c ->> 'origin', '') not in ('manual', 'none') then
        raise exception 'associação inválida' using errcode = '22023';
      end if;
      if not exists (select 1 from sync.series where user_id = uid and series_key = v_series and not removed) then
        skipped := skipped + 1;
        continue;
      end if;
      insert into sync.mal_links as l (user_id, series_key, mal_id, mal_title, origin, changed_at, seq)
      values (uid, v_series, case when c ->> 'origin' = 'manual' then (c ->> 'mal_id')::integer end,
              case when c ->> 'origin' = 'manual' then sync.text(c -> 'mal_title', 512, false) end,
              c ->> 'origin', sync.ts(c -> 'at'), 0)
      on conflict (user_id, series_key) do update
        set mal_id = excluded.mal_id, mal_title = excluded.mal_title, origin = excluded.origin, changed_at = excluded.changed_at
        where excluded.changed_at > l.changed_at;

    else
      raise exception 'tipo de mudança desconhecido' using errcode = '22023';
    end if;
    applied := applied + 1;
  end loop;

  -- Cotas: contadas só quando algo novo pode ter entrado.
  if (select count(*) from sync.series where user_id = uid) > (lim ->> 'max_series')::integer
     or (new_rows > 0 and (select count(*) from sync.chapters where user_id = uid) > (lim ->> 'max_chapters')::integer)
     or (select count(*) from sync.repositories where user_id = uid) > (lim ->> 'max_repositories')::integer
     or (select count(*) from sync.extensions where user_id = uid) > (lim ->> 'max_extensions')::integer
     or (select count(*) from sync.collections where user_id = uid) > (lim ->> 'max_collections')::integer then
    raise exception 'a conta passou do limite de itens sincronizados' using errcode = '54000';
  end if;

  if random() < 0.01 then
    perform sync.prune();
  end if;

  return jsonb_build_object('applied', applied, 'skipped', skipped, 'cursor', sync.cursor(uid));
end;
$$;

-- A posição (a_at, a_dev) é mais nova que (b_at, b_dev)? Empate exato decide pelo id do aparelho.
create function sync.newer(a_at bigint, a_dev uuid, b_at bigint, b_dev uuid) returns boolean
language sql immutable
set search_path = ''
as $$
  select a_at is not null and (b_at is null or a_at > b_at or (a_at = b_at and a_dev::text > coalesce(b_dev::text, '')))
$$;

-- O maior seq da conta em qualquer tabela: o cursor de quem acabou de enviar.
create function sync.cursor(uid uuid) returns bigint
language sql stable
set search_path = ''
as $$
  select greatest(
    (select max(seq) from sync.repositories where user_id = uid),
    (select max(seq) from sync.extensions where user_id = uid),
    (select max(seq) from sync.series where user_id = uid),
    (select max(seq) from sync.chapters where user_id = uid),
    (select max(seq) from sync.collections where user_id = uid),
    (select max(seq) from sync.series_collections where user_id = uid),
    (select max(seq) from sync.mal_links where user_id = uid),
    0)
$$;

-- ---------------------------------------------------------------------------------------------------------------------
-- Recebimento
-- ---------------------------------------------------------------------------------------------------------------------

-- Tudo da conta com seq > p_since, em ordem de seq. "more" diz se há outra página; "next" é o cursor para ela.
-- "reset": o cursor é mais antigo que a última limpeza de remoções; o app recomeça de 0 e descarta o que não voltar.
create function sync.changes(uid uuid, p_since bigint, p_limit integer) returns table (seq bigint, item jsonb)
language sql stable
set search_path = ''
as $$
  select * from (
    select r.seq, jsonb_build_object('t', 'repository', 'url', r.url, 'fingerprint', r.key_fingerprint, 'public_key', r.public_key,
                                     'enabled', r.enabled, 'removed', r.removed, 'at', r.changed_at)
      from sync.repositories r where r.user_id = uid and r.seq > p_since
    union all
    select e.seq, jsonb_build_object('t', 'extension', 'id', e.extension_id, 'runtime', e.runtime, 'repository_url', e.repository_url,
                                     'version', e.version, 'enabled', e.enabled, 'removed', e.removed, 'at', e.changed_at)
      from sync.extensions e where e.user_id = uid and e.seq > p_since
    union all
    select s.seq, jsonb_build_object('t', 'series', 'key', s.series_key, 'extension_id', s.extension_id, 'source_id', s.source_id,
                                     'item_key', s.item_key, 'title', s.title, 'cover_url', s.cover_url, 'meta_at', s.meta_at,
                                     'following', s.following, 'following_at', s.following_at, 'favorite', s.favorite,
                                     'favorite_at', s.favorite_at, 'chapter_sort', s.chapter_sort, 'chapter_sort_at', s.chapter_sort_at,
                                     'reader_mode', s.reader_mode, 'reader_mode_at', s.reader_mode_at, 'removed', s.removed,
                                     'removed_at', s.removed_at)
      from sync.series s where s.user_id = uid and s.seq > p_since
    union all
    select ch.seq, jsonb_build_object('t', 'chapter', 'series', ch.series_key, 'key', ch.chapter_key, 'started_at', ch.started_at,
                                      'read', ch.read, 'read_at', ch.read_at, 'completed_at', ch.completed_at,
                                      'acknowledged_at', ch.acknowledged_at, 'page_index', ch.page_index, 'page_count', ch.page_count,
                                      'scroll_ratio', ch.scroll_ratio, 'position_at', ch.position_at, 'position_device', ch.position_device)
      from sync.chapters ch where ch.user_id = uid and ch.seq > p_since
    union all
    select k.seq, jsonb_build_object('t', 'collection', 'id', k.collection_id, 'name', k.name, 'removed', k.removed, 'at', k.changed_at)
      from sync.collections k where k.user_id = uid and k.seq > p_since
    union all
    select m.seq, jsonb_build_object('t', 'series_collection', 'series', m.series_key, 'collection', m.collection_id,
                                     'member', m.member, 'at', m.changed_at)
      from sync.series_collections m where m.user_id = uid and m.seq > p_since
    union all
    select l.seq, jsonb_build_object('t', 'mal_link', 'series', l.series_key, 'mal_id', l.mal_id, 'mal_title', l.mal_title,
                                     'origin', l.origin, 'at', l.changed_at)
      from sync.mal_links l where l.user_id = uid and l.seq > p_since
  ) x
  order by x.seq
  limit p_limit
$$;

create function public.sync_pull(p_since bigint default 0, p_limit integer default 1000) returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := sync.enter();
  lim jsonb := sync.limits();
  v_limit integer := least(greatest(coalesce(p_limit, 1000), 1), (lim ->> 'pull_limit')::integer);
  v_since bigint := greatest(coalesce(p_since, 0), 0);
  v_floor bigint;
  v_reset boolean := false;
  v_items jsonb;
  v_last bigint;
  v_count integer;
begin
  select floor_seq into v_floor from sync.accounts where user_id = uid;
  if v_since > 0 and v_since < v_floor then
    v_reset := true;
    v_since := 0;
  end if;
  select coalesce(jsonb_agg(c.item order by c.seq), '[]'::jsonb), max(c.seq), count(*)
    into v_items, v_last, v_count
    from sync.changes(uid, v_since, v_limit) c;
  return jsonb_build_object(
    'changes', v_items,
    'next', coalesce(v_last, v_since),
    'more', v_count = v_limit and exists (select 1 from sync.changes(uid, v_last, 1)),
    'reset', v_reset,
    'server_time', sync.now_ms());
end;
$$;

-- ---------------------------------------------------------------------------------------------------------------------
-- Aparelhos, exportação e exclusão
-- ---------------------------------------------------------------------------------------------------------------------

create function public.sync_devices() returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := sync.enter();
begin
  return coalesce((select jsonb_agg(jsonb_build_object('id', d.id, 'name', d.name, 'platform', d.platform,
                                                       'version', d.app_version, 'created_at', d.created_at,
                                                       'last_seen_at', d.last_seen_at) order by d.last_seen_at desc)
                     from sync.devices d where d.user_id = uid), '[]'::jsonb);
end;
$$;

create function public.sync_remove_device(p_id uuid) returns boolean
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := sync.enter();
begin
  delete from sync.devices where user_id = uid and id = p_id;
  return found;
end;
$$;

create function public.sync_export() returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := sync.enter();
begin
  return jsonb_build_object(
    'exported_at', now(),
    'devices', public.sync_devices(),
    'items', coalesce((select jsonb_agg(item order by seq) from sync.changes(uid, 0, 2147483647)), '[]'::jsonb));
end;
$$;

-- Apaga tudo o que a conta tem aqui e a própria conta de login. Não mexe na telemetria, que não sabe de contas.
create function public.sync_forget_me() returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  removed integer;
begin
  if uid is null then
    raise exception 'entre na sua conta para apagar' using errcode = '42501';
  end if;
  select count(*) into removed from sync.chapters where user_id = uid;
  delete from sync.mal_links where user_id = uid;
  delete from sync.series_collections where user_id = uid;
  delete from sync.chapters where user_id = uid;
  delete from sync.series where user_id = uid;
  delete from sync.collections where user_id = uid;
  delete from sync.extensions where user_id = uid;
  delete from sync.repositories where user_id = uid;
  delete from sync.devices where user_id = uid;
  delete from sync.accounts where user_id = uid;
  delete from auth.users where id = uid;
  return jsonb_build_object('removed', true, 'chapters', removed);
end;
$$;

-- Marcas de remoção valem 90 dias; depois somem, e o piso do cursor de cada conta sobe até a maior que sumiu.
-- Um aparelho parado há mais tempo que isso recebe "reset" no próximo pull e recomeça do zero.
create function sync.prune() returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  cutoff bigint := sync.now_ms() - ((sync.limits() ->> 'tombstone_days')::bigint * 86400000);
begin
  update sync.accounts a
     set floor_seq = greatest(a.floor_seq, g.max_seq)
    from (select x.user_id, max(x.seq) as max_seq from (
            select user_id, seq from sync.series where removed and removed_at < cutoff
            union all select user_id, seq from sync.repositories where removed and changed_at < cutoff
            union all select user_id, seq from sync.extensions where removed and changed_at < cutoff
            union all select user_id, seq from sync.collections where removed and changed_at < cutoff
            union all select user_id, seq from sync.series_collections where not member and changed_at < cutoff
          ) x group by x.user_id) g
   where a.user_id = g.user_id;
  delete from sync.series where removed and removed_at < cutoff;
  delete from sync.repositories where removed and changed_at < cutoff;
  delete from sync.extensions where removed and changed_at < cutoff;
  delete from sync.collections where removed and changed_at < cutoff;
  delete from sync.series_collections where not member and changed_at < cutoff;
  -- Aparelhos sem contato há um ano saem da lista.
  delete from sync.devices where last_seen_at < now() - interval '365 days';
end;
$$;

-- ---------------------------------------------------------------------------------------------------------------------
-- Permissões: só authenticated chama as portas; nada em sync é chamável de fora.
-- ---------------------------------------------------------------------------------------------------------------------

revoke all on all functions in schema sync from public, anon, authenticated;
revoke all on function public.sync_push(jsonb, jsonb) from public, anon;
revoke all on function public.sync_pull(bigint, integer) from public, anon;
revoke all on function public.sync_devices() from public, anon;
revoke all on function public.sync_remove_device(uuid) from public, anon;
revoke all on function public.sync_export() from public, anon;
revoke all on function public.sync_forget_me() from public, anon;
grant execute on function public.sync_push(jsonb, jsonb) to authenticated;
grant execute on function public.sync_pull(bigint, integer) to authenticated;
grant execute on function public.sync_devices() to authenticated;
grant execute on function public.sync_remove_device(uuid) to authenticated;
grant execute on function public.sync_export() to authenticated;
grant execute on function public.sync_forget_me() to authenticated;

-- Limpeza diária pelo pg_cron, quando a extensão existe (no Supabase existe; no Postgres dos testes, não).
do $$
begin
  if exists (select 1 from pg_available_extensions where name = 'pg_cron') then
    create extension if not exists pg_cron;
    perform cron.schedule('sync-prune', '37 3 * * *', 'select sync.prune()');
  end if;
end;
$$;
