# Prancha — Plano de implementação

Documento vivo. O estado de cada etapa é atualizado conforme o trabalho avança.
Legenda: ✅ concluída · 🟡 parcial · ⛔ bloqueada · ⬜ pendente

## 0. Ambiente inspecionado (2026-10-20)

| Item | Situação encontrada | Ação |
|---|---|---|
| SO | Windows 11 Pro 10.0.26200 | — |
| Node / npm | v22.23.2 / 11.5.2 | usado como está |
| Rust | ausente | instalado via `rustup` oficial (stable 1.99.1, perfil minimal, escopo do usuário) |
| MSVC + Windows SDK | VS Community 2022 sem workload C++ | adicionados `VC.Tools.x86.x64` + Windows 11 SDK (autorizado pelo usuário) |
| WebView2 | 155.0.4283.62 | presente |
| Git | 2.50 | repositório novo |
| Projeto existente | diretório vazio | projeto criado do zero |

## 1. Decisões de stack (versões verificadas no registro em 2026-10-20)

| Camada | Escolha | Motivo |
|---|---|---|
| Desktop | Tauri 2.12.x (crate `tauri` 2.12, CLI 2.12) | stack pedida; 3.x ainda é alpha |
| Frontend | React 19.3, Vite 8, TypeScript 6.0 (estrito), Tailwind 4.3 | TS 7 ainda não é suportado pelo `typescript-eslint` (peer `<6.1`) |
| Ícones | `lucide-react` | consistente, tree-shakeable, ISC |
| Fontes | `@fontsource/ibm-plex-sans`/`-mono` (OFL) | fontes locais, sem rede |
| Virtualização | `@tanstack/react-virtual` | biblioteca extensa sem travar |
| PDF | `pdfjs-dist` 6.x no frontend + `lopdf::Document::load_metadata` no backend | PDF.js renderiza; lopdf valida e conta páginas na importação sem renderizar |
| Banco | `rusqlite` 0.40 com `bundled` | SQLite embutido, sem dependência do sistema |
| CBZ | `zip` 9 (deflate) | puro Rust, acesso aleatório |
| CBR | `unrar` 0.5.8 (fonte UnRAR oficial vendorizada, compilada com MSVC) | única opção madura; licença UnRAR permite distribuir descompressão |
| CB7 | `sevenz-rust2` 0.24 | puro Rust, Apache-2.0, mantido |
| CBT | `tar` 0.4 | puro Rust |
| Imagens | `image` 0.25 (jpeg, png, webp, gif, bmp) | miniaturas no backend |
| Hash | `blake3` | impressão digital de duplicatas rápida |
| Lixeira | `trash` 5 | "excluir original" vai para a Lixeira, nunca apaga definitivamente |
| Testes | `cargo test`, Vitest + Testing Library, Playwright via CDP no app real (WebView2) | E2E no aplicativo de verdade, não num mock |

## 2. Arquitetura (resumo — detalhes em `ARCHITECTURE.md`)

- **Rust (`src-tauri/src`)**: `db` (conexão, migrações versionadas por `PRAGMA user_version`), `library` (repositório de HQs, coleções, tags, progresso, configurações), `formats` (um adaptador por formato + ordenação natural + ComicInfo.xml), `covers` (miniaturas e cache), `import` (fila em thread dedicada com eventos de progresso), `pages` (servir páginas; cache de extração para RAR/7z/TAR), `protocol` (esquema `prancha://` que só serve itens do catálogo por id), `commands` (API Tauri).
- **Frontend (`src`)**: `app` (layout, navegação, provedores), `features/*` (library, reader, onboarding, settings), `components` (UI compartilhada), `services` (chamadas `invoke`/eventos/URLs), `domain` (tipos e regras puras testáveis).

## 3. Etapas

| # | Etapa | Depende de | Critério de conclusão | Estado |
|---|---|---|---|---|
| 1 | Ambiente: Rust + MSVC | — | `cargo build` de crate com código C compila | ✅ (UnRAR exige `advapi32`, adicionado em `build.rs`) |
| 2 | Scaffold Tauri 2 + React + Vite + Tailwind + lint/typecheck | 1 | `npm run build` e `cargo check` passam | ✅ |
| 3 | Banco + migrações + repositórios | 2 | testes Rust de repositório passam | ✅ |
| 4 | Adaptadores de formato + ordenação natural + ComicInfo | 2 | testes com arquivos gerados (CBZ, CBR, CB7, CBT, pasta, PDF) passam | ✅ |
| 5 | Capas (metadados → nome de capa → primeira página → substituta) | 4 | testes de capa real e substituta passam | ✅ |
| 6 | Importação em fila (duplicatas, corrompidos, permissões, pastas vazias) | 3,4,5 | testes de importação passam | ✅ |
| 7 | Protocolo de páginas + cache de extração | 4 | leitor carrega páginas de todos os formatos | ✅ (deadlock encontrado e corrigido) |
| 8 | Shell da UI, temas, tokens, tipografia | 2 | temas claro/escuro/sistema persistem sem flash | ✅ (ver limitação 3) |
| 9 | Início, Biblioteca (grade/lista, busca, filtros, ordenação), Detalhes, Favoritos, Recentes, Coleções | 3,8 | fluxos exercitados no app real | ✅ |
| 10 | Leitor (página única/dupla/vertical, ajustes, zoom, RTL, tela cheia, atalhos, progresso) | 7,8 | progresso sobrevive a reinício | ✅ |
| 11 | Tutorial de primeira execução | 9,10 | E2E: exibir, importar de verdade, pular, persistir, reabrir | ✅ |
| 12 | Configurações (aparência, leitura, biblioteca, armazenamento, geral, licenças) | 8 | todas as opções exibidas funcionam | ✅ (pastas monitoradas não implementadas e não exibidas) |
| 13 | Testes unitários/integração/E2E + relatório | todos | `docs/TEST_REPORT.md` com resultados reais | ✅ |
| 14 | Validação visual com capturas | 9–12 | `artifacts/screenshots/` revisado | ✅ (inclui revisão pelas diretrizes da Apple) |
| 15 | Instalador Windows (NSIS/MSI) + README/ARCHITECTURE | todos | instalador gerado e verificado | ✅ NSIS instalado/executado/desinstalado em modo silencioso; MSI gerado, não instalado |

## 4. Riscos

| Risco | Mitigação |
|---|---|
| Não existe compressor RAR livre para gerar testes | gerador mínimo de RAR5 "stored" nos testes, lido pela biblioteca UnRAR real; limitação documentada |
| RAR/7z sólidos não permitem acesso aleatório barato | extração única por HQ para cache interno do app (arquivos nomeados por índice, sem caminhos do arquivo → sem path traversal), com limite de tamanho e limpeza |
| PDFs grandes | `load_metadata` só lê a estrutura; PDF.js usa requisições de intervalo (`Range`) no protocolo |
| Arquivos maliciosos (zip bomb, entradas gigantes) | limites por entrada, por arquivo e de dimensão de imagem; leitura limitada com `take()` |
| Diálogo nativo não é automatizável no E2E | em builds de depuração, `PRANCHA_E2E_PICK` substitui apenas a escolha do caminho; a importação continua real |
| Fechamento inesperado | SQLite WAL + transações; progresso gravado com debounce e no fechamento |

## 5. Limitações conhecidas

1. EPUB e outros formatos de livro digital não são suportados (são relatados como "Formato não suportado").
2. Pastas monitoradas não foram implementadas; a opção não aparece nas configurações.
3. Na primeira pintura, a janela nativa usa fundo escuro; quem escolheu o tema claro pode ver um instante de fundo escuro antes do conteúdo (o conteúdo em si já nasce no tema certo).
4. Binários e instaladores não são assinados digitalmente.
5. Ver também "Não verificado / limitações" em `TEST_REPORT.md`.
