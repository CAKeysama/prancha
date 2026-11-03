# Formato dos repositórios

Um repositório é um conjunto de arquivos estáticos em HTTPS. Não precisa de servidor próprio: qualquer hospedagem de arquivos serve. O [`prancha-ext`](PUBLISHING.md) gera tudo isto.

```
prancha-repo.json              manifesto, com a chave pública
prancha-index.json             catálogo de extensões
prancha-index.json.sig         assinatura do catálogo
<id>-<versão>.wasm             pacote da extensão
<id>-<versão>.wasm.sig         assinatura do pacote
```

## Manifesto: `prancha-repo.json`

Até 64 KB.

```json
{
  "schemaVersion": 1,
  "id": "exemplo.repositorio",
  "name": "Repositório de exemplo",
  "description": "Opcional.",
  "website": "https://exemplo.com",
  "indexUrl": "https://exemplo.com/repo/prancha-index.json",
  "publicKey": "UMhYLbeR0WdNiQ0QquvTaGV5n6yAElwmLwEvsxzo11I=",
  "updatedAt": "2026-11-02T19:19:37Z"
}
```

| Campo | Regra |
|---|---|
| `schemaVersion` | `1`. Um valor maior é recusado com a mensagem de formato incompatível. |
| `id` | De 1 a 128 caracteres: letras minúsculas, dígitos, `.`, `-` e `_`, sem começar por pontuação. |
| `name` | Até 120 caracteres. |
| `indexUrl` | HTTPS na porta 443, em host público. |
| `publicKey` | Chave pública Ed25519 de 32 bytes, em base64. Chaves fracas são recusadas. |

## Índice: `prancha-index.json`

Até 8 MB. O índice é validado por inteiro: uma entrada errada recusa o arquivo todo, e o último catálogo válido continua em uso.

```json
{
  "schemaVersion": 1,
  "repositoryId": "exemplo.repositorio",
  "generatedAt": "2026-11-02T19:19:37Z",
  "extensions": [
    {
      "id": "exemplo.repositorio.fonte",
      "name": "Fonte de exemplo",
      "version": "1.0.0",
      "description": "Opcional.",
      "authors": ["Alguém"],
      "languages": ["pt-BR"],
      "runtime": "wasm-component",
      "apiVersion": "1",
      "minAppVersion": "0.2.0",
      "maxAppVersion": "1.0.0",
      "artifactUrl": "https://exemplo.com/repo/exemplo.repositorio.fonte-1.0.0.wasm",
      "signatureUrl": "https://exemplo.com/repo/exemplo.repositorio.fonte-1.0.0.wasm.sig",
      "size": 117243,
      "sha256": "b707349906f992e8e99f2f89739f71df3745760f90677125474e47bf9f09a043",
      "icon": "data:image/png;base64,...",
      "supportedKinds": ["comic", "manga"],
      "contentRating": "general",
      "sources": [{ "id": "fonte", "name": "Fonte de exemplo", "language": "pt-BR", "contentRating": "general" }],
      "capabilities": { "network": true, "storage": false, "authentication": false, "allowedHosts": ["exemplo.com", "*.cdn.exemplo.com"] }
    }
  ]
}
```

| Campo | Regra |
|---|---|
| `generatedAt` | UTC, como `2026-11-02T19:19:37Z`. Um índice mais antigo que o último aceito é recusado, para impedir a volta a uma versão anterior. |
| `id` | Mesma regra do manifesto. Repetir um `id` recusa o índice. |
| `name`, `authors[]` | Até 120 caracteres cada. |
| `description` | Até 2000 caracteres. |
| `version`, `minAppVersion`, `maxAppVersion` | Versão semântica. `maxAppVersion` é opcional. |
| `runtime` | Só `wasm-component`. Outro valor, de até 40 caracteres, aparece no catálogo como incompatível. |
| `apiVersion` | Só `1`. Outro valor, de até 20 caracteres, aparece no catálogo como incompatível. |
| `artifactUrl`, `signatureUrl` | HTTPS na porta 443, em host público. |
| `size` | De 1 byte a 32 MB, e o tamanho real é conferido no download. |
| `sha256` | 64 caracteres hexadecimais minúsculos. |
| `icon` | Opcional: `data:image/png`, `jpeg` ou `webp` em base64, até 32 KB. |
| `contentRating` | `general`, `teen`, `adult` ou `unknown`. Entradas `adult` só aparecem com o filtro +18 desligado. |
| `sources` | Ao menos uma. Na instalação, os identificadores e as classificações precisam bater com o que o componente declara. |
| `supportedKinds` | Até 40 caracteres cada. |
| `capabilities.network` | Com `true`, `allowedHosts` não pode ser vazio. Com `false`, precisa ser vazio. |
| `capabilities.allowedHosts` | Nomes exatos ou `*.dominio.com`. Endereços IP e nomes locais são recusados. |
| `capabilities.authentication` | Reservado. Nenhum recurso de login existe nesta versão. |

## Assinaturas

Todas são Ed25519, gravadas como base64 dos 64 bytes. O Prancha verifica com a regra estrita (`verify_strict`).

| O que assina | Mensagem assinada |
|---|---|
| Índice | `prancha-index-v1\0` seguido dos bytes exatos de `prancha-index.json` |
| Pacote | `prancha-package-v1\0<repositório>\0<extensão>\0<versão>\0<sha256>` |

`\0` é o byte zero. O prefixo impede que a assinatura de um tipo valha para o outro, e a mensagem do pacote prende a assinatura a esse repositório, a essa extensão e a essa versão. Trocar uma pela outra, ou reaproveitar o pacote de outra extensão, falha na verificação.

A assinatura do índice fica sempre em `<indexUrl>.sig`.

**Impressão digital:** SHA-256 dos 32 bytes da chave pública, em hexadecimal. O Prancha mostra em blocos de quatro caracteres.

## Respostas do servidor

- Só HTTPS na porta 443. Até 3 redirecionamentos, cada um conferido de novo.
- `ETag` é opcional, e um `304` evita baixar o índice de novo.
- O tempo limite é de 30 segundos por arquivo do repositório e de 120 segundos para o pacote.
