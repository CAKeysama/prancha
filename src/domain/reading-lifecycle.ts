// Quando o leitor diz que um capítulo começou a ser lido, deixou de ser lido ou terminou.
// Regras puras, sem React nem chamadas ao backend: quem usa passa as funções que gravam (tests/unit/reading.test.ts).

/** Como a pessoa chegou ao que está na tela. */
export type Navigation =
  /** Posição restaurada ao abrir, troca de modo, redimensionamento: não é leitura. */
  | "restore"
  /** Índice de páginas, barra de progresso, atalho de primeira/última página: começa a leitura, mas não conclui. */
  | "jump"
  /** Próxima ou anterior página (botões, teclado, roda do mouse, Leitura Guiada). */
  | "step"
  /** Rolagem feita pela pessoa no modo vertical. */
  | "scroll";

export interface LifecycleChapter {
  id: number;
  first: number;
  count: number;
}

export interface LifecycleHandlers {
  /** O capítulo apareceu na tela: a novidade some, o estado de leitura não muda. */
  acknowledge: (id: number) => void;
  /** Primeira interação que avança a leitura nesta sessão. */
  start: (id: number) => void;
  /** Saiu do capítulo antes de concluir. */
  end: (id: number) => void;
  /** Chegou de fato ao fim. */
  complete: (id: number) => void;
}

export interface View {
  /** Páginas na tela (uma, ou duas no modo de página dupla), na numeração única do leitor. */
  pages: number[];
  how: Navigation;
  /**
   * Modo vertical: a maior página cuja borda de baixo já passou pela tela, com a tolerância de arredondamento.
   * Estar no meio da última página não conta; chegar ao limite inferior dela, sim.
   */
  passed?: number;
}

const counts = (how: Navigation) => how !== "restore";
const concludes = (how: Navigation) => how === "step" || how === "scroll";

/**
 * Uma sessão de leitura: do momento em que o leitor abre até ele fechar. Cada capítulo da sequência é acompanhado
 * à parte. Nada é avisado duas vezes: repetir a mesma página, renderizar de novo ou receber eventos repetidos
 * não gera gravação.
 */
export class ReadingLifecycle {
  private chapters: LifecycleChapter[] = [];
  private readonly acknowledged = new Set<number>();
  private readonly started = new Set<number>();
  private readonly completed = new Set<number>();
  private readonly rendered = new Set<number>();
  private current: number | null = null;
  private last: View | null = null;

  constructor(private readonly handlers: LifecycleHandlers) {}

  setChapters(chapters: LifecycleChapter[]) {
    this.chapters = chapters;
  }

  /** O capítulo com sessão aberta neste leitor, se houver. */
  get active(): number | null {
    return this.current !== null && this.started.has(this.current) && !this.completed.has(this.current) ? this.current : null;
  }

  private chapterAt(page: number): LifecycleChapter | undefined {
    let found: LifecycleChapter | undefined;
    for (const chapter of this.chapters) {
      if (page >= chapter.first) found = chapter;
      else break;
    }
    return found && page < found.first + found.count ? found : undefined;
  }

  private leave(id: number) {
    if (this.started.has(id) && !this.completed.has(id)) this.handlers.end(id);
    this.started.delete(id);
  }

  /** O que está na tela mudou. */
  view(view: View) {
    if (view.pages.length === 0) return;
    const chapter = this.chapterAt(view.pages[0]!);
    if (!chapter) return;
    const previous = this.last;
    const moved = !previous || previous.pages.join() !== view.pages.join() || view.how === "scroll";
    this.last = view;
    // Uma rolagem que já cai no capítulo seguinte também passou pelo fim do anterior: conclui antes de sair dele.
    this.checkCompletion();

    if (this.current !== chapter.id) {
      if (this.current !== null) this.leave(this.current);
      this.current = chapter.id;
    }
    if (!this.acknowledged.has(chapter.id)) {
      this.acknowledged.add(chapter.id);
      this.handlers.acknowledge(chapter.id);
    }
    if (counts(view.how) && moved && !this.started.has(chapter.id) && !this.completed.has(chapter.id)) {
      this.started.add(chapter.id);
      this.handlers.start(chapter.id);
    }
    this.checkCompletion();
  }

  /** "Próxima" na última página: não muda a tela, mas é navegação de verdade até o fim. */
  pressedNextAtEnd() {
    if (!this.last) return;
    const chapter = this.chapterAt(this.last.pages[0]!);
    if (chapter && !this.started.has(chapter.id) && !this.completed.has(chapter.id)) {
      this.started.add(chapter.id);
      this.handlers.start(chapter.id);
    }
    this.last = { ...this.last, how: "step" };
    this.checkCompletion();
  }

  /** A imagem da página carregou e foi desenhada. */
  pageRendered(page: number) {
    if (this.rendered.has(page)) return;
    this.rendered.add(page);
    this.checkCompletion();
  }

  private checkCompletion() {
    const view = this.last;
    if (!view || !concludes(view.how)) return;
    for (const chapter of this.chapters) {
      if (this.completed.has(chapter.id)) continue;
      const lastPage = chapter.first + chapter.count - 1;
      const reached = view.passed !== undefined ? view.passed >= lastPage : view.pages.includes(lastPage);
      if (!reached || !this.rendered.has(lastPage)) continue;
      // Rolando, passar por um capítulo inteiro conclui; virando páginas, só o capítulo cuja última página está na tela.
      if (view.passed === undefined && !view.pages.includes(lastPage)) continue;
      this.completed.add(chapter.id);
      this.started.delete(chapter.id);
      this.handlers.complete(chapter.id);
    }
  }

  /**
   * O leitor fechou (voltar, outro capítulo, janela fechando): a sessão do capítulo atual termina.
   * Repetir não faz nada. Se a tela continuar (o React remonta efeitos em desenvolvimento), a próxima
   * interação efetiva abre uma sessão nova.
   */
  close() {
    if (this.current !== null) this.leave(this.current);
    this.current = null;
    this.last = null;
  }
}

/**
 * Executa as gravações na ordem em que foram pedidas: iniciar, concluir e encerrar do mesmo capítulo não podem
 * chegar trocados ao backend. Uma falha não impede as seguintes.
 */
export function serialQueue() {
  let tail: Promise<unknown> = Promise.resolve();
  return {
    push(task: () => Promise<unknown>) {
      tail = tail.then(task).catch(() => undefined);
      return tail;
    },
    get idle() {
      return tail;
    },
  };
}
