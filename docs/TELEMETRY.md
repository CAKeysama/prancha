# Telemetria

Estatísticas de uso voluntárias. Ficam **desligadas** até a pessoa aceitar e servem para saber quais versões estão em uso, quais fontes são mais usadas ou falham e onde o Prancha trava ou fica lento. O aviso para quem usa o app está em [PRIVACIDADE.md](PRIVACIDADE.md).

A telemetria é independente do resto do app: sem consentimento, sem rede, sem servidor ou com o Supabase fora do ar, o Prancha funciona igual. Ela também é separada das métricas públicas do projeto (releases, downloads e CI do GitHub): os dois conjuntos de dados não se cruzam.

## Visão geral

```
App (Rust, telemetry.rs)                       Vercel (projeto prancha, pasta site/)              Supabase (projeto prancha)
 contadores em memória, por dia                 /api/telemetry/ingest  ── service_role ──►        public.telemetry_ingest(lote)
 só com consentimento                           /api/telemetry/forget  ── service_role ──►        public.telemetry_forget(id)
 envio a cada 10 min, em segundo plano  ──►     /api/telemetry/report  ── token do admin ─►       public.telemetry_report(...)
                                                /api/telemetry/config  (URL + chave pública)       schema telemetry (fora da API, RLS)
                                                /admin  (painel, login pelo Supabase Auth)  ◄──
```

- **App.** `src-tauri/src/telemetry.rs` soma contadores em memória e envia lotes pela rede do backend (a CSP da interface continua sem endereços externos). A interface só repassa falhas e tempos por `telemetry_record`, e o backend confere tudo de novo.
- **Vercel.** Funções em `site/api/telemetry/`, no mesmo projeto do site oficial. Elas validam cada lote (`site/api/_telemetry.mjs`) antes de chegar ao banco e nunca guardam IP, cabeçalhos ou corpo.
- **Supabase.** Um schema `telemetry` que a API não expõe, com RLS ligado e sem políticas. Só três funções `security definer` entram nele, cada uma liberada para um papel.
- **Painel.** `https://prancha-beta.vercel.app/admin`, com login por link mágico do Supabase Auth. Quem decide se a pessoa é administradora é o banco.

## Consentimento

| Situação | O que acontece |
|---|---|
| Primeira abertura, tutorial em andamento | Nada é registrado. Nenhum convite. |
| Depois do tutorial, fora do leitor | Um convite único, **Ajudar a melhorar o Prancha?**, com o que é e o que não é coletado. **Agora não** grava a recusa; fechar no X não decide nada e o convite volta na próxima abertura. |
| Configurações › Privacidade | Estado atual (Ligada ou Desligada, com a data), interruptor, o aviso completo, o identificador, **Gerar outro** e **Apagar dados**. |
| Aceitar | Cria um identificador aleatório (UUID v4) e começa a contar. O primeiro envio sai alguns segundos depois. |
| Desligar | Para na hora: a interface para de repassar, o backend descarta a fila em memória e o que ficou guardado do último fechamento, cancela um envio em andamento e apaga o identificador do computador. Não volta a ligar sozinho. |
| Apagar dados | Desliga como acima e pede ao servidor que apague tudo do identificador (`/api/telemetry/forget`). Sem resposta, o pedido fica guardado e o botão vira **Tentar de novo**. |
| Gerar outro identificador | Troca o UUID e descarta a fila, para o identificador antigo não ficar ligado ao novo. |

Builds de depuração não enviam nada (o convite nem aparece). Para testar o envio localmente, rode um build de depuração com `PRANCHA_TELEMETRY_URL=http://127.0.0.1:porta/api/telemetry`.

As chaves `telemetry.*` da tabela `settings` só mudam pelos comandos da telemetria: `set_setting` as recusa.

## Eventos

Tudo é agregado no app antes de sair: um lote leva somas por dia, nunca eventos individuais.

| Tipo | Campos | Origem |
|---|---|---|
| `session` | `day`, `launches`, `uncleanExits` | Abertura do app. A saída inesperada é a marca `telemetry.running` que o fechamento normal apaga: se ela ainda estiver lá na abertura seguinte, a anterior não fechou direito. |
| `source` | `day`, `source`, `queries`, `opens`, `chapters`, `errors` | `source_popular`, `source_latest`, `source_search` (consultas), `series_open` (obras), `chapter_open` (capítulos). `source` é `extensão/fonte`, os identificadores técnicos do repositório. Cancelar uma busca ao trocar de tela não conta como erro. |
| `error` | `day`, `hour`, `category`, `component`, `code`, `httpStatus`, `count` | Falhas de fontes (código do `AppError`), saídas inesperadas (`crash/app/unclean_exit`) e erros não tratados da interface (`interface/window/uncaught_error` e `unhandled_rejection`). |
| `perf` | `day`, `metric`, `count`, `totalMs`, `maxMs` | `startup` (do início do processo até a primeira tela), `explore_results`, `series_open`, `chapter_open`. As métricas `library_load` e `reader_first_page` já são aceitas pelo servidor, mas o app ainda não as mede. |

Cada lote ainda leva `schema: 1`, `batchId` (UUID para o servidor descartar repetidos), `installId` e `app` (`version` sem sufixo de pré-lançamento, `platform`, `arch`, `channel`: `nsis`, `msi`, `dev` ou `unknown`).

A versão do sistema operacional não é enviada (`app.os` fica vazio): o Prancha não tem hoje uma forma confiável de lê-la sem uma dependência nova.

### Sanitização

- **Lista fechada.** Categorias, plataformas, canais e métricas só de uma lista. Campos desconhecidos recusam o evento inteiro, no app (`telemetry.rs`), na Vercel (`_telemetry.mjs`) e no banco (`telemetry_ingest`).
- **Identificadores técnicos.** `component` e `code` aceitam só `a-z 0-9 . _ : -`, começando por letra ou dígito, sem trechos com mais de 32 caracteres seguidos (que seriam mais provavelmente um token ou hash). Um valor que não passa é **descartado**, nunca "limpo": assim nenhum pedaço de um caminho, e-mail ou mensagem sai por engano.
- **Sem texto livre.** Nenhuma mensagem de erro, título, busca, URL, nome de arquivo ou caminho é enviado.
- **Limites.** No app: até 400 combinações em memória, até 200 eventos por lote, até 4 lotes esperando nova tentativa. Na Vercel: corpo de até 64 KB, só `application/json`. No banco: até 12 lotes por hora por instalação, até 600 lotes por minuto no total, datas de até 7 dias atrás, contagens de até 100 000.

## Envio

- A primeira tentativa sai 90 segundos depois de abrir; depois, a cada 10 minutos. Aceitar pede um envio na hora.
- Falha de rede, 429 ou 5xx: o lote fica guardado com o mesmo `batchId` e a espera dobra, até 6 horas. O servidor ignora o lote repetido se ele já tinha chegado.
- 400, 413 e 415: o lote é descartado, porque tentar de novo não muda o resultado.
- Fechamento normal: o que não foi enviado vai para `telemetry.pending`, no disco, sem rede. Uma queda perde o que estava em memória.
- Nada disso roda na thread da interface ou bloqueia a abertura, a biblioteca ou a leitura.

## Banco (Supabase)

Migração: [`supabase/migrations/20261109140000_telemetry.sql`](../supabase/migrations/20261109140000_telemetry.sql). Projeto `prancha` (ref `irbyfncttguwcoznfiii`, região sa-east-1).

| Tabela | Chave | Conteúdo |
|---|---|---|
| `telemetry.admins` | `user_id` → `auth.users` | Quem pode ver o painel. |
| `telemetry.batches` | `batch_id` | Lotes recebidos: repetição e frequência. Índices por instalação e por data. |
| `telemetry.install_days` | `(install_id, day)` | Versão, plataforma, arquitetura, canal, aberturas e saídas inesperadas de cada instalação em cada dia. |
| `telemetry.source_usage` | `(install_id, day, source)` | Consultas, obras, capítulos e erros por fonte. |
| `telemetry.errors` | `(install_id, day, hour, app_version, category, component, code, http_status)` | Ocorrências por hora. |
| `telemetry.performance` | `(install_id, day, app_version, metric)` | Amostras, soma e máximo em ms. |

**Retenção.** 180 dias para os dados e 30 para os lotes. `telemetry.prune()` roda todo dia às 03:17 UTC pelo `pg_cron` (a migração agenda quando a extensão existe) e, além disso, em 1% das gravações.

### Segurança e RLS

- `anon` e `authenticated` não têm `usage` no schema `telemetry` nem permissão em nenhuma tabela. RLS está ligado em todas, **sem políticas**: mesmo que alguém conceda acesso por engano, nenhuma linha é lida ou gravada por esses papéis.
- `public.telemetry_ingest(jsonb)` e `public.telemetry_forget(uuid)`: só `service_role`, usada pelas funções da Vercel.
- `public.telemetry_report(de, até, versão, categoria)` e `public.telemetry_is_admin()`: só `authenticated`, e o relatório confere `auth.uid()` em `telemetry.admins` antes de qualquer consulta (erro `42501` para quem não está). Ele devolve somente totais: nenhuma linha leva `install_id`.
- As funções usam `security definer` com `search_path = ''` e nomes qualificados.
- Uma instalação não consegue ler nada, nem os próprios dados: o app só grava.

Os testes do banco ([`supabase/tests/telemetry.test.sql`](../supabase/tests/telemetry.test.sql)) sobem um Postgres vazio, criam os papéis do Supabase e conferem permissões, validação, repetição, frequência, painel, exclusão e retenção:

```bash
bash supabase/tests/run.sh
```

## Endpoints (Vercel)

| Endereço | Método | Autorização | Resposta |
|---|---|---|---|
| `/api/telemetry/ingest` | POST JSON | nenhuma (o lote é validado) | `200 {accepted, duplicate}`, `400` inválido, `413` grande demais, `415` tipo errado, `429` frequência (com `Retry-After: 3600`), `502` Supabase indisponível, `503` sem configuração |
| `/api/telemetry/forget` | POST `{installId}` | quem tem o identificador | `200 {removed}` |
| `/api/telemetry/report` | GET `?from&to&version&category` | `Authorization: Bearer <token do Supabase Auth>` | totais agregados; `401` sem login ou sessão vencida; `403` não é administrador |
| `/api/telemetry/config` | GET | nenhuma | `{supabaseUrl, anonKey}` (chave pública) para o login do painel |

Todas respondem `Cache-Control: no-store`. O `site/vercel.json` também manda `no-store` e `noindex` para `/api/*` e uma CSP restrita para `/admin`.

Os endpoints entram no ar junto com o site: o `pages.yml` publica a pasta `site/` na Vercel quando algo nela muda na `main`.

## Variáveis de ambiente (Vercel, projeto `prancha`)

| Nome | Tipo | Valor |
|---|---|---|
| `SUPABASE_URL` | Simples | `https://irbyfncttguwcoznfiii.supabase.co` |
| `SUPABASE_ANON_KEY` | Simples | Chave pública (`publishable` ou `anon`) do projeto. Pode aparecer no navegador. |
| `SUPABASE_SERVICE_ROLE_KEY` | **Sensível** | Chave secreta (`secret` ou `service_role`). Só nas funções; nunca no app, no site estático ou no git. |

Nomes alternativos aceitos: `SUPABASE_PUBLISHABLE_KEY` e `SUPABASE_SECRET_KEY`. Sem as variáveis, os endpoints respondem `503` e o painel explica o que falta; o app trata como servidor indisponível e tenta mais tarde.

## Painel administrativo

`https://prancha-beta.vercel.app/admin` (arquivos em `site/admin/`, sem dependências).

- **Login:** link mágico do Supabase Auth com `create_user: false`, então o login não cria contas. A resposta é a mesma com ou sem conta, para não revelar quem é administrador. Os tokens ficam no `sessionStorage` da aba e são renovados quando vencem.
- **Autorização:** a conta precisa existir em **Authentication › Users** e estar em `telemetry.admins`. Para incluir alguém (no SQL Editor do Supabase):

  ```sql
  insert into telemetry.admins (user_id) select id from auth.users where email = 'pessoa@exemplo.com';
  ```

- **Seções:** Visão geral (instalações ativas com telemetria, aberturas, falhas, falhas por abertura, saídas inesperadas, eventos, séries por dia, versões, plataformas, fontes, tempos), Versões (instalações por versão, percentual, versões antigas, adoção por dia, canais), Fontes (ranking com consultas, obras, capítulos, instalações, dias com uso e taxa de erro) e Falhas (por versão, plataforma, componente e categoria, mais recorrentes, por dia).
- **Filtros:** período (7, 30, 90 ou 180 dias), versão e categoria de falha. Atualiza sozinho a cada 5 minutos com a aba à vista.
- **Definições:** "instalações ativas" são as que aceitaram a telemetria e abriram o app no período, não o total de usuários do Prancha. "Falhas por abertura" é falhas ÷ aberturas no período; "saídas inesperadas" é aberturas cuja anterior não fechou direito ÷ aberturas. Sem dados, o painel mostra um estado vazio e nenhuma estimativa.
- Todo gráfico tem a tabela equivalente em **Ver como tabela**.

## Limites da anonimização

- O identificador é aleatório e não deriva de nada do computador ou da pessoa, mas é o mesmo em todos os envios até ser trocado ou apagado. Os dados são, portanto, **pseudônimos**: dá para saber que a mesma instalação abriu o app em vários dias, com quais fontes e falhas.
- O servidor recebe a conexão e, com ela, o endereço IP, como qualquer site. As funções não o gravam nem o repassam ao Supabase, mas a Vercel registra requisições nos logs da plataforma pelo prazo dela.
- Uma instalação com uma combinação rara (por exemplo, a única em Linux usando uma fonte pouco conhecida) pode ser reconhecível por quem já sabe disso por outro meio. O painel mostra só totais, e o acesso é restrito aos administradores.
- Identificadores de fontes vêm dos repositórios. Uma extensão particular, num repositório próprio, aparece pelo identificador que o autor escolheu.

## Testes

| O que | Onde | Como rodar |
|---|---|---|
| Consentimento, revogação, envio em andamento cancelado, repetição com o mesmo lote, saída inesperada, sanitização, limite de memória, exclusão | `src-tauri/src/telemetry.rs` (`mod tests`) | `cargo test --manifest-path src-tauri/Cargo.toml --lib telemetry` |
| Tela de Privacidade, convite, revogação, cliente de falhas da interface, backend indisponível | `tests/unit/telemetry.test.tsx` | `npx vitest run tests/unit/telemetry.test.tsx` |
| Validação, limites, chaves que não vazam, mapeamento de erros, autorização do relatório | `tests/unit/telemetry-api.test.ts` | `npx vitest run tests/unit/telemetry-api.test.ts` |
| Permissões, RLS, validação no banco, frequência, painel só para admin, exclusão, retenção | `supabase/tests/telemetry.test.sql` | `bash supabase/tests/run.sh` |

O CI roda os três primeiros grupos e o teste do banco no job de interface (Ubuntu já traz o Postgres).
