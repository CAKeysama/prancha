# Integração com o MyAnimeList

Com a conta conectada, o Prancha associa cada obra das fontes de leitura à obra correspondente no [MyAnimeList](https://myanimelist.net) (MAL) e atualiza os capítulos lidos na lista da pessoa quando ela termina um capítulo. Sem conta, nada muda: o MAL não é consultado e o app funciona como antes. O código fica em `src-tauri/src/mal/` e a interface em `src/features/mal/`.

## Configuração feita uma vez

O MAL só aceita aplicativos cadastrados. O do Prancha já está cadastrado em <https://myanimelist.net/apiconfig>, com a conta de quem mantém o projeto, assim:

| Campo | Valor |
|---|---|
| App Type | **other** (aplicativo nativo, público: não recebe segredo e usa PKCE) |
| App Redirect URL | `http://localhost:8080/callback` |
| Homepage URL | `https://github.com/CAKeysama/prancha` |
| Commercial / Non-Commercial | Non-Commercial |

O endereço de retorno é fixo no código (`REDIRECT_PORT` e `REDIRECT_PATH` em `src-tauri/src/mal/oauth.rs`, com um teste que confere o valor): mudar um exige mudar o outro. O **Client ID** do cadastro fica na variável do repositório (não segredo, porque ele aparece no endereço de autorização):

```bash
gh variable set MAL_CLIENT_ID --repo CAKeysama/prancha
```

O Client ID é público (aparece no endereço de autorização), não é segredo; tokens de usuários nunca vão para variáveis.

Como ele chega ao app:

1. No job **Gerar os instaladores** do `release.yml`, `vars.MAL_CLIENT_ID` vira a variável de ambiente `PRANCHA_MAL_CLIENT_ID` do `npm run tauri build`, e o Rust a embute no executável com `option_env!` (`client_id_from_env` em `src-tauri/src/mal/mod.rs`). Ela não existe em arquivo nenhum do repositório.
2. Antes de compilar, o mesmo job confere que a variável existe e tem formato de Client ID; depois, que o valor está dentro do `prancha.exe`. Nenhuma das duas etapas mostra o valor. Sem ela, nenhuma versão sai.
3. O job **Client ID do MyAnimeList** do `ci.yml` pesquisa na API real (`GET /v2/manga?q=berserk&limit=1` com `X-MAL-CLIENT-ID`) a cada PR e push na main: uma recusa (401/403) mostra que a variável não corresponde ao cadastro. Ele fica fora da proteção da main, porque depende de um serviço de fora.

Sem o Client ID (num build local, por exemplo), a categoria **Integrações** diz que a integração não está configurada e nada é consultado. Em builds de depuração, `PRANCHA_MAL_CLIENT_ID` também vale como variável de ambiente na execução, para testar com `npm run tauri dev`.

## Conexão e tokens

- **OAuth 2.0 com código e PKCE** ([guia oficial](https://myanimelist.net/apiconfig/references/authorization)). O MAL só aceita o método `plain`, então o desafio é o próprio verificador: 96 caracteres aleatórios, novos a cada conexão, junto com um `state` aleatório que a resposta precisa trazer de volta.
- O Prancha abre `https://myanimelist.net/v1/oauth2/authorize` no navegador da pessoa e espera a volta num endereço local `http://localhost:8080/callback` (só em `127.0.0.1` e `::1`, nunca na rede), que só existe durante a conexão (até 5 minutos, ou até **Cancelar**). Respostas com outro `state` (proteção contra CSRF), em outro caminho, sem código ou com outro método são recusadas e não encerram a espera; um `error` com o `state` certo encerra como recusa. A página de volta não repete o código e pede ao navegador para não guardá-la nem enviar o endereço adiante. A senha nunca passa pelo Prancha.
- O código é trocado em `https://myanimelist.net/v1/oauth2/token` (`grant_type=authorization_code`, com o verificador e sem segredo do cliente). O nome da conta vem de `GET /v2/users/@me`.
- **Armazenamento:** os tokens de acesso e de renovação ficam só cifrados, na tabela `mal_account`, pela DPAPI do Windows (`CryptProtectData`, presa à conta do Windows, com entropia própria do Prancha). Outro usuário do computador ou uma cópia do banco em outra máquina não abre. A tabela não passa pelas configurações e nenhum comando devolve tokens à interface. Fora do Windows, sem um cofre do sistema, a conexão não é guardada.
- **Logs e erros:** o cabeçalho `Authorization` e os corpos com código ou token só existem no envio; o `Debug` das requisições os esconde, e as mensagens de erro dizem o que aconteceu sem repetir a requisição.
- **Renovação:** cinco minutos antes de vencer (o MAL informa em `expires_in`) ou depois de um 401, o token é renovado (`grant_type=refresh_token`) uma vez, e o novo token de renovação substitui o anterior. Se o MAL recusar a renovação, a conta fica como **Sessão expirada**: a fila espera, sem gastar tentativas, até a pessoa conectar de novo.
- **Desconectar** apaga os tokens e as atualizações que ainda não foram enviadas (eram para aquela conta). As associações ficam. O MAL não tem um endereço para revogar tokens; para tirar o acesso do lado dele, use as configurações da conta no site.

## Associação das obras

Ao abrir uma obra das fontes, com a conta conectada e nada decidido ainda, o Prancha procura uma vez:

1. Se a descrição da fonte traz o endereço `myanimelist.net/manga/<id>`, essa obra é usada.
2. Senão, pesquisa o título (sem o que está entre parênteses ou colchetes) em `GET /v2/manga?q=…` e dá uma nota a cada resultado:
   - a maior semelhança entre o título da fonte e **todos** os nomes da obra no MAL (original, inglês, japonês e sinônimos), depois de tirar acentos e pontuação;
   - romances (`novel`, `light_novel`) ficam de fora;
   - o autor: mais pontos se um sobrenome bate, menos se a obra do MAL tem outro autor;
   - os capítulos: a fonte com mais capítulos do que a obra tem no MAL tira pontos (one-shot ou outra obra com o mesmo nome).
3. Associa sozinho só com nota 88 ou mais e pelo menos 12 pontos à frente da segunda. Em qualquer outro caso, as melhores candidatas ficam guardadas por uma semana e a página da obra pede para a pessoa **Escolher**.

A associação fica na tabela `mal_links`, pelo endereço da obra na fonte, e vale mesmo se a obra sair da biblioteca e voltar. Na página da obra, **Corrigir** abre a pesquisa manual (por título, número ou endereço da obra no MAL) e **Não está no MyAnimeList** grava que não há correspondente: nada mais é procurado nem sincronizado para ela. Trocar a associação tira da fila o que era da obra errada.

## Sincronização

- **Só ao concluir um capítulo no leitor** (`chapter_reading_complete`, quando a leitura chega ao fim de verdade). Abrir a obra, abrir ou pré-carregar um capítulo, começar a ler e marcar como lido ou não lido à mão não enviam nada. Por segurança, um capítulo sem `completed_at` nunca entra na fila.
- O número enviado é o do capítulo (o campo de número da fonte, ou "Capítulo 12", "Ch. 12" no título). Capítulos como "10.5" contam 10. Sem número claro, nada é enviado: melhor não sincronizar que errar.
- **Fila persistente:** `mal_sync_queue`, uma linha por obra do MAL. Concluir outro capítulo só aumenta o alvo; repetir o mesmo, ou um anterior, não muda nada. O que já foi confirmado no MAL (`synced_chapters`) nem entra.
- Uma thread própria esvazia a fila em segundo plano, com 0,7 s entre chamadas, sem nunca segurar a leitura. Ela acorda com cada capítulo concluído, ao conectar, em **Tentar agora** e 15 segundos depois de abrir o app, retomando o que ficou pendente.
- **Antes de gravar, o progresso do MAL é consultado** (`GET /v2/manga/{id}?fields=num_chapters,my_list_status`):
  - se o MAL já tem o mesmo número ou mais, nada é enviado, e o número do MAL passa a ser a referência;
  - se a obra está **concluída** no MAL (e não sendo relida), nada é enviado;
  - o alvo nunca passa do total de capítulos do MAL;
  - senão, `PUT /v2/manga/{id}/my_list_status` com `num_chapters_read` e, só se a obra não estava na lista ou estava em **Planejo ler**, `status=reading`. Pausada ou abandonada continua como a pessoa deixou.
  - **Nunca** é enviado `status=completed`: terminar a obra é decisão da pessoa, mesmo que todos os capítulos da fonte tenham sido lidos.
- **Falhas:** sem rede, tempo esgotado, 403/429 (limite do MAL) ou 5xx: nova tentativa com espera crescente (1, 2, 4… minutos, até 1 hora), até 10 vezes. Depois disso, ou com uma recusa definitiva (404, 400), a atualização aparece como falha em **Configurações › Integrações** e na página da obra, com **Tentar agora** e **Descartar falhas**. Nada é perdido ao fechar o app.

## Endpoints usados

| Endpoint | Para quê | Autenticação |
|---|---|---|
| `GET https://myanimelist.net/v1/oauth2/authorize` | Autorização no navegador | — |
| `POST https://myanimelist.net/v1/oauth2/token` | Trocar o código e renovar tokens | `client_id` + PKCE |
| `GET /v2/users/@me` | Nome da conta | Bearer |
| `GET /v2/manga?q=&limit=10&fields=…&nsfw=true` | Pesquisa | `X-MAL-CLIENT-ID` |
| `GET /v2/manga/{id}?fields=…` | Detalhes da obra escolhida | `X-MAL-CLIENT-ID` |
| `GET /v2/manga/{id}?fields=num_chapters,my_list_status` | Progresso atual na lista | Bearer |
| `PUT /v2/manga/{id}/my_list_status` | Capítulos lidos (e "lendo") | Bearer |

A documentação descreve a atualização da lista como `PATCH`, e o exemplo dela usa `PUT`; o Prancha usa `PUT`, como os clientes conhecidos.

## Limitações

- Só obras das fontes de leitura. HQs importadas de arquivos não têm capítulos numerados e não são associadas.
- Marcar capítulos como lidos à mão (inclusive "marcar anteriores") não sincroniza: só a leitura concluída no leitor.
- A porta 8080 precisa estar livre durante a conexão (só nesses segundos). Se outro programa estiver nela, o Prancha avisa e pede para fechá-lo; usar outra porta exigiria mudar o cadastro no MAL.
- O login de verdade (navegador, conta e autorização) não roda no CI: os testes fazem a volta por uma requisição real ao endereço local, mas com o MAL simulado. A conferência final é conectar uma conta no app instalado.
