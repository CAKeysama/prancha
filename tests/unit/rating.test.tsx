// Classificação +18: preferências, selos e correção, com o backend simulado. Quem esconde obras é o backend
// (testado em Rust); aqui, que toda tela marca do mesmo jeito o que aparece e que a correção chega ao backend.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../../src/app/App";
import type { LibrarySeries, SeriesCard, SourceSummary } from "../../src/domain/online";
import { ratingBadge, type RatingDetails, type RatingState } from "../../src/domain/rating";
import { sanitizeSettings } from "../../src/domain/settings";

let handlers: Record<string, (args: Record<string, unknown>) => unknown>;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;
let settings: Record<string, unknown>;

function card(id: number, title: string, ratingState: RatingState): SeriesCard {
  return {
    id,
    extensionId: "ext",
    sourceId: "a",
    sourceName: "Fonte A",
    remoteId: String(id),
    title,
    hasCover: false,
    summary: null,
    author: null,
    genres: [],
    status: null,
    language: "pt-BR",
    following: true,
    chapterSort: "desc",
    lastCheckedAt: null,
    lastUpdateAt: null,
    favorite: false,
    newCount: 0,
    ratingState,
    ratingOrigin: "inferred",
  };
}

function librarySeries(id: number, title: string, ratingState: RatingState): LibrarySeries {
  return { id, title, hasCover: false, sourceName: "Fonte A", favorite: true, addedAt: id, lastOpenedAt: null, following: true, newCount: 0, ratingState, readStatus: "unread", chapterCount: 1, chaptersRead: 0, resume: null, collections: [] };
}

const source: SourceSummary = {
  extensionId: "ext",
  extensionName: "Extensão",
  repositoryName: "Repositório",
  sourceId: "a",
  name: "Fonte A",
  language: "pt-BR",
  description: null,
  kinds: ["comic"],
  rating: "general",
  direction: "left-to-right",
  supportsLatest: true,
  supportsSearch: true,
};

const SHOWN = [card(1, "Livre", "none"), card(2, "Suspeita", "suspect"), card(3, "Sem dados", "insufficient"), card(4, "Adulta", "confirmed")];

beforeEach(() => {
  calls = [];
  settings = { "plugins.online": true, onboarding: { status: "completed", step: 4, comicId: null } };
  handlers = {
    get_settings: () => settings,
    set_setting: (args) => {
      settings = { ...settings, [args.key as string]: args.value };
      return null;
    },
    get_home: () => ({ total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] }),
    list_comics: () => [],
    list_collections: () => [],
    updates_snapshot: () => ({ currentVersion: "0.5.0", available: null, declinedVersion: null, failedVersion: null, welcome: null, updatedAt: null, lastCheckAt: null, needsAdmin: false }),
    sources_list: () => [source],
    source_popular: () => ({ results: SHOWN, hasMore: false, hidden: 0, unverified: 0 }),
    series_following: () => SHOWN,
    series_library: () => SHOWN.map((item) => librarySeries(item.id, item.title, item.ratingState)),
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

async function open(section: string) {
  render(<App />);
  await userEvent.click(await screen.findByRole("button", { name: section }));
}

/** O selo de cada obra visível, pelo título. */
async function marks(): Promise<Record<string, string | null>> {
  const out: Record<string, string | null> = {};
  for (const item of SHOWN) {
    const cover = await screen.findByRole("img", { name: `Capa substituta de ${item.title}` });
    out[item.title] = within(cover).queryByTestId("rating-mark")?.getAttribute("title") ?? null;
  }
  return out;
}

describe("selo da classificação", () => {
  it("segue uma regra só", () => {
    expect(ratingBadge("confirmed", "show")?.label).toBe("+18");
    expect(ratingBadge("suspect", "mark")?.label).toBe("+18?");
    expect(ratingBadge("insufficient", "hide")?.label).toBe("?");
    expect(ratingBadge("suspect", "show")).toBeNull();
    expect(ratingBadge("none", "mark")).toBeNull();
    expect(ratingBadge(undefined, "mark")).toBeNull();
  });

  it("a preferência inválida ou ausente volta para esconder", () => {
    expect(sanitizeSettings({})["content.suspect"]).toBe("hide");
    expect(sanitizeSettings({ "content.suspect": "liberar" })["content.suspect"]).toBe("hide");
    expect(sanitizeSettings({ "content.suspect": "mark" })["content.suspect"]).toBe("mark");
  });

  const EXPECTED_MARKED = { Livre: null, Suspeita: "Suspeito", "Sem dados": "Sem informações suficientes", Adulta: "Confirmado +18" };
  const EXPECTED_SHOWN = { Livre: null, Suspeita: null, "Sem dados": null, Adulta: "Confirmado +18" };

  it("Explorar, Acompanhando e a biblioteca marcam igual, com aviso", async () => {
    settings["content.suspect"] = "mark";
    await open("Explorar");
    expect(await marks()).toEqual(EXPECTED_MARKED);
    await userEvent.click(screen.getByRole("radio", { name: /Acompanhando/ }));
    expect(await marks()).toEqual(EXPECTED_MARKED);
    await userEvent.click(screen.getByRole("button", { name: "Minha biblioteca" }));
    expect(await marks()).toEqual(EXPECTED_MARKED);
  });

  it("com 'mostrar', só a confirmada leva selo, em todas as telas", async () => {
    settings["content.suspect"] = "show";
    await open("Explorar");
    expect(await marks()).toEqual(EXPECTED_SHOWN);
    await userEvent.click(screen.getByRole("button", { name: "Minha biblioteca" }));
    expect(await marks()).toEqual(EXPECTED_SHOWN);
  });
});

describe("preferência de obras suspeitas", () => {
  it("aparece com o filtro ligado, separada da das confirmadas, e fica salva", async () => {
    await open("Plugins");
    const group = await screen.findByRole("radiogroup", { name: "Obras suspeitas ou sem informações" });
    expect(within(group).getByRole("radio", { name: /Esconder/ }).getAttribute("aria-checked")).toBe("true");
    await userEvent.click(within(group).getByRole("radio", { name: /Mostrar com aviso/ }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "set_setting" && c.args.key === "content.suspect")?.args.value).toBe("mark"));
    expect(calls.some((c) => c.cmd === "set_setting" && c.args.key === "content.adult")).toBe(false);
  });

  it("com o filtro desligado, tudo aparece e a escolha some da tela", async () => {
    settings["content.adult"] = true;
    await open("Plugins");
    await screen.findByRole("switch", { name: "Esconder conteúdo +18" });
    expect(screen.queryByRole("radiogroup", { name: "Obras suspeitas ou sem informações" })).toBeNull();
  });
});

describe("detalhes e correção", () => {
  const details = (overrides: Partial<RatingDetails> = {}): RatingDetails => ({
    state: "suspect",
    automaticState: "suspect",
    score: 59,
    origin: "inferred",
    ratedAt: 1_700_000_000_000,
    classifierVersion: 1,
    evidence: [
      { kind: "tag", origin: "Categorias de Fonte A", value: "Mature", reliability: 0.9, points: 65, verdict: "suggestive", observedAt: 1 },
      { kind: "tag", origin: "Categorias de Fonte A", value: "Young Adult", reliability: 0.9, points: 0, verdict: "ignored", observedAt: 1 },
    ],
    manual: null,
    history: [],
    ...overrides,
  });

  it("mostra o motivo só quando pedido, e a correção vai para o backend", async () => {
    settings["content.suspect"] = "mark";
    const view = { ...card(2, "Suspeita", "suspect"), chapters: [], readStatus: "unread", chapterCount: 0, chaptersRead: 0, resume: null, collections: [], sourceError: null, sourceProtected: false };
    handlers.series_stored = () => view;
    handlers.series_open = () => view;
    handlers.series_rating = () => details();
    handlers.series_set_rating = (args) => details({ state: args.rating as RatingState, origin: "manual", manual: { state: args.rating as RatingState, reason: args.reason as string, setAt: 2 } });
    await open("Explorar");
    await userEvent.click(await screen.findByRole("button", { name: /Suspeita/ }));

    const fact = (await screen.findByText("Classificação")).parentElement!;
    expect(fact.textContent).toContain("Suspeito");
    expect(fact.textContent).toContain("Deduzida pelo Prancha");
    expect(screen.queryByText("Pontuação de risco")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Ver motivo ou corrigir" }));
    const dialog = await screen.findByRole("dialog", { name: "Classificação +18" });
    expect(await within(dialog).findByText("59 de 100")).toBeTruthy();
    const evidence = within(dialog).getByRole("list", { name: "Evidências" });
    expect(evidence.textContent).toContain("Mature");
    expect(evidence.textContent).toContain("desconsiderada");

    await userEvent.type(within(dialog).getByLabelText("Motivo da correção (opcional)"), "li a obra");
    await userEvent.click(within(dialog).getByRole("button", { name: "Não é +18" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "series_set_rating")?.args).toEqual({ id: 2, rating: "none", reason: "li a obra" }));
    expect(await within(dialog).findByText("Corrigida por você")).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "Voltar ao automático" })).toBeTruthy();
  });
});
