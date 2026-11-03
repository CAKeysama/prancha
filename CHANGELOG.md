# Histórico de versões

Tudo o que mudou em cada versão do Prancha. O projeto segue o [versionamento semântico](https://semver.org/lang/pt-BR/),
e as notas das releases e a [página de downloads](https://cakeysama.github.io/prancha/) são geradas a partir deste arquivo.

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
