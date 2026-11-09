# Como publicar uma versão

## Numeração

Toda mudança que entra na `main` sai como uma versão nova. O Prancha segue o [versionamento semântico](https://semver.org/lang/pt-BR/), no formato `vMAJOR.MINOR.PATCH`, e quem decide qual parte sobe são os rótulos dos commits:

| Parte | Quando sobe | Rótulo do commit | Exemplo |
|---|---|---|---|
| `MAJOR` | Mudanças incompatíveis: o que já existia deixa de funcionar ou muda muito, e a pessoa precisa se adaptar | `!` depois do tipo (`feat!:`, `refactor!:`) ou uma linha `BREAKING CHANGE: ...` no corpo | uma biblioteca que não abre mais na versão antiga: 2.5.1 → 3.0.0 |
| `MINOR` | Funcionalidades novas, com tudo o que já existia funcionando igual | `feat:` | um novo modo de leitura: 2.4.1 → 2.5.0 |
| `PATCH` | Correções, ajustes internos e de segurança, sem recurso novo e sem quebrar nada | `fix:`, `perf:`, `refactor:`, `style:`, `test:`, `docs:`, `build:`, `ci:`, `chore:`, `revert:`, `site:` | índice de páginas que perdia a rolagem: 2.5.0 → 2.5.1 |

Vale o maior rótulo entre os commits do PR: um PR com um `feat:` e três `fix:` é uma `MINOR`.

### Como rotular um commit

A primeira linha é `tipo: descrição` ou `tipo(área): descrição`, em minúsculas, com espaço depois dos dois-pontos:

```text
feat(leitura): marca capítulos novos na biblioteca
fix: o filtro +18 escondia obras livres
refactor!: guarda o progresso numa tabela nova

BREAKING CHANGE: a biblioteca da 2.x precisa ser importada de novo.
```

Escolha pelo que muda para quem usa o Prancha: um recurso novo é `feat`, mesmo que também corrija algo; mexer só por dentro, sem mudar o que a pessoa vê, é `refactor` ou `chore`. O título do PR segue o mesmo formato. Commits de merge não contam.

O workflow `versao.yml` confere tudo isso em cada PR e não deixa uma `MINOR` sair como `PATCH`.

## Passo a passo

Todo PR traz a própria versão. Com os commits já feitos e a `main` trazida para o branch:

1. **Suba a versão** com `npm run version:bump`. O script lê os commits desde a `main`, calcula a versão e a troca nos cinco arquivos que a guardam (`package.json`, `package-lock.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` e `src-tauri/Cargo.lock`), e abre a entrada no `CHANGELOG.md`. Faça o commit como `chore(release): versão X.Y.Z`. Se as dependências mudaram, rode `npm run licenses`, porque a tela de licenças do app sai desse arquivo.
2. **Escreva as notas** na entrada aberta no topo do `CHANGELOG.md`, no formato `## [X.Y.Z] - AAAA-MM-DD`, com um resumo e as seções `Novidades`, `Melhorias`, `Correções` e `Observações` (só as que tiverem conteúdo, apague as vazias). Cada item ocupa uma linha. `npm run version:check` confere tudo antes do push.
3. **Rode localmente** o que precisa de páginas reais e da janela do app:

   ```bash
   npm run e2e:build
   npm run test:e2e
   cargo test --release --manifest-path src-tauri/Cargo.toml --test guided -- --test-threads=1
   npm run test:update -- --build
   ```

4. **Abra o pull request com a mudança, a versão e as notas.** O merge na `main` publica tudo sozinho: instaladores, release, atualizador, página de downloads e site oficial.
5. **Traga a `main` para o branch logo antes do merge.** Se outro PR entrou antes e já usou a versão, a verificação fica vermelha: rode `npm run version:bump` de novo, que passa as notas para a versão seguinte.

Também dá para publicar enviando a tag à mão, como antes: `git tag -a vX.Y.Z -m "Prancha vX.Y.Z"` e `git push origin main vX.Y.Z`.

## O que acontece sozinho

| Workflow | Quando roda | O que faz |
|---|---|---|
| `ci.yml` | Push na `main` e pull requests | Tipos, lint, testes de interface e do backend |
| `versao.yml` | Pull requests e push na `main` | No PR, confere o título, o rótulo de cada commit, se a versão é a que os rótulos pedem nos cinco arquivos e se a entrada do changelog está preenchida. Na `main`, fica vermelho se algo entrou sem versão nova |
| `release.yml` | Versão nova no `package.json` da `main`, ou tag `vX.Y.Z` | Confere se a versão ainda não saiu, repete as verificações, gera e assina os instaladores, calcula os SHA-256, gera o `latest.json`, cria a tag e publica a release com as notas do changelog |
| `pages.yml` | Fim do `release.yml`, ou mudança no changelog, no site ou nos scripts das páginas | Regenera e publica a página de downloads no GitHub Pages e o site oficial na Vercel, com a versão, os links e os SHA-256 da release mais recente |

Se qualquer verificação ou build falhar, nada é publicado e o site continua na versão anterior. Corrija e faça um novo push na `main`: como a tag só nasce junto com a release, a mesma versão é tentada de novo.

Um merge que não muda a versão não publica nada, e o `versao.yml` fica vermelho na `main` para isso não passar despercebido. Abra um PR só com a versão e as notas para publicar o que entrou.

Depois de editar uma release à mão, atualize as páginas com `gh workflow run pages.yml --ref main`.

Para ensaiar sem publicar nada, rode o `release.yml` à mão, na aba Actions ou com `gh workflow run release.yml --ref main`. Os instaladores ficam guardados como artefato da execução.

Uma versão com sufixo, como `1.3.0-beta.1`, sai como pré-lançamento. O GitHub não a considera a mais recente, então ela não chega aos aplicativos instalados.

## Configuração feita uma vez

Ficam em **Settings → Secrets and variables → Actions** do repositório. Nada disso vai para arquivos do projeto.

| Nome | Tipo | Para quê |
|---|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Segredo | Assinar as atualizações. Ver abaixo. |
| `RELEASE_TOKEN` | Segredo | Publicar a release e a tag na conta CAKeysama. É um [token fine-grained](https://github.com/settings/personal-access-tokens/new) só deste repositório, com **Contents: Read and write**. Sem ele, ou vencido, a release sai como `github-actions[bot]`, com um aviso na execução. |
| `VERCEL_TOKEN` | Segredo | Publicar o site oficial. É um [token da Vercel](https://vercel.com/account/tokens) com acesso ao projeto `prancha`. |
| `VERCEL_ORG_ID` e `VERCEL_PROJECT_ID` | Variável | Qual projeto da Vercel recebe o site: são o `orgId` e o `projectId` de `site/.vercel/project.json`. Sem elas, o job do site fica de fora. |

Para cadastrar um segredo, rode `gh secret set NOME --repo CAKeysama/prancha` e cole o valor quando ele pedir.

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

## Site oficial

O site em <https://prancha-beta.vercel.app> sai da pasta `site/`. O `pages.yml` roda `scripts/build-site.mjs`, que põe no `site/index.html` a versão, os links, o tamanho e os SHA-256 da release mais recente, e publica a pasta na Vercel. O `site/index.html` do git não é alterado: os números certos só existem no site no ar.

As imagens de vitrine mostram obras de terceiros, então ficam fora do git, em `site/assets/prints`. O workflow baixa do próprio site no ar as que o `site/index.html` usa. Uma imagem nova ainda não está lá, e o workflow falha avisando qual falta. Nesse caso, publique uma vez à mão, de dentro da pasta `site`, nunca da raiz, porque da raiz a Vercel publica o aplicativo e arquivos ignorados pelo git:

```bash
npm run site:build
cd site
vercel deploy --prod
```
