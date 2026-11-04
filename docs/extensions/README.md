# Extensões do Prancha

Extensões acrescentam fontes de leitura à área **Explorar**. Você lê capítulo por capítulo, sem baixar, e o que decidir baixar vira um CBZ na sua biblioteca e continua disponível sem internet.

Tudo isso é opcional e fica no plugin **Fontes de leitura**, na aba **Plugins**. Sem ele, o menu não tem Explorar, Downloads, Extensões nem Repositórios. Remover o plugin tira esses itens do menu, pausa os downloads e para o que estiver rodando. Biblioteca, extensões e progresso ficam guardados e voltam se você instalar de novo.

> **Extensões são código de terceiros.** As do Prancha rodam isoladas e só fazem o que foi declarado, mas o conteúdo que mostram é responsabilidade de quem as publica. As do [Mihon](COMPATIBILITY.md) rodam **sem isolamento**. Só adicione repositórios em que você confia. Veja o [modelo de segurança](SECURITY.md).

O Prancha não vem com nenhuma fonte de leitura instalada. A biblioteca local, o leitor e a importação de arquivos funcionam sem extensões e sem internet.

## Palavras usadas aqui

| Palavra | O que é |
|---|---|
| **Repositório** | Um endereço que publica extensões, com um catálogo assinado por uma chave. |
| **Extensão** | Um pacote instalado a partir de um repositório: WebAssembly nos repositórios do Prancha, Java nos do Mihon. É o código. |
| **Fonte** | Um lugar de leitura oferecido por uma extensão. Uma extensão pode ter várias, por exemplo uma por idioma. É o que aparece em Explorar. |
| **Obra** | Uma série encontrada em uma fonte, com seus capítulos. |

## Adicionar um repositório

1. Abra **Repositórios** e escolha **Adicionar**.
2. Informe o endereço, que pode ser o do `prancha-repo.json` ou a pasta onde ele está. O [link de um repositório do Mihon](COMPATIBILITY.md) também vale.
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

**Explorar** pesquisa em todas as fontes ao mesmo tempo e mostra cada obra com as fontes onde ela está, cada uma com a sua capa. Só títulos iguais ficam juntos, sem contar maiúsculas, acentos e pontuação. Título igual não prova que é a mesma obra, por isso vale conferir as capas, e a mesma obra pode ter outro título em outra fonte.

Cada fonte responde no seu tempo. Uma lenta não segura as outras, e a que falhar aparece no topo com o motivo e o que dá para fazer, sem contar como se a obra não estivesse lá. Com uma fonte só, e fora da pesquisa, os resultados ficam em grade, separados por fonte. Também dá para filtrar por fonte, idioma e tipo, ver populares e recentes, e carregar mais resultados.

## Filtro +18

Em **Plugins**, **Esconder conteúdo +18** começa ligado. Enquanto estiver, obras, fontes e extensões adultas ou sem classificação não aparecem em tela nenhuma, e as extensões assim não instalam. Para desligar, você confirma ter 18 anos, e pode ligar de novo quando quiser.

Nas fontes do Keiyoushi, a marca vem da fonte inteira, e cada obra só é reconhecida como +18 pelos gêneros (adulto, hentai, ecchi e parecidos). Como a lista nem sempre traz os gêneros, o Prancha consulta os detalhes da obra antes de mostrá-la e guarda a resposta. A primeira visita a uma lista demora mais, e num site lento pode passar de um minuto. As seguintes não consultam de novo. A tela diz o que ficou de fora, sem dizer os títulos:

- Com gêneros adultos: só aparece com o filtro desligado.
- Sem gêneros, porque a fonte não informa: só aparece com o filtro desligado. Uma fonte que nunca informa gêneros fica vazia com o filtro ligado, e o Explorar avisa.
- Sem resposta, porque o site falhou: fica de fora nos dois casos, e **Tentar de novo** confere outra vez.

Título ou gênero que indique sexualização de menores nunca aparece, com o filtro ligado ou desligado. A obra sai do aplicativo com a capa e não é consultada de novo. Capítulos que já estavam na biblioteca continuam lá, como qualquer arquivo.

Nada disso é garantia. A classificação é a declarada por terceiros e pode estar errada, e o Prancha lê o título e os gêneros, não as imagens nem a sinopse: uma obra com gêneros comuns e capa sexualizada ainda aparece, e a lista de palavras não reconhece tudo.

## Obras na biblioteca

Na página da obra, **Favoritar** e **Coleções** fazem o mesmo que nas HQs. A obra que você favoritou, guardou em uma coleção, acompanha ou já leu aparece no Início, em Minha biblioteca e nas telas de Continuar lendo, Recentes e Favoritos, mostrando onde você parou. O menu dela marca como concluída ou não lida e remove da biblioteca, o que apaga o progresso, o favorito e as coleções da obra. Capítulos baixados continuam lá.

Sem o plugin, essas obras saem das telas e voltam quando ele é instalado de novo, com tudo no lugar.

## Sites com verificação anti-robô

Quando um site pede uma verificação, o erro vem com o botão **Verificar no site**, e [o resto está aqui](COMPATIBILITY.md#sites-com-verificação-anti-robô). Se o site liberar e continuar recusando, o botão some até você abrir o Prancha de novo, porque repetir não resolve. **Procurar em outras fontes**, no próprio aviso e no menu da obra, faz a mesma pesquisa nas demais.

## Ler sem baixar

Todo capítulo tem **Ler**. As páginas vêm da fonte conforme você avança, com as mesmas regras de rede dos downloads, e nada vai para a biblioteca nem para a lista de Downloads. A página da obra oferece **Começar a ler** ou **Continuar**, e cada capítulo mostra onde você parou ou **Lido**.

Os capítulos formam uma leitura só. Quando você se aproxima do fim de um, o seguinte já é pedido à fonte, e depois da última página vem a primeira dele, rolando ou virando a página, sem botão. Uma barrinha marca o ponto em que ele começa, e o título no alto acompanha. Os botões « e » pulam de capítulo. Rolar para cima só atravessa os capítulos que já entraram na leitura: para voltar a um anterior ao que você abriu, use «.

Obras em tiras abrem no modo **Webtoon**: uma coluna estreita e contínua, sem espaço entre as imagens. O Prancha reconhece essas obras pelo formato das páginas, pelos gêneros e por capítulos com muitas páginas. É uma estimativa: se errar, troque o modo no leitor, e a escolha fica guardada para a obra. A Leitura Guiada só aparece em capítulos baixados, porque precisa do arquivo na biblioteca.

## Baixar capítulos

Baixar só é preciso para ler sem internet. Na página da obra, baixe um capítulo, os selecionados ou todos. Os estados de um capítulo são:

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
