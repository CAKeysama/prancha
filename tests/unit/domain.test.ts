import { describe, expect, it } from "vitest";
import { formatBytes, progressLabel, progressRatio, readActionLabel, subtitle } from "../../src/domain/progress";
import {
  buildSpreads,
  clampPage,
  nextMode,
  pageIndicator,
  spreadHeight,
  spreadIndexOf,
  verticalPageWidth,
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
    expect(nextMode("vertical")).toBe("single");
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
