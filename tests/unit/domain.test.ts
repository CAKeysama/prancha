import { describe, expect, it } from "vitest";
import { homeItems, mergeItems, seriesMatches, seriesProgressLabel, seriesRatio } from "../../src/domain/library-items";
import type { LibrarySeries } from "../../src/domain/online";
import type { ComicSummary } from "../../src/domain/types";
import { formatBytes, progressLabel, progressRatio, readActionLabel, subtitle } from "../../src/domain/progress";
import {
  buildSpreads,
  chapterAt,
  chapterLeftBehind,
  clampPage,
  hasTallPages,
  isContinuous,
  nextMode,
  pageIndicator,
  spreadHeight,
  spreadIndexOf,
  verticalPageWidth,
  webtoonHint,
  webtoonWidth,
  zoomIn,
  zoomOut,
} from "../../src/domain/reader";
import { DEFAULT_SETTINGS, resolveTheme, sanitizeSettings } from "../../src/domain/settings";
import { DEFAULT_SHORTCUTS, actionForKey, conflictFor, formatKey, keyFromEvent, resolveArrow, sanitizeShortcuts } from "../../src/domain/shortcuts";

const base = { pageCount: 186, progress: null, status: "unread" as const };

describe("indicador de progresso", () => {
  it("nunca inventa posição para itens não iniciados", () => {
    expect(progressLabel(base)).toBe("Não iniciado");
    expect(progressRatio(base)).toBeNull();
  });
  it("mostra a página salva e o total real", () => {
    const c = { ...base, status: "reading" as const, progress: { pageIndex: 41, pageCount: 186, scrollRatio: 0, updatedAt: 0 } };
    expect(progressLabel(c)).toBe("Parou na página 42 de 186");
    expect(progressRatio(c)).toBeCloseTo(42 / 186);
    expect(readActionLabel(c)).toBe("Continuar leitura");
  });
  it("usa o total confirmado pelo leitor e omite total desconhecido", () => {
    const c = { pageCount: null, status: "reading" as const, progress: { pageIndex: 4, pageCount: 0, scrollRatio: 0, updatedAt: 0 } };
    expect(progressLabel(c)).toBe("Parou na página 5");
    expect(progressRatio(c)).toBeNull();
  });
  it("leitura concluída", () => {
    const c = { ...base, status: "completed" as const };
    expect(progressLabel(c)).toBe("Leitura concluída");
    expect(progressRatio(c)).toBe(1);
    expect(readActionLabel(c)).toBe("Ler novamente");
  });
  it("subtítulo e tamanhos", () => {
    expect(subtitle({ series: "Saga", volume: "2", issueNumber: "7" })).toBe("Saga · vol. 2 · #7");
    expect(subtitle({ series: null, volume: null, issueNumber: null })).toBeNull();
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1,5 KB");
  });
});

describe("visões de página", () => {
  const none = new Map();
  it("página única", () => {
    expect(buildSpreads(3, "single", true, none)).toEqual([[0], [1], [2]]);
  });
  it("página dupla com capa sozinha", () => {
    expect(buildSpreads(6, "double", true, none)).toEqual([[0], [1, 2], [3, 4], [5]]);
    expect(buildSpreads(4, "double", false, none)).toEqual([[0, 1], [2, 3]]);
  });
  it("páginas largas nunca são pareadas", () => {
    const sizes = new Map([[2, { width: 2000, height: 1400 }]]);
    expect(buildSpreads(5, "double", true, sizes)).toEqual([[0], [1], [2], [3, 4]]);
  });
  it("página dupla não junta páginas de capítulos diferentes", () => {
    const starts = new Set([3]);
    expect(buildSpreads(6, "double", false, none, starts)).toEqual([[0, 1], [2], [3, 4], [5]]);
    expect(buildSpreads(6, "double", true, none, starts)).toEqual([[0], [1, 2], [3], [4, 5]]);
    expect(buildSpreads(6, "single", true, none, starts)).toEqual([[0], [1], [2], [3], [4], [5]]);
  });
  it("sabe em que capítulo cada página está e quando um foi lido até o fim", () => {
    const chapter = (first: number, count: number) => ({ title: "c", first, count, guidedComicId: null });
    const chapters = [chapter(0, 6), chapter(6, 40), chapter(46, 5)];
    expect(chapterAt(chapters, 0)).toBe(chapters[0]);
    expect(chapterAt(chapters, 5)).toBe(chapters[0]);
    expect(chapterAt(chapters, 6)).toBe(chapters[1]);
    expect(chapterAt(chapters, 99)).toBe(chapters[2]);
    expect(chapterLeftBehind(chapters, 5, 6)).toBe(chapters[0]);
    // Página dupla e rolagem rápida não caem exatamente na última página.
    expect(chapterLeftBehind(chapters, 4, 6)).toBe(chapters[0]);
    expect(chapterLeftBehind(chapters, 42, 46)).toBe(chapters[1]);
    // Pular um capítulo, voltar ou andar dentro do mesmo não conta.
    expect(chapterLeftBehind(chapters, 0, 6)).toBeNull();
    expect(chapterLeftBehind(chapters, 5, 46)).toBeNull();
    expect(chapterLeftBehind(chapters, 6, 5)).toBeNull();
    expect(chapterLeftBehind(chapters, 4, 5)).toBeNull();
    expect(chapterLeftBehind(chapters, 50, 51)).toBeNull();
  });
  it("localiza a visão de uma página", () => {
    const s = buildSpreads(6, "double", true, none);
    expect(spreadIndexOf(s, 4)).toBe(2);
    expect(spreadIndexOf(s, 99)).toBe(0);
  });
  it("ajustes de tamanho", () => {
    const vp = { width: 1000, height: 800 };
    expect(spreadHeight([0.5], "page", 1, vp)).toBe(800);
    expect(spreadHeight([0.5, 0.5], "width", 1, vp)).toBe(1000);
    expect(spreadHeight([2], "page", 1, vp)).toBe(500);
    expect(spreadHeight([0.5], "height", 2, vp)).toBe(1600);
    expect(verticalPageWidth(0.5, "width", 1, vp)).toBe(1000);
    expect(verticalPageWidth(0.5, "page", 1, vp)).toBe(400);
  });
  it("zoom em passos limitados", () => {
    expect(zoomIn(1)).toBe(1.1);
    expect(zoomOut(1)).toBe(0.9);
    expect(zoomIn(4)).toBe(4);
    expect(zoomOut(0.25)).toBe(0.25);
  });
  it("indicador e limites", () => {
    expect(pageIndicator([1, 2], 20)).toBe("Páginas 2–3 de 20");
    expect(pageIndicator([2, 1], 20)).toBe("Páginas 2–3 de 20");
    expect(pageIndicator([0], 20)).toBe("Página 1 de 20");
    expect(clampPage(-3, 10)).toBe(0);
    expect(clampPage(42, 10)).toBe(9);
    expect(nextMode("vertical")).toBe("webtoon");
    expect(nextMode("webtoon")).toBe("single");
  });
  it("webtoon é contínuo e numa coluna que não acompanha a janela", () => {
    expect(isContinuous("webtoon")).toBe(true);
    expect(isContinuous("vertical")).toBe(true);
    expect(isContinuous("double")).toBe(false);
    expect(webtoonWidth(1, 1920)).toBe(800);
    expect(webtoonWidth(1.5, 1920)).toBe(1200);
    expect(webtoonWidth(1, 600)).toBe(600);
    expect(webtoonWidth(1, 0)).toBe(1);
  });
  it("reconhece tiras pelas páginas, sem se enganar com uma página alta num mangá", () => {
    const manga = { width: 800, height: 1200 };
    const strip = { width: 800, height: 6000 };
    expect(hasTallPages(new Map())).toBe(false);
    expect(hasTallPages(new Map([[0, manga], [1, manga], [2, manga], [3, strip]]))).toBe(false);
    expect(hasTallPages(new Map([[0, strip]]))).toBe(true);
    expect(hasTallPages(new Map([[0, strip], [1, manga]]))).toBe(true);
    expect(hasTallPages(new Map([[0, { width: 0, height: 0 }]]))).toBe(false);
  });
  it("pistas de webtoon antes da primeira página", () => {
    expect(webtoonHint(["Ação", "Manhwa"], 12)).toBe(true);
    expect(webtoonHint(["Long Strip"], 12)).toBe(true);
    expect(webtoonHint(["Ação"], 77)).toBe(true);
    expect(webtoonHint(["Ação"], 20)).toBe(false);
    expect(webtoonHint([], 39)).toBe(false);
  });
});

describe("atalhos", () => {
  const ev = (key: string, mods: Partial<KeyboardEvent> = {}) =>
    ({ key, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods }) as KeyboardEvent;
  it("normaliza teclas", () => {
    expect(keyFromEvent(ev("f"))).toBe("F");
    expect(keyFromEvent(ev(" "))).toBe("Space");
    expect(keyFromEvent(ev(" ", { shiftKey: true }))).toBe("Shift+Space");
    expect(keyFromEvent(ev("+", { shiftKey: true }))).toBe("+");
    expect(keyFromEvent(ev("=", { ctrlKey: true }))).toBe("Ctrl+=");
    expect(keyFromEvent(ev("Shift"))).toBeNull();
    expect(formatKey("ArrowRight")).toBe("→");
  });
  it("direita para a esquerda inverte as setas", () => {
    const right = actionForKey(DEFAULT_SHORTCUTS, "ArrowRight");
    expect(right).toBe("next");
    expect(resolveArrow(right, "ArrowRight", true)).toBe("prev");
    expect(resolveArrow(actionForKey(DEFAULT_SHORTCUTS, "ArrowLeft"), "ArrowLeft", true)).toBe("next");
    expect(resolveArrow(actionForKey(DEFAULT_SHORTCUTS, "PageDown"), "PageDown", true)).toBe("next");
    expect(resolveArrow(right, "ArrowRight", false)).toBe("next");
  });
  it("detecta conflitos e valida mapas salvos", () => {
    expect(conflictFor(DEFAULT_SHORTCUTS, "F", "help")).toBe("fullscreen");
    expect(conflictFor(DEFAULT_SHORTCUTS, "F", "fullscreen")).toBeNull();
    const s = sanitizeShortcuts({ next: ["N"], prev: "inválido", zoomIn: [1, 2] });
    expect(s.next).toEqual(["N"]);
    expect(s.prev).toEqual(DEFAULT_SHORTCUTS.prev);
    expect(s.zoomIn).toEqual(DEFAULT_SHORTCUTS.zoomIn);
  });
});

describe("configurações", () => {
  it("valores inválidos voltam ao padrão", () => {
    const s = sanitizeSettings({ "appearance.theme": "roxo", "reader.mode": "double", "reader.coverAlone": "sim", onboarding: { status: "completed", step: 9 } });
    expect(s["appearance.theme"]).toBe("dark");
    expect(s["reader.mode"]).toBe("double");
    expect(s["reader.coverAlone"]).toBe(true);
    expect(s.onboarding).toEqual({ status: "completed", step: 1, comicId: null });
  });
  it("padrões iniciais", () => {
    expect(DEFAULT_SETTINGS["appearance.theme"]).toBe("dark");
    expect(DEFAULT_SETTINGS.onboarding.status).toBe("active");
  });
  it("tema do sistema", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
  });
});

describe("biblioteca com obras das fontes", () => {
  const comic = (id: number, title: string, overrides: Partial<ComicSummary> = {}) => ({ id, title, addedAt: id, lastOpenedAt: null, status: "unread", progress: null, favorite: false, ...overrides }) as ComicSummary;
  const series = (id: number, title: string, overrides: Partial<LibrarySeries> = {}): LibrarySeries => ({
    id,
    title,
    hasCover: false,
    sourceName: "Fonte",
    favorite: false,
    addedAt: id,
    lastOpenedAt: null,
    following: false,
    newCount: 0,
    readStatus: "unread",
    chapterCount: 10,
    chaptersRead: 0,
    resume: null,
    collections: [],
    ratingState: "none",
    ...overrides,
  });

  it("filtra como as HQs, e filtrar por tipo de arquivo deixa só os quadrinhos", () => {
    const saved = series(1, "Solo Levelling", { favorite: true, readStatus: "reading", lastOpenedAt: 5, collections: [3] });
    expect(seriesMatches(saved, {})).toBe(true);
    expect(seriesMatches(saved, { text: "solo fonte" })).toBe(true);
    expect(seriesMatches(saved, { text: "outra" })).toBe(false);
    expect(seriesMatches(saved, { favoritesOnly: true, openedOnly: true, statuses: ["reading"], collectionId: 3 })).toBe(true);
    expect(seriesMatches(saved, { collectionId: 4 })).toBe(false);
    expect(seriesMatches(series(2, "b"), { favoritesOnly: true })).toBe(false);
    expect(seriesMatches(series(2, "b"), { openedOnly: true })).toBe(false);
    expect(seriesMatches(saved, { formats: ["cbz"] })).toBe(false);
  });

  it("mistura na ordem pedida e sem obras não mexe na ordem do backend", () => {
    const comics = [comic(2, "Zeta"), comic(1, "Alfa")];
    expect(mergeItems(comics, [])).toBe(comics);
    expect(mergeItems(comics, [series(9, "Beta")]).map((item) => item.title)).toEqual(["Alfa", "Beta", "Zeta"]);
    expect(mergeItems(comics, [series(9, "Beta")], "added", true).map((item) => item.title)).toEqual(["Beta", "Zeta", "Alfa"]);
  });

  it("mostra onde parou e conta o progresso em capítulos", () => {
    const resume = { chapterId: 1, title: "Capítulo 3", progress: { pageIndex: 4, pageCount: 20 }, state: "in_progress" as const };
    expect(seriesProgressLabel(series(1, "a", { readStatus: "reading", resume }))).toBe("Capítulo 3 · Em andamento");
    expect(seriesProgressLabel(series(1, "a", { readStatus: "reading", resume: { ...resume, state: "reading" } }))).toBe("Capítulo 3 · Lendo");
    // O próximo capítulo, ainda não começado: só o título.
    expect(seriesProgressLabel(series(1, "a", { readStatus: "reading", resume: { ...resume, progress: null, state: "unread" } }))).toBe("Capítulo 3");
    expect(seriesProgressLabel(series(1, "a"))).toBe("Não iniciado");
    expect(seriesProgressLabel(series(1, "a", { readStatus: "completed" }))).toBe("Leitura concluída");
    expect(seriesRatio(series(1, "a", { readStatus: "reading", chaptersRead: 5 }))).toBe(0.5);
    expect(seriesRatio(series(1, "a"))).toBeNull();
    expect(seriesRatio(series(1, "a", { readStatus: "completed" }))).toBe(1);
  });

  it("a tela inicial põe a leitura mais recente na frente, venha de onde vier", () => {
    const home = { total: 1, continueReading: [comic(1, "HQ", { status: "reading", progress: { updatedAt: 10 } as ComicSummary["progress"] })], recentlyAdded: [comic(1, "HQ")], favorites: [], recentlyOpened: [] };
    const reading = series(2, "Obra", { readStatus: "reading", lastOpenedAt: 20, favorite: true });
    const items = homeItems(home, [reading, series(3, "Parada")]);
    expect(items.total).toBe(3);
    expect(items.continueReading.map((item) => item.title)).toEqual(["Obra", "HQ"]);
    expect(items.favorites.map((item) => item.title)).toEqual(["Obra"]);
    expect(items.recentlyOpened.map((item) => item.title)).toEqual(["Obra"]);
    expect(items.recentlyAdded.map((item) => item.title)).toEqual(["Parada", "Obra", "HQ"]);
  });
});
