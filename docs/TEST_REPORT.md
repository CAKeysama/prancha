# Relatório de testes — Prancha 0.1.0

Executado em 2026-10-20, Windows 11 Pro 10.0.26200, WebView2 155, Rust 1.99.1, Node 22.23.

## Resumo

| Suíte | Comando | Resultado |
|---|---|---|
| TypeScript estrito | `npm run typecheck` | ✅ sem erros |
| ESLint (+ React Compiler) | `npm run lint` | ✅ 0 erros, 1 aviso conhecido (`useVirtualizer` não memoizável — limitação da biblioteca) |
| Rust — unitários | `cargo test --lib` | ✅ 18/18 |
| Rust — integração | `cargo test --test backend` | ✅ 28/28 |
| Frontend — Vitest | `npm test` | ✅ 29/29 |
| E2E no executável real | `npm run test:e2e` | ✅ 18/18 |
| Instalador | `npm run tauri build` | ✅ NSIS e MSI gerados; NSIS verificado (ver abaixo) |

## Instalador

| Arquivo | Tamanho | SHA-256 |
|---|---|---|
| `artifacts/installer/Prancha_0.1.0_x64-setup.exe` (NSIS, por usuário) | 6,8 MB | `86072a8c96006f8dd08702cc9ff1a9ebb0d69760d055a593072b61fa19b7d166` |
| `artifacts/installer/Prancha_0.1.0_x64_pt-BR.msi` | 8,4 MB | `efbf2e0220e669316fa8d71a7da238b4300beea46812eaaf78ce958f56dea5d1` |

Verificação do NSIS: instalação silenciosa (`/S /D=<pasta temporária>`) → o teste de fumaça E2E rodou no `prancha.exe` instalado (build de release, frontend embutido, pasta de dados isolada) e passou → desinstalação silenciosa removeu a pasta. O MSI foi gerado, mas não instalado (instalação por máquina exige administrador).

## O que cada suíte cobre

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

### Defeitos encontrados e corrigidos durante a validação
- *Deadlock* no cache de páginas RAR/7z (trava reentrante) — corrigido, coberto por teste.
- Palavra reservada `NATURAL` do SQLite usada como *collation* — renomeada para `NATSORT`.
- Caminho do projeto longo demais para o linker MSVC — build em `CARGO_TARGET_DIR` curto (documentado).
- EPUB e similares dentro de pastas eram ignorados em silêncio — agora aparecem como "Formato não suportado".
- `cargo test` sobrescrevia o executável de depuração usado no E2E — o E2E usa uma cópia dedicada.
- Botão "Coleções" nos detalhes tinha o mesmo nome do item da barra lateral — renomeado para "Organizar em coleções".
- Texto "1 quadrinho encontrados" — concordância corrigida.
- Alternador Grade/Lista quebrava em duas linhas em janelas estreitas — agora mostra só ícones.
- Revisão de UX (diretrizes da Apple): filtros movidos para um botão "Filtrar" com contador e chips removíveis; um só seletor de ordenação; estado redundante removido dos cartões e dos detalhes; legendas óbvias e notificações para mudanças já visíveis removidas; a página do leitor não fica mais sob as barras; barra do leitor reduzida ao essencial (o restante no painel de configurações); resposta visual ao pressionar botões; barras sólidas com "reduzir transparência".

### Observação não reproduzida
Em uma execução do E2E, o leitor avançou páginas sozinho após as teclas (página 5 → 11 em alguns segundos). Repetições com captura de todos os eventos de teclado e roda do mouse não reproduziram o problema (três teclas → página 4, como esperado) e o cenário passou nas três execuções seguintes. A hipótese mais provável é a roda do mouse real sobre a janela visível durante o teste (a roda vira a página). Fica registrado como ponto a observar.

## Não verificado / limitações
- **RAR comprimido:** não existe compressor RAR livre para gerar testes; os testes usam RAR5 "stored" lido pela biblioteca UnRAR oficial (a descompressão em si é responsabilidade dela). RAR4 e volumes múltiplos não foram testados com arquivos reais.
- **7z sólido grande e PDFs > 1 GB:** suportados pelo desenho (cache de extração, leitura por intervalo), sem teste de carga.
- **Diálogo nativo de arquivos:** não foi aberto nem em teste automatizado nem manualmente nesta sessão; nos testes a escolha do caminho é injetada e o diálogo usa o plugin oficial `tauri-plugin-dialog` (filtro de extensões dos formatos suportados).
- **Arrastar e soltar:** implementado com o evento nativo do Tauri; não automatizado.
- **Leitores de tela:** nomes acessíveis verificados via árvore de acessibilidade nos testes; não houve teste com NVDA/Narrador.
- **Bibliotecas muito grandes (10 000+ itens):** virtualização implementada, sem teste de desempenho medido.
