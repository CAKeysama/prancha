# Extensões do Prancha

Extensões acrescentam fontes de leitura à área **Explorar**. O que você baixa vira um CBZ na sua biblioteca, abre no leitor de sempre e continua disponível sem internet.

> **Extensões são código de terceiros.** Elas rodam isoladas e só fazem o que foi declarado, mas o conteúdo que mostram é responsabilidade de quem as publica. Só adicione repositórios em que você confia. Veja o [modelo de segurança](SECURITY.md).

O Prancha não vem com nenhuma fonte de leitura instalada. A biblioteca local, o leitor e a importação de arquivos funcionam sem extensões e sem internet.

## Palavras usadas aqui

| Palavra | O que é |
|---|---|
| **Repositório** | Um endereço que publica extensões, com um catálogo assinado por uma chave. |
| **Extensão** | Um pacote WebAssembly instalado a partir de um repositório. É o código. |
| **Fonte** | Um lugar de leitura oferecido por uma extensão. Uma extensão pode ter várias, por exemplo uma por idioma. É o que aparece em Explorar. |
| **Obra** | Uma série encontrada em uma fonte, com seus capítulos. |

## Adicionar um repositório

1. Abra **Repositórios** e escolha **Adicionar**.
2. Informe o endereço, que pode ser o do `prancha-repo.json` ou a pasta onde ele está.
3. Confira o que aparece: nome, endereço, site, quantidade de extensões, chave pública e impressão digital.
4. Se confiar, escolha **Confiar e adicionar**. Nada fica gravado antes disso.

A impressão digital é o que identifica o repositório. Para ter certeza de que ela é a certa, compare com a que o autor publica em um canal próprio dele.

Se a chave de um repositório mudar, a sincronização para, o último catálogo válido continua na tela e o Prancha pede uma nova confirmação. Nada troca sozinho.

Remover um repositório tira o catálogo dele da lista. As extensões já instaladas continuam funcionando, mas não recebem atualizações, e nada da biblioteca é apagado.

## Instalar, atualizar e remover extensões

Em **Extensões**, a aba **Disponíveis** mostra o catálogo dos seus repositórios, com pesquisa, filtro por idioma e ordenação. Abra uma extensão para ver as permissões dela antes de instalar:

- **Rede:** os únicos endereços que ela pode acessar.
- **Armazenamento próprio:** um espaço pequeno só dela, e nenhum outro dado do aplicativo.

Na instalação, o Prancha confere o tamanho, o SHA-256 e a assinatura do pacote, e depois carrega o componente para ver se as fontes batem com o que foi declarado. Se algo falhar, nada é instalado.

Em **Instaladas** você vê as fontes de cada extensão, atualiza quando há versão nova, desabilita ou remove. Se a atualização falhar, a versão anterior continua no lugar. Remover uma extensão tira as fontes dela de Explorar, e os capítulos baixados continuam na biblioteca.

## Pesquisar e navegar

**Explorar** pesquisa em todas as fontes ao mesmo tempo. Os resultados ficam separados por fonte, e cada uma responde no seu tempo: uma lenta ou fora do ar mostra o próprio erro sem atrapalhar as outras. Também dá para filtrar por fonte, idioma e tipo, ver populares e recentes, e carregar mais resultados.

Conteúdo adulto não aparece. Uma fonte adulta ou sem classificação fica desabilitada, e não há opção para mudar isso.

## Baixar capítulos

Na página da obra, baixe um capítulo, os selecionados ou todos. Os estados de um capítulo são:

| Estado | O que quer dizer |
|---|---|
| Disponível online | Só existe na fonte. |
| Baixado | O arquivo existe, mas não está mais na biblioteca. |
| Na biblioteca | Está na biblioteca e abre sem internet. |

**Downloads** mostra o progresso, a velocidade e os erros, com pausa, retomada, cancelamento e nova tentativa. Pausar guarda as páginas já baixadas. Se o download falhar ou for cancelado, os temporários são apagados e a nova tentativa começa do zero. Fechar o app no meio de um download coloca o capítulo de volta na fila na próxima vez.

## Biblioteca online

Não existe uma segunda biblioteca. Um capítulo baixado é um CBZ comum, na pasta de dados do Prancha, importado como qualquer outro arquivo: tem capa, progresso, coleções e leitura pelo mesmo leitor. Se você baixar um capítulo cujo conteúdo já está na biblioteca, o Prancha aponta para o item que já existe e não duplica o arquivo.

**Acompanhar** uma obra guarda ela em Explorar, na lista **Acompanhando**, mesmo sem baixar nada. Com a fonte fora do ar, a página da obra mostra o último conhecido, e os capítulos da biblioteca abrem normalmente.

## Repositório de demonstração

O projeto publica um repositório próprio em `extensions/demo-repo`, com a extensão **Acervo de demonstração** e obras originais criadas só para testar o fluxo. As imagens são formas geométricas geradas por [`scripts/build-demo-repo.mjs`](../../scripts/build-demo-repo.mjs). Não é uma fonte comercial nem uma integração com um site de terceiros.

O endereço público é `https://raw.githubusercontent.com/CAKeysama/prancha/main/extensions/demo-repo/prancha-repo.json`, e a impressão digital da chave é:

```
d21e 9bfc dcbb 02e8 14ff d812 dd5e 094a 22db 3660 182a d227 0b52 2c12 b64f 8cd5
```

## Para quem desenvolve

- [Desenvolver uma extensão](DEVELOPMENT.md)
- [Contrato da API](EXTENSION_API.md)
- [Publicar um repositório](PUBLISHING.md)
- [Formato dos repositórios](REPOSITORY_FORMAT.md)
- [Modelo de segurança](SECURITY.md)
- [Compatibilidade com o Mihon](COMPATIBILITY.md)
