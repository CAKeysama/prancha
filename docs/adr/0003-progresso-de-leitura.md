# ADR 0003: estado de leitura por capítulo e capítulos novos

**Estado:** aceita (2026-11-09).

## Contexto

Até a 0.5.0, o estado de um capítulo de obra era deduzido do progresso: havia linha em `chapter_progress`, estava "lendo"; a página salva era a última, estava "lido". Abrir um capítulo já gravava a primeira página, então abrir e fechar contava como começar. Não existia "Lendo agora", nem marcação manual por capítulo, nem forma de saber o que acabou de chegar da fonte.

## Decisão

### Quatro estados, mais a novidade à parte

| Estado | Regra | Onde |
|---|---|---|
| Não lido | `completed_at IS NULL` e (`reading_started_at IS NULL` ou `manual_unread = 1`), sem sessão ativa | `online_chapters` |
| Lendo | há sessão aberta com atividade nos últimos 3 min (`SESSION_STALE_MS`) | `reading_sessions` |
| Em andamento | `reading_started_at IS NOT NULL`, sem conclusão, sem `manual_unread`, sem sessão ativa | `online_chapters` |
| Lido | `completed_at IS NOT NULL` | `online_chapters` |
| Novo (independente) | `acknowledged_at IS NULL` | `online_chapters` |

A hierarquia está em `reading::derive_state`: sessão ativa > concluído > marcado como não lido > iniciado > não lido. Reler um capítulo concluído mostra "Lendo" durante a sessão e volta a "Lido" ao sair; a conclusão anterior nunca é apagada por abrir ou reler.

A posição continua onde estava (`chapter_progress`, ou `reading_progress` quando o capítulo está na biblioteca). Nenhuma mudança de estado apaga a posição, nem "Marcar como não lido".

### Sessão separada do estado que dura

`reading_sessions` guarda só a sessão do leitor (`started_at`, `last_activity_at`, `ended_at`, `end_reason`). O estado que dura nunca depende de um evento de fechamento:

- a sessão abre no **início efetivo** (`chapter_reading_start`), e não ao abrir o capítulo;
- gravar a posição e um aviso a cada minuto (`chapter_reading_touch`) mantêm `last_activity_at`;
- ela fecha ao sair do capítulo, trocar de capítulo, concluir ou marcar à mão; abrir outro capítulo fecha a anterior (`replaced`);
- uma sessão sem atividade há mais de 3 min já não conta como "Lendo", mesmo que ninguém a tenha fechado;
- na abertura do app (`Core::init` → `reading::recover_sessions`) toda sessão aberta termina na última atividade, com `end_reason = 'recovered'`. O capítulo, que já tinha `reading_started_at`, aparece "Em andamento". Nada vira "Lido" por isso.

### Quem decide o que é ler

O leitor (`src/domain/reading-lifecycle.ts`) sabe como a pessoa chegou a cada página:

| Navegação | Inicia | Conclui |
|---|---|---|
| `restore` (posição restaurada, troca de modo, redimensionar) | não | não |
| `jump` (índice, barra de progresso, atalho de primeira/última) | sim | não |
| `step` (próxima/anterior por botão, teclado, roda, Leitura Guiada) | sim | sim |
| `scroll` (rolagem da pessoa no modo vertical: roda, toque, teclado, barra) | sim | sim |

No modo paginado, o capítulo termina quando a última página está na tela por `step` **e** a imagem dela carregou (`onSize` do `PageView`). "Próxima" na última página também conclui, mesmo depois de um salto. No modo vertical, termina quando a borda de baixo da última página entrou na tela (tolerância de 4 px, `BOTTOM_TOLERANCE_PX`) por rolagem da pessoa, com a página carregada; estar no meio dela não basta. Passar rolando de um capítulo ao seguinte conclui o anterior. Cada aviso sai uma vez por leitor, numa fila (`serialQueue`) que mantém a ordem início → conclusão → fim, e o backend é idempotente: `completed_at = coalesce(completed_at, agora)`.

### Capítulos novos

- Todo capítulo recebe `first_seen_at` ao entrar no catálogo local. A data de publicação da fonte não é usada.
- **Ponto de referência:** a primeira sincronização de uma obra nunca marca nada como novo. Um capítulo só nasce novo (`acknowledged_at = NULL`) se a obra está acompanhada (`following = 1`) **e** já tinha capítulos registrados. Obras não acompanhadas não geram novidades; acompanhar uma obra não transforma o histórico em novidade.
- Ressincronizar não muda `first_seen_at`, `acknowledged_at` nem o estado de quem já existia (o `ON CONFLICT` só atualiza título, número, data e ordem).
- A novidade some quando o capítulo aparece de fato no leitor (`chapter_acknowledge`; o capítulo pré-carregado no fim do anterior não conta), quando é concluído e quando é marcado como lido. Abrir a página da obra ou ver a lista não reconhece nada. Marcar como não lido não traz a novidade de volta.
- Na migração 0007, tudo o que já existia ficou reconhecido (`acknowledged_at = first_seen_at`), para quem atualiza não ver o histórico inteiro como novo.
- Contador da obra: `count(*) FROM online_chapters WHERE series_id = ? AND acknowledged_at IS NULL`, com índice parcial `idx_online_chapters_new`. A biblioteca usa uma consulta agrupada só (`reading::new_counts`).
- `store_chapters` devolve os ids recém-marcados como novos, para quem quiser avisar a pessoa. A busca por novidades é feita ao abrir a obra e pelo botão "Procurar capítulos novos" em Explorar › Acompanhando (`series_refresh_following`), que consulta só as obras acompanhadas.

### Marcar anteriores

`reading::previous_ids` usa só `sort_order`, a ordem de leitura informada pela fonte. Título e número não entram ("10 Extra", "Especial" e "Vol. 2 Cap. 3" não se ordenam por texto). Capítulos com a mesma posição do escolhido não são anteriores: sem como saber, ficam como estão. Se a fonte não informa ordem (todas iguais), nada é marcado. A operação é uma transação só.

### Integridade

- Remover a extensão não apaga capítulos nem estados (`online_series` não tem chave estrangeira para a extensão, como antes).
- Um capítulo que some da fonte continua registrado se tem arquivo, está na biblioteca **ou tem histórico de leitura** (`reading_started_at`/`completed_at`).
- O capítulo baixado também é uma HQ da biblioteca: lido/não lido acompanha em `comics.status`, sem mexer em `reading_progress`.

## Limites conhecidos

- As HQs avulsas da biblioteca (arquivos importados que não são capítulo de uma obra) continuam com o modelo antigo de `comics.status` (`unread`/`reading`/`completed`).
- Não há verificação periódica em segundo plano: novidades chegam ao abrir a obra ou ao pedir em Acompanhando.
- O contador de novos conta capítulos da fonte, não números de capítulo: uma fonte que republica o mesmo capítulo com outro id gera uma novidade.
