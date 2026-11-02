# Compatibilidade com o Mihon

**O Prancha não instala nem executa extensões do Mihon.** Repositórios do Mihon e da Keiyoushi não são repositórios do Prancha, e colar um desses endereços mostra:

> Este repositório distribui extensões para Mihon e não possui pacotes compatíveis com o Prancha.

O endereço continua no campo, nada é baixado além do necessário para reconhecer o formato, e nada é instalado. O que existe é o reconhecimento do formato, para dar uma resposta clara, e nenhuma integração.

## Por que não funcionam

| | Mihon | Prancha |
|---|---|---|
| Pacote | APK Android | Componente WebAssembly |
| Linguagem | Kotlin em uma JVM Android | Qualquer uma que gere um componente (SDK em Rust) |
| Permissões | As de um app Android | Só as funções do [contrato](EXTENSION_API.md) |
| Rede | Livre, feita pela própria extensão | Só o que foi declarado, pelo Prancha |
| Assinatura | Do APK e do repositório | Ed25519 do índice e de cada pacote, com confiança explícita |
| Plataforma | Android | Windows |

Rodar um APK no Windows exigiria uma camada Android ou JVM, o que traria muito mais código e daria à extensão uma superfície que o Prancha não consegue limitar. Isso contraria o modelo de segurança.

## Formato de um repositório Keiyoushi

Verificado em 2026-11-02, nos arquivos publicados em `https://raw.githubusercontent.com/keiyoushi/extensions/repo/`:

`repo.json`:

```json
{
  "index_v2": ".../index.pb",
  "meta": { "name": "...", "website": "...", "signingKeyFingerprint": "..." }
}
```

`index.min.json`: uma lista de objetos com `name`, `pkg`, `apk`, `lang`, `code`, `version`, `nsfw` e `sources` (`name`, `lang`, `id`, `baseUrl`).

| | Keiyoushi | Prancha |
|---|---|---|
| Manifesto | `repo.json` | `prancha-repo.json` |
| Índice | `index.min.json` e `index.pb` | `prancha-index.json` |
| Pacote | `.apk` | `.wasm`, com `.sig` |
| Identificador | `pkg` | `id` |
| Classificação | `nsfw` (0 ou 1) | `contentRating` e, por fonte, `rating` |
| Capacidades declaradas | Não há | `capabilities`, com `allowedHosts` |
| Chave | Impressão digital da chave que assina os APKs | Chave pública Ed25519 no manifesto |

### Como o reconhecimento funciona

O endereço é tratado como Mihon quando termina em `.apk` ou `/index.min.json`, quando o JSON tem `meta.name` sem `schemaVersion`, ou quando é uma lista com `pkg` e `apk`. Um endereço-base sem `prancha-repo.json` também é testado com `repo.json` e `index.min.json`. Nada desses arquivos é guardado.

## Portar uma fonte

Uma fonte do Mihon pode ser reescrita como extensão do Prancha, já que a lógica de "como consultar este site" é parecida:

| Mihon | Prancha |
|---|---|
| `getPopularManga` | `popular` |
| `getLatestUpdates` | `latest` |
| `getSearchManga` | `search` |
| `getMangaDetails` | `details` |
| `getChapterList` | `chapters` |
| `getPageList` | `pages` |

Verifique a licença do código original e se o site permite esse acesso. Fontes que dependem de login, cookies de sessão ou execução de JavaScript no navegador não funcionam: a API 1 não tem esses recursos.

## Adaptador Suwayomi (trabalho futuro, não implementado)

Uma forma de aproveitar o ecossistema do Mihon sem rodar APK dentro do Prancha seria conversar com um servidor [Suwayomi](https://github.com/Suwayomi/Suwayomi-Server) que a própria pessoa instale e mantenha:

- Seria um adaptador opcional, com o Suwayomi rodando em processo separado, com Java, e fora do instalador do Prancha.
- O Prancha falaria apenas com o endereço local configurado pela pessoa.
- As fontes apareceriam em Explorar, identificadas como vindas do adaptador, e os capítulos entrariam na biblioteca como CBZ, como os demais.

Os pontos em aberto são: o isolamento do Prancha **não valeria** para essas extensões, que rodariam no Suwayomi com as permissões dele; a filtragem de conteúdo adulto dependeria do campo `nsfw` declarado; e a exceção de rede local precisaria de um desenho próprio. Por isso esse caminho exige uma decisão de produto e não faz parte desta versão. Nem o Suwayomi nem o Java são instalados ou exigidos pelo Prancha.
