// Cache da guia Explorar e das outras telas: o que já foi carregado continua ao sair e voltar, e só se consulta de novo o que venceu.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { App } from "../../src/app/App";
import { useScrollMemory } from "../../src/app/scroll-memory";
import type { SeriesCard, SourceSummary } from "../../src/domain/online";
import { EXPLORE_TTL_MS, ensure, exploreStats, sectionOf, type Search } from "../../src/features/online/explore-store";
import { fetchInto, peek } from "../../src/services/query-cache";

function source(id: string): SourceSummary {
  return { extensionId: "ext", extensionName: "Extensão", repositoryName: "Repositório", sourceId: id, name: `Fonte ${id.toUpperCase()}`, language: "pt-BR", description: null, kinds: ["comic"], rating: "general", direction: "left-to-right", supportsLatest: true, supportsSearch: true };
}

function card(id: number, title: string, sourceId = "a"): SeriesCard {
  return { id, extensionId: "ext", sourceId, sourceName: null, remoteId: String(id), title, hasCover: false, summary: null, author: null, genres: [], status: null, language: "pt-BR", following: false, chapterSort: "desc", lastCheckedAt: null, lastUpdateAt: null, favorite: false, newCount: 0 };
}

/** Uma resposta que o teste libera quando quiser. */
function later<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}

let handlers: Record<string, (args: Record<string, unknown>) => unknown>;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;
let now = 1_000_000;

beforeEach(() => {
  calls = [];
  now = 1_000_000;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  handlers = {
    get_settings: () => ({ "plugins.online": true, onboarding: { status: "completed", step: 4, comicId: null } }),
    set_setting: () => null,
    get_home: () => ({ total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] }),
    list_comics: () => [],
    list_collections: () => [],
    updates_snapshot: () => ({ currentVersion: "0.2.0", available: null, declinedVersion: null, failedVersion: null, welcome: null, updatedAt: null, lastCheckAt: null, needsAdmin: false }),
    sources_list: () => [source("a"), source("b")],
    series_following: () => [],
    series_library: () => [],
    downloads_list: () => [],
    source_popular: (args) => ({ results: [card(args.sourceId === "a" ? 1 : 2, `Obra ${String(args.sourceId)}`, String(args.sourceId))], hasMore: false, unverified: 0 }),
    source_search: () => ({ results: [], hasMore: false, unverified: 0 }),
  };
  mockWindows("main");
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
      const handler = handlers[cmd];
      return handler ? handler((args ?? {}) as Record<string, unknown>) : null;
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

const browsed = (sourceId?: string) => calls.filter((c) => c.cmd === "source_popular" && (!sourceId || c.args.sourceId === sourceId)).length;

async function openExplore() {
  render(<App />);
  await userEvent.click(await screen.findByRole("button", { name: "Explorar" }));
  // Os testes de online.test.tsx deixam a pesquisa guardada; aqui todos começam em Populares.
  const clear = screen.queryByRole("button", { name: "Limpar pesquisa" });
  if (clear) await userEvent.click(clear);
  const popular = await screen.findByRole("radio", { name: /Populares/ });
  await userEvent.click(popular);
}

async function leaveAndReturn() {
  await userEvent.click(screen.getByRole("button", { name: "Minha biblioteca" }));
  await screen.findByRole("heading", { name: "Minha biblioteca" });
  await userEvent.click(screen.getByRole("button", { name: "Explorar" }));
}

describe("cache da guia Explorar", () => {
  it("primeiro acesso: cada fonte é consultada uma vez", async () => {
    await openExplore();
    expect(await screen.findByRole("button", { name: /Obra a/ })).toBeTruthy();
    expect(await screen.findByRole("button", { name: /Obra b/ })).toBeTruthy();
    expect(browsed("a")).toBe(1);
    expect(browsed("b")).toBe(1);
  });

  it("voltar à guia mostra os resultados na hora, sem consultar as fontes", async () => {
    await openExplore();
    await screen.findByRole("button", { name: /Obra b/ });
    const before = browsed();
    const reused = exploreStats().reused;
    await leaveAndReturn();
    // Já estão na tela no primeiro desenho, sem esqueleto nem espera.
    expect(screen.getByRole("button", { name: /Obra a/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Obra b/ })).toBeTruthy();
    expect(screen.queryByText(/Consultando/)).toBeNull();
    expect(browsed()).toBe(before);
    expect(exploreStats().reused).toBe(reused + 2);
  });

  it("depois do prazo, mostra a lista anterior e atualiza em segundo plano", async () => {
    await openExplore();
    await screen.findByRole("button", { name: /Obra b/ });
    const fresh = later<unknown>();
    handlers.source_popular = (args) => (args.sourceId === "a" ? fresh.promise : { results: [card(2, "Obra b")], hasMore: false, unverified: 0 });
    now += EXPLORE_TTL_MS + 1;
    await leaveAndReturn();
    // A lista antiga continua enquanto a nova não chega.
    expect(screen.getByRole("button", { name: /Obra a/ })).toBeTruthy();
    expect(await screen.findByText("Atualizando…")).toBeTruthy();
    await waitFor(() => expect(browsed("a")).toBe(2));
    await act(async () => fresh.resolve({ results: [card(5, "Obra nova")], hasMore: false, unverified: 0 }));
    expect(await screen.findByRole("button", { name: /Obra nova/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Obra a/ })).toBeNull();
  });

  it("a atualização manual consulta de novo, mesmo dentro do prazo", async () => {
    await openExplore();
    await screen.findByRole("button", { name: /Obra b/ });
    await userEvent.click(screen.getByRole("button", { name: "Atualizar" }));
    await waitFor(() => expect(browsed("a")).toBe(2));
    expect(browsed("b")).toBe(2);
    expect(screen.getByRole("button", { name: /Obra a/ })).toBeTruthy();
  });

  it("pedidos iguais ao mesmo tempo viram uma consulta só", async () => {
    const pending = later<unknown>();
    handlers.source_popular = () => pending.promise;
    const search: Search = { mode: "popular", query: "", kind: "" };
    ensure(search, [source("a")]);
    ensure(search, [source("a")]);
    ensure(search, [source("a")]);
    await waitFor(() => expect(browsed("a")).toBe(1));
    await act(async () => pending.resolve({ results: [card(1, "Uma")], hasMore: false, unverified: 0 }));
    await waitFor(() => expect(sectionOf(search, source("a"))?.status).toBe("ready"));
    expect(browsed("a")).toBe(1);

    // O cache das outras telas faz o mesmo.
    let loads = 0;
    const slow = later<number>();
    const load = () => {
      loads++;
      return slow.promise;
    };
    const first = fetchInto("teste", load);
    const second = fetchInto("teste", load);
    expect(first).toBe(second);
    slow.resolve(7);
    await first;
    expect(loads).toBe(1);
    expect(peek<number>("teste").data).toBe(7);
  });

  it("uma fonte lenta não segura as outras, e uma fora do ar não apaga nada", async () => {
    const slow = later<unknown>();
    handlers.sources_list = () => [source("a"), source("b"), source("c")];
    handlers.source_popular = (args) => {
      if (args.sourceId === "b") return slow.promise;
      if (args.sourceId === "c") throw { code: "source_failed", message: "Fora do ar." };
      return { results: [card(1, "Obra a")], hasMore: false, unverified: 0 };
    };
    await openExplore();
    expect(await screen.findByRole("button", { name: /Obra a/ })).toBeTruthy();
    expect(await within(screen.getByRole("region", { name: "Fonte C" })).findByText("Fora do ar.")).toBeTruthy();
    expect(screen.getByText(/Consultando 1 de 3 fontes/)).toBeTruthy();
    await act(async () => slow.resolve({ results: [card(2, "Obra b", "b")], hasMore: false, unverified: 0 }));
    expect(await screen.findByRole("button", { name: /Obra b/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Obra a/ })).toBeTruthy();
  });

  it("uma atualização que falha mantém a lista anterior e avisa sem alarde", async () => {
    await openExplore();
    await screen.findByRole("button", { name: /Obra b/ });
    handlers.source_popular = () => {
      throw { code: "network", message: "Sem conexão." };
    };
    await userEvent.click(screen.getByRole("button", { name: "Atualizar" }));
    expect(await within(screen.getByRole("region", { name: "Fonte A" })).findByText(/Não deu para atualizar agora/)).toBeTruthy();
    expect(screen.getByRole("button", { name: /Obra a/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Obra b/ })).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("a fonte que falhou não é consultada a cada volta, só depois de um tempo", async () => {
    handlers.source_popular = (args) => {
      if (args.sourceId === "b") throw { code: "source_failed", message: "Fora do ar." };
      return { results: [card(1, "Obra a")], hasMore: false, unverified: 0 };
    };
    await openExplore();
    await screen.findByText("Fora do ar.");
    await leaveAndReturn();
    expect(screen.getByText("Fora do ar.")).toBeTruthy();
    expect(browsed("b")).toBe(1);
    now += 61_000;
    await leaveAndReturn();
    await waitFor(() => expect(browsed("b")).toBe(2));
    expect(browsed("a")).toBe(1);
  });

  it("filtros e pesquisa continuam ao voltar", async () => {
    await openExplore();
    await screen.findByRole("button", { name: /Obra b/ });
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Fonte" }), "ext/b");
    expect(screen.queryByRole("region", { name: "Fonte A" })).toBeNull();
    await leaveAndReturn();
    expect((screen.getByRole("combobox", { name: "Fonte" }) as HTMLSelectElement).value).toBe("ext/b");
    expect(screen.getByRole("region", { name: "Fonte B" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Fonte A" })).toBeNull();
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Fonte" }), "");
    expect(browsed()).toBe(2);
  });

  it("mudanças na biblioteca e a lista de fontes lida de novo não recomeçam a guia", async () => {
    await openExplore();
    await screen.findByRole("button", { name: /Obra b/ });
    const before = browsed();
    // Um download que termina avisa a biblioteca; a lista de fontes volta igual.
    await act(() => emit("library:changed", {}));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "sources_list").length).toBeGreaterThan(1), { timeout: 2000 });
    expect(browsed()).toBe(before);
    expect(screen.getByRole("button", { name: /Obra a/ })).toBeTruthy();
  });

  it("a resposta de uma consulta antiga não passa por cima da mais nova", async () => {
    const old = later<unknown>();
    const fresh = later<unknown>();
    const answers = [old, fresh];
    handlers.source_popular = () => answers.shift()!.promise;
    const search: Search = { mode: "popular", query: "", kind: "" };
    ensure(search, [source("a")]);
    await waitFor(() => expect(browsed("a")).toBe(1));
    // A pessoa pede para tentar de novo antes de a primeira responder.
    const { retry } = await import("../../src/features/online/explore-store");
    retry(search, source("a"));
    await waitFor(() => expect(browsed("a")).toBe(2));
    fresh.resolve({ results: [card(2, "Nova")], hasMore: false, unverified: 0 });
    await waitFor(() => expect(sectionOf(search, source("a"))?.status).toBe("ready"));
    old.resolve({ results: [card(1, "Antiga")], hasMore: false, unverified: 0 });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(sectionOf(search, source("a"))?.results.map((item) => item.title)).toEqual(["Nova"]);
  });
});

describe("as outras telas", () => {
  it("o Início volta com o que tinha, sem tela de carregamento", async () => {
    handlers.get_home = () => ({ total: 1, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] });
    render(<App />);
    await screen.findByRole("button", { name: "Minha biblioteca" });
    await waitFor(() => expect(calls.some((c) => c.cmd === "get_home")).toBe(true));
    const html = () => screen.getByRole("main").innerHTML;
    await waitFor(() => expect(screen.queryByText("Carregando…")).toBeNull());
    const shown = html();
    await userEvent.click(screen.getByRole("button", { name: "Minha biblioteca" }));
    await userEvent.click(screen.getByRole("button", { name: "Início" }));
    expect(html()).toBe(shown);
  });
});

describe("obra e biblioteca", () => {
  const view = { ...card(1, "Obra a"), chapters: [], readStatus: "unread", chapterCount: 0, chaptersRead: 0, resume: null, collections: [], sourceError: null, sourceProtected: false };

  it("a obra aberta há pouco volta com o que está gravado, sem consultar a fonte", async () => {
    handlers.series_stored = () => view;
    handlers.series_open = () => view;
    await openExplore();
    await userEvent.click(await screen.findByRole("button", { name: /Obra a/ }));
    await screen.findByRole("heading", { name: "Obra a" });
    await waitFor(() => expect(calls.filter((c) => c.cmd === "series_open")).toHaveLength(1));
    await userEvent.click(screen.getByRole("button", { name: "Voltar" }));
    // A guia Explorar volta como estava.
    await userEvent.click(screen.getByRole("button", { name: /Obra a/ }));
    expect(screen.getByRole("heading", { name: "Obra a" })).toBeTruthy();
    await waitFor(() => expect(calls.filter((c) => c.cmd === "series_stored")).toHaveLength(2));
    expect(calls.filter((c) => c.cmd === "series_open")).toHaveLength(1);
    expect(browsed()).toBe(2);
  });

  it("a pesquisa da biblioteca continua depois de sair e voltar", async () => {
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Minha biblioteca" }));
    await userEvent.type(await screen.findByPlaceholderText("Pesquisar"), "batman");
    await userEvent.click(screen.getByRole("button", { name: "Início" }));
    await userEvent.click(screen.getByRole("button", { name: "Minha biblioteca" }));
    expect((screen.getByPlaceholderText("Pesquisar") as HTMLInputElement).value).toBe("batman");
  });
});

describe("posição da rolagem", () => {
  it("cada chave volta para onde estava, e uma chave nova começa do topo", () => {
    const element = document.createElement("div");
    let top = 0;
    Object.defineProperty(element, "scrollTop", { configurable: true, get: () => top, set: (value: number) => (top = value) });
    element.scrollTo = ((_: number, y: number) => (top = y)) as unknown as typeof element.scrollTo;

    const first = renderHook(({ id }) => useScrollMemory<HTMLDivElement>(id), { initialProps: { id: "lista" } });
    act(() => first.result.current(element));
    top = 480;
    element.dispatchEvent(new Event("scroll"));
    first.unmount();

    top = 0;
    const again = renderHook(() => useScrollMemory<HTMLDivElement>("lista"));
    act(() => again.result.current(element));
    expect(top).toBe(480);
    again.unmount();

    const other = renderHook(() => useScrollMemory<HTMLDivElement>("outra"));
    act(() => other.result.current(element));
    expect(top).toBe(0);
  });
});
