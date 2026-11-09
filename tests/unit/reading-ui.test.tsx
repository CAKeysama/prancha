// Estados de leitura na tela: tooltips dos ícones, lista de capítulos da obra, favoritar/acompanhar e a biblioteca.
// O backend é simulado como em online.test.tsx.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { Download } from "lucide-react";
import { App } from "../../src/app/App";
import { IconButton } from "../../src/components/Button";
import { TOOLTIP_DELAY_MS } from "../../src/components/Tooltip";
import type { ChapterReadInfo, ChapterView, LibrarySeries, SeriesCard, SourceSummary } from "../../src/domain/online";

describe("Tooltip dos botões só com ícone", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    // A Testing Library só avança os temporizadores falsos sozinha quando existe um `jest` global; sem ele, o userEvent trava.
    vi.stubGlobal("jest", { advanceTimersByTime: (ms: number) => vi.advanceTimersByTime(ms) });
  });
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  const setup = () => {
    const user = userEvent.setup({ delay: null, advanceTimers: vi.advanceTimersByTime });
    render(
      <>
        <IconButton icon={Download} label="Baixar capítulo" tooltipDelay={TOOLTIP_DELAY_MS} />
        <button type="button">Outro</button>
      </>,
    );
    return { user, button: screen.getByRole("button", { name: "Baixar capítulo" }) };
  };
  const wait = (ms: number) => act(() => vi.advanceTimersByTime(ms));

  it("o atraso é de 2 segundos", () => {
    expect(TOOLTIP_DELAY_MS).toBe(2000);
  });

  it("no hover, não aparece antes de 2 s e aparece aos 2 s", async () => {
    const { user, button } = setup();
    await user.hover(button);
    await wait(TOOLTIP_DELAY_MS - 1);
    expect(screen.queryByRole("tooltip")).toBeNull();
    await wait(1);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Baixar capítulo");
  });

  it("sair antes dos 2 s cancela", async () => {
    const { user, button } = setup();
    await user.hover(button);
    await wait(1500);
    await user.unhover(button);
    await wait(5000);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("some quando o cursor sai", async () => {
    const { user, button } = setup();
    await user.hover(button);
    await wait(TOOLTIP_DELAY_MS);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    await user.unhover(button);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("some com Escape", async () => {
    const { user, button } = setup();
    await user.hover(button);
    await wait(TOOLTIP_DELAY_MS);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("no foco por teclado, aparece só depois de 2 s e some ao sair do foco", async () => {
    const { user } = setup();
    await user.tab();
    expect(screen.getByRole("button", { name: "Baixar capítulo" })).toHaveFocus();
    await wait(TOOLTIP_DELAY_MS - 1);
    expect(screen.queryByRole("tooltip")).toBeNull();
    await wait(1);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Baixar capítulo");
    await user.tab();
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("clicar com o mouse dá foco, mas não abre o tooltip (o foco não é visível)", async () => {
    const { user, button } = setup();
    await user.click(button);
    expect(button).toHaveFocus();
    expect(button.matches(":focus-visible")).toBe(false);
    await wait(5000);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("o botão não tem texto fixo: o nome fica no aria-label", () => {
    const { button } = setup();
    expect(button.textContent).toBe("");
    expect(button).toHaveAttribute("aria-label", "Baixar capítulo");
    expect(screen.queryByText("Baixar capítulo")).toBeNull();
  });

  it("toque não abre tooltip; mouse, sim", async () => {
    const { button } = setup();
    fireEvent.pointerOver(button, { pointerType: "touch" });
    await wait(5000);
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.pointerOut(button, { pointerType: "touch" });
    // Controle: o mesmo evento com mouse abre, então o teste acima não passa por acaso.
    fireEvent.pointerOver(button, { pointerType: "mouse" });
    await wait(TOOLTIP_DELAY_MS);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
  });

  it("um só temporizador: passar para outro ícone cancela o primeiro", async () => {
    const user = userEvent.setup({ delay: null, advanceTimers: vi.advanceTimersByTime });
    render(
      <>
        <IconButton icon={Download} label="Primeiro" tooltipDelay={TOOLTIP_DELAY_MS} />
        <IconButton icon={Download} label="Segundo" tooltipDelay={TOOLTIP_DELAY_MS} />
      </>,
    );
    await user.hover(screen.getByRole("button", { name: "Primeiro" }));
    await wait(1000);
    await user.hover(screen.getByRole("button", { name: "Segundo" }));
    await wait(1500);
    expect(screen.queryByRole("tooltip")).toBeNull();
    await wait(500);
    expect(screen.getAllByRole("tooltip")).toHaveLength(1);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Segundo");
  });
});

// ---------------------------------------------------------------------------------------------------------------
// Telas com o App inteiro e o backend simulado.

function source(id: string, name: string): SourceSummary {
  return { extensionId: "ext", extensionName: "Extensão", repositoryName: "Repositório", sourceId: id, name, language: "pt-BR", description: null, kinds: ["comic"], rating: "general", direction: "left-to-right", supportsLatest: true, supportsSearch: true };
}

function card(id: number, title: string, overrides: Partial<SeriesCard> = {}): SeriesCard {
  return { id, extensionId: "ext", sourceId: "a", sourceName: "Fonte A", remoteId: String(id), title, hasCover: false, summary: null, author: null, genres: [], status: null, language: "pt-BR", following: false, chapterSort: "asc", lastCheckedAt: null, lastUpdateAt: null, favorite: false, newCount: 0, ratingState: "none", ratingOrigin: "inferred", ...overrides };
}

function chapter(id: number, title: string, overrides: Partial<ChapterView> = {}): ChapterView {
  return { id, remoteId: String(id), title, number: null, publishedAt: null, order: id, state: "available", comicId: null, progress: null, download: null, readState: "unread", isNew: false, firstSeenAt: null, completedAt: null, ...overrides };
}

let handlers: Record<string, (args: Record<string, unknown>) => unknown>;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;

beforeEach(() => {
  calls = [];
  handlers = {
    get_settings: () => ({ "plugins.online": true, onboarding: { status: "completed", step: 4, comicId: null } }),
    set_setting: () => null,
    get_home: () => ({ total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] }),
    list_comics: () => [],
    list_collections: () => [],
    updates_snapshot: () => ({ currentVersion: "0.2.0", available: null, declinedVersion: null, failedVersion: null, welcome: null, updatedAt: null, lastCheckAt: null, needsAdmin: false }),
    repositories_list: () => [],
    repositories_recommended: () => ({ entries: [], origin: "bundled", fetchedAt: null, stale: false, error: null }),
    extension_catalog: () => [],
    extension_catalog_languages: () => [],
    extensions_list: () => [],
    sources_list: () => [],
    series_following: () => [],
    series_library: () => [],
    downloads_list: () => [],
  };
  mockWindows("main");
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    const handler = handlers[cmd];
    return handler ? handler((args ?? {}) as Record<string, unknown>) : null;
  });
});

afterEach(() => {
  cleanup();
  clearMocks();
});

/** Uma promessa que o teste resolve quando quiser: para ver a tela antes de o backend responder. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}

const info = (id: number, readState: ChapterReadInfo["readState"], isNew = false): ChapterReadInfo => ({ id, seriesId: 1, readState, isNew, completedAt: readState === "read" ? 1 : null });

describe("Lista de capítulos da obra", () => {
  // Em ordem de leitura: Um (lido), Dois (em andamento), Três (lendo), Quatro (não lido e novo).
  const CHAPTERS = [
    chapter(1, "Um", { order: 0, readState: "read", completedAt: 1, progress: { pageIndex: 9, pageCount: 10 } }),
    chapter(2, "Dois", { order: 1, readState: "in_progress", progress: { pageIndex: 2, pageCount: 8 } }),
    chapter(3, "Três", { order: 2, readState: "reading", progress: { pageIndex: 0, pageCount: 6 } }),
    chapter(4, "Quatro", { order: 3, isNew: true }),
  ];
  let series: SeriesCard;

  const view = (chapters: ChapterView[]) => ({
    ...series,
    chapters,
    readStatus: "reading",
    chapterCount: chapters.length,
    chaptersRead: 1,
    resume: { chapterId: 2, title: "Dois", progress: { pageIndex: 2, pageCount: 8 }, state: "in_progress" },
    collections: [],
    sourceError: null,
    sourceProtected: false,
  });

  async function openSeries(chapters: ChapterView[] = CHAPTERS) {
    handlers.sources_list = () => [source("a", "Fonte A")];
    handlers.source_popular = () => ({ results: [series], hasMore: false });
    handlers.source_search = () => ({ results: [series], hasMore: false });
    handlers.series_stored = () => view(chapters);
    handlers.series_open = () => view(chapters);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Explorar" }));
    await userEvent.click(await screen.findByRole("button", { name: /Obra/ }));
    await screen.findAllByTestId("chapter");
  }
  const row = (title: string) => screen.getAllByTestId("chapter").find((item) => within(item).queryByText(title, { exact: true }))!;

  beforeEach(() => {
    series = card(1, "Obra");
  });

  it("mostra os quatro estados, e a marca de novo só onde é novo", async () => {
    await openSeries();
    expect(row("Um").dataset.readState).toBe("read");
    expect(within(row("Um")).getByTestId("chapter-read")).toHaveTextContent("Lido");
    expect(row("Dois").dataset.readState).toBe("in_progress");
    expect(within(row("Dois")).getByTestId("chapter-in-progress")).toHaveTextContent("Em andamento");
    expect(row("Dois").textContent).toContain("Página 3 de 8");
    expect(row("Três").dataset.readState).toBe("reading");
    expect(within(row("Três")).getByTestId("chapter-reading")).toHaveTextContent("Lendo");
    expect(row("Quatro").dataset.readState).toBe("unread");
    for (const id of ["chapter-read", "chapter-in-progress", "chapter-reading"]) expect(within(row("Quatro")).queryByTestId(id)).toBeNull();
    // Lido não mostra a página em que parou.
    expect(row("Um").textContent).not.toContain("Página 10 de 10");

    expect(screen.getAllByTestId("chapter-new")).toHaveLength(1);
    expect(within(row("Quatro")).getByTestId("chapter-new")).toHaveTextContent("Novo");
    expect(row("Quatro").dataset.new).toBe("true");
    expect(row("Um").dataset.new).toBeUndefined();
  });

  it("um capítulo lido continua com Ler habilitado e oferece desmarcar", async () => {
    await openSeries();
    const ler = within(row("Um")).getByRole("button", { name: "Ler" });
    expect(ler).toBeEnabled();
    expect(within(row("Um")).getByRole("button", { name: "Marcar como não lido" })).toBeInTheDocument();
    expect(within(row("Um")).queryByRole("button", { name: "Marcar como lido" })).toBeNull();
    handlers.chapter_open = () => new Promise(() => undefined);
    await userEvent.click(ler);
    await waitFor(() => expect(calls.find((c) => c.cmd === "chapter_open")?.args.id).toBe(1));
  });

  it("marcar como lido grava e a linha muda na hora, antes da resposta", async () => {
    const answer = deferred<ChapterReadInfo[]>();
    handlers.chapters_set_read = () => answer.promise;
    await openSeries();
    await userEvent.click(within(row("Quatro")).getByRole("button", { name: "Marcar como lido" }));
    expect(calls.find((c) => c.cmd === "chapters_set_read")?.args).toEqual({ ids: [4], read: true });
    expect(row("Quatro").dataset.readState).toBe("read");
    expect(within(row("Quatro")).getByTestId("chapter-read")).toBeInTheDocument();
    expect(within(row("Quatro")).queryByTestId("chapter-new")).toBeNull();
    // Enquanto a gravação não volta, os botões do capítulo ficam desligados.
    expect(within(row("Quatro")).getByRole("button", { name: "Marcar como não lido" })).toBeDisabled();
    await act(async () => answer.resolve([info(4, "read")]));
    await waitFor(() => expect(within(row("Quatro")).getByRole("button", { name: "Marcar como não lido" })).toBeEnabled());
    expect(row("Quatro").dataset.readState).toBe("read");
  });

  it("marcar como não lido grava read: false e não traz a marca de novo de volta", async () => {
    handlers.chapters_set_read = (args) => (args.ids as number[]).map((id) => info(id, args.read ? "read" : "unread"));
    await openSeries();
    await userEvent.click(within(row("Quatro")).getByRole("button", { name: "Marcar como lido" }));
    await waitFor(() => expect(within(row("Quatro")).getByRole("button", { name: "Marcar como não lido" })).toBeEnabled());
    await userEvent.click(within(row("Quatro")).getByRole("button", { name: "Marcar como não lido" }));
    expect(calls.filter((c) => c.cmd === "chapters_set_read").at(-1)?.args).toEqual({ ids: [4], read: false });
    await waitFor(() => expect(row("Quatro").dataset.readState).toBe("unread"));
    expect(within(row("Quatro")).queryByTestId("chapter-new")).toBeNull();
    expect(row("Quatro").dataset.new).toBeUndefined();
    expect(within(row("Quatro")).getByRole("button", { name: "Marcar como lido" })).toBeInTheDocument();
  });

  it("marcar os anteriores como lidos muda só os capítulos antes dele, pela ordem da fonte", async () => {
    // Ordem da fonte diferente da do id: o anterior de "Três" (order 2) são "Um" e "Dois", não o id 4.
    const chapters = [chapter(1, "Um", { order: 0 }), chapter(2, "Dois", { order: 1 }), chapter(4, "Três", { order: 2 }), chapter(3, "Quatro", { order: 3, isNew: true })];
    const answer = deferred<ChapterReadInfo[]>();
    handlers.chapters_mark_previous_read = () => answer.promise;
    await openSeries(chapters);
    await userEvent.click(within(row("Três")).getByRole("button", { name: "Marcar capítulos anteriores como lidos" }));
    expect(calls.find((c) => c.cmd === "chapters_mark_previous_read")?.args).toEqual({ id: 4 });
    expect(row("Um").dataset.readState).toBe("read");
    expect(row("Dois").dataset.readState).toBe("read");
    expect(row("Três").dataset.readState).toBe("unread");
    expect(row("Quatro").dataset.readState).toBe("unread");
    expect(within(row("Quatro")).getByTestId("chapter-new")).toBeInTheDocument();
    await act(async () => answer.resolve([info(1, "read"), info(2, "read")]));
    await waitFor(() => expect(within(row("Um")).getByRole("button", { name: "Marcar como não lido" })).toBeEnabled());
    expect(row("Três").dataset.readState).toBe("unread");
    expect(row("Quatro").dataset.readState).toBe("unread");
  });

  it("o primeiro capítulo não tem anteriores para marcar", async () => {
    await openSeries();
    expect(within(row("Um")).getByRole("button", { name: "Marcar capítulos anteriores como lidos" })).toBeDisabled();
    expect(within(row("Dois")).getByRole("button", { name: "Marcar capítulos anteriores como lidos" })).toBeEnabled();
  });

  it("baixar um capítulo é um botão só com ícone", async () => {
    handlers.downloads_enqueue = () => [];
    await openSeries();
    const button = within(row("Dois")).getByRole("button", { name: "Baixar capítulo" });
    expect(button.textContent).toBe("");
    await userEvent.click(button);
    await waitFor(() => expect(calls.find((c) => c.cmd === "downloads_enqueue")?.args).toEqual({ chapterIds: [2] }));
    const all = screen.getByRole("button", { name: "Baixar todos os capítulos" });
    expect(all.textContent).toBe("");
  });

  describe("favoritar e acompanhar", () => {
    const icon = (button: HTMLElement) => button.querySelector("svg")!;
    // O jsdom não tem AnimationEvent, e por isso o React escuta o nome com prefixo: dispara os dois.
    const endAnimation = (element: Element) => {
      fireEvent.animationEnd(element);
      fireEvent(element, new Event("webkitAnimationEnd", { bubbles: true }));
    };

    it("favoritar troca o texto, liga o estado e anima só ao ligar", async () => {
      handlers.series_set_favorite = (args) => {
        series = { ...series, favorite: args.favorite as boolean };
        return null;
      };
      await openSeries();
      const button = screen.getByTestId("series-favorite");
      expect(button).toHaveTextContent("Favoritar");
      expect(button).toHaveAttribute("aria-pressed", "false");
      expect(icon(button)).toHaveAttribute("fill", "none");
      expect(icon(button).getAttribute("class")).not.toContain("text-attention");

      await userEvent.click(button);
      await waitFor(() => expect(calls.find((c) => c.cmd === "series_set_favorite")?.args).toEqual({ id: 1, favorite: true }));
      expect(button).toHaveTextContent("Favorito");
      expect(button).toHaveAttribute("aria-pressed", "true");
      expect(icon(button)).toHaveAttribute("fill", "currentColor");
      expect(icon(button).getAttribute("class")).toContain("text-attention");
      expect(icon(button).getAttribute("class")).toContain("p-pop");
      endAnimation(icon(button));
      expect(icon(button).getAttribute("class")).not.toContain("p-pop");

      await userEvent.click(button);
      await waitFor(() => expect(calls.filter((c) => c.cmd === "series_set_favorite").at(-1)?.args).toEqual({ id: 1, favorite: false }));
      expect(button).toHaveTextContent("Favoritar");
      expect(button).toHaveAttribute("aria-pressed", "false");
      expect(icon(button)).toHaveAttribute("fill", "none");
      expect(icon(button).getAttribute("class")).not.toContain("text-attention");
      expect(icon(button).getAttribute("class")).not.toContain("p-pop");
    });

    it("acompanhar troca o texto, liga o estado e balança o sino só ao ligar", async () => {
      handlers.series_set_following = (args) => {
        series = { ...series, following: args.following as boolean };
        return series;
      };
      await openSeries();
      const button = screen.getByTestId("series-follow");
      expect(button).toHaveTextContent("Acompanhar");
      expect(button).toHaveAttribute("aria-pressed", "false");

      await userEvent.click(button);
      await waitFor(() => expect(calls.find((c) => c.cmd === "series_set_following")?.args).toEqual({ id: 1, following: true }));
      await waitFor(() => expect(button).toHaveTextContent("Acompanhando"));
      expect(button).toHaveAttribute("aria-pressed", "true");
      expect(icon(button)).toHaveAttribute("fill", "currentColor");
      expect(icon(button).getAttribute("class")).toContain("text-attention");
      expect(icon(button).getAttribute("class")).toContain("p-ring");
      endAnimation(icon(button));
      expect(icon(button).getAttribute("class")).not.toContain("p-ring");

      await userEvent.click(button);
      await waitFor(() => expect(calls.filter((c) => c.cmd === "series_set_following").at(-1)?.args).toEqual({ id: 1, following: false }));
      await waitFor(() => expect(button).toHaveTextContent("Acompanhar"));
      expect(button).toHaveAttribute("aria-pressed", "false");
      expect(icon(button)).toHaveAttribute("fill", "none");
      expect(icon(button).getAttribute("class")).not.toContain("p-ring");
    });
  });
});

describe("Obras na biblioteca", () => {
  function librarySeries(id: number, title: string, overrides: Partial<LibrarySeries> = {}): LibrarySeries {
    return { id, title, hasCover: false, sourceName: "Fonte A", favorite: false, addedAt: id, lastOpenedAt: null, following: true, newCount: 0, ratingState: "none", readStatus: "unread", chapterCount: 10, chaptersRead: 0, resume: null, collections: [], ...overrides };
  }

  it("mostra quantos capítulos novos a obra tem, só acima de zero", async () => {
    handlers.series_library = () => [librarySeries(1, "Com novos", { newCount: 5 }), librarySeries(2, "Sem novos")];
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Minha biblioteca" }));
    const badges = await screen.findAllByTestId("new-count");
    expect(badges).toHaveLength(1);
    expect(badges[0]).toHaveTextContent("5");
    expect(badges[0]!.closest("[data-series-id]")?.getAttribute("data-series-id")).toBe("1");
  });

  it("onde parou diz o capítulo e o estado, e leva direto a ele", async () => {
    handlers.series_library = () => [librarySeries(1, "Obra", { readStatus: "reading", chaptersRead: 2, resume: { chapterId: 33, title: "Capítulo 3", progress: { pageIndex: 4, pageCount: 20 }, state: "in_progress" } })];
    handlers.chapter_open = () => new Promise(() => undefined);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Minha biblioteca" }));
    const resume = await screen.findByTestId("resume-chapter");
    expect(resume).toHaveTextContent("Capítulo 3 · Em andamento");
    await userEvent.click(resume);
    await waitFor(() => expect(calls.find((c) => c.cmd === "chapter_open")?.args.id).toBe(33));
  });
});
