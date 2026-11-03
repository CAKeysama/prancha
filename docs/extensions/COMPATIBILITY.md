# Compatibilidade com o Mihon

**Repositórios do Mihon no formato do Keiyoushi funcionam, sem o isolamento do Prancha.** Cole o endereço em **Repositórios** e siga o mesmo caminho de qualquer outro repositório:

```text
https://raw.githubusercontent.com/keiyoushi/extensions/repo
```

Também valem o `repo.json`, o `index.pb` e o `index.min.json` que ficam nesse endereço.

A decisão e as alternativas estão na [ADR 0002](../adr/0002-repositorios-do-mihon-pelo-suwayomi.md).

## O que acontece por baixo

1. O Prancha lê o `repo.json` e o `index.json` e mostra o nome, a quantidade de extensões e a impressão digital.
2. Na primeira instalação, ele baixa o [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server), um programa de terceiros com o próprio Java, de 343 MB. Depois de conferir o SHA-256, guarda só o que usa: 229 MB, em `suwayomi/` dentro da pasta de dados.
3. O Suwayomi instala e executa a extensão em um processo separado, só em `127.0.0.1`, com porta e senha novas a cada início. Ele fecha junto com o Prancha.
4. As fontes aparecem em Explorar, e os capítulos abrem direto, sem baixar. O que você baixar vira CBZ na biblioteca, como nas outras extensões, e abre sem internet e sem o Suwayomi.
5. Remover o plugin **Fontes de leitura** para o Suwayomi na hora, mas não apaga os 229 MB nem as extensões, para que tudo volte igual se você instalar de novo.

## O que muda em relação às extensões do Prancha

| | Extensão do Prancha | Extensão do Mihon |
|---|---|---|
| Pacote | Componente WebAssembly | `.jar` executado pelo Suwayomi |
| Acesso | Só as funções do [contrato](EXTENSION_API.md) | **Sem isolamento:** seus arquivos e a internet, como qualquer programa |
| Rede | Só os hosts declarados, pelo Prancha | Livre, feita pela própria extensão |
| Assinatura | Ed25519 do índice e de cada pacote, conferida pelo Prancha | **Nenhuma conferência de pacote.** O Prancha só garante que o repositório continua anunciando a chave aprovada |
| Conteúdo adulto | Declarado por quem publica | Também declarado por quem publica. `SAFE` é o único que passa pelo filtro +18 ligado |
| Memória | Pequena, por chamada | Cerca de 500 MB enquanto o Prancha estiver aberto, depois do primeiro uso |

Por isso, só instale uma extensão do Mihon se confiar em quem a publica. O Prancha avisa antes de adicionar o repositório e antes de instalar.

## O que não funciona

- **Extensões marcadas como `MIXED` ou `NSFW`** não aparecem com o filtro +18 ligado, que é o padrão. No Keiyoushi, em 2026-11-02, isso deixa 564 das 1448.
- **Fontes que dependem de um navegador embutido** falham com um erro.
- **Login:** o Prancha não oferece onde informar conta nem preferências da extensão.
- **Repositórios só com `index.min.json`** continuam recebendo a mensagem abaixo, e endereços de `.apk` também:

> Este repositório distribui extensões para Mihon e não possui pacotes compatíveis com o Prancha.

## Sites com verificação anti-robô

Alguns sites respondem com uma verificação (Cloudflare, por exemplo). O Prancha não responde a ela por ninguém. O erro vem com o botão **Verificar no site**, que abre o próprio site em uma janela:

1. Você passa pela verificação, se ela aparecer.
2. A janela fecha sozinha quando o site libera, ou quando você fecha.
3. O Prancha entrega ao servidor das fontes os cookies que o site deu e tenta de novo.

A janela é anônima e não alcança nada do Prancha. Ela se apresenta com o mesmo navegador que o servidor declara, porque a liberação de um site vale só para o navegador que passou por ela.

A liberação expira, e aí é só verificar de novo. O site também pode recusar mesmo depois de liberar, porque nem tudo que ele confere é igual entre a janela e o servidor. Em 2026-11-03 foi o que aconteceu com a Lycan Toons, e o Prancha avisa quando isso acontece. A passagem reinicia o servidor, então um download em andamento pode falhar e pedir nova tentativa.

- **APKs não são executados.** O que roda é o `.jar` publicado no `index.json`.

## Formato de um repositório Keiyoushi

Verificado em 2026-11-02, nos arquivos publicados em `https://raw.githubusercontent.com/keiyoushi/extensions/repo/`:

`repo.json`:

```json
{
  "index_v2": ".../index.pb",
  "meta": { "name": "...", "website": "...", "signingKeyFingerprint": "..." }
}
```

`index.json`, com `extensionList.extensions`, em que cada item tem `name`, `packageName`, `versionName`, `contentWarning`, `resources.jarUrl` e `sources` (`id`, `name`, `language`, `homeUrl`). O `index.min.json` que está lá hoje é um aviso para apps antigos, sem extensões.

| | Keiyoushi | Prancha |
|---|---|---|
| Manifesto | `repo.json` | `prancha-repo.json` |
| Índice | `index.json` e `index.pb` | `prancha-index.json` |
| Pacote | `.jar` e `.apk` | `.wasm`, com `.sig` |
| Identificador | `packageName` | `id` |
| Classificação | `contentWarning` | `contentRating` e, por fonte, `rating` |
| Capacidades declaradas | Não há | `capabilities`, com `allowedHosts` |
| Chave | Impressão digital da chave que assina os APKs | Chave pública Ed25519 no manifesto |

### Como o endereço é reconhecido

O endereço é tratado como Mihon quando termina em `.apk`, `/index.min.json` ou `/index.pb`, quando o JSON tem `meta.name` sem `schemaVersion`, ou quando é uma lista com `pkg` e `apk`. Um endereço-base sem `prancha-repo.json` também é testado com `repo.json` e `index.min.json`. Só depois disso o Prancha procura `repo.json` e `index.json`.

## Portar uma fonte

Uma fonte do Mihon também pode ser reescrita como extensão do Prancha e ganhar o isolamento, já que a lógica de "como consultar este site" é parecida:

| Mihon | Prancha |
|---|---|
| `getPopularManga` | `popular` |
| `getLatestUpdates` | `latest` |
| `getSearchManga` | `search` |
| `getMangaDetails` | `details` |
| `getChapterList` | `chapters` |
| `getPageList` | `pages` |

Verifique a licença do código original e se o site permite esse acesso. Fontes que dependem de login, cookies de sessão ou execução de JavaScript no navegador não funcionam: a API 1 não tem esses recursos.
