# Histórico de versões

Tudo o que mudou em cada versão do Prancha. O projeto segue o [versionamento semântico](https://semver.org/lang/pt-BR/),
e as notas das releases e a [página de downloads](https://cakeysama.github.io/prancha/) são geradas a partir deste arquivo.

## [0.2.0] - 2026-11-01

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
