# Relatório de testes — Prancha 0.2.0 e extensões

Executado em 2026-11-02, com as extensões ainda sem versão publicada, em Windows 11 Pro 10.0.26200, WebView2 156, Rust 1.99.1 e Node 22.23.

## Resumo

| Suíte | Comando | Resultado |
|---|---|---|
| TypeScript estrito | `npm run typecheck` | ✅ sem erros |
| ESLint (+ React Compiler) | `npm run lint` | ✅ 0 erros, 1 aviso conhecido (`useVirtualizer` não memoizável — limitação da biblioteca) |
| Rust — unitários | `cargo test --lib` | ✅ 81/81 |
| Rust — integração | `cargo test --test backend` | ✅ 28/28 |
| Rust — extensões | `cargo test --test extensions` | ✅ 89/89, com os 15 testes de download repetidos 10 vezes depois da última correção; mais 1 fora da execução normal, que usa a internet, também aprovado |
| Rust — páginas reais | `npm run test:rust`, que roda todas as suítes de Rust | ✅ 6/6, com as páginas reais locais |
| Frontend — Vitest | `npm test` | ✅ 95/95 |
| E2E no executável real | `npm run test:e2e` | ✅ 49/49 |
| Atualização instalada de verdade | `npm run test:update -- --build` | ✅ 23 verificações na 0.2.0; não repetida nas extensões, porque `updates.rs` e `tauri.conf.json` não mudaram |
| Instalador da 0.2.0 | `npm run tauri build` | ✅ NSIS e MSI gerados, com 2 assinaturas; verificação abaixo |
| Build de release com extensões | `npm run tauri build`, sem os artefatos do atualizador | ✅ NSIS e MSI gerados; tamanhos e conferência do executável abaixo |

## Instaladores

Os instaladores da 0.2.0 saem da automação do GitHub, e os SHA-256 deles ficam na própria release. Os das versões anteriores são os das [releases](https://github.com/CAKeysama/prancha/releases), com estes SHA-256:

| Arquivo | Tamanho | SHA-256 |
|---|---|---|
| `Prancha_0.1.2_x64-setup.exe` (NSIS, por usuário) | 6,9 MB | `d735bfc94e1ae2d672f6a9e2baaedf7cf8a8e730ee0937b96ddc14dc57ed015b` |
| `Prancha_0.1.2_x64_pt-BR.msi` | 8,6 MB | `6a07da8cc8b0b964a78f1804a02e6c49a08499957f7d023a1597ae53b404fe35` |
| `Prancha_0.1.1_x64-setup.exe` (NSIS, por usuário) | 6,8 MB | `c8636e8ced20ca628af2cbddf05ace9d56e2e4badcf0accf4168967002034ce3` |
| `Prancha_0.1.1_x64_pt-BR.msi` | 8,4 MB | `a1166dc5d3339790a5ab23f6ca3ba2998c88c86b2642ddf1efe357e6991008db` |
| `Prancha_0.1.0_x64-setup.exe` (NSIS, por usuário) | 6,8 MB | `86072a8c96006f8dd08702cc9ff1a9ebb0d69760d055a593072b61fa19b7d166` |
| `Prancha_0.1.0_x64_pt-BR.msi` | 8,4 MB | `efbf2e0220e669316fa8d71a7da238b4300beea46812eaaf78ce958f56dea5d1` |

Verificação do NSIS 0.1.2: instalado por cima da versão anterior em uma biblioteca real com 12 quadrinhos. A migração do banco da versão 1 para a 2 passou e a checagem de integridade do SQLite deu `ok`.

Verificação do NSIS 0.1.0: instalação silenciosa (`/S /D=<pasta temporária>`) → o teste de fumaça E2E rodou no `prancha.exe` instalado (build de release, frontend embutido, pasta de dados isolada) e passou → desinstalação silenciosa removeu a pasta.

Os MSI das três versões foram extraídos em modo administrativo: a versão do pacote e a do `prancha.exe` conferem. Nenhum MSI foi instalado de fato, porque a instalação por máquina exige administrador.

## O que cada suíte cobre

### Rust com páginas reais (Leitura Guiada)
Roda sobre 13 páginas de 2 quadrinhos reais, com 81 balões marcados à mão e 5 páginas sem nenhum balão. Os quadrinhos e o gabarito (`tests/real/`) ficam fora do git por direitos autorais: sem eles a suíte falha de propósito, para ninguém achar que a detecção foi conferida. Por isso ela roda só localmente, não na automação do GitHub.
- Cada balão do gabarito é encontrado uma única vez, na ordem de leitura, e o número de regiões a mais é exatamente o previsto.
- As linhas de um mesmo balão ficam na mesma região.
- A leitura da direita para a esquerda inverte quadros e balões.
- Balões em fundo duvidoso ficam marcados para revisão.
- O contorno do balão fica dentro da região e chega às bordas dela.
- A análise real fica salva, e as correções manuais a substituem.

Os limites conhecidos da detecção estão em [ARCHITECTURE.md](ARCHITECTURE.md).

### Rust (backend real, arquivos gerados nos testes)
- **Ordenação natural:** `1, 2, 3, 10`; zeros à esquerda; maiúsculas; pastas aninhadas; números enormes.
- **Formatos:** CBZ, CBR (RAR5 real lido pela biblioteca UnRAR), CB7, CBT, pasta de imagens e PDF; detecção pelo conteúdo (`.cbr` que é ZIP); `ComicInfo.xml` em CBZ/CBR/CB7/CBT; extração em uma passada.
- **Importação:** arquivo válido; pasta com subpastas e arquivos; duplicata por conteúdo (nome diferente) vs. mesmo nome com conteúdo diferente; política "importar mesmo assim"; arquivo movido → relocalizado mantendo o progresso; cancelamento da fila.
- **Erros:** ZIP corrompido, cabeçalho desconhecido, arquivo vazio, extensão não suportada, EPUB dentro de pasta (relatado, não ignorado), arquivo sem imagens, RAR truncado, arquivo bloqueado por outro processo, página danificada (CRC) sem derrubar as demais, arquivo removido do disco.
- **Capas:** preferência metadados → nome de capa → primeira página válida (pulando página inválida) → substituta com motivo; proporção preservada; sem ampliação; recorte de tiras longas; limpar cache e regenerar sem perder progresso.
- **Catálogo:** busca sem acentos por título, arquivo, série, autor e tag; filtros por estado, favorito, formato e coleção; ordenação natural e por progresso; edição de metadados com origem "editado"; validação de título/ano; favoritos; coleções (nome único sem diferenciar maiúsculas, renomear, excluir sem remover HQs, busca dentro da coleção); remoção mantendo o arquivo original e apagando o progresso em cascata.
- **Progresso:** salvar, reabrir o banco ("fechar o app") e retomar na página salva; conclusão na última página; reiniciar; marcar não lida/concluída; valores fora do intervalo.
- **Banco:** migrações idempotentes, chaves estrangeiras ativas, *collation* natural.

### Vitest (interface com IPC simulado pela API oficial `@tauri-apps/api/mocks`)
- Domínio: texto de progresso ("Não iniciado", "Parou na página 42 de 186", "Leitura concluída"), visões de página dupla com capa sozinha e páginas largas, cálculo de ajuste/zoom, atalhos (normalização, inversão RTL, conflitos), validação de configurações, tema do sistema.
- Interface: tutorial aparece na primeira execução, pode ser pulado, não reaparece após concluído, retoma na etapa salva, trata cancelamento do seletor, reabre pelas configurações; troca de tema imediata e persistida; restauração do tema salvo; cartões com progresso real e ações separadas; remoção com confirmação e sem apagar o original por padrão; pesquisa enviada ao banco; capa substituta e capa pendente identificadas.

### E2E (executável real, backend Rust + WebView2, conectado por CDP)
Somente a **escolha de caminho** do diálogo nativo é substituída por um arquivo de resposta (`PRANCHA_E2E_PICK_FILE`, compilado apenas em builds de depuração). Importação, banco, capas e leitura são reais.

1. Biblioteca vazia e tutorial — etapa 1.
2. Etapa 2: cancelamento do seletor e arquivo corrompido mostram mensagens compreensíveis.
3. Importação real de CBZ avança para a etapa 3 com a capa real.
4. Etapa 4 no leitor, "Entendi", navegação por teclado.
5. **Fechar e reabrir o app**: tutorial não reaparece, "Parou na página 4 de 14", retomada na página 4.
6. **Encerramento forçado do processo** após mudar de página: banco íntegro e progresso salvo.
7. Importação de pastas com os 7 itens + erros: resumo "7 adicionados · 3 com falhas", detalhes com motivo de cada falha.
8. Toda HQ com capa real carregada (inclui PDF renderizado pelo PDF.js) ou capa substituta identificada.
9. Abrir e navegar (próxima/anterior) em CBR, CB7, CBT, pasta e PDF.
10. Zoom, ajuste à largura/página, página dupla (capa sozinha), direita→esquerda, rolagem vertical, última página, diálogo de atalhos.
11. Biblioteca com progresso variado, lista, pesquisa, filtros por estado e tipo.
12. Detalhes, edição (origem "editado"), favoritos, criação de coleção.
13. Configurações nos temas escuro e claro; leitor no tema claro.
14. Janela pequena (760×560) sem rolagem horizontal e com "Adicionar quadrinhos" visível.
15. Limpar miniaturas mantém o progresso e recria as capas.
16. Remover da biblioteca preserva o arquivo original.
17. Arquivo renomeado no disco é indicado como ausente e localizado novamente.
18. Teste de fumaça de inicialização.
19. O índice de páginas respeita a rolagem do usuário, e o painel do leitor usa menus.

Leitura Guiada, sobre as mesmas páginas reais da suíte em Rust:

20. Destaca cada balão sem mexer na página, navega entre eles e respeita o início e o fim.
21. Página sem balões não ganha regiões inventadas e pode receber regiões à mão.
22. Balões difíceis aparecem, e o duvidoso fica marcado.
23. Os controles acompanham o tema claro.
24. A transição mantém os quadros da animação (76 quadros, p95 de 6,2 ms nesta execução).
25. A ordem segue a direção de leitura.
26. Funciona nos modos duplo e vertical, sem mudar o modo.
27. Acompanha a janela redimensionada.
28. As correções sobrevivem ao reinício e podem ser desfeitas.
29. Páginas reais em PDF são analisadas a partir do PDF.js.

Atualizações, nos temas escuro e claro. Builds de depuração não acessam a rede, então o estado é gravado direto:

30. Janela de nova versão com as duas versões, "Depois", indicador e reabertura pelo indicador.
31. A versão recusada fica gravada, e a janela não volta sozinha depois de reabrir o app.
32. Seção Atualizações nas configurações.
33. Novidades depois de atualizar, sem repetir na abertura seguinte.
34. Histórico e menor janela (720×520), sem rolagem horizontal.

### Atualização instalada de verdade (`tests/update/run.mjs`)
Instala um pacote NSIS assinado e atualiza da 9.8.0 para a 9.8.1. O pacote, a assinatura e o instalador são reais. O servidor é local, a chave é descartável e o produto tem outro nome, para não tocar no Prancha instalado. O GitHub não participa.

- **Consulta:** sem versão nova, nada aparece. Servidor travado não segura a abertura nem mostra erro. Sem conexão e com erro do servidor, o app abre normalmente e sem alerta. Release sem pacote para o sistema, ou com pacote fora das releases oficiais, não é oferecida.
- **Janela e indicador:** mostra as versões e as notas. "Depois" deixa só o indicador, navegar não reabre a janela, o indicador reabre, e a versão recusada continua só no indicador depois de reabrir o app.
- **Integridade:** pacote adulterado ou incompleto é recusado pela assinatura, download interrompido dá mensagem clara, SHA-256 errado é recusado.
- **Download:** progresso real em porcentagem. Cancelar, ou fechar o app durante o download, não instala nada.
- **Instalação:** o app fecha para o instalador, o executável é trocado pela 9.8.1, e a versão nova abre sozinha.
- **Depois de atualizar:** a janela de novidades agradece e usa as notas locais sem internet, volta se for fechada sem ser vista e não se repete. O indicador some.
- **Dados:** biblioteca, progresso, favorito e preferências preservados.
- **Falha:** instalação que não terminou é informada na abertura seguinte.

### Build de produção e GitHub real
O build com a chave de produção gerou os 2 pacotes e as 2 assinaturas. O `latest.json` traz `windows-x86_64`, `windows-x86_64-nsis` e `windows-x86_64-msi`. Nas três entradas, o SHA-256 confere, a chave é a mesma da configuração e a assinatura declara `version:0.2.0`.

Antes da publicação, esse executável rodou contra o GitHub real, com dados isolados. O histórico veio da API com as notas de v0.1.0, v0.1.1 e v0.1.2. O `latest.json` ainda não existia: a consulta falhou como previsto e nenhuma janela abriu.

### Release publicada
Em 2026-11-02, a v0.2.0 foi conferida pelos endereços públicos. Os instaladores, as assinaturas, o `SHA256SUMS.txt` e o `latest.json` passaram nas mesmas verificações, incluindo as assinaturas contra a chave pública do aplicativo e a recusa de um arquivo adulterado.

O `prancha.exe` extraído do MSI publicado rodou sem instalar, com dados isolados, e leu o `latest.json` publicado. Ele não acusou versão nova nem erro, nenhuma janela abriu, o `updates.log` ficou vazio e a seção Atualizações mostrou as quatro versões, com a v0.2.0 como instalada.

## Extensões

Na execução normal, nenhum teste depende de site externo. O servidor é local e controlado pelo teste, os componentes WebAssembly são compilados de verdade (`extensions/test-fixture`, `extensions/template` e `extensions/demo`) e as assinaturas usam chaves geradas na hora. Esse servidor só é alcançável pela exceção de rede para `127.0.0.1`, que existe apenas em builds de depuração.

### Rust (`src-tauri/tests/extensions.rs` e testes de unidade em `src/extensions/`)
- **Isolamento:** laço infinito para por tempo e por cancelamento, o excesso de processamento esgota o combustível, memória acima do limite é recusada, estouro de pilha e pânico viram erro e o motor continua funcionando. Arquivos e ambiente não existem para a extensão, e um componente que importa WASI nem carrega.
- **Rede da extensão:** exige a capacidade e um host autorizado, redirecionamentos não saem da lista, cabeçalhos privados são recusados, resposta grande não chega à extensão e o número de requisições por chamada é limitado.
- **Rede do Prancha:** endereços privados, loopback, link-local e metadados são bloqueados na URL e na resolução de DNS. Redirecionamentos são conferidos de novo, até 3. Limite de tamanho sem tamanho declarado, servidor lento, cancelamento durante a espera, tipo de conteúdo e `ETag`.
- **Repositórios:** endereço válido e inválido, manifesto ausente e JSON inválido, formato mais novo, Mihon com a mensagem exata, chave fixada só depois da revisão, mudança de chave, assinatura errada, índice assinado com outra chave, índice mais antigo, entrada ruim recusando o índice inteiro, falha de rede preservando o catálogo, vários repositórios com um falhando e índice sem mudança não baixado de novo.
- **Instalação:** extensão válida, hash, tamanho, assinatura, compatibilidade e runtime errados, componente que não carrega, divergência com o índice, atualização mantendo os dados da extensão, falha de atualização mantendo a versão anterior, procedência que não muda em silêncio, extensão desabilitada e remoção sem tocar na biblioteca.
- **Fontes:** populares, recentes, pesquisa, detalhes, capítulos, páginas, paginação, fonte sem resultados, tipos de falha, fonte indisponível, conteúdo adulto filtrado e esquecido, e capas pelo backend com cache e dentro dos hosts da extensão.
- **Downloads:** CBZ válido na biblioteca, ordem declarada, posições repetidas ou ausentes, página inválida limpando só os próprios arquivos, status ruim e host bloqueado, duplicação, cancelamento, pausa e retomada sem baixar de novo, continuação depois de reiniciar, uma por fonte com o intervalo respeitado, CBZ corrompido e os mesmos bytes para o mesmo capítulo.
- **Biblioteca:** o capítulo sobrevive à remoção da extensão e do repositório e à falta de rede, remover da biblioteca só desfaz o vínculo e capítulo que sumiu da fonte continua local.
- **Demonstração:** do repositório à leitura sem rede, sem interface.
- **Banco:** migração em banco novo e em banco usado da 0.2.0, regras de exclusão e unicidade.

### Vitest (`tests/unit/online.test.tsx`, IPC simulado)
Telas de Repositórios, Extensões, Explorar, obra e Downloads: revisão de confiança, mudança de chave, catálogo com paginação, revisão de permissões antes de atualizar, pesquisa com fonte falhando, seleção de capítulos e estados de download. Também as regras sem interface de `src/domain/online.ts`.

### E2E (`tests/e2e/extensions.spec.ts`, executável real)
O executável real fala com um servidor HTTP local que publica o repositório de demonstração, gerado na hora com uma chave descartável. Os 16 passos da definição de pronto, nesta ordem:

| Passos | O que acontece |
|---|---|
| 1 a 5 | Abre o app e vai a Repositórios. Um endereço do Mihon é recusado com a mensagem exata. O repositório é validado, a impressão digital mostrada confere com a calculada pelo teste, nada fica salvo antes da confirmação, e o catálogo sincroniza. |
| 6 a 8 | Consulta o catálogo, vê as permissões, instala a extensão assinada e vê as fontes. |
| 9 a 12 | Em Explorar: paginação, recentes, pesquisa vazia, pesquisa sem acento, detalhes, capítulos, ordem e acompanhamento. |
| 13 a 15 | Baixa um capítulo, encontra o CBZ na biblioteca e abre no leitor de sempre. |
| 16 | Com o servidor desligado e o app reaberto, a leitura continua de onde parou. |

Outros cenários:

- **Downloads:** progresso, pausa, retomada, falha e nova tentativa. O quadrinho local continua abrindo durante um download lento.
- **Telas:** nos dois temas, em 1280 e 760 de largura, sem rolagem horizontal. As capturas ficam em `tests/.tmp/extension-shots`, fora do git, e foram revisadas à mão.
- **Remoção:** remover a extensão e o repositório não apaga nenhum dos 3 CBZ.

### Desempenho
Revisão do código, com o comportamento coberto pelos testes acima. Não há medição de carga.

| Ponto | Como funciona | Evidência |
|---|---|---|
| Nada de polling | As telas online não têm `setInterval` nem `setTimeout`. Os downloads avisam por evento. | Busca no código |
| Parado, sem custo | Os downloads esperam em uma `Condvar`, e o relógio das extensões só anda com chamadas em andamento. O motor só é criado no primeiro uso. | `downloads.rs`, `runtime.rs`, `core.rs` |
| Catálogo fora da memória | Fica no SQLite e chega à tela de 100 em 100. | Teste de unidade da tela |
| Sincronização | Só sob pedido, com `ETag`, até 4 repositórios ao mesmo tempo. Um repositório com falha não para os outros. | Testes de integração |
| Concorrência | 4 chamadas ao todo e 2 por extensão. 3 downloads e 1 por fonte, com o intervalo declarado. 3 fontes por vez em Explorar e 4 capas. | Testes de integração |
| Extensão lenta | Não atrasa outra extensão nem a leitura local. | Teste de integração e E2E |
| Conexões | Um único cliente HTTP reaproveita as conexões. | Só no código, sem teste próprio |
| Limpeza | Temporários são limpos ao abrir, e obras não acompanhadas saem depois de 30 dias. | Testes de integração |

### Executável de release
Os E2E usam o build de depuração, então o executável de release foi conferido uma vez, à mão, com dados isolados. Como não há repositório em HTTPS antes do merge, a extensão de demonstração assinada foi colocada direto na pasta e no banco.

- Com `PRANCHA_E2E_ALLOW_LOOPBACK=1` definido, um endereço em `127.0.0.1` é recusado com "Endereço não permitido: só HTTPS é aceito.", e o servidor local não recebe nenhuma requisição.
- O `repo.json` da Keiyoushi é buscado em HTTPS público e reconhecido como Mihon, em 261 ms.
- Em Explorar, o componente compila e executa, e o host busca o catálogo em HTTPS. Como esse catálogo só existe depois do merge, a tela mostra o erro da própria fonte, "O acervo respondeu 404.", em cerca de 1 s.

### Tamanho
Mesmo perfil de release, antes e depois das extensões. O MSI "antes" é o publicado na v0.2.0, e os outros dois vêm de um build local da mesma versão.

| Arquivo | Antes (0.2.0) | Com extensões | Diferença |
|---|---|---|---|
| `prancha.exe` | 21.607.424 bytes | 36.260.352 bytes | +14,7 MB (+68%) |
| Instalador NSIS | 8.308.041 bytes | 11.571.650 bytes | +3,3 MB (+39%) |
| Instalador MSI | 10.461.184 bytes | 15.310.848 bytes | +4,8 MB (+46%) |

O build de release levou 17 min, compilando Wasmtime e Cranelift do zero, com outras suítes rodando ao mesmo tempo.

## Validação visual

Capturas em `artifacts/screenshots/`, geradas pelos testes E2E e revisadas manualmente:

| Situação | Arquivo |
|---|---|
| Primeira abertura (biblioteca vazia + tutorial) | `01-primeira-abertura-tutorial.png` |
| Tutorial: adicionar HQ / arquivo inválido / HQ na biblioteca / primeira leitura | `02`…`05` |
| Biblioteca com capas e progressos | `06-biblioteca-capas-e-progresso.png`, `06b-biblioteca-lista.png` |
| Detalhes | `07-detalhes.png` |
| Importação com erros | `08-importacao-resultado-com-erros.png` |
| Leitor: página única, dupla, vertical, PDF | `09`, `09a`, `09b`, `09c` |
| Configurações escuro / claro | `10-configuracoes-tema-escuro.png`, `11-configuracoes-tema-claro.png` |
| Biblioteca e leitor no tema claro | `11b`, `11c` |
| Janela pequena | `12-biblioteca-janela-pequena.png` |
| Arquivo ausente | `13-detalhes-arquivo-ausente.png` |

As capturas das atualizações ficam em `tests/.tmp/update-shots/`, fora do git, e foram revisadas nos temas escuro e claro e na menor janela.

### Defeitos encontrados e corrigidos durante a validação
- *Deadlock* no cache de páginas RAR/7z (trava reentrante) — corrigido, coberto por teste.
- Palavra reservada `NATURAL` do SQLite usada como *collation* — renomeada para `NATSORT`.
- Caminho do projeto longo demais para o linker MSVC — build em `CARGO_TARGET_DIR` curto (documentado).
- EPUB e similares dentro de pastas eram ignorados em silêncio — agora aparecem como "Formato não suportado".
- `cargo test` sobrescrevia o executável de depuração usado no E2E — o E2E usa uma cópia dedicada.
- Botão "Coleções" nos detalhes tinha o mesmo nome do item da barra lateral — renomeado para "Organizar em coleções".
- Texto "1 quadrinho encontrados" — concordância corrigida.
- Alternador Grade/Lista quebrava em duas linhas em janelas estreitas — agora mostra só ícones.
- O teste E2E de PDF falhava por tempo em cerca de 1 a cada 6 execuções. A causa era o próprio teste: ele clicava em "Biblioteca" depois que os controles do leitor já tinham se escondido. Agora o teste move o ponteiro antes de sair do leitor.
- O teste de cancelamento de um laço infinito falhou uma vez com todos os testes em paralelo. A causa era o próprio teste: o relógio incluía a compilação do componente, e o cancelamento podia chegar antes de o laço começar. Agora o componente é aquecido antes.
- O teste de página inválida falhou uma vez. A causa estava no código: o download publicava o estado final antes de apagar a pasta temporária, e um "tentar de novo" imediato podia ter a pasta nova apagada. Agora a limpeza vem antes.
- Na revisão antes do merge, um índice assinado com fração de segundo não numérica, como `.12é`, derrubava a consulta com um pânico. Agora a data é recusada, e o ano passa a ter limite.
- Na mesma revisão, uma extensão com `network: false` e hosts declarados aparecia como "sem rede", mas o Prancha baixaria capas e páginas desses hosts. Agora o índice é recusado.
- `runtime`, `apiVersion` e `supportedKinds` no índice, e nome, idioma, descrição e tipos declarados pelo componente, não tinham limite de tamanho. Agora têm.
- A regra de IPv6 listava o que bloquear e deixava passar o NAT64 local (`64:ff9b:1::/48`) e o IPv4 traduzido (`::ffff:0:0/96`). Agora só a faixa global `2000::/3` é aceita.
- Uma fonte podia pedir até 49 dias de intervalo e ocupar um download por todo esse tempo. O intervalo agora vai até 60 s.
- Um download novo podia ser marcado de volta como "na fila" se começasse enquanto a verificação de arquivos da abertura ainda rodava. A retomada agora vem antes dessa verificação.
- Atualizar uma extensão deixava o componente antigo na memória até fechar o app. Agora ele sai na atualização.
- Atualizar uma extensão pulava a revisão: agora mostra as duas versões e o acesso novo antes de confirmar.
- O catálogo de extensões parava em 200 itens, sem aviso: agora tem "Carregar mais".
- O filtro de tipo em Explorar cortava "Todos os tipos".
- Revisão de UX (diretrizes da Apple): filtros movidos para um botão "Filtrar" com contador e chips removíveis; um só seletor de ordenação; estado redundante removido dos cartões e dos detalhes; legendas óbvias e notificações para mudanças já visíveis removidas; a página do leitor não fica mais sob as barras; barra do leitor reduzida ao essencial (o restante no painel de configurações); resposta visual ao pressionar botões; barras sólidas com "reduzir transparência".

### Observação não reproduzida
Em uma execução do E2E, o leitor avançou páginas sozinho após as teclas (página 5 → 11 em alguns segundos). Repetições com captura de todos os eventos de teclado e roda do mouse não reproduziram o problema (três teclas → página 4, como esperado) e o cenário passou nas três execuções seguintes. A hipótese mais provável é a roda do mouse real sobre a janela visível durante o teste (a roda vira a página). Fica registrado como ponto a observar.

## Não verificado / limitações
- **Fontes reais:** nenhuma extensão falou com um site de verdade. O fluxo foi provado com a demonstração, em servidor local, e o endereço público do repositório de demonstração só funciona depois de ir para a `main`. Em HTTPS público, a única conferência é `a_public_https_address_is_really_fetched`, que fica fora da execução normal: o Prancha busca o `repo.json` da Keiyoushi com TLS e DNS reais e reconhece o formato do Mihon. Assinatura, instalação e download nunca rodaram por HTTPS, e o executável de release só foi conferido até a chamada da fonte, como descrito acima.
- **Compilação de extensão:** não pode ser cancelada. É curta (37 ms no build de release, para 24 KB) e o pacote tem tamanho limitado, mas conta como espera fora do limite de tempo da chamada.
- **Permissões novas em uma atualização:** a comparação é feita na tela, antes de confirmar, e o backend não a repete.
- **Download com falha:** uma nova tentativa recomeça do zero. Só a pausa e o reinício aproveitam as páginas já baixadas.
- **Reaproveitamento de conexões:** vem do cliente único, sem teste próprio.
- **Telas online em temas e janela pequena:** conferidas no E2E, porque o ambiente do Vitest não calcula layout.
- **Capturas versionadas:** `artifacts/screenshots` ainda mostra a barra lateral antiga, sem as novas seções.
- **Atualização pelo GitHub:** nenhum app instalado baixou uma atualização das releases, porque isso exige duas versões publicadas com o atualizador. A primeira oportunidade é a versão seguinte à 0.2.0.
- **Atualização por MSI:** nunca executada, porque exige administrador. O caminho testado é o do NSIS. Com o MSI, a confirmação do Windows vem depois que o app fechou, e uma recusa só aparece na abertura seguinte.
- **Sem espaço em disco e sem permissão:** só a tradução das mensagens de erro foi testada. Falha ao reiniciar foi simulada a partir do estado pendente salvo.
- **Assinatura do Windows:** os instaladores não têm Authenticode, então o SmartScreen pode pedir confirmação.
- **Mangá real:** a leitura da direita para a esquerda da Leitura Guiada só foi conferida em páginas ocidentais espelhadas.
- **Automação do GitHub:** roda typecheck, lint, Vitest, os testes de Rust sem páginas reais e o build. O E2E e a suíte de páginas reais dependem de arquivos que só existem localmente.
- **RAR comprimido:** não existe compressor RAR livre para gerar testes; os testes usam RAR5 "stored" lido pela biblioteca UnRAR oficial (a descompressão em si é responsabilidade dela). RAR4 e volumes múltiplos não foram testados com arquivos reais.
- **7z sólido grande e PDFs > 1 GB:** suportados pelo desenho (cache de extração, leitura por intervalo), sem teste de carga.
- **Diálogo nativo de arquivos:** não foi aberto nem em teste automatizado nem manualmente nesta sessão; nos testes a escolha do caminho é injetada e o diálogo usa o plugin oficial `tauri-plugin-dialog` (filtro de extensões dos formatos suportados).
- **Arrastar e soltar:** implementado com o evento nativo do Tauri; não automatizado.
- **Leitores de tela:** nomes acessíveis verificados via árvore de acessibilidade nos testes; não houve teste com NVDA/Narrador.
- **Bibliotecas muito grandes (10 000+ itens):** virtualização implementada, sem teste de desempenho medido.
