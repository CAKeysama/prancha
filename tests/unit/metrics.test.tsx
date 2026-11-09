// Medições de navegação com o backend simulado: quantas consultas cada visita faz e quanto tempo a tela leva para mostrar algo.
// Os tempos dependem das esperas simuladas abaixo (fonte rápida, média e lenta), não de uma fonte de verdade.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { App } from "../../src/app/App";
import type { SeriesCard, SourceSummary } from "../../src/domain/online";

const DELAYS: Record<string, number> = { a: 50, b: 300, c: 900 };

function source(id: string): SourceSummary {
  return { extensionId: "ext", extensionName: "Extensão", repositoryName: "Repositório", sourceId: id, name: `Fonte ${id.toUpperCase()}`, language: "pt-BR", description: null, kinds: ["comic"], rating: "general", direction: "left-to-right", supportsLatest: true, supportsSearch: true };
}

function card(id: number, sourceId: string): SeriesCard {
  return { id, extensionId: "ext", sourceId, sourceName: null, remoteId: String(id), title: `Obra ${sourceId}${id}`, hasCover: false, summary: null, author: null, genres: [], status: null, language: "pt-BR", following: false, chapterSort: "desc", lastCheckedAt: null, lastUpdateAt: null, favorite: false, newCount: 0 };
}

let calls: string[];
/** `PRANCHA_METRICS=1 npx vitest run tests/unit/metrics.test.tsx --silent=false` mostra os números. */
const report = (values: Record<string, unknown>) => {
  if ((globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.PRANCHA_METRICS) console.log(JSON.stringify(values));
};

beforeEach(() => {
  calls = [];
  const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
    get_settings: () => ({ "plugins.online": true, onboarding: { status: "completed", step: 4, comicId: null }, "online.searchHistory": [] }),
    set_setting: () => null,
    get_home: () => ({ total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] }),
    list_comics: () => [],
    list_collections: () => [],
    updates_snapshot: () => ({ currentVersion: "0.2.0", available: null, declinedVersion: null, failedVersion: null, welcome: null, updatedAt: null, lastCheckAt: null, needsAdmin: false }),
    sources_list: () => [source("a"), source("b"), source("c")],
    series_following: () => [],
    series_library: () => [],
    downloads_list: () => [],
    source_popular: (args) => {
      const id = String(args.sourceId);
      const base = id.charCodeAt(0) * 100;
      return new Promise((resolve) => setTimeout(() => resolve({ results: Array.from({ length: 10 }, (_, i) => card(base + i, id)), hasMore: true, unverified: 0 }), DELAYS[id]));
    },
  };
  mockWindows("main");
  mockIPC(
    (cmd, args) => {
      calls.push(cmd);
      const handler = handlers[cmd];
      return handler ? handler((args ?? {}) as Record<string, unknown>) : null;
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  cleanup();
  clearMocks();
});

const count = (cmd: string) => calls.filter((c) => c === cmd).length;

async function timed(action: () => Promise<unknown>) {
  const start = performance.now();
  await action();
  return Math.round(performance.now() - start);
}

describe("métricas de navegação", () => {
  it("Explorar: primeira visita, volta depois de outra tela e mudança na biblioteca", async () => {
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Explorar" }));
    const first = await timed(() => screen.findByRole("button", { name: /Obra a9700$/ }, { timeout: 5000 }));
    const all = first + (await timed(() => screen.findByRole("button", { name: /Obra c9900$/ }, { timeout: 5000 })));
    const firstVisit = count("source_popular");

    await userEvent.click(screen.getByRole("button", { name: "Minha biblioteca" }));
    await screen.findByRole("heading", { name: "Minha biblioteca" });
    await userEvent.click(screen.getByRole("button", { name: "Explorar" }));
    const back = await timed(() => screen.findByRole("button", { name: /Obra c9900$/ }, { timeout: 5000 }));
    const onReturn = count("source_popular") - firstVisit;

    const beforeEvent = count("source_popular");
    await act(() => emit("library:changed", {}));
    await new Promise((resolve) => setTimeout(resolve, 1500));
    const onLibraryEvent = count("source_popular") - beforeEvent;

    report({ firstResultsMs: first, allSourcesMs: all, requestsFirstVisit: firstVisit, returnUntilAllShownMs: back, requestsOnReturn: onReturn, requestsAfterLibraryEvent: onLibraryEvent });
    expect(firstVisit).toBe(3);
    expect(onReturn).toBe(0);
    expect(onLibraryEvent).toBe(0);
  }, 20000);

  it("Obra: abrir, voltar para Explorar e abrir de novo", async () => {
    const view = { ...card(9700, "a"), chapters: [], readStatus: "unread", chapterCount: 0, chaptersRead: 0, resume: null, collections: [], sourceError: null, sourceProtected: false };
    mockIPC(
      (cmd, args) => {
        calls.push(cmd);
        if (cmd === "series_stored") return view;
        if (cmd === "series_open") return new Promise((resolve) => setTimeout(() => resolve(view), 400));
        if (cmd === "sources_list") return [source("a")];
        if (cmd === "source_popular") return { results: [card(9700, "a")], hasMore: false, unverified: 0 };
        if (cmd === "get_settings") return { "plugins.online": true, onboarding: { status: "completed", step: 4, comicId: null } };
        if (cmd === "get_home") return { total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] };
        void args;
        return cmd.endsWith("_list") || cmd === "list_comics" || cmd === "series_following" || cmd === "series_library" ? [] : null;
      },
      { shouldMockEvents: true },
    );
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Explorar" }));
    await userEvent.click(await screen.findByRole("button", { name: /Obra a9700$/ }, { timeout: 5000 }));
    await screen.findByRole("heading", { name: "Obra a9700" });
    await new Promise((resolve) => setTimeout(resolve, 600));
    const browseBefore = count("source_popular");
    await userEvent.click(screen.getByRole("button", { name: "Voltar" }));
    const backToExplore = await timed(() => screen.findByRole("button", { name: /Obra a9700$/ }, { timeout: 5000 }));
    const openBefore = count("series_open");
    await userEvent.click(screen.getByRole("button", { name: /Obra a9700$/ }));
    await new Promise((resolve) => setTimeout(resolve, 600));
    report({ backToExploreMs: backToExplore, browseRequestsOnBack: count("source_popular") - browseBefore, seriesOpenOnReopen: count("series_open") - openBefore });
    expect(openBefore).toBe(1);
    expect(count("source_popular")).toBe(browseBefore);
    expect(count("series_open")).toBe(openBefore);
  }, 20000);

  it("Início: volta depois de outra tela", async () => {
    render(<App />);
    await screen.findByRole("button", { name: "Minha biblioteca" });
    const before = count("get_home");
    await userEvent.click(screen.getByRole("button", { name: "Minha biblioteca" }));
    await userEvent.click(screen.getByRole("button", { name: "Início" }));
    const spinnerOnReturn = screen.queryByText("Carregando…") !== null;
    report({ homeRequestsOnReturn: count("get_home") - before, spinnerOnReturn });
    expect(before).toBeGreaterThan(0);
  });
});
