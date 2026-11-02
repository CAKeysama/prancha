# Modelo de segurança

Extensões são código de terceiros. O objetivo é que uma extensão ruim, ou um repositório comprometido, não consiga passar do que foi declarado e aceito, e que o pior resultado seja uma fonte que não funciona.

## Em que você confia

Ao confiar em um repositório, você aceita que quem tem a chave privada pode publicar extensões para você instalar. Isso **não** dá acesso aos seus arquivos ou à sua biblioteca. Mas a extensão escolhe o que aparece na fonte dela: o Prancha não confere o conteúdo das imagens, nem se a pessoa tem o direito de distribuí-las.

## O que é verificado

| Momento | Verificação |
|---|---|
| Adicionar um repositório | Manifesto válido e chave forte. A impressão digital é mostrada, e só a que foi revisada é aceita: se o servidor trocar a chave entre a revisão e a confirmação, a adição é recusada. |
| Sincronizar | Assinatura do índice, identificador do repositório, formato de cada entrada e data, para impedir a volta a um índice antigo. Qualquer falha mantém o último catálogo válido. |
| Mudança de chave | A sincronização para, o aviso aparece e a chave só muda com uma nova confirmação. |
| Instalar | Tamanho, SHA-256 e assinatura do pacote, que prende o pacote a esse repositório, a essa extensão e a essa versão. Depois, o componente precisa carregar e declarar as mesmas fontes e classificações do índice. |
| Carregar | Na primeira carga de cada sessão, o SHA-256 do pacote gravado em disco é conferido de novo. Um arquivo alterado não carrega. |

Assinaturas inválidas não têm opção de "ignorar".

## Isolamento

- **WebAssembly sem WASI.** O componente só enxerga as funções do host listadas na [API](EXTENSION_API.md). Sem o import, não há como pedir arquivo, relógio, ambiente ou socket. Um componente que importe outra coisa não carrega.
- **Instância nova por chamada.** Nada fica guardado entre chamadas, exceto o armazenamento próprio.
- **Limites de CPU, memória, pilha e tempo**, com os valores da [API](EXTENSION_API.md#limites-de-execução). Um laço infinito, um pânico ou uma explosão de memória viram erro só naquela chamada.
- **Concorrência limitada.** Uma extensão lenta ocupa no máximo 2 das 4 vagas e não passa à frente das outras.
- **Dados privados.** Extensões não leem a biblioteca, as configurações, o progresso nem os dados de outras extensões.
- **Interface.** A extensão não entrega HTML nem script: só dados, tratados como texto. As capas passam pelo backend e são entregues pelo protocolo do app, e a CSP continua sem endereços externos.
- **Arquivos.** A extensão nunca escolhe um caminho. Os nomes dos arquivos gerados são limpos pelo Prancha e terminam em um hash, e o CBZ é conferido antes de entrar na biblioteca.

## Rede

Toda requisição passa pelo módulo `src-tauri/src/extensions/net.rs`, inclusive as de repositórios, capas e páginas.

- Só HTTPS na porta 443, sem proxy e sem credenciais no endereço.
- Extensões só alcançam os hosts de `allowedHosts`, e o mesmo vale para as URLs de capas e páginas que elas devolvem.
- O DNS é resolvido por esse módulo: um nome que aponte para a rede local não chega a conectar. Isso cobre loopback, redes privadas, link-local (incluindo endereços de metadados de nuvem), multicast e os IPv6 equivalentes, inclusive IPv4 mapeado.
- Cada redirecionamento é conferido de novo, com o mesmo limite de hosts.
- Downloads de páginas recusam tipos de conteúdo que uma fonte de leitura não teria motivo para enviar.

A exceção de loopback usada nos testes existe em duas formas: `Net::new(true)` nos testes de integração, e `PRANCHA_E2E_ALLOW_LOOPBACK` em build de depuração. No build de release, a variável não tem efeito.

## O que nunca é permitido

Conteúdo adulto, execução de comandos, acesso irrestrito a arquivos ou à rede, execução de APKs, contorno de DRM, paywall, CAPTCHA ou login. Nenhuma configuração muda isso.

## Atualizações

Atualizar é pedir uma nova instalação da versão do catálogo, com as mesmas verificações, e só quando você escolhe. Se uma versão nova pedir hosts ou permissões a mais, elas aparecem no detalhe da extensão antes de atualizar. Se algo falhar, a versão anterior continua.

## Remoção

Remover uma extensão apaga o pacote e o armazenamento próprio dela. Os capítulos baixados são da biblioteca e ficam. Remover um repositório não remove as extensões instaladas: elas guardam a procedência (nome, chave e impressão digital) e continuam funcionando, sem atualizações.

## Como verificar um repositório

```bash
prancha-ext verify <pasta do repositório>
```

Esse comando confere a assinatura do índice e o SHA-256 e a assinatura de cada pacote, e mostra a impressão digital. Compare esse valor com o que o autor publica em um canal próprio.

## Limites conhecidos

- A confiança é na primeira confirmação: se a chave que você aceitou já for a de um atacante, o Prancha não tem como saber. Por isso a impressão digital aparece, e vale compará-la.
- Não há revogação central. Um repositório comprometido é tratado removendo o repositório e as extensões dele.
- A compilação de um componente grande não pode ser interrompida no meio. O prazo vale a partir da execução. O tamanho do pacote é limitado a 32 MB.
- O conteúdo das imagens não é analisado. A classificação é a declarada por quem publica.

## Testes

A suíte `src-tauri/tests/extensions.rs` usa uma extensão de teste real (`extensions/test-fixture`) e servidores locais para cobrir: laço infinito, memória, pilha, pânico, importação de arquivo, ambiente, rede sem capacidade, host não autorizado, redirecionamento para endereço bloqueado, índice e pacote adulterados, chave trocada, índice antigo e pacotes trocados.

Para informar um problema de segurança, abra uma issue em <https://github.com/CAKeysama/prancha/issues>.
