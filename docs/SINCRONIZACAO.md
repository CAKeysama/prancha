# Sincronização entre aparelhos

**Estado:** o banco está pronto ([`supabase/migrations/20261115220000_sync.sql`](../supabase/migrations/20261115220000_sync.sql)); o app ainda não sincroniza. A proposta completa, com as decisões, está no documento de design.

A sincronização deixa a pessoa ler um capítulo online no PC e continuar no celular de onde parou. É opcional e só funciona com conta (Google ou Discord). Vai para a nuvem só o que é preciso para continuar a leitura online:

| Sobe | Não sobe |
|---|---|
| Repositórios (endereço e a chave confiada) | HQs locais (PDF, CBZ, CBR, pastas) e o progresso delas |
| Extensões instaladas (id, versão, ativa) | Imagens, capas baixadas, capítulos baixados |
| Obras das fontes na biblioteca: acompanhando, favorita, ordem, modo de leitura, título e endereço da capa | Lista de capítulos, sinopse e gêneros (cada aparelho busca na fonte) |
| Estado de cada capítulo: iniciado, lido ou não lido, novidade vista | Dados que as extensões guardam (`extension_storage`), que podem ter logins de sites |
| Posição no capítulo (página, total, rolagem) | Tokens do MyAnimeList |
| Pastas da biblioteca e associações manuais com o MyAnimeList | Sessões do leitor, preferências globais, telemetria |

## Identidade das obras e dos capítulos

O banco local identifica tudo por números que só valem naquele aparelho. Na nuvem, cada obra e cada capítulo são identificados por uma chave portátil:

- **Extensão do Prancha:** o `remote_id` que a extensão devolve, que já precisa ser estável ([DEVELOPMENT.md](extensions/DEVELOPMENT.md)).
- **Extensão do Mihon (pelo Suwayomi):** o `remote_id` local é o número da obra no banco do Suwayomi daquele PC e não serve. A chave portátil é o endereço (`url`) da obra e do capítulo na fonte, que o Suwayomi também devolve, junto com o id da fonte, que é o mesmo em qualquer instalação do Mihon.

`series_key` é o BLAKE3 de `extension_id`, `source_id` e a chave portátil da obra, cortado em 16 bytes e escrito em hexadecimal (32 caracteres). `chapter_key` é o mesmo sobre a chave portátil do capítulo. A obra sobe também com as três partes legíveis, porque o outro aparelho precisa delas para abri-la na fonte; o capítulo sobe só com o hash, e o outro aparelho o reconhece calculando o mesmo hash sobre a lista de capítulos que a fonte devolve.

## Protocolo

O app fala direto com o Supabase, com a chave pública (anon) e o token da pessoa, por `POST /rest/v1/rpc/<função>`. Não há `service_role` no app nem função na Vercel.

| Função | Entrada | Saída |
|---|---|---|
| `sync_push` | `p_device` `{id, name, platform, version}` e `p_changes` (lista de até 500 mudanças, 256 KB) | `{applied, skipped, cursor}` |
| `sync_pull` | `p_since` (cursor) e `p_limit` (até 1000) | `{changes, next, more, reset, server_time}` |
| `sync_devices` / `sync_remove_device` | / `p_id` | lista de aparelhos / `true` se removeu |
| `sync_export` | | `{exported_at, devices, items}` |
| `sync_forget_me` | | apaga os dados e a conta de login |

Uma mudança é um objeto com `t` e os campos daquele tipo. Datas são milissegundos Unix; uma data no futuro vira a hora do servidor. Campos ausentes não mudam nada.

| `t` | Chave | Campos |
|---|---|---|
| `repository` | `url` | `fingerprint`, `public_key`, `enabled`, `removed`, `at` |
| `extension` | `id` | `runtime` (`prancha` ou `mihon`), `repository_url`, `version`, `enabled`, `removed`, `at` |
| `series` | `key` | `extension_id`, `source_id`, `item_key` (sempre); `title` + `cover_url` + `meta_at`; `following` + `following_at`; `favorite` + `favorite_at`; `chapter_sort` + `chapter_sort_at`; `reader_mode` + `reader_mode_at`; `removed` + `removed_at` |
| `chapter` | `series`, `key` | `started_at`; `read` + `read_at` (+ `completed_at`); `acknowledged_at`; `page_index` + `page_count` + `scroll_ratio` + `position_at` |
| `collection` | `id` (uuid gerado no aparelho) | `name`, `removed`, `at` |
| `series_collection` | `series`, `collection` | `member`, `at` |
| `mal_link` | `series` | `origin` (`manual` ou `none`), `mal_id`, `mal_title`, `at` |

As mudanças são aplicadas em ordem, então a obra vai antes dos capítulos dela. Uma mudança que não pode entrar (capítulo de obra desconhecida ou removida, pasta inexistente) é pulada e contada em `skipped`; uma mudança inválida recusa o envio inteiro.

O pull devolve as mudanças de todas as tabelas em ordem de `seq`. O app guarda `next` e pede de novo enquanto `more` for verdadeiro. `reset` verdadeiro quer dizer que o cursor é mais antigo que a última limpeza de remoções (90 dias): o app puxa tudo de novo a partir de 0 e remove o que não voltar. Cada envio trava a conta, então o pull nunca pula uma mudança que ainda estava sendo gravada.

## Regras de conflito

O banco aplica as regras ao receber, e o app aplica as mesmas ao puxar, então dois aparelhos chegam ao mesmo resultado em qualquer ordem.

| Campo | Regra |
|---|---|
| `read` / `read_at` / `completed_at` | O último "lido" ou "não lido" vence. Dois "lido" seguidos ficam com a conclusão mais antiga. |
| `started_at`, `acknowledged_at` | A data mais antiga. |
| Posição | A de `position_at` mais recente; empate exato decide pelo id do aparelho. |
| Campos da obra | Cada um com a própria data; o mais recente vence, campo a campo. |
| Repositório, extensão, pasta, obra na pasta, associação com o MAL | A linha inteira; a de `at` mais recente vence. |
| Remoção da obra | Apaga o estado dos capítulos e as pastas dela; capítulos que chegam depois são pulados até a obra ser adicionada de novo com `removed_at` mais novo. |

Uma conclusão que chega pela sincronização não entra na fila do MyAnimeList: só o aparelho onde o capítulo foi concluído no leitor avisa o MAL.

## Limites

Por conta: 120 chamadas por minuto, 20 aparelhos, 5.000 obras, 50.000 capítulos, 200 repositórios, 1.000 extensões e 500 pastas. Passar de um limite recusa o envio com uma mensagem para a pessoa.

## Privacidade

Estes dados são pessoais e ligados a uma conta, ao contrário da telemetria. As tabelas ficam no schema `sync`, fora da API, com RLS ligada; as funções só enxergam as linhas de `auth.uid()`. Nenhuma tabela liga a instalação da telemetria à conta, e o painel admin não lê o schema `sync`. A pessoa baixa tudo (`sync_export`) e apaga a conta (`sync_forget_me`) pelo app. Ver [PRIVACIDADE.md](PRIVACIDADE.md).

## Testes

`bash supabase/tests/run.sh` sobe um Postgres vazio, aplica as migrações e roda [`supabase/tests/sync.test.sql`](../supabase/tests/sync.test.sql): permissões, isolamento entre contas, cada regra de conflito, remoções, páginas do pull, limpeza com `reset`, limites, exportação e exclusão. O CI roda o mesmo script.

## Configurar o login (uma vez, no painel)

Feito por quem administra o projeto. Os segredos ficam só no Supabase; nada disto vai para o repositório. O endereço de retorno do Supabase, usado nos dois provedores, é:

```text
https://irbyfncttguwcoznfiii.supabase.co/auth/v1/callback
```

### Google

1. Em [console.cloud.google.com/auth](https://console.cloud.google.com/auth), escolha ou crie um projeto (por exemplo, "Prancha").
2. **Branding:** nome "Prancha", e-mail de suporte e logo. Em domínios autorizados, `supabase.co`.
3. **Audience:** tipo "Externo". Publique o app ("Em produção"): em teste, só os e-mails da lista de testadores conseguem entrar.
4. **Data Access:** deixe só `openid`, `.../auth/userinfo.email` e `.../auth/userinfo.profile`. Nada além disso, para o Google não pedir verificação demorada.
5. **Clients › Create client › Web application:** nome "Prancha (Supabase)". Origens JavaScript: nenhuma. URIs de redirecionamento: o endereço de retorno acima. Guarde o Client ID e o Client Secret.
6. **Clients › Create client › Android** (só quando o app Android for assinar o login): pacote `app.prancha` e o SHA-1 do certificado que assina o APK (o de depuração e o de lançamento, um cliente para cada).
7. No Supabase, **Authentication › Sign In / Providers › Google**: ligue, cole o Client Secret e, em Client IDs, os ids separados por vírgula, **o da Web primeiro**. Deixe "Skip nonce check" desligado.

### Discord

1. Em [discord.com/developers/applications](https://discord.com/developers/applications), **New Application**, nome "Prancha".
2. **OAuth2 › Redirects › Add Redirect:** o endereço de retorno acima. **Save Changes**.
3. Copie o Client ID e o Client Secret (em "Client information"; o segredo aparece ao clicar em "Reset Secret").
4. No Supabase, **Authentication › Sign In / Providers › Discord**: ligue, cole os dois e salve.

### Endereços de retorno do app

No Supabase, **Authentication › URL Configuration › Redirect URLs**, adicione exatamente:

```text
http://localhost:3642/auth
app.prancha://auth
```

O primeiro é o do PC (o app escuta só durante o login, como no MyAnimeList, que usa a 3641). O segundo é o do Android, para o Discord. Não mude o Site URL, que é o do painel da telemetria.
