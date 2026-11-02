# Contrato da API de extensões

A fonte de verdade é [`extensions/wit/world.wit`](../../extensions/wit/world.wit), pacote `prancha:source@1.0.0`. Uma extensão é um componente WebAssembly do mundo `extension`.

## O que a extensão exporta

| Função | Retorno | Observação |
|---|---|---|
| `list-sources()` | `list<source-info>` | Lida na instalação. As fontes precisam bater com o índice assinado. |
| `popular(source, page, filters)` | `result<search-page, source-error>` | `page` começa em 1. |
| `latest(source, page, filters)` | `result<search-page, source-error>` | Só chamada se `supports-latest`. |
| `search(source, query, page, filters)` | `result<search-page, source-error>` | Só chamada se `supports-search`. Até 512 bytes de pesquisa. |
| `details(source, id)` | `result<comic-details, source-error>` | |
| `chapters(source, id)` | `result<list<chapter>, source-error>` | |
| `pages(source, chapter-id)` | `result<list<page>, source-error>` | |

Hoje o único filtro enviado é `kind`, com o nome de um `content-kind`. Ignore os que não conhecer.

## O que o Prancha oferece

```wit
http-get: func(request: http-request) -> result<http-response, http-error>;
storage-get: func(key: string) -> option<list<u8>>;
storage-set: func(key: string, value: list<u8>) -> result<_, string>;
storage-delete: func(key: string);
log: func(message: string);
```

Isso é tudo. Não há arquivos, relógio, ambiente, processos, sockets ou acesso ao banco. Um componente que importe qualquer outra coisa nem carrega.

### `http-get`

- Só GET. Um POST pode chegar em uma versão `1.x` sem quebrar as extensões existentes.
- Só HTTPS na porta 443, nos hosts de `allowedHosts`.
- Só os cabeçalhos `referer` e `accept`. Qualquer outro faz a requisição ser recusada com `blocked`.
- Até 3 redirecionamentos, e cada destino é conferido de novo.

| Erro | Quando |
|---|---|
| `blocked(motivo)` | Host não autorizado, HTTP, outra porta ou endereço local. |
| `network(motivo)` | Falha de conexão. |
| `timeout` | Mais de 30 segundos. |
| `too-large` | Resposta acima de 8 MB. |
| `too-many-requests` | Mais de 20 requisições na mesma chamada. |

Um status como 404 ou 500 não é erro de `http-get`: vem em `http-response.status`, e a extensão decide o que fazer.

### Armazenamento

Exige `capabilities.storage`. Sem ela, `storage-get` não retorna nada e `storage-set` retorna erro. É um espaço só da extensão, apagado quando ela é removida.

## Tipos principais

| Tipo | Campos |
|---|---|
| `source-info` | `id`, `name`, `language` (BCP 47), `description?`, `kinds`, `rating`, `direction`, `supports-latest`, `supports-search`, `min-interval-ms` |
| `search-page` | `results`, `has-more`. Página vazia com `has-more` falso significa que não há resultados. |
| `search-result` | `id`, `title`, `cover-url?`, `summary?`, `author?`, `status?`, `rating?` |
| `comic-details` | `id`, `title`, `synopsis?`, `cover-url?`, `authors`, `genres`, `status?`, `language?`, `rating?` |
| `chapter` | `id`, `title`, `number?`, `published-at?` (ms UTC), `order` (quanto menor, mais cedo na obra) |
| `page` | `index` (a partir de 0), `url`, `headers` |

O que a fonte não souber vai como `none`. O Prancha não inventa valor e deixa o campo fora da tela.

`min-interval-ms` é o menor intervalo entre requisições de página que a fonte aceita. O gerenciador de downloads respeita esse valor, com um capítulo por vez em cada fonte.

## Erros

```wit
variant source-error {
    network(string),        // não foi possível falar com o site
    source(string),         // o site respondeu, mas com erro
    authentication(string), // o conteúdo exige login, que o Prancha não oferece
    data(string),           // a resposta do site não pôde ser entendida
    unsupported(string),    // a fonte não faz isso
}
```

O texto aparece para a pessoa: escreva em português e diga o que aconteceu. Um pânico da extensão vira erro para quem chamou e não afeta o resto do aplicativo.

## Classificação de conteúdo

| Valor | Comportamento |
|---|---|
| `general`, `teen` | Disponível. |
| `unknown` | A fonte fica desabilitada. Declare a classificação real. |
| `adult` | Nunca aparece. A extensão não instala, e resultados marcados como `adult` são descartados, em qualquer fonte. |

## Limites de execução

Passar de um limite encerra só aquela chamada, e o erro fica naquela fonte.

| Limite | Valor |
|---|---|
| Instruções (fuel) por chamada | 10 bilhões |
| Tempo por chamada | 45 segundos |
| Memória | 64 MB |
| Pilha | 512 KB |
| Requisições por chamada | 20 |
| Tamanho de cada resposta | 8 MB |
| Chamadas simultâneas | 4 no total, 2 por extensão |
| Identificadores | 1024 bytes |
| Filtros por chamada | 32 |
| Linhas de log por chamada | 64, com até 300 caracteres |
| Chave de armazenamento | 256 bytes |
| Valor de armazenamento | 256 KB |
| Armazenamento total | 1 MB por extensão |

O Prancha também limita o que aceita de volta:

| Dado | Limite |
|---|---|
| Resultados por página | 200 |
| Capítulos por obra | 5000 |
| Título | 300 caracteres |
| Sinopse e resumo | 4000 caracteres |
| Autores e gêneros | 32, com até 120 caracteres |
| Páginas por capítulo | 2000 |
| Tamanho de uma página | 32 MB |
| Tamanho de um capítulo | 3 GB |
| Tamanho de uma capa | 16 MB |

Cada chamada usa uma instância nova: não guarde estado em variáveis globais entre chamadas.

Os índices de `page` precisam ir de 0 a n-1, sem repetir nem pular. O Prancha ordena as páginas por índice, e se a sequência estiver errada o download falha em vez de gerar um capítulo fora de ordem.

## Versões

- **API `1.x`:** só acréscimos. Uma extensão feita para a 1.0 continua funcionando.
- **Qualquer outra mudança:** vira a versão 2, e `apiVersion` no índice muda junto. O catálogo mostra a extensão antiga como incompatível, com o motivo, em vez de tentar executá-la.
- `minAppVersion` e `maxAppVersion` limitam as versões do Prancha em que a extensão pode ser instalada.
