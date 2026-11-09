# ADR 0002: repositórios do Mihon pelo Suwayomi

**Estado:** aceita (2026-11-02). Muda a [ADR 0001](0001-runtime-de-extensoes.md) só no que diz respeito ao Mihon: as extensões do Prancha continuam em WebAssembly isolado.

## Contexto

O pedido é colar o link do Keiyoushi e usar, sem mais nada. As extensões do Mihon são código Kotlin feito para Android, e o `index.json` do Keiyoushi publica versões em `.jar` delas. Reescrever cada uma para WebAssembly não é viável, e rodar esse código exige uma JVM.

## Decisão

O Prancha baixa e controla o [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server), um servidor de código aberto (MPL-2.0) que já executa essas extensões, e fala com ele por GraphQL em `127.0.0.1`.

| Tema | Decisão | Motivo |
|---|---|---|
| Distribuição | Baixado na primeira instalação de uma extensão do Mihon, fora do instalador | São 343 MB que quem não usa o Keiyoushi não precisa levar. |
| Versão | v2.4.2366, com endereço e SHA-256 fixos no código | Um pacote trocado é recusado, e uma versão nova passa pelos testes antes. |
| O que fica em disco | Só `jre/` e `bin/Suwayomi-Server.jar` (229 MB) | O resto do pacote é um Electron que o Prancha não usa. |
| Início | Sob demanda | Abrir o Prancha continua sem depender de Java, e sem repositório do Mihon nada é baixado nem iniciado. |
| Rede | Só `127.0.0.1`, com porta livre e senha aleatória a cada início | O servidor libera CORS para qualquer origem: sem senha, qualquer site aberto no navegador alcançaria a API. Com ela, tudo responde 401. |
| Fim do processo | Job Object do Windows com `KILL_ON_JOB_CLOSE` | O Java termina junto com o Prancha, mesmo em uma falha ou na troca de versão do instalador, e não deixa o banco dele travado. |
| Recursos do servidor | WebUI, bandeja, FlareSolverr e KCEF desligados por `-D` | Sem isso ele baixa um Chromium de 268 MB por conta própria. |
| Conteúdo adulto | `MIXED`, `NSFW` e o que não se conhece são guardados como `adult`. Desde a 0.6.3 o Prancha não tem filtro +18, então a marca é só informação | `MIXED` também traz obras adultas. |
| Verificação anti-robô | Uma janela anônima do próprio site, em que a pessoa passa pela verificação à mão. Os cookies que o site entrega vão para `dados/settings/cookie_store.xml`, com o servidor parado, porque ele só lê esse arquivo ao iniciar | A liberação vale só para o navegador que passou por ela, então a janela declara o mesmo que o servidor (`USER_AGENT`). |
| Modelo de dados | Sem migração: a chave fixada é a impressão digital de 64 dígitos, e o `runtime` da entrada do catálogo é `suwayomi` | O fluxo de confiança, troca de chave, catálogo, Explorar, downloads e biblioteca é o mesmo. |
| Chamadas locais | `net::send_local`, separado de `Net` | `Net` continua sem nenhuma exceção de loopback em produção. |

## O que se perde

Estas extensões **não têm o isolamento do Prancha**: são Java com o acesso da pessoa aos arquivos e à internet. A assinatura de cada pacote também não é conferida, nem pelo Prancha nem pelo Suwayomi: `signingKey` é só guardado por ele. O Prancha confere apenas se o repositório continua anunciando a chave aprovada. A interface diz isso antes de adicionar o repositório e antes de instalar cada extensão.

Sem o KCEF, extensões que dependem de um navegador embutido falham com um erro. Com o Java no ar, são cerca de 500 MB de memória, até o Prancha fechar ou o plugin Fontes de leitura ser removido.

## Alternativas

- **Traduzir o `.jar` ou reescrever as extensões:** não escala para mais de mil extensões que mudam toda semana.
- **Levar o Suwayomi no instalador:** o instalador iria de 8 MB para mais de 300 MB, para todo mundo.
- **Pedir que a pessoa instale o Suwayomi:** foi o desenho anterior, descartado pelo pedido de "só colar o link".
- **JVM embutida no processo do Prancha:** daria às extensões o mesmo processo da biblioteca e do banco.
- **Ligar o KCEF:** resolve mais extensões, mas cada instalação passaria a ocupar 563 MB a mais, e as que dependem dele costumam estar atrás de verificações anti-robô, que o Prancha não contorna.
- **FlareSolverr ou Byparr:** resolvem a verificação sozinhos. Isso é contornar, e o Prancha não faz: quem passa é a pessoa.

## Consequências

- Atualizar o Suwayomi é trocar três constantes em `suwayomi.rs`, conferir `USER_AGENT` com o `NetworkHelper.kt` da versão nova e rodar o teste `a_real_keiyoushi_extension_goes_from_the_link_to_an_image`.
- Só o formato com `repo.json` e `index.json` é entendido. Repositórios só com `index.min.json` continuam recebendo a mensagem do Mihon.
- O código do Suwayomi não vem no pacote do Prancha: é baixado depois, do projeto original, e por isso não entra em `src/generated/licenses.json`.
