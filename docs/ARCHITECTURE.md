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
| Comunicação de progresso | Eventos `import:progress`, `library:changed`, `covers:progress` | A interface não faz polling. |

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
