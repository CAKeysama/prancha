# Código do Mihon

A API que as extensões do Mihon usam, copiada do [Mihon](https://github.com/mihonapp/mihon) (commit `4823ba69ca65fa707fb89406f3a4c43c9e0053dd`,
licença Apache 2.0, em [LICENSE](LICENSE)) para o Prancha carregar essas extensões no Android (`app/prancha/MihonPlugin.kt`).
As extensões chamam estas classes pelo nome e pela assinatura, então elas ficam como no Mihon. O que mudou:

- `source/`, `util/JsoupExtensions.kt` e o `awaitSingle` (`tachiyomi/core/common/util/lang`) e o `EMPTY` (`mihon/core/common/extensions`) de que dependem: sem mudanças, só sem a anotação `@Stable` do Compose em `FilterList`.
- `network/NetworkHelper.kt`: reescrito sem a injeção de dependências e sem as preferências do Mihon, e sem o resolvedor automático do Cloudflare (a pessoa faz a verificação numa tela do site, `VerifyActivity`).
- `network/JavaScriptEngine.kt`: sem as anotações de injeção.
- `AppInfo.kt`: versão do Prancha e os tipos de imagem que o leitor do Prancha mostra.
- Ficaram de fora o `CloudflareInterceptor`, o `WebViewInterceptor`, os provedores de DNS e as preferências de rede.
