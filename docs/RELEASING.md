# Como publicar uma versão

## Numeração

O Prancha segue o [versionamento semântico](https://semver.org/lang/pt-BR/), no formato `vMAJOR.MINOR.PATCH`:

| Parte | Quando sobe | Exemplo |
|---|---|---|
| `PATCH` | Correções e pequenos ajustes, sem mudança incompatível | índice de páginas que perdia a rolagem |
| `MINOR` | Funcionalidades novas e compatíveis | um novo modo de leitura |
| `MAJOR` | Mudanças incompatíveis ou grandes alterações de comportamento | uma biblioteca que não abre mais na versão antiga |

Até a 1.0.0, mudanças maiores ainda podem sair em `PATCH` ou `MINOR`, como as versões 0.1.x já fizeram.

## Passo a passo

1. **Suba a versão** nestes arquivos, com o mesmo número: `package.json`, `package-lock.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` e `src-tauri/Cargo.lock`.
2. **Escreva as notas** no topo do `CHANGELOG.md`, no formato `## [X.Y.Z] - AAAA-MM-DD`, com um resumo e as seções `Novidades`, `Melhorias`, `Correções` e `Observações` (só as que tiverem conteúdo). Cada item ocupa uma linha.
3. **Rode localmente** o que precisa de páginas reais e da janela do app:

   ```bash
   npm run e2e:build
   npm run test:e2e
   cargo test --release --manifest-path src-tauri/Cargo.toml --test guided -- --test-threads=1
   npm run test:update -- --build
   ```

4. **Faça o commit na `main`, crie a tag e envie:**

   ```bash
   git tag -a vX.Y.Z -m "Prancha vX.Y.Z"
   git push origin main vX.Y.Z
   ```

5. **Atualize o site oficial** quando a release estiver publicada:

   ```bash
   npm run site:build
   ```

   Depois, envie o `site/index.html` alterado num commit e publique a pasta `site/` na Vercel (`vercel deploy --prod`). O site oficial não está ligado ao GitHub, então não atualiza sozinho.

## O que acontece sozinho

| Workflow | Quando roda | O que faz |
|---|---|---|
| `ci.yml` | Push na `main` e pull requests | Tipos, lint, testes de interface e do backend |
| `release.yml` | Tag `vX.Y.Z` | Repete as verificações, confere a tag com a versão do projeto, gera e assina os instaladores, calcula os SHA-256, gera o `latest.json` e publica a release com as notas do changelog |
| `pages.yml` | Fim do `release.yml` ou mudança no changelog e na página | Regenera e publica a página de downloads |

Se qualquer verificação ou build falhar, a release não é criada.

Depois de editar uma release à mão, atualize a página com `gh workflow run pages.yml --ref main`.

Para ensaiar sem publicar nada, rode o `release.yml` à mão, na aba Actions ou com `gh workflow run release.yml --ref main`. Os instaladores ficam guardados como artefato da execução.

Uma versão com sufixo, como `1.3.0-beta.1`, sai como pré-lançamento. O GitHub não a considera a mais recente, então ela não chega aos aplicativos instalados.

## Chave de assinatura das atualizações

O aplicativo só instala um pacote assinado com esta chave. A parte pública está em `src-tauri/tauri.conf.json`. A parte privada e a senha ficam fora do repositório, em `%USERPROFILE%\.tauri\`, e no GitHub como os segredos `TAURI_SIGNING_PRIVATE_KEY` e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

Guarde uma cópia da chave e da senha em lugar seguro. **Sem elas, quem já instalou o Prancha nunca mais recebe atualizações** e precisa instalar a versão nova à mão, porque o aplicativo recusa qualquer outra chave.

Sem os segredos, o `release.yml` falha na compilação e nada é publicado. Para gerar os instaladores no seu computador, defina as duas variáveis no terminal antes de `npm run tauri build`.

## O que não roda no GitHub

- **E2E, detecção em páginas reais e atualização real:** usam quadrinhos do seu disco e o gabarito em `tests/real/`, que não vão para o git, e instalam o aplicativo de verdade. Por isso o passo 3 é local.
- **Assinatura digital do Windows:** é outra assinatura, diferente da chave acima. Os instaladores não têm essa, então o Windows SmartScreen pode pedir confirmação.

## Página de downloads

A página fica em <https://cakeysama.github.io/prancha/> e o site oficial traz a mesma página em `/downloads`. Ela é um arquivo estático gerado por `scripts/build-downloads.mjs`, a partir de `scripts/downloads.template.html`, do `CHANGELOG.md` e das releases publicadas. O mesmo script gera o manifesto `releases.json`.

Uma versão só aparece na página depois de publicada como release, e uma release sem entrada no changelog faz a geração falhar.
