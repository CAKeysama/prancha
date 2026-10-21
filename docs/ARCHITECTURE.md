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
