-- Testes da sincronização: permissões, isolamento entre contas, regras de conflito, remoções, cursor, limites,
-- exportação e exclusão. Roda com supabase/tests/run.sh, depois das migrações.
\set ON_ERROR_STOP on
\set QUIET on
\o /dev/null

insert into auth.users (id, email) values
  ('00000000-0000-0000-0000-0000000000c1', 'ana@example.com'),
  ('00000000-0000-0000-0000-0000000000c2', 'bia@example.com');

-- Aparelhos e chaves de exemplo.
create function pg_temp.pc() returns jsonb language sql as $$
  select '{"id":"11111111-1111-1111-1111-111111111111","name":"PC","platform":"windows","version":"0.13.0"}'::jsonb $$;
create function pg_temp.cel() returns jsonb language sql as $$
  select '{"id":"22222222-2222-2222-2222-222222222222","name":"Celular","platform":"android","version":"0.13.0"}'::jsonb $$;
create function pg_temp.obra(key text default 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', extra jsonb default '{}') returns jsonb language sql as $$
  select jsonb_build_object('t', 'series', 'key', key, 'extension_id', 'demo', 'source_id', 'demo', 'item_key', 'obra-1',
                            'title', 'Farol de Papel', 'meta_at', 1763200000000, 'following', true, 'following_at', 1763200000000) || extra $$;
create function pg_temp.cap(extra jsonb, key text default 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb') returns jsonb language sql as $$
  select jsonb_build_object('t', 'chapter', 'series', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'key', key) || extra $$;
create function pg_temp.linha(uid text, key text default 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb') returns sync.chapters language sql as $$
  select * from sync.chapters where user_id = uid::uuid and chapter_key = key $$;
grant execute on all functions in schema pg_temp to anon, authenticated;

\echo '1. anon não chama nada e authenticated não lê as tabelas'
set role anon;
do $$ begin
  perform public.sync_pull(0, 10);
  raise exception 'anon chamou sync_pull';
exception when insufficient_privilege then null; end $$;
do $$ begin
  perform public.sync_push(pg_temp.pc(), '[]');
  raise exception 'anon chamou sync_push';
exception when insufficient_privilege then null; end $$;
reset role;
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
do $$ begin
  perform 1 from sync.chapters;
  raise exception 'authenticated leu sync.chapters direto';
exception when insufficient_privilege then null; end $$;
do $$ begin
  perform sync.prune();
  raise exception 'authenticated chamou sync.prune';
exception when insufficient_privilege then null; end $$;
reset role;

\echo '2. sem login não sincroniza'
set role authenticated;
set request.jwt.claim.sub = '';
do $$ begin
  perform public.sync_pull(0, 10);
  raise exception 'pull sem login passou';
exception when insufficient_privilege then null; end $$;
reset role;

\echo '3. o PC envia obra, capítulo e posição; o celular recebe'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
do $$
declare r jsonb; p jsonb; ch jsonb;
begin
  r := public.sync_push(pg_temp.pc(), jsonb_build_array(
    jsonb_build_object('t', 'repository', 'url', 'https://exemplo.dev/prancha-repo.json', 'fingerprint', 'SHA256:abc', 'public_key', 'chave', 'at', 1763200000000),
    jsonb_build_object('t', 'extension', 'id', 'demo', 'runtime', 'prancha', 'repository_url', 'https://exemplo.dev/prancha-repo.json', 'version', '1.0.0', 'at', 1763200000000),
    pg_temp.obra(),
    pg_temp.cap('{"started_at": 1763200100000, "page_index": 11, "page_count": 30, "scroll_ratio": 0.5, "position_at": 1763200200000}')));
  if (r ->> 'applied')::int <> 4 or (r ->> 'skipped')::int <> 0 or (r ->> 'cursor')::bigint <= 0 then
    raise exception 'envio inesperado: %', r;
  end if;
  p := public.sync_pull(0, 100);
  if jsonb_array_length(p -> 'changes') <> 4 or (p ->> 'more')::boolean or (p ->> 'reset')::boolean then
    raise exception 'pull inesperado: %', p;
  end if;
  select value into ch from jsonb_array_elements(p -> 'changes') where value ->> 't' = 'chapter';
  if (ch ->> 'page_index')::int <> 11 or (ch ->> 'position_device') <> '11111111-1111-1111-1111-111111111111' then
    raise exception 'posição errada: %', ch;
  end if;
  if (p ->> 'next')::bigint <> (r ->> 'cursor')::bigint then
    raise exception 'cursor do pull diferente do envio: % %', p ->> 'next', r ->> 'cursor';
  end if;
  -- Nada novo depois do cursor.
  if jsonb_array_length(public.sync_pull((p ->> 'next')::bigint, 100) -> 'changes') <> 0 then
    raise exception 'pull repetiu mudanças';
  end if;
end $$;
reset role;

\echo '4. outra conta não vê nada da primeira'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c2';
do $$ begin
  if jsonb_array_length(public.sync_pull(0, 100) -> 'changes') <> 0 then
    raise exception 'conta B viu dados da conta A';
  end if;
  -- E não grava capítulo numa obra que não é dela.
  if (public.sync_push(pg_temp.cel(), jsonb_build_array(pg_temp.cap('{"started_at": 1763200100000}'))) ->> 'skipped')::int <> 1 then
    raise exception 'conta B gravou capítulo de obra alheia';
  end if;
end $$;
reset role;
do $$ begin
  if (select count(*) from sync.chapters where user_id = '00000000-0000-0000-0000-0000000000c2') <> 0 then
    raise exception 'capítulo da conta B entrou';
  end if;
end $$;

\echo '5. posição: a mais recente vence, em qualquer ordem de chegada'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
select public.sync_push(pg_temp.cel(), jsonb_build_array(pg_temp.cap('{"page_index": 14, "page_count": 30, "position_at": 1763200300000}'))) is not null;
-- Chega atrasada uma posição mais antiga do PC: não volta.
select public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.cap('{"page_index": 5, "page_count": 30, "position_at": 1763200250000}'))) is not null;
reset role;
do $$ declare c sync.chapters := pg_temp.linha('00000000-0000-0000-0000-0000000000c1'); begin
  if c.page_index <> 14 or c.position_device <> '22222222-2222-2222-2222-222222222222' then
    raise exception 'posição antiga venceu: % %', c.page_index, c.position_device;
  end if;
  if c.started_at <> 1763200100000 then
    raise exception 'início mudou: %', c.started_at;
  end if;
end $$;

\echo '6. lido: concluir nos dois fica com a conclusão mais antiga; não lido mais novo vence; mais antigo não'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
select public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.cap('{"read": true, "read_at": 1763200500000, "completed_at": 1763200500000}'))) is not null;
select public.sync_push(pg_temp.cel(), jsonb_build_array(pg_temp.cap('{"read": true, "read_at": 1763200400000, "completed_at": 1763200400000}'))) is not null;
reset role;
do $$ declare c sync.chapters := pg_temp.linha('00000000-0000-0000-0000-0000000000c1'); begin
  if c.read is not true or c.completed_at <> 1763200400000 or c.read_at <> 1763200500000 then
    raise exception 'conclusão errada: % % %', c.read, c.completed_at, c.read_at;
  end if;
end $$;
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
-- "Não lido" de antes da última conclusão (celular offline) não desfaz.
select public.sync_push(pg_temp.cel(), jsonb_build_array(pg_temp.cap('{"read": false, "read_at": 1763200450000}'))) is not null;
reset role;
do $$ begin
  if (pg_temp.linha('00000000-0000-0000-0000-0000000000c1')).read is not true then
    raise exception 'não lido antigo desfez a conclusão';
  end if;
end $$;
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
select public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.cap('{"read": false, "read_at": 1763200600000}'))) is not null;
reset role;
do $$ declare c sync.chapters := pg_temp.linha('00000000-0000-0000-0000-0000000000c1'); begin
  if c.read is not false or c.completed_at is not null then
    raise exception 'não lido novo não venceu: % %', c.read, c.completed_at;
  end if;
  -- A posição não muda com o estado.
  if c.page_index <> 14 then
    raise exception 'marcar como não lido mexeu na posição';
  end if;
end $$;

\echo '7. novidade vista: a primeira vez vale para todos'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
select public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.cap('{"acknowledged_at": 1763200700000}'))) is not null;
select public.sync_push(pg_temp.cel(), jsonb_build_array(pg_temp.cap('{"acknowledged_at": 1763200650000}'))) is not null;
reset role;
do $$ begin
  if (pg_temp.linha('00000000-0000-0000-0000-0000000000c1')).acknowledged_at <> 1763200650000 then
    raise exception 'novidade vista errada';
  end if;
end $$;

\echo '8. obra: cada campo tem a própria data'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
select public.sync_push(pg_temp.cel(), jsonb_build_array(pg_temp.obra(extra => '{"favorite": true, "favorite_at": 1763200800000}'))) is not null;
select public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.obra(extra => '{"reader_mode": "webtoon", "reader_mode_at": 1763200900000, "favorite": false, "favorite_at": 1763200100000}'))) is not null;
select public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.obra(extra => '{"following": false, "following_at": 1763100000000}'))) is not null;
reset role;
do $$ declare s sync.series; begin
  select * into s from sync.series where user_id = '00000000-0000-0000-0000-0000000000c1';
  if s.favorite is not true or s.reader_mode <> 'webtoon' or s.following is not true then
    raise exception 'campos da obra se apagaram: fav % modo % acompanhando %', s.favorite, s.reader_mode, s.following;
  end if;
end $$;

\echo '9. data no futuro vira a hora do servidor'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
select public.sync_push(pg_temp.cel(), jsonb_build_array(pg_temp.cap('{"page_index": 29, "page_count": 30, "position_at": 4102444800000}', 'cccccccccccccccccccccccccccccccc'))) is not null;
reset role;
do $$ begin
  if (pg_temp.linha('00000000-0000-0000-0000-0000000000c1', 'cccccccccccccccccccccccccccccccc')).position_at > sync.now_ms() then
    raise exception 'data no futuro foi aceita';
  end if;
end $$;

\echo '10. repetir o mesmo envio não gera mudança nova'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
do $$ declare antes bigint; depois jsonb; begin
  antes := (public.sync_pull(0, 1000) ->> 'next')::bigint;
  perform public.sync_push(pg_temp.cel(), jsonb_build_array(pg_temp.cap('{"acknowledged_at": 1763200650000}')));
  depois := public.sync_pull(antes, 1000);
  if jsonb_array_length(depois -> 'changes') <> 0 then
    raise exception 'envio repetido gerou mudança: %', depois;
  end if;
end $$;
reset role;

\echo '11. envios inválidos são recusados inteiros'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
do $$ begin
  perform public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.cap('{"started_at": 1763200100000}', 'dddddddddddddddddddddddddddddddd'), '{"t": "senha"}'::jsonb));
  raise exception 'tipo desconhecido passou';
exception when invalid_parameter_value then null; end $$;
do $$ begin
  perform public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.cap('{}', 'NAO-E-HEX')));
  raise exception 'chave inválida passou';
exception when invalid_parameter_value then null; end $$;
do $$ begin
  perform public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.cap('{"page_index": 3, "page_count": 10}')));
  raise exception 'posição sem data passou';
exception when invalid_parameter_value then null; end $$;
do $$ begin
  perform public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.cap('{"started_at": 12345}')));
  raise exception 'data impossível passou';
exception when invalid_parameter_value then null; end $$;
do $$ begin
  perform public.sync_push('{"id":"nao-e-uuid","name":"x","platform":"windows","version":"1"}', '[]');
  raise exception 'aparelho inválido passou';
exception when invalid_parameter_value then null; end $$;
do $$ begin
  perform public.sync_push(pg_temp.pc(), (select jsonb_agg(pg_temp.cap('{"acknowledged_at": 1763200650000}')) from generate_series(1, 501)));
  raise exception 'envio com 501 mudanças passou';
exception when program_limit_exceeded then null; end $$;
reset role;
do $$ begin
  if exists (select 1 from sync.chapters where chapter_key = 'dddddddddddddddddddddddddddddddd') then
    raise exception 'parte do envio recusado ficou gravada';
  end if;
end $$;

\echo '12. pastas e associação com o MAL'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
do $$ declare r jsonb; begin
  r := public.sync_push(pg_temp.pc(), jsonb_build_array(
    '{"t": "collection", "id": "33333333-3333-3333-3333-333333333333", "name": "Lendo no ônibus", "at": 1763201000000}'::jsonb,
    '{"t": "series_collection", "series": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "collection": "33333333-3333-3333-3333-333333333333", "member": true, "at": 1763201000000}'::jsonb,
    '{"t": "mal_link", "series": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "origin": "manual", "mal_id": 123, "mal_title": "Farol", "at": 1763201000000}'::jsonb));
  if (r ->> 'applied')::int <> 3 then
    raise exception 'pastas/MAL: %', r;
  end if;
end $$;
do $$ begin
  perform public.sync_push(pg_temp.pc(), '[{"t": "mal_link", "series": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "origin": "none", "mal_id": 5, "at": 1763201100000}]');
end $$;
reset role;
do $$ begin
  if (select mal_id from sync.mal_links where user_id = '00000000-0000-0000-0000-0000000000c1') is not null then
    raise exception '"não está no MAL" ficou com mal_id';
  end if;
end $$;

\echo '13. pull em páginas devolve tudo, uma vez, em ordem'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
do $$ declare cur bigint := 0; total int := 0; p jsonb; voltas int := 0; begin
  loop
    p := public.sync_pull(cur, 2);
    total := total + jsonb_array_length(p -> 'changes');
    cur := (p ->> 'next')::bigint;
    voltas := voltas + 1;
    exit when not (p ->> 'more')::boolean or voltas > 50;
  end loop;
  if total <> jsonb_array_length(public.sync_pull(0, 1000) -> 'changes') then
    raise exception 'páginas somaram % mudanças', total;
  end if;
end $$;
reset role;

\echo '14. remover a obra apaga os capítulos; capítulo atrasado não volta; adicionar de novo volta'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
select public.sync_push(pg_temp.pc(), jsonb_build_array(pg_temp.obra(extra => '{"removed": true, "removed_at": 1763202000000}'))) is not null;
do $$ declare r jsonb; begin
  r := public.sync_push(pg_temp.cel(), jsonb_build_array(pg_temp.cap('{"page_index": 2, "page_count": 30, "position_at": 1763202100000}')));
  if (r ->> 'skipped')::int <> 1 then
    raise exception 'capítulo de obra removida entrou: %', r;
  end if;
end $$;
reset role;
do $$ begin
  if exists (select 1 from sync.chapters where user_id = '00000000-0000-0000-0000-0000000000c1') then
    raise exception 'capítulos da obra removida ficaram';
  end if;
  if exists (select 1 from sync.series_collections where user_id = '00000000-0000-0000-0000-0000000000c1') then
    raise exception 'pastas da obra removida ficaram';
  end if;
end $$;
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
select public.sync_push(pg_temp.pc(), jsonb_build_array(
  pg_temp.obra(extra => '{"removed": false, "removed_at": 1763203000000}'),
  pg_temp.cap('{"started_at": 1763203100000}'))) is not null;
reset role;
do $$ begin
  if (pg_temp.linha('00000000-0000-0000-0000-0000000000c1')).started_at is null then
    raise exception 'obra adicionada de novo não aceitou capítulo';
  end if;
end $$;

\echo '15. limpeza de remoções antigas: cursor velho recebe reset'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
select public.sync_push(pg_temp.pc(), jsonb_build_array(
  '{"t": "extension", "id": "velha", "runtime": "prancha", "repository_url": "https://exemplo.dev/prancha-repo.json", "version": "0.1.0", "removed": true, "at": 1700000000000}'::jsonb)) is not null;
reset role;
select sync.prune() is null;
do $$ begin
  if exists (select 1 from sync.extensions where extension_id = 'velha') then
    raise exception 'remoção antiga não saiu';
  end if;
end $$;
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
do $$ declare p jsonb; begin
  p := public.sync_pull(1, 1000);
  if not (p ->> 'reset')::boolean then
    raise exception 'cursor anterior à limpeza não recebeu reset';
  end if;
  if (public.sync_pull(0, 1000) ->> 'reset')::boolean then
    raise exception 'pull do zero recebeu reset';
  end if;
end $$;
reset role;

\echo '16. aparelhos, exportação e exclusão da conta'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c1';
do $$ declare d jsonb; e jsonb; begin
  d := public.sync_devices();
  if jsonb_array_length(d) <> 2 then
    raise exception 'aparelhos: %', d;
  end if;
  if not public.sync_remove_device('22222222-2222-2222-2222-222222222222') then
    raise exception 'não removeu o aparelho';
  end if;
  e := public.sync_export();
  if jsonb_array_length(e -> 'items') = 0 or jsonb_array_length(e -> 'devices') <> 1 then
    raise exception 'exportação: %', e;
  end if;
  perform public.sync_forget_me();
end $$;
reset role;
do $$ begin
  if exists (select 1 from auth.users where id = '00000000-0000-0000-0000-0000000000c1')
     or exists (select 1 from sync.series where user_id = '00000000-0000-0000-0000-0000000000c1')
     or exists (select 1 from sync.accounts where user_id = '00000000-0000-0000-0000-0000000000c1')
     or exists (select 1 from sync.devices where user_id = '00000000-0000-0000-0000-0000000000c1') then
    raise exception 'sobrou algo da conta apagada';
  end if;
  if not exists (select 1 from auth.users where id = '00000000-0000-0000-0000-0000000000c2') then
    raise exception 'apagou a conta errada';
  end if;
end $$;

\echo '17. limite de frequência'
set role authenticated;
set request.jwt.claim.sub = '00000000-0000-0000-0000-0000000000c2';
do $$ begin
  for i in 1..130 loop
    perform public.sync_pull(0, 1);
  end loop;
  raise exception 'limite de frequência não segurou';
exception when program_limit_exceeded then null; end $$;
reset role;

\echo 'Sincronização: tudo certo.'
