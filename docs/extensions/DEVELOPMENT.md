# Desenvolver uma extensão

Uma extensão é um crate Rust compilado para WebAssembly. O ponto de partida é [`extensions/template`](../../extensions/template), e um exemplo completo que consulta um site de verdade é [`extensions/demo`](../../extensions/demo).

Outras linguagens funcionam se gerarem um componente do mundo `extension` sem importar WASI, mas só Rust tem SDK e foi testado.

## Antes de começar

Só escreva uma fonte para um site que você tem permissão de consultar. Não contorne login, paywall, DRM ou CAPTCHA. Declare a classificação real: conteúdo adulto só aparece para quem desligou o filtro +18.

## O que você precisa

- Rust 1.95 ou mais novo
- O alvo WebAssembly:

```bash
rustup target add wasm32-unknown-unknown
```

## 1. Criar o crate

Copie `extensions/template` para `extensions/minha-fonte`, troque o `name` no `Cargo.toml` dela e acrescente a pasta em `members` no `extensions/Cargo.toml`.

```toml
[package]
name = "minha-fonte"
edition.workspace = true
license.workspace = true
version.workspace = true
publish.workspace = true

[lib]
crate-type = ["cdylib"]

[dependencies]
prancha-extension-sdk = { path = "../sdk" }
```

## 2. Implementar as funções

Implemente `Guest` e chame `export_extension!`. O [contrato](EXTENSION_API.md) descreve cada função.

```rust
use prancha_extension_sdk::host::{self, HttpRequest};
use prancha_extension_sdk::{Filter, Guest, SearchPage, SourceError, SourceInfo, export_extension /* ... */};

struct Extension;

impl Guest for Extension {
    fn list_sources() -> Vec<SourceInfo> { /* ... */ }

    fn search(_source: String, query: String, page: u32, _filters: Vec<Filter>) -> Result<SearchPage, SourceError> {
        // Codifique a pesquisa antes de colocá-la no endereço: ela vem digitada pela pessoa.
        let response = host::http_get(&HttpRequest {
            url: format!("https://exemplo.com/api/busca?q={}&p={page}", codificar(&query)),
            headers: vec![],
        })
        .map_err(|_| SourceError::Network("Não foi possível falar com o site.".into()))?;
        if response.status != 200 {
            return Err(SourceError::Source(format!("O site respondeu {}.", response.status)));
        }
        // Interprete response.body e devolva os resultados.
        Ok(SearchPage { results: vec![], has_more: false })
    }

    // popular, latest, details, chapters e pages
}

export_extension!(Extension);
```

Regras que evitam surpresas:

- **Sem estado entre chamadas.** Cada chamada usa uma instância nova. Use `host::storage_*` se a capacidade `storage` fizer sentido.
- **Sem `std::env`, `std::fs` ou relógio.** Em `wasm32-unknown-unknown`, esses recursos falham ou entram em pânico. Tudo que vem de fora chega pelo host.
- **`id` estável.** Os identificadores de obra e capítulo ficam guardados na biblioteca: mudar quebra o vínculo com o que já foi baixado.
- **`order` por capítulo.** É o que define a sequência, e não o número.
- **Erro claro.** O texto do `SourceError` aparece para a pessoa, então escreva em português.
- **Só os campos que a fonte tem.** O que ela não informar vai como `None`.

## 3. Compilar e gerar o componente

Dentro da pasta do repositório do Prancha:

```bash
cargo build --release --manifest-path extensions/Cargo.toml -p prancha-ext
```

```bash
cargo build --release --target wasm32-unknown-unknown --manifest-path extensions/Cargo.toml -p minha-fonte
```

```bash
extensions/target/release/prancha-ext component extensions/target/wasm32-unknown-unknown/release/minha_fonte.wasm minha-fonte.wasm
```

Se a variável `CARGO_TARGET_DIR` estiver definida, os arquivos ficam nessa pasta.

## 4. Testar

A forma mais fiel é rodar a extensão no Source Engine, como os testes do Prancha fazem:

1. Em `src-tauri/tests/extensions.rs`, veja como `tests/common/wasm.rs` compila um pacote e como `tests/common/http.rs` sobe um servidor local que imita o site.
2. Use o mesmo caminho na sua extensão, com respostas gravadas do site. Assim os testes não dependem da internet.

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test extensions
```

Para ver no aplicativo, publique em uma pasta (próximo passo) e sirva em HTTPS. Um build de depuração do Prancha, iniciado com `PRANCHA_E2E_ALLOW_LOOPBACK=1`, também aceita um servidor em `http://127.0.0.1`. Nesse caso, publique com `baseUrl` e `allowedHosts` apontando para esse endereço. `tests/e2e/extensions.spec.ts` faz exatamente isso.

## 5. Publicar

Veja [Publicar um repositório](PUBLISHING.md).

## Checklist antes de publicar

- [ ] `list-sources` declara a classificação real, e não `unknown`.
- [ ] `allowedHosts` tem só o necessário, incluindo os hosts de capas e imagens.
- [ ] Sem resultados não é erro: devolva uma página vazia.
- [ ] `min-interval-ms` respeita o site.
- [ ] Os índices de `page` vão de 0 a n-1.
- [ ] Mensagens em português, e sem detalhe técnico que a pessoa não possa usar.
