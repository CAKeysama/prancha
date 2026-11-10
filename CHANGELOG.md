# Histórico de versões

Tudo o que mudou em cada versão do Prancha. O projeto segue o [versionamento semântico](https://semver.org/lang/pt-BR/),
e as notas das releases e a [página de downloads](https://cakeysama.github.io/prancha/) são geradas a partir deste arquivo.

## [0.11.3] - 2026-11-10

O botão de baixar um capítulo vira o de cancelar no mesmo clique.

### Correções

- Ao baixar um capítulo pela página da obra, o botão vira **Cancelar download** na hora, sem esperar a fila responder. Cancelar nesse instante também funciona: o download é cancelado assim que entra na fila.

## [0.11.2] - 2026-11-09

O login do MyAnimeList passa a usar uma porta que raramente está ocupada.

### Correções

- A volta do login do MyAnimeList usava a porta 8080, comum em outros programas: com ela ocupada, não dava para conectar. Agora a volta é em `http://localhost:3641/callback`.

## [0.11.1] - 2026-11-09

A conexão com o MyAnimeList passa a funcionar: o Prancha espera a volta do login no endereço cadastrado no aplicativo do MAL.

### Melhorias

- Depois de autorizar no navegador, a janela do Prancha volta para a frente, já com a conta conectada.
- A publicação confere o identificador do aplicativo no MyAnimeList antes de compilar e dentro do executável, e as verificações o testam contra a API real do MAL: nenhuma versão sai com a integração escondida.

### Correções

- O login do MyAnimeList voltava para um endereço diferente do cadastrado, e o MAL recusaria a autorização. Agora a volta é em `http://localhost:8080/callback`, só neste computador e só enquanto a conexão está em andamento.

## [0.11.0] - 2026-11-09

Integração com o MyAnimeList: conecte a conta e o Prancha associa as obras das fontes ao catálogo do MAL e atualiza os capítulos lidos quando você termina um capítulo. O menu do botão direito do navegador saiu do app.

### Novidades

- Configurações › Integrações: conecte e desconecte a conta do MyAnimeList pelo site do MAL, sem digitar a senha no Prancha, e veja o estado da conta e da sincronização.
- Ao abrir uma obra das fontes, o Prancha procura a obra correspondente no MAL pelos títulos alternativos, pelo tipo, pelo autor e pela quantidade de capítulos, e associa sozinho só quando não há dúvida.
- Na página da obra, a linha MyAnimeList mostra a obra associada e a sincronização, e Corrigir abre a pesquisa para escolher outra ou marcar que a obra não está no MAL.
- Concluir um capítulo no leitor atualiza os capítulos lidos no MAL em segundo plano, sem segurar a leitura.

### Melhorias

- A sincronização nunca diminui um progresso mais adiantado no MAL, não marca a obra como concluída e não envia nada ao abrir uma obra ou um capítulo.
- Atualizações que falham ficam numa fila guardada no computador e são tentadas de novo com espera crescente, inclusive depois de fechar e abrir o app.
- O acesso ao MAL fica cifrado pelo Windows e é renovado sozinho; se expirar, o app avisa e nada pendente se perde.
- O menu do botão direito do navegador (Voltar, Atualizar, Imprimir…) não aparece mais em nenhuma tela; nos campos de texto, o menu de copiar e colar continua.

## [0.10.0] - 2026-11-09

O projeto ganhou uma página de visão geral, com as versões publicadas, os downloads dos instaladores e as verificações automáticas, tudo com dados públicos do GitHub.

### Novidades

- **Visão geral do projeto:** a nova página **Visão geral** do site oficial (`prancha-beta.vercel.app/visao-geral`) mostra a versão atual, o histórico de versões com notas e instaladores, os downloads por versão e como andam as verificações automáticas: quantas passam, quanto tempo levam e as execuções recentes.
- **Configurações › Geral:** a opção **Visão geral do projeto** abre essa página no navegador.

### Observações

- Os números vêm só do GitHub e são atualizados a cada 15 minutos. A página não usa a telemetria nem nenhum dado de quem usa o Prancha, e o app não faz nenhuma consulta nova para mostrá-la.
- Downloads contam os instaladores baixados, inclusive pelas atualizações automáticas. Não são o número de pessoas que usam o Prancha.

## [0.9.0] - 2026-11-09

Estatísticas de uso opcionais: com a sua permissão, o Prancha passa a contar quais versões e fontes estão em uso e onde ele falha, sem nada do que você lê. Começam desligadas.

### Novidades

- **Configurações › Privacidade:** liga e desliga as estatísticas de uso, mostra o que é enviado e o que nunca é, a data da sua escolha, o identificador da instalação, **Gerar outro** e **Apagar dados**, que pede ao servidor que apague o que já foi enviado.
- **Convite:** depois do tutorial, uma única vez e nunca sobre o leitor, o Prancha pergunta se você quer ajudar. **Agora não** vale como resposta e não pergunta de novo.
- **Aviso de privacidade** novo, em [docs/PRIVACIDADE.md](https://github.com/CAKeysama/prancha/blob/main/docs/PRIVACIDADE.md).

### Observações

- Só sai algo depois que você aceita. Vão somas por dia: versão, sistema, aberturas do app, uso de cada fonte pelo identificador técnico, códigos de falha e tempos de carregamento. Títulos, buscas, páginas, caminhos, mensagens de erro e dados pessoais nunca saem.
- Desligar para os envios na hora e descarta o que ainda não tinha saído. Sem internet ou com o servidor fora do ar, nada muda no uso do app.

## [0.8.0] - 2026-11-09

Navegar ficou mais rápido: as telas guardam o que já carregaram, e a guia Explorar só consulta as fontes quando a lista venceu ou quando você pede.

### Melhorias

- **Explorar:** sair e voltar mostra os resultados na hora, sem consultar as fontes de novo. Depois de 15 minutos, a lista é atualizada em segundo plano e a anterior fica na tela até a nova chegar. O botão **Atualizar** consulta tudo de novo quando você quiser.
- **Explorar:** um download que termina, ou outra mudança na biblioteca, não recomeça mais a guia inteira.
- **Explorar:** se uma atualização falhar, a lista anterior continua, com um aviso discreto. Uma fonte fora do ar não é consultada a cada volta, só depois de um minuto ou pelo **Tentar de novo**.
- **Obra:** uma obra aberta há menos de 5 minutos volta direto, com o que está gravado, sem consultar a fonte outra vez.
- **Em todo o app:** Início, biblioteca, coleções, extensões, repositórios e notificações mostram na hora o que já tinham e conferem em segundo plano. A posição da rolagem, a pesquisa e os filtros continuam ao sair e voltar.

### Observações

- O que fica guardado vale enquanto o Prancha está aberto. As obras continuam gravadas no banco do aplicativo, como antes; a primeira visita depois de abrir o Prancha consulta as fontes normalmente.

## [0.7.3] - 2026-11-09

Outra versão de bastidores: a página de downloads não fica mais para trás quando duas publicações chegam juntas.

### Correções

- A página de downloads espera a publicação anterior terminar e tenta de novo, em vez de falhar quando duas versões saem uma logo depois da outra.

## [0.7.2] - 2026-11-09

Uma versão de bastidores: mudanças que entram uma logo depois da outra agora saem cada uma como a sua versão, sem uma cancelar a outra.

### Correções

- Um aviso que ainda ia sumir da tela não tenta mais se atualizar depois que a janela foi fechada.

### Observações

- Nada muda no uso do aplicativo. A 0.7.1 tinha ficado sem publicar porque a fila de publicação descartou ela enquanto a 0.7.0 terminava. Agora cada versão tem a sua própria fila, as publicações podem correr ao mesmo tempo, e a atualização oferecida aos aplicativos instalados é sempre a maior versão publicada.

## [0.7.1] - 2026-11-09

O filtro +18 saiu do Prancha: ele decidia de forma ambígua o que aparecia, e agora todas as obras, fontes e extensões aparecem.

### Melhorias

- **Sem filtro +18:** a opção **Esconder conteúdo +18** saiu de Plugins, junto com a escolha das obras suspeitas, o selo na capa e o **Ver motivo ou corrigir** na página da obra. Explorar, biblioteca, coleções, catálogo de extensões e notificações mostram tudo, e as notificações do sistema voltam a dizer o nome da obra.
- **Recomendados:** a demonstração do Prancha saiu da lista de repositórios recomendados. Ela continua disponível pelo endereço, no [guia de extensões](https://github.com/CAKeysama/prancha/blob/main/docs/extensions/README.md#repositório-de-demonstração).

### Observações

- Obras com título ou gênero que indique sexualização de menores continuam bloqueadas, como antes.
- Ao atualizar, o Prancha apaga a classificação +18 guardada, as correções feitas à mão e as preferências do filtro. Nada da biblioteca é apagado.

## [0.7.0] - 2026-11-09

Agora dá para ir mais fundo no catálogo do Explorar: cada fonte mostra 10 obras por vez, e o botão "Mostrar mais" traz as próximas 10.

### Novidades

- **Mostrar mais no Explorar:** em Populares, Recentes e nas pesquisas, cada fonte começa com 10 obras e o botão "Mostrar mais" traz mais 10 por clique, sem repetir obra. Na pesquisa em várias fontes, a leva é de 10 títulos. O Prancha só pede outra página à fonte quando falta obra para completar a leva.

## [0.6.2] - 2026-11-09

Uma versão de bastidores: as verificações e a publicação de cada versão ficaram bem mais rápidas, para as correções e novidades chegarem antes.

### Observações

- Nada muda no aplicativo. As verificações automáticas de cada mudança passaram de 7 a 19 minutos para cerca de 4 minutos, sem deixar nenhum teste de fora, e os instaladores passam a ser gerados ao mesmo tempo que as verificações.

## [0.6.1] - 2026-11-09

Uma versão de organização: daqui em diante, toda mudança no Prancha sai como versão nova, com o número que ela pede.

### Observações

- Nada muda no aplicativo. A partir desta versão, cada correção, melhoria ou novidade chega como atualização assim que fica pronta, numerada pelo [versionamento semântico](https://semver.org/lang/pt-BR/): novidades sobem o número do meio, correções sobem o último.

## [0.6.0] - 2026-11-09

Agora o Prancha avisa o que mudou e lembra onde você está em cada capítulo. Há uma central de notificações, cada capítulo mostra se foi lido, está em andamento ou é novo, e o filtro +18 explica o que decidiu e aceita correção.

### Novidades

- **Central de notificações:** um sino no menu lateral mostra as recentes, e a tela Notificações guarda o histórico, com filtros e seleção de várias. Avisa capítulos novos de obras acompanhadas, fontes que falharam ao sincronizar, versão nova do Prancha, downloads concluídos ou com falha, importações com arquivos recusados e HQs com arquivo ausente.
- **Notificações do Windows:** os avisos importantes também aparecem no sistema quando o Prancha está em segundo plano, e clicar neles leva direto ao que mudou. Em Configurações, na categoria Notificações, você escolhe os tipos, desliga os avisos do sistema ou silencia tudo.
- **Progresso por capítulo:** cada capítulo fica Não lido, Lendo, Em andamento ou Lido, sozinho, conforme você lê. Chegar ao fim da última página conclui o capítulo, e sair no meio guarda onde parou.
- **Capítulos novos:** obras acompanhadas mostram quantos capítulos novos chegaram, na biblioteca e em Acompanhando, e cada um tem a marca Novo. O botão **Procurar capítulos novos** em Acompanhando confere todas de uma vez.
- **Classificação +18 com motivo:** cada obra pode ficar como +18, suspeita, livre ou sem informação suficiente, com um selo discreto na capa. Na obra, **Ver motivo ou corrigir** mostra por que ela foi classificada assim e deixa marcar como +18, como livre ou voltar ao automático.
- **Obras suspeitas:** em Plugins, você escolhe se as obras suspeitas ficam escondidas, aparecem com marca ou aparecem normalmente, separado do filtro das confirmadas.

### Melhorias

- Na lista de capítulos, ícones marcam como lido, como não lido ou todos os anteriores como lidos. A dica de cada ícone aparece depois de 2 segundos, com o mouse ou pelo teclado.
- A biblioteca mostra o capítulo e o estado, como "Capítulo 42 · Em andamento", e clicar abre o capítulo no ponto salvo.
- A estrela de Favoritar e o sino de Acompanhar ficam amarelos quando ligados.

### Correções

- **Filtro +18:** gêneros como "Young Adult", "Ecchi" e "Mature" faziam obras livres sumirem como +18, e uma obra marcada como adulta nunca voltava. Agora o título, os títulos alternativos e a descrição também contam, e uma obra sem informação suficiente não passa como livre.

### Observações

- O banco de dados se atualiza sozinho na primeira abertura, e a biblioteca e o progresso são mantidos. O progresso que já existia vira o estado do capítulo, e nenhum capítulo antigo aparece como novo.
- Os capítulos novos chegam ao abrir a obra ou pelo botão em Acompanhando: o Prancha ainda não procura sozinho em segundo plano. A primeira sincronização de uma obra serve de referência e não gera aviso.
- Na primeira abertura, as obras já guardadas são reclassificadas aos poucos, sem travar o app. Até lá, elas seguem a regra do filtro anterior.
- O filtro +18 lê o título, os gêneros e a descrição, não as imagens. A classificação continua sendo a declarada por terceiros e não é garantia.
- As notificações do sistema nunca mostram o nome de uma obra +18.
- Requer Windows 10 ou 11 de 64 bits. Os instaladores não têm assinatura digital, então o Windows SmartScreen pode pedir confirmação.

## [0.5.0] - 2026-11-08

Ficou mais fácil saber onde ler uma obra e achar uma opção. A pesquisa mostra em quais fontes cada obra está, o filtro +18 confere as obras antes de mostrar, e as configurações ganharam categorias e pesquisa.

### Novidades

- **Em quais fontes a obra está:** pesquisando em mais de uma fonte, cada obra aparece uma vez, com as fontes onde ela está, cada uma com a sua capa. Só títulos iguais ficam juntos, e a fonte que falhou aparece como "não respondeu", sem contar como se a obra não estivesse lá.
- **Repositórios recomendados:** Repositórios mostra uma lista curta mantida pelo projeto, só com o que o Prancha adiciona e usa: a demonstração e o Keiyoushi. Adicionar um deles mostra a chave antes de você confiar, como um endereço digitado.
- **Pesquisa nas configurações:** procure pelo que a opção faz, como tema, idioma, atualização ou atalhos, e vá direto até ela.
- **Sugerir melhoria:** em Configurações, na categoria Geral, escreva a ideia no próprio app. O Prancha abre o formulário do GitHub com o texto preenchido, e você confere e envia por lá.

### Melhorias

- As configurações agora são separadas por categoria: Aparência, Leitura, Biblioteca, Armazenamento, Atualizações e Geral.
- Downloads saiu do grupo principal do menu e fica embaixo, antes de Plugins.
- O aviso de verificação anti-robô tem uma mensagem só. Se o site liberar e continuar recusando, **Procurar em outras fontes** vira o caminho principal.

### Correções

- **Filtro +18 nas listas:** nas fontes do Keiyoushi, obras e capas adultas podiam aparecer em populares, recentes e na pesquisa, porque a lista vem sem gêneros. Agora o Prancha consulta os detalhes da obra antes de mostrar e guarda a resposta.
- Nas configurações, a janela inteira rolava e a barra lateral saía do lugar.
- A tela vazia de Repositórios tinha dois botões Adicionar.

### Observações

- Com o filtro +18 ligado, a primeira visita a uma lista do Keiyoushi demora mais, porque cada obra é conferida, e num site lento pode passar de um minuto. As seguintes usam a resposta guardada. Uma fonte que nunca informa gêneros fica vazia com o filtro ligado.
- Título ou gênero que indique sexualização de menores nunca aparece, com o filtro ligado ou desligado. Nada disso é garantia: a classificação é a declarada por terceiros, e o Prancha lê o título e os gêneros, não as imagens.
- Para enviar uma sugestão, é preciso ter uma conta no GitHub. Nada é enviado antes de você confirmar na página.
- O banco de dados se atualiza sozinho na primeira abertura, e a biblioteca e o progresso são mantidos.
- Requer Windows 10 ou 11 de 64 bits. Os instaladores não têm assinatura digital, então o Windows SmartScreen pode pedir confirmação.

## [0.4.0] - 2026-11-03

Agora é clicar e ler, de um capítulo para o outro, sem baixar nada e sem botão de próximo. As obras das fontes entram na biblioteca como as HQs, as fontes viraram um plugin opcional, e obras em tiras têm um modo próprio.

### Novidades

- **Plugins:** uma aba nova, onde você instala as fontes de leitura. Sem o plugin, o menu fica só com o leitor e a biblioteca. Quem já usava extensões continua com ele instalado.
- **Ler sem baixar:** todo capítulo tem o botão Ler. Nada vai para a biblioteca nem para os Downloads, e a obra guarda onde você parou.
- **Um capítulo emenda no outro:** no fim do capítulo, é só continuar rolando ou virando a página. Uma barrinha marca onde começa o seguinte.
- **Modo webtoon:** coluna estreita e contínua, sem espaço entre as imagens. O Prancha reconhece obras em tiras e continua assim nos capítulos seguintes. O modo também pode ser escolhido nas configurações do leitor.
- **Obras na biblioteca:** favorite um mangá, manhwa ou webtoon, ou guarde em uma coleção. Ele aparece no Início, em Minha biblioteca, em Favoritos e em Continuar lendo, mostrando onde você parou, com o mesmo menu das HQs.
- **Repositórios do Keiyoushi:** cole o link em Repositórios e instale as extensões. Elas rodam pelo Suwayomi-Server, baixado na primeira instalação.
- **Filtro +18:** fica em Plugins e começa ligado. Com ele, obras, fontes e extensões adultas ou sem classificação não aparecem em tela nenhuma, e as obras do Keiyoushi são conferidas pelos gêneros que a fonte informa. Para desligar, você confirma ter 18 anos.
- **Sites com verificação anti-robô:** o botão Verificar no site abre uma janela do próprio site. Você passa pela verificação, a janela fecha, e o Prancha tenta de novo.

### Melhorias

- Na rolagem vertical, as setas e o espaço já funcionam sem clicar na página antes, e chegar ao fim da rolagem conta como ter lido a última página.

### Observações

- As extensões do Keiyoushi rodam **sem o isolamento do Prancha**: têm acesso aos seus arquivos e à internet, como qualquer programa, e a assinatura de cada pacote não é conferida. O Prancha avisa antes de adicionar o repositório e antes de instalar. Só instale as de quem você confia.
- A primeira instalação de uma dessas extensões baixa 343 MB e ocupa 229 MB no disco. Depois do primeiro uso, são cerca de 500 MB de memória enquanto o Prancha estiver aberto. Remover o plugin para o programa, mas mantém os arquivos.
- Fontes que dependem de um navegador embutido ou de login mostram um erro.
- Quem passa pela verificação anti-robô é você: o Prancha não responde por ninguém. O site pode recusar mesmo assim, e a passagem reinicia o servidor das fontes, então um download em andamento pode falhar e pedir nova tentativa.
- A classificação +18 é a que cada repositório informa, e pode falhar. O que mistura conteúdo ou não informa conta como +18: com o filtro ligado, do Keiyoushi aparecem 564 das 1448 extensões em 2026-11-02. Numa fonte marcada como segura, a obra só é reconhecida como +18 pelos gêneros, que nem sempre vêm na lista: ela pode aparecer em Explorar até ser aberta. A confirmação de idade é só uma declaração. Capítulos já baixados de uma fonte +18 continuam na biblioteca, como qualquer arquivo.
- Só o formato do Keiyoushi, com `repo.json` e `index.json`, é aceito. Outros repositórios do Mihon continuam recebendo a mensagem de incompatibilidade.
- Reconhecer uma obra em tiras é uma estimativa. Se errar, troque o modo no leitor, e a escolha fica guardada para a obra.
- Quem está na 0.1.x precisa instalar esta versão por cima uma vez. Da 0.2.0 em diante, o Prancha avisa ao abrir. A biblioteca e o progresso são mantidos, e o banco de dados se atualiza sozinho na primeira abertura.
- Requer Windows 10 ou 11 de 64 bits. Os instaladores não têm assinatura digital, então o Windows SmartScreen pode pedir confirmação.

## [0.3.0] - 2026-11-02

Chegam as extensões: você adiciona os repositórios em que confia, pesquisa em Explorar e baixa capítulos direto para a biblioteca.

### Novidades

- **Repositórios:** adicione um repositório pelo endereço e confira a chave antes de confiar. O catálogo é assinado, e uma troca de chave nunca é aceita sem a sua confirmação.
- **Extensões:** veja as permissões antes de instalar. Cada extensão roda em WebAssembly isolado e só acessa os endereços que declarou, sem tocar nos seus arquivos nem nos outros dados do aplicativo.
- **Explorar:** pesquise em todas as fontes ao mesmo tempo e veja populares e recentes. Uma fonte lenta ou fora do ar mostra o próprio erro sem atrapalhar as outras.
- **Downloads:** baixe um capítulo, os selecionados ou todos, com pausa, retomada, cancelamento e nova tentativa.
- **Na mesma biblioteca:** o capítulo baixado vira um CBZ comum, com capa, progresso e coleções, e abre no leitor de sempre, mesmo sem internet.
- **Acompanhar:** guarde uma obra em Explorar sem baixar nada. Com a fonte fora do ar, a página da obra mostra o último conhecido.

### Correções

- Ao trocar o arquivo de um PDF, os Detalhes não mostram mais 0 páginas enquanto o número é desconhecido: o campo fica vazio.
- O menu das configurações agora acompanha a rolagem. Antes, o destaque só mudava ao clicar nele.

### Observações

- O Prancha não vem com nenhuma fonte de leitura. Por enquanto, o projeto só publica a extensão de demonstração, com obras originais criadas para testar o fluxo. O endereço está em `docs/extensions/README.md`, no repositório do projeto.
- Até aqui, o fluxo foi testado só com essa demonstração: nenhuma extensão foi testada ainda com um site de terceiros.
- Extensões são código de terceiros, e o conteúdo que mostram é responsabilidade de quem as publica. Só adicione repositórios em que você confia.
- Conteúdo adulto não aparece: fontes adultas ou sem classificação ficam desabilitadas. A classificação é a declarada por quem publica.
- Repositórios do Mihon são reconhecidos e recusados, porque o Prancha não executa APKs.
- Quem está na 0.1.x precisa instalar esta versão por cima uma vez. Da 0.2.0 em diante, o Prancha avisa ao abrir. A biblioteca e o progresso são mantidos, e o banco de dados se atualiza sozinho na primeira abertura.
- Requer Windows 10 ou 11 de 64 bits. Os instaladores não têm assinatura digital, então o Windows SmartScreen pode pedir confirmação.

## [0.2.0] - 2026-11-02

O Prancha agora avisa quando há versão nova e se atualiza sem sair do aplicativo, sempre com a sua confirmação.

### Novidades

- **Atualização automática:** ao abrir, o Prancha procura uma versão nova em segundo plano, sem atrapalhar o uso. Se houver, ele pergunta antes: "Atualizar agora" ou "Depois".
- **Indicador discreto:** quem escolhe "Depois" continua usando normalmente e vê a versão disponível no menu lateral. Um clique reabre a janela.
- **Instalação conferida:** o pacote só é instalado depois de confirmar a assinatura, o SHA-256 e a origem nas releases oficiais. O aplicativo fecha e abre de novo sozinho.
- **Novidades depois de atualizar:** uma janela mostra o que mudou na versão instalada, uma vez só e mesmo sem internet.
- **Seção Atualizações nas configurações:** versão instalada, data da última atualização, verificação manual e notas das versões anteriores.

### Observações

- Esta é a primeira versão com atualizador: quem está na 0.1.x precisa instalar a 0.2.0 por cima uma vez. A biblioteca e o progresso são mantidos.
- Ao abrir, o Prancha consulta as releases no GitHub. Sem internet, ele abre e funciona normalmente.
- Requer Windows 10 ou 11 de 64 bits. Os instaladores não têm assinatura digital, então o Windows SmartScreen pode pedir confirmação.

## [0.1.2] - 2026-11-01

Chega a Leitura Guiada: a página fica inteira na tela e cada balão salta para fora, maior, na ordem de leitura.

### Novidades

- **Leitura Guiada:** abra pelo botão na barra do leitor ou pela tecla `G`. A página fica parada e inteira, e cada balão sai dela maior e com o formato real, sem zoom na arte.
- **Detecção no próprio computador:** balões, legendas e caixas de narração são encontrados sem internet e sem serviço externo. Páginas sem texto são reconhecidas e seguem normalmente.
- **Ordem de leitura:** segue os quadros da página e respeita a direção escolhida no leitor, esquerda→direita ou direita→esquerda.
- **Navegação:** botões Anterior e Próximo, setas, `Espaço` e `Enter` para avançar, `Esc` para sair, e também toque e mouse. No último balão, a página mostra a conclusão e oferece seguir para a próxima ou rever o balão.
- **Correção manual:** em "Corrigir regiões" dá para adicionar, redimensionar e apagar regiões, mudar o tipo e a ordem. As correções ficam guardadas à parte e podem ser desfeitas para voltar à detecção automática.

### Melhorias

- A análise de cada página fica guardada: ao voltar a uma página, os balões aparecem na hora, e a próxima página já fica preparada.
- Ao sair da Leitura Guiada, o leitor comum continua na mesma página.

### Observações

- Ao atualizar de uma versão anterior, a biblioteca e o progresso são mantidos. O banco de dados se atualiza sozinho na primeira abertura.
- Limites conhecidos da detecção: texto branco direto sobre a arte, sem balão, pode não ser encontrado; balões ligados por uma emenda larga podem virar uma região só; o tipo do balão às vezes vem errado; e o balão ampliado cobre os vizinhos enquanto está ativo.
- A detecção foi testada com páginas reais de quadrinhos ocidentais. Mangás reais ainda não foram testados: a leitura da direita para a esquerda só foi conferida com páginas ocidentais espelhadas.
- Requer Windows 10 ou 11 de 64 bits. Os instaladores não têm assinatura digital, então o Windows SmartScreen pode pedir confirmação.

## [0.1.1] - 2026-10-20

Ajustes no leitor pedidos depois do primeiro uso: configurações mais simples e um índice de páginas que respeita a rolagem.

### Melhorias

- Nas configurações de leitura, "Modo de página" e "Direção de leitura" agora são menus: só a escolha atual fica visível.

### Correções

- A rolagem do índice de páginas não é mais puxada de volta para a página atual a cada movimento do mouse.

### Observações

- Ao atualizar da 0.1.0, a biblioteca e o progresso são mantidos.
- Requer Windows 10 ou 11 de 64 bits. Os instaladores não têm assinatura digital, então o Windows SmartScreen pode pedir confirmação.

## [0.1.0] - 2026-10-20

Primeira versão do Prancha: leitor e organizador local de quadrinhos, mangás e graphic novels, sem conta, sem internet e sem assinatura.

### Novidades

- **Formatos:** CBZ, CBR, CB7, CBT, PDF e pastas com imagens.
- **Biblioteca:** capas automáticas, pesquisa sem acentos, filtros, ordenação, grade ou lista, favoritos e coleções.
- **Leitor:** página única, dupla e rolagem vertical; ajuste à página, à largura e à altura; zoom; direita→esquerda; tela cheia; índice de páginas e atalhos configuráveis.
- **Progresso:** salvo automaticamente, para retomar de onde parou.
- **Tutorial** de primeira execução, com importação real de uma HQ.
- **Temas** claro e escuro, com a interface em português do Brasil.

### Observações

- Requer Windows 10 ou 11 de 64 bits. Os instaladores não têm assinatura digital, então o Windows SmartScreen pode pedir confirmação.
