# Arquitetura do Prancha

## Visão geral

```
┌──────────────────────── WebView2 (React + TypeScript) ────────────────────────┐
│ app/        navegação, layout, tema, eventos do catálogo                      │
│ features/   library · reader · onboarding · settings                           │
│ components/ Button, Dialog, Menu, Form, Cover, Toast, Tooltip…                 │
│ domain/     regras puras: progresso, leitor (visões/zoom), atalhos, config     │
│ services/   api.ts (invoke) · events.ts · urls.ts · pdf.ts (PDF.js)            │
└──────────────┬───────────────────────────────┬──────────────────────────────────┘
               │ comandos Tauri (JSON)         │ prancha:// (bytes de capas/páginas/PDF)
┌──────────────▼───────────────────────────────▼──────────────────────────────────┐
│ commands.rs  API para a interface (async; trabalho pesado em spawn_blocking)    │
│ protocol.rs  /cover/<id> · /page/<id>/<i> · /file/<id> (Range)                  │
│ core.rs      estado compartilhado: caminhos, conexão SQLite, serviços           │
│ library.rs   repositório (HQs, progresso, coleções, tags, configurações)        │
│ import.rs    fila de importação e manutenção de capas (thread dedicada)        │
│ formats/     um adaptador por formato + ComicInfo.xml                           │
│ covers.rs    escolha e geração de miniaturas                                    │
│ pages.rs     leitura de páginas + cache de extração (RAR/7z)                    │
│ db.rs        conexão, PRAGMAs e migrações versionadas                           │
└─────────────────────────────────────────────────────────────────────────────────┘
```

A interface nunca recebe acesso a caminhos arbitrários: arquivos são escolhidos pelo diálogo
nativo **no backend** e, depois de importados, são referenciados apenas por `id`. As permissões
do Tauri (`src-tauri/capabilities/default.json`) liberam só o necessário para a janela
(tela cheia, tema, título, fechar) — nenhum plugin de sistema de arquivos é exposto ao JavaScript.
A CSP restringe imagens/conexões a `self`, `ipc` e `prancha.localhost`.

## Decisões técnicas

| Tema | Decisão | Motivo |
|---|---|---|
| RAR | `unrar` 0.5.8 (fonte oficial do UnRAR vendorizada, compilada com MSVC) | Única opção madura para RAR5; licença permite distribuir descompressão. Não há dependência externa para o usuário. |
| 7z | `sevenz-rust2` (Rust puro) | Mantido, sem código nativo. |
| PDF | Validação e contagem com `lopdf::Document::load_metadata`; renderização com PDF.js no WebView | Evita empacotar PDFium; PDF.js lê por `Range`, carregando só o necessário. |
| Capas de PDF | Renderizadas pelo PDF.js em segundo plano (`PdfCoverWorker`), enviadas ao backend, **revalidadas e reencodadas** pelo crate `image` | O backend não confia em bytes da interface. |
| Acesso a páginas | ZIP/TAR/pasta: leitura direta por entrada. RAR/7z: uma passada extrai todas as páginas para `cache/pages/<id>-<impressão>/<índice>` | Arquivos sólidos não têm acesso aleatório barato. Arquivos no cache são nomeados por índice: nenhum nome vindo do arquivo vira caminho (sem *path traversal*). Cache limitado a 2 GB, com descarte dos menos usados. |
| Banco | SQLite (`rusqlite` bundled), WAL, `synchronous=NORMAL`, chaves estrangeiras com `ON DELETE CASCADE` | Consistente após fechamento inesperado (testado com encerramento forçado no E2E). |
| Migrações | Lista ordenada em `db.rs`, versão em `PRAGMA user_version`, cada uma em transação | Simples e verificável; banco mais novo que o app é recusado com mensagem clara. |
| Ordenação natural | `natural.rs`, também registrada como *collation* SQLite `NATSORT` | "Vol 2" antes de "Vol 10" na interface e nas consultas. |
| Busca | Coluna `search_text` sem acentos e minúscula (título, arquivo, série, autores, editora, número, tags) com `LIKE` por termo | Rápida para dezenas de milhares de itens; "acao" encontra "Ação". |
| Lista grande | `@tanstack/react-virtual` na grade e na lista | Só as linhas visíveis são montadas. |
| Comunicação de progresso | Eventos `import:progress`, `library:changed`, `covers:progress`, `notifications:changed` | A interface não faz polling. |

## Identificação de arquivos e duplicatas

- **Chave estável:** `comics.id` (inteiro). O caminho absoluto canônico (`path`, único) é um atributo, não a identidade.
- **Impressão digital (`fingerprint`):**
  - arquivos: BLAKE3 de todos os bytes;
  - pastas: BLAKE3 da lista ordenada `(nome, tamanho)` das páginas + bytes da primeira página.
- **Na importação:**
  1. mesmo caminho já catalogado → "Já está na biblioteca";
  2. mesma impressão de um item cujo arquivo sumiu → **relocalização**: o item existente passa a apontar para o novo caminho, mantendo progresso, favoritos e coleções;
  3. mesma impressão de um item presente → "Duplicata" (padrão) ou importação como novo item (Configurações › Biblioteca);
  4. mesmo nome com conteúdo diferente → item novo (o nome nunca é usado para deduplicar).
- **Arquivos movidos/excluídos:** verificados ao iniciar (em segundo plano), ao abrir detalhes e ao abrir o leitor. O item fica marcado como ausente; os detalhes oferecem "Localizar arquivo…", que compara a impressão digital e pede confirmação se o conteúdo for diferente.
- **Remover** tira o item do catálogo. **Excluir o original** é opcional, explícito e usa a Lixeira do sistema (`trash`), nunca exclusão definitiva.

## Capas

Ordem: `ComicInfo.xml` (`Page Type="FrontCover"`) → imagem nomeada como capa (`cover`, `capa`, `folder`, `front`, `000_cover`…) → primeira das 5 primeiras páginas que decodificar → **capa substituta**, desenhada pela interface com o título, padrão geométrico e o aviso "Capa indisponível", com o motivo registrado. Miniaturas JPEG de 400 px de largura (sem ampliar imagens pequenas; tiras muito altas são recortadas no topo), em `cache/covers/<id>.jpg`. `COVER_PARAMS_VERSION` invalida miniaturas quando os parâmetros mudam; limpar o cache recria todas em segundo plano sem tocar no progresso.

## Progresso de leitura

`reading_progress(comic_id, page_index, page_count, scroll_ratio)`. A interface agrupa gravações (700 ms nos modos paginados, 1 s na rolagem) e força a gravação ao sair do leitor, trocar de HQ, ocultar a janela e no fechamento (`onCloseRequested`). Chegar à última página marca "Concluído"; HQs concluídas recomeçam da página 1 mantendo o estado. Na rolagem vertical, `scroll_ratio` guarda a posição dentro da página.

## Leitura Guiada

Roda inteira no computador, sem OCR, rede ou dependência nova. Fica dentro do leitor: ao sair, a página e o estado de leitura continuam como estavam.

**Detecção (`src-tauri/src/guided/`).** A página é reduzida a 1600 px no lado maior. `panels` encontra os quadros pelas calhas (XY-cut, mais ilhas). `balloons` procura áreas de cor contínua cujas letras aparecem como buracos alinhados, o que vale para qualquer idioma e para papel branco, colorido ou escuro. Dois balões colados são separados no pescoço entre eles; texto solto sobre um fundo vira uma região em volta das letras. `classify` escolhe o tipo pela forma e pela cor. Letras muito maiores que as da página são onomatopeias e ficam de fora, a não ser que estejam num balão de verdade ("RAHHH!").

**Ordem (`order.rs`).** XY-cut recursivo sobre quadros e balões, invertendo os dois em direita→esquerda. A direção só entra na leitura do resultado, então a mesma análise vale para os dois sentidos.

**Persistência (`store.rs`, migração `0002`).** `guided_pages` guarda os quadros e a validade da análise (impressão digital do arquivo e `DETECTOR_VERSION`); `guided_regions` guarda as regiões separadas por `source` (`auto` ou `user`); `guided_corrections` diz quais páginas têm correção. Refazer a análise nunca toca no que o usuário corrigiu. Cada região tem id, página, retângulo normalizado, tipo, ordem, confiança, estado de revisão e o contorno do balão.

**Tela (`GuidedReader.tsx`, `domain/guided.ts`).** A página fica parada, inteira e acima dos controles, sem escurecer. Cada balão é uma cópia recortada no próprio contorno e posicionada sobre o original: `computePop` calcula o tamanho (o suficiente para ler, sem cobrir a página toda) e mantém o balão inteiro na tela. A cópia é montada já no tamanho final, para não perder nitidez, e só `transform`, `opacity` e `filter` animam, por isso entra e volta sem emenda e qualquer gesto pode interromper. Sem contorno conhecido (região desenhada à mão, movida ou de texto solto), o recorte é um retângulo arredondado. Em PDF, o recorte vem de uma imagem desenhada uma vez pelo PDF.js.

**Correção (`RegionEditor.tsx`).** Adicionar, mover, redimensionar, excluir, trocar o tipo e reordenar sobre a própria página. Mover ou redimensionar descarta o contorno, porque a região deixa de coincidir com o balão detectado.

**Limites conhecidos.** Texto branco direto sobre o céu passa despercebido; dois balões unidos por um pescoço largo viram uma região só; o tipo é aproximado (legenda pode virar sussurro); logotipos de capa podem aparecer como "Outro texto"; o contorno guarda só o extremo esquerdo e o direito de cada faixa, então o vão entre dois lóbulos sobrepostos vem junto no recorte. Nenhum mangá real foi testado.

## Atualizações

Usa o `tauri-plugin-updater` e as GitHub Releases, sem servidor próprio. Toda a rede e toda a conferência ficam no backend (`updates.rs`, `commands.rs`): a CSP da interface continua sem endereços externos, e os comandos JS do plugin não são liberados.

**Origem.** O aplicativo lê `releases/latest/download/latest.json`, gerado por `scripts/updater-manifest.mjs` em cada release. O "latest" do GitHub não inclui pré-lançamentos nem rascunhos, e esse endereço não gasta o limite da API. A API só é usada para o histórico nas configurações, no máximo uma vez por hora, salvo pedido da pessoa. Builds de depuração não acessam a rede.

**Conferências antes de instalar.** A versão é comparada por semver e precisa ser estável e maior que a instalada. O pacote precisa estar na pasta da versão anunciada, nas releases configuradas no próprio aplicativo. O plugin escolhe o pacote pelo sistema, pela arquitetura e pelo tipo de instalação (NSIS ou MSI) e confere a assinatura minisign, que também declara a versão (`requireSignedVersion`). Depois vem o SHA-256 do manifesto. O download fica em memória: nada toca a instalação até o instalador abrir. O instalador é o oficial do Tauri, em modo passivo, e reabre o aplicativo. Ele não mexe na pasta de dados.

**Estado (`updates.state`, tabela `settings`).** Sobrevive a um fechamento inesperado. `reconcile`, a cada início, é o único lugar que dá uma atualização por concluída, porque a prova é a versão nova estar rodando: compara a versão em execução com `installedVersion` e abre as novidades. Se `pendingVersion` ficou maior que a versão em execução, o instalador não terminou, e a próxima abertura avisa. As notas ficam em `updates.releases`, copiadas das releases: as da versão baixada são gravadas antes do download, para a janela de novidades funcionar sem internet.

**Tela (`features/updates/`).** `updates-context.tsx` faz uma verificação por abertura, sem segurar a interface, e não abre nada sobre o leitor ou o tutorial. Recusar grava a versão: ela só fica no indicador do menu lateral até surgir outra mais nova. Erros de verificação automática ficam em `updates.log`, na pasta de dados, sem aparecer para a pessoa.

**Limites conhecidos.** Com o MSI, o Windows pede a confirmação depois que o aplicativo já fechou: se a pessoa recusar, ele não reabre sozinho, e o aviso aparece na próxima abertura. Falta de disco durante a escrita do instalador e falta de permissão têm mensagem própria, mas só foram testadas por mapeamento de erro.

## Extensões

Extensões acrescentam fontes de leitura em Explorar. São componentes WebAssembly executados pelo Wasmtime 48, sem WASI: a decisão e as medições estão na [ADR 0001](adr/0001-runtime-de-extensoes.md), e o guia, os formatos e o modelo de segurança em [`docs/extensions/`](extensions/README.md).

```
repositório (prancha-repo.json, índice e pacotes assinados)
   │ extensions/repository.rs   manifesto, chave fixada, índice assinado, catálogo no SQLite
   │ extensions/install.rs      pacote: tamanho, SHA-256, assinatura, fontes iguais às declaradas
   ▼
extensions/runtime.rs           Source Engine: instância por chamada, fuel, epoch, memória, sem WASI
   │ extensions/net.rs          única saída de rede: HTTPS, DNS próprio, hosts autorizados
   ▼
extensions/sources.rs           API de fontes, classificação de conteúdo, obras e capítulos no SQLite, capas
   ▼
extensions/downloads.rs         fila persistente → CBZ → import::process_one → biblioteca e leitor de sempre
```

**Uma biblioteca só.** Um capítulo baixado é um CBZ importado pelo mesmo caminho de qualquer arquivo. `online_chapters.comic_id` liga o capítulo à biblioteca com `ON DELETE SET NULL`: remover o item da biblioteca, a extensão ou o repositório não apaga o resto. A biblioteca e o leitor não dependem de nenhuma tabela nova.

**Leitura sem baixar.** `sources::open_chapter` pede a lista de páginas à fonte e guarda só na memória, em `ChapterPages`. Cada imagem chega pelo protocolo do app, com as mesmas regras de rede e de imagem dos downloads. O progresso fica em `chapter_progress`, ligado ao capítulo. O leitor é o mesmo da biblioteca: `ReaderContent` diz de onde vêm as páginas e como salvar a posição. Os capítulos entram em sequência, numa numeração de páginas única que só cresce no fim: `chapterAt` traduz essa numeração para o capítulo e a página de verdade, e por isso rolar para cima não traz capítulos anteriores ao que foi aberto.

**Plugin.** `plugins.online` é um interruptor da interface para código que já vem no app, e não um pacote à parte. Desligar tira as telas do menu e chama `online_stop`, que pausa os downloads e para o Suwayomi. A migração 0004 deixa ligado para quem já tinha repositório ou extensão.

**Obras na biblioteca.** Uma obra guardada não vira HQ: fica em `online_series`, com `favorite` e `series_collections` (migração 0005), e `series_library` devolve as favoritadas, acompanhadas, em coleção ou com progresso online. `domain/library-items.ts` mistura essa lista com a das HQs no Início, na biblioteca e nas coleções, na ordem que a tela pediu. `reading_of` calcula onde a leitura continua. Uma obra guardada nunca é podada, e `remote_id` é o identificador interno do Suwayomi, por isso a pasta `dados` dele não pode ser apagada enquanto houver obras.

**Estado de leitura e capítulos novos.** Cada capítulo de obra tem quatro estados (Não lido, Lendo, Em andamento, Lido) e, à parte, a marca de novo. O estado que dura fica em `online_chapters` (migração 0007: `first_seen_at`, `acknowledged_at`, `reading_started_at`, `last_read_at`, `completed_at`, `manual_unread`); a sessão do leitor fica em `reading_sessions` e é fechada na abertura do app se uma queda a deixou aberta. `extensions/reading.rs` grava as transições, e `src/domain/reading-lifecycle.ts` decide, no leitor, o que é início efetivo e o que é chegar ao fim. Só obras acompanhadas geram capítulos novos, e a primeira sincronização é o ponto de referência. Detalhes em [ADR 0003](adr/0003-progresso-de-leitura.md).

**Filtro +18.** `content.adult` é falso por padrão, e `library::adult_allowed` é o que todas as consultas perguntam: catálogo, fontes, resultados, obra aberta, instalação e biblioteca. O filtro esconde sem apagar nada, e `adult` em `online_series` fica marcado de vez quando a fonte classifica a obra assim. As fontes do Mihon não classificam obra por obra, então `suwayomi::rating_from` olha os gêneros, e a obra recusada ao abrir continua registrada para não voltar às listas. Como a lista pode vir sem gêneros, `classification` em `online_series` diz o que se sabe: `pending` ninguém consultou, `unknown` a fonte não informa, `known` há classificação. `sources::store_page` consulta os detalhes do que está `pending` antes de responder, com até 4 consultas ao mesmo tempo por fonte, e só `known` e não adulta passa com o filtro ligado (`library::SERIES_SAFE_SQL`). `cover` confere o mesmo antes de ler o cache e de pedir a imagem.

**Sexualização de menores.** `extensions::minors` procura rótulos e combinações no título e nos gêneros, sem depender do filtro. A obra é apagada com a capa e as páginas temporárias, e `blocked_series` guarda só o endereço, para não consultar de novo. `sources::forget_blocked` faz o mesmo no início com o que já estava guardado. Capítulos na biblioteca são arquivos da pessoa e ficam.

**Verificação anti-robô.** `source_verify` abre a janela `verificacao` no endereço do capítulo, da obra ou da fonte, e espera ela fechar. O Prancha só lê os cookies dali (`cookies_for_url`) e fecha a janela ao ver `cf_clearance`. Depois para o Java, grava `cookie_store.xml` e deixa o próximo uso iniciar o servidor. O protocolo `prancha://` recusa tudo que não venha da janela `main`, porque as respostas dele têm `Access-Control-Allow-Origin: *`.

**Rede.** Tudo que sai em nome de um repositório ou de uma extensão passa por `net.rs`. A interface recebe capas e páginas pelo protocolo do app, em `/online-cover/<id>` e `/online-page/<capítulo>/<índice>`, e a CSP continua sem endereços externos.

**Sem polling.** Os downloads avisam por evento (`downloads:changed`), e a fila usa `Condvar`. A sincronização só roda a pedido, com `ETag`. Pesquisas e obras abertas são canceladas quando a tela muda.

**Limites conhecidos.** A compilação de um componente não pode ser interrompida. Um download que falha recomeça do zero. O comportamento de rede das extensões nativas foi provado contra servidores locais e, em HTTPS público, só com a demonstração. As fontes do plugin foram conferidas com sites de verdade uma vez, sem teste permanente.

## Central de notificações

Um histórico que fica no SQLite, separado dos avisos temporários (`Toast`), que continuam para respostas imediatas a uma ação. Uma notificação nunca é dona de nada: ler, deixar de ler ou remover mexe só nela, nunca no capítulo, na obra, no progresso ou na novidade do capítulo.

```
evento (capítulos novos, download, atualização, importação, arquivos ausentes, sincronização)
   │ NewNotification: tipo, referências, chaves de evento
   ▼
notifications::record     transação: deduplica, agrupa, respeita preferências, decide a nativa
   │ notifications:changed { unread, created }
   ▼
NotificationsProvider     contagem global, cache do popup, ações → useNav
   ├─ NotificationBell    sino + badge no menu lateral, popup com as 8 mais recentes
   └─ NotificationsScreen histórico paginado, filtros, seleção múltipla
```

**Banco (migração `0008`).** `notifications` guarda tipo (`kind`), categoria, título, texto, `series_id`/`chapter_id`/`comic_id` (chaves estrangeiras com `ON DELETE SET NULL`), a ação principal, um `payload` JSON com contagem e versão, `group_key`, `created_at`, `updated_at` e `read_at`. `notification_chapters` liga a notificação aos capítulos que ela anuncia, com `ON DELETE CASCADE` dos dois lados. `notification_events` guarda cada chave de evento já anunciada. Índices: recentes (`created_at DESC, id DESC`), por categoria, parcial de não lidas, por obra e parcial de grupo. `id` usa `AUTOINCREMENT`, para o clique numa notificação nativa antiga nunca cair em outra.

**Deduplicação.** Cada evento tem uma chave estável pelo tipo e pelas referências: `chapter:new:<obra>:<id remoto>`, `download:completed:<capítulo>:<pedido>`, `download:failed:<capítulo>:<pedido>:<fim da tentativa>`, `update_available:<versão>`, `update_installed:<versão>`, `update_failed:<versão>`, `import:problems:<tarefa>`, `library:missing:<HQ>:<verificação>`. `record` insere a chave com `INSERT OR IGNORE` na mesma transação da notificação: um evento repetido não grava nada, e a chave fica mesmo depois de a notificação ser removida, marcada como não lida ou de a categoria ser desligada. Eventos diferentes têm chaves diferentes, então nada legítimo é descartado.

**Agrupamento.** Capítulos novos e downloads da mesma obra, e falhas de sincronização, somam na notificação ainda não lida do mesmo grupo criada há pouco (30 minutos; 6 horas para sincronização). O título e o texto são refeitos com a contagem ("One Piece recebeu 5 capítulos novos"), e a ação principal muda de "Abrir capítulo" para "Abrir obra" quando passa de um. Uma notificação lida não recebe novidades escondidas: o próximo evento cria outra.

**Badge.** A contagem é `count(*)` das não lidas visíveis, pelo índice parcial. A interface pede uma vez ao abrir; depois, cada gravação ou alteração devolve a contagem nova, e o evento `notifications:changed` a leva a todas as telas. Notificações de obras que o filtro +18 esconde também não aparecem nem contam.

**Popup e tela.** O popup consulta o banco só quando chegou notificação nova desde a última abertura; leituras e remoções feitas pela interface ajustam as listas abertas sem nova consulta. A tela pagina por cursor (`created_at`, `id`), 30 por vez, e uma notificação nova entra no topo sem perder as páginas carregadas.

**Ações.** O destino nunca é uma rota guardada. `notification_target` lê as referências da própria notificação, confere se a obra, o capítulo ou a HQ ainda existem e se o plugin de leitura online está ligado, e só então a interface navega pelas rotas de sempre. Se falhar, a notificação continua como estava e a pessoa vê o motivo. Nenhuma ação apaga ou altera dados.

**Retenção.** A cada início, `prune` apaga lidas com mais de 90 dias, qualquer uma com mais de um ano e o que passar de 2000. Chaves de evento ficam 400 dias.

**Notificações do sistema.** `tauri-plugin-notification` 2.8 (oficial), chamado só pelo backend: a interface não recebe permissões novas. Só capítulos novos, nova versão disponível e problemas de sincronização (downloads concluídos, para quem ligar), só na criação (somar num grupo não repete o aviso) e só com a janela fora de foco. Obras +18 aparecem sem o nome. Desligar ou silenciar não muda nada na central. Um clique traz a janela para a frente e executa a ação da notificação pelo mesmo caminho do popup.

**Limites por plataforma.** No desktop, o plugin sempre responde que a permissão foi concedida: se o sistema bloquear o Prancha (Windows: Configurações › Notificações; macOS: Ajustes › Notificações), o aviso simplesmente não aparece, e o botão de teste nas configurações serve para conferir. No Windows, o clique só chega enquanto o aviso está na tela, não depois de ele ir para a Central de Ações. No Linux, depende de um servidor de notificações compatível com o D-Bus.

## Limites de segurança (`formats/mod.rs`)

Até 20 000 entradas por arquivo, 150 MB por página, 2 MB para `ComicInfo.xml`, imagens de até 30 000 px e 1 GB de alocação na decodificação. Leituras usam `take(limite + 1)` para não confiar no tamanho declarado.

## Design

Tokens de cor em `src/styles/index.css` (variáveis CSS para claro/escuro, base Primer). Contrastes verificados (WCAG):

| Par | Escuro | Claro |
|---|---|---|
| Texto principal / fundo | 17,4 : 1 | 15,8 : 1 |
| Texto secundário / superfície elevada | 4,95 : 1 | 6,1 : 1 |
| Texto de link (`accent-fg`) / superfície elevada | 6,0 : 1 (`#58A6FF`) | 5,2 : 1 |
| Texto branco / botão primário | 4,6 : 1 (`#1F6FEB`) | 5,2 : 1 |
| Borda de campos / fundo | 4,1 : 1 (`#6E7681`) | 3,5 : 1 (`#818B98`) |

`#2F81F7` (azul pedido) é usado para foco, seleção, ícones e barras de progresso; com texto branco ele dá 3,75 : 1, abaixo de 4,5 : 1, por isso os botões primários usam o tom `#1F6FEB`.
Tipografia IBM Plex Sans/Mono local (`@fontsource`), escala de 12–24 px, espaçamento em múltiplos de 4 px.
A revisão de UX seguiu as diretrizes de design da Apple: revelação progressiva (filtros um nível abaixo), sem texto redundante, feedback só quando a mudança não é visível por si, controles do leitor que nunca cobrem a página.
