# Visão geral do projeto (métricas públicas do GitHub)

A página <https://prancha-beta.vercel.app/visao-geral> mostra como o projeto evolui: versões publicadas, downloads dos instaladores e as verificações do GitHub Actions. No app, ela abre em **Configurações › Geral › Visão geral do projeto**.

É uma frente separada da telemetria. A página usa só dados públicos do GitHub, funciona sem consentimento e não lê, não guarda e não cruza nenhum dado de quem usa o Prancha.

## Como funciona

```text
navegador ──► /visao-geral (HTML estático, site/visao-geral.html)
          └─► /api/github-metrics ──(CDN da Vercel, 15 min)──► função site/api/github-metrics.mjs
                                                                 └─(memória, 15 min)──► API do GitHub
```

| Peça | Arquivo | Papel |
|---|---|---|
| Página | `site/visao-geral.html` | Cartões, gráficos em SVG sem biblioteca, tabelas, estados de carregamento, vazio e erro. Atualiza sozinha a cada 15 minutos enquanto está visível |
| Função | `site/api/github-metrics.mjs` | Única rota: `GET /api/github-metrics`. Aceita `GET` e `HEAD`, e responde 405 para o resto |
| Coleta e resumo | `site/lib/github-metrics.mjs` | Consulta o GitHub, resume e guarda o cache em memória |
| App | `open_project_overview` (`commands.rs`) | Só abre o endereço fixo no navegador. O app não consulta o GitHub para isso e não ganhou nenhuma liberação de rede nova |

### O que é consultado no GitHub

| Chamada | Quantas | Para quê |
|---|---|---|
| `GET /repos/CAKeysama/prancha/releases?per_page=100` | 1 | Versões, datas, links e downloads |
| `GET /repos/CAKeysama/prancha/actions/runs?created=>=<30 dias>` | 1 a 3 páginas | Execuções dos workflows nos últimos 30 dias |
| `GET /repos/CAKeysama/prancha/actions/runs/{id}/jobs` | até 20, só com token | Separar a espera por máquina da execução |

Sem token, são de 2 a 4 chamadas a cada 15 minutos.

### Resposta de `/api/github-metrics`

```jsonc
{
  "repository": "https://github.com/CAKeysama/prancha",
  "generatedAt": "2026-11-09T13:55:00.000Z",
  "stale": false,                     // true: o GitHub falhou e este é o último resumo bom
  "releases": {
    "latest": { "version": "0.8.0", "publishedAt": "…", "url": "…", "downloads": 3, "assets": [ … ] },
    "count": 15,
    "downloads": { "total": 65, "byKind": { "exe": 41, "msi": 24 } },
    "releases": [ { "version": "0.8.0", "downloads": 3, "share": 0.05, "assets": [ { "kind": "exe", "url": "…" } ] } ]
  },
  "ci": {
    "windowDays": 30,
    "summary": { "runs": 143, "success": 135, "failure": 4, "other": 4, "successRate": 0.97, "avgDurationSeconds": 382, "medianDurationSeconds": 256, "sampled": 20, "avgQueueSeconds": 5, "avgExecutionSeconds": 143 },
    "daily": [ { "date": "2026-11-09", "success": 80, "failure": 1, "avgDurationSeconds": 300 } ],
    "workflows": [ { "name": "Verificações", "path": ".github/workflows/ci.yml", "…": "mesmos campos de summary, mais daily e recent" } ],
    "recent": [ { "workflow": "Verificações", "outcome": "success", "createdAt": "…", "durationSeconds": 220, "queueSeconds": 4, "executionSeconds": 210, "url": "…" } ]
  }
}
```

Se não houver nenhum resumo, a resposta é `503` com `{ "error": "…", "retryAt": "…" | null }`.

## Definições

- **Downloads:** soma do `download_count` dos instaladores (`*-setup.exe` e `*.msi`) das releases publicadas. Rascunhos, pré-lançamentos, assinaturas `.sig`, `latest.json` e `SHA256SUMS.txt` ficam de fora. A conta inclui as atualizações automáticas, porque o atualizador baixa o mesmo instalador, e repetições do mesmo computador. **Não é o número de usuários nem de instalações.**
- **Evolução dos downloads:** o GitHub só informa o total atual de cada arquivo, sem histórico por data. Por isso a página mostra os downloads por versão, em ordem de publicação, e a fatia de cada versão no total. Uma série diária exigiria guardar fotos periódicas, o que não foi feito para não criar outra infraestrutura.
- **Taxa de aprovação:** aprovadas ÷ (aprovadas + com falha). Contam como falha `failure`, `timed_out` e `startup_failure`. Canceladas, puladas e neutras não entram no denominador, e execuções em andamento também não.
- **Duração total:** de `run_started_at` a `updated_at`, como o GitHub mostra. Inclui as esperas entre jobs.
- **Espera por máquina:** do disparo (`created_at`) até o primeiro job começar. **Execução:** do início do primeiro job ao fim do último. As duas vêm do detalhe dos jobs, só nas 20 execuções concluídas mais recentes, e só com token.
- **Cartões "Tempo médio" e "Aprovadas":** usam o workflow `Verificações` (`ci.yml`), que é o CI de cada mudança. O seletor da página mostra cada workflow separado.

## Cache e limites

1. **CDN da Vercel:** uma resposta boa sai com `Cache-Control: public, max-age=300, s-maxage=900, stale-while-revalidate=86400`. Abrir a página não chega à função enquanto a cópia da CDN vale, e depois disso a CDN entrega a cópia antiga enquanto busca a nova.
   O `site/vercel.json` põe `no-store` nas outras rotas de `/api` (as da telemetria) e deixa `github-metrics` de fora dessa regra, para valer o cabeçalho da função.
2. **Memória da função:** guarda o último resumo por 15 minutos e junta consultas simultâneas numa só.
3. **Falhas:** se o GitHub falhar, a função devolve o último resumo bom, de até 24 horas, com `stale: true` e cache de 60 s. Depois de uma falha comum, só tenta de novo após 1 minuto.
4. **Limite de requisições:** com `403` e `x-ratelimit-remaining: 0`, ou com `429` ou `retry-after`, a função não chama o GitHub até `x-ratelimit-reset` ou `retry-after`.
5. **Token recusado (401):** a função continua pela API pública, sem o detalhe por job, e registra um aviso no log, sem o token.
6. Cada chamada ao GitHub tem tempo máximo de 8 segundos.

## Configuração na Vercel

| Variável | Onde | Obrigatória | Valor |
|---|---|---|---|
| `GITHUB_TOKEN` | Vercel › projeto `prancha` › Settings › Environment Variables, ambiente **Production** (e Preview, se quiser testar) | Não, mas recomendada | Um *fine-grained token* do GitHub com acesso só a `CAKeysama/prancha` e **Actions: Read-only** (Metadata: Read-only vem junto) |

Sem o token, a página funciona, mas a API pública do GitHub permite 60 chamadas por hora **por IP**, e as funções da Vercel dividem IPs com outros projetos. Por isso o limite pode acabar por causa de terceiros. Também ficam de fora a espera e a execução separadas. Com o token, o limite passa a 5.000 por hora.

O token nunca vai para o navegador nem para o app. Ele só aparece no cabeçalho `Authorization` das chamadas da função ao GitHub, e os testes conferem que ele não sai na resposta. Nunca coloque o token no código, em `vercel.json` ou em variável com prefixo `VITE_`.

**Como conferir:** depois do deploy, abra `https://prancha-beta.vercel.app/api/github-metrics`. Com o token, `ci.summary.sampled` é maior que zero.

## Publicação

A pasta `site/` continua sendo publicada pelo job "Site oficial na Vercel" do `pages.yml`, com `vercel deploy --prod`. A Vercel reconhece `site/api/*.mjs` como função Node.js sem nenhuma configuração extra. Como todo merge na `main` publica uma versão e o site, a página só entra no ar com o merge.

## Testes

`tests/unit/github-metrics.test.ts`, com o GitHub simulado e sem rede:

- releases: rascunhos e pré-lançamentos fora, só instaladores contam, ordem e fatias certas, zeros sem versão;
- CI: espera separada da execução, jobs pulados ignorados, taxa de aprovação sem as canceladas, médias, um dia por linha, médias vazias sem execuções;
- token: só no cabeçalho `Authorization`, nunca na URL nem na resposta; sem token, nenhum pedido de jobs; com token, no máximo 20; token recusado volta à API pública;
- limites: `403` com o limite esgotado, `429` com `retry-after`, `500` e falha de rede;
- cache: uma consulta por período, consultas simultâneas juntadas, resumo antigo marcado como `stale`, espera após falha, nenhuma chamada até o limite passar, nada mais velho que 24 horas;
- função: cabeçalhos de cache, `503` sem dados, `405` para escrita.

`tests/unit/app.test.tsx` confere que o botão das Configurações só chama `open_project_overview`.
