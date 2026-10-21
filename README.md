# Prancha

Leitor e organizador local de quadrinhos, mangás e graphic novels para Windows.
Funciona sem conta, sem internet e sem assinatura. Interface em português do Brasil.

- **Formatos testados:** CBZ, CBR (RAR4/RAR5 via UnRAR), CB7, CBT, PDF e pastas com imagens (JPEG, PNG, WebP, GIF, BMP; AVIF exibido no leitor).
- **Biblioteca:** capas automáticas (metadados → imagem de capa → primeira página válida → capa substituta identificada), pesquisa sem acentos, filtros, ordenação, grade/lista, favoritos e coleções.
- **Leitor:** página única, dupla e rolagem vertical; ajustar à página/largura/altura; zoom; direita→esquerda; tela cheia; índice de páginas; atalhos configuráveis; progresso salvo automaticamente.
- **Tutorial** de primeira execução com importação real.

Capturas de tela: [`artifacts/screenshots/`](artifacts/screenshots/).

## Pré-requisitos (desenvolvimento)

| Ferramenta | Versão usada | Observação |
|---|---|---|
| Windows | 10/11 x64 | WebView2 já vem no Windows 11 |
| Node.js | 22.13+ | npm 10+ |
| Rust | stable (1.85+) | via [rustup](https://rustup.rs) |
| Visual Studio Build Tools | 2022 | workload "Desenvolvimento para desktop com C++" + Windows 11 SDK (o UnRAR e o SQLite são compilados em C/C++) |

> **Caminhos longos:** se a pasta do projeto tiver um caminho muito longo, o linker do Windows
> pode falhar (limite de 260 caracteres). Use uma pasta de build curta:
> `set CARGO_TARGET_DIR=C:\Users\<você>\.cache\prancha-target` (PowerShell: `$env:CARGO_TARGET_DIR=...`).

## Instalação das dependências

```bash
npm install
```

## Desenvolvimento

```bash
npm run tauri dev
```

O frontend roda em `http://localhost:1420` com recarga automática; o backend Rust é recompilado ao salvar.
Os dados ficam em `%APPDATA%\app.prancha.desktop`. Para usar outra pasta (testes, modo portátil):
`PRANCHA_DATA_DIR=C:\caminho\dados`.

## Testes

```bash
npm run typecheck      # TypeScript estrito
npm run lint           # ESLint (+ regras do React Compiler)
npm test               # Vitest: regras de domínio e componentes (IPC simulado)
npm run test:rust      # cargo test: formatos, importação, capas, catálogo, progresso
npm run e2e:build      # compila o app de depuração e gera os arquivos de teste
npm run test:e2e       # Playwright no executável real (WebView2 via CDP) + capturas de tela
```

Os arquivos de teste são gerados por `src-tauri/examples/make_fixtures.rs` (desenhos geométricos
próprios; nenhum quadrinho comercial no repositório). Resultados: [`docs/TEST_REPORT.md`](docs/TEST_REPORT.md).

## Compilação e instalador

```bash
npm run tauri build
```

Gera, em `<target>/release/bundle/`:

- `nsis/Prancha_0.1.0_x64-setup.exe` — instalador por usuário (não exige administrador), em português;
- `msi/Prancha_0.1.0_x64_pt-BR.msi` — pacote MSI.

O instalador baixa o WebView2 se ele não estiver presente. Os binários não são assinados
digitalmente; o Windows SmartScreen pode pedir confirmação na primeira execução.

## Renomear o aplicativo

O nome está centralizado em dois pontos: `src/app/config.ts` (`APP_NAME`, usado em toda a interface)
e `src-tauri/tauri.conf.json` (`productName` e `app.windows[0].title`, usados no executável e no instalador).

## Licenças

Código do Prancha: MIT. A lista completa de componentes de terceiros fica em
Configurações › Geral › Licenças de terceiros (gerada por `npm run licenses`).
A leitura de CBR usa o código-fonte do **UnRAR**, de Alexander Roshal, sem modificações, sob esta condição:

> UnRAR source code may be used in any software to handle RAR archives without limitations free of charge,
> but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR compression algorithm,
> which is proprietary. Distribution of modified UnRAR source code in separate form or as a part of other
> software is permitted, provided that full text of this paragraph, starting from "UnRAR source code" words,
> is included in license, or in documentation if license is not available, and in source code comments of
> resulting package.

Documentação técnica: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) · plano: [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md).
