import { describe, expect, it } from "vitest";
import { computePop, edited, fitPage, moveInList, moveRect, newRegion, pageOrigin, rectFromDrag, resizeRect, isDoubtful, type GuidedRegion } from "../../src/domain/guided";

const RESERVED = 72;
const view = { width: 1280, height: 800 };
const page = fitPage({ width: 1000, height: 1500 }, { width: view.width, height: view.height - RESERVED });

describe("balão destacado da Leitura Guiada", () => {
  it("a página fica inteira acima dos controles", () => {
    const origin = pageOrigin(page, view, RESERVED);
    expect(origin.y).toBeCloseTo(0);
    expect(origin.y + page.height).toBeLessThanOrEqual(view.height - RESERVED + 0.01);
    expect(origin.x).toBeCloseTo((view.width - page.width) / 2);
  });

  it("cresce em volta do próprio lugar", () => {
    const r = { x: 0.4, y: 0.4, w: 0.2, h: 0.1 };
    const pop = computePop(r, page, view, RESERVED);
    const origin = pageOrigin(page, view, RESERVED);
    expect(pop.scale).toBeGreaterThan(1.3);
    expect(pop.width).toBeCloseTo(r.w * page.width * pop.scale);
    expect(pop.left + pop.width / 2).toBeCloseTo(origin.x + (r.x + r.w / 2) * page.width);
    expect(pop.top + pop.height / 2).toBeCloseTo(origin.y + (r.y + r.h / 2) * page.height);
  });

  it("volta exatamente para o lugar dele na página", () => {
    const r = { x: 0.05, y: 0.9, w: 0.3, h: 0.08 };
    const pop = computePop(r, page, view, RESERVED);
    const origin = pageOrigin(page, view, RESERVED);
    expect(pop.left + pop.fromX).toBeCloseTo(origin.x + r.x * page.width);
    expect(pop.top + pop.fromY).toBeCloseTo(origin.y + r.y * page.height);
    expect(pop.width / pop.scale).toBeCloseTo(r.w * page.width);
  });

  it("fica inteiro na tela e acima dos controles, mesmo nos cantos", () => {
    for (const r of [
      { x: 0, y: 0, w: 0.3, h: 0.1 },
      { x: 0.7, y: 0.9, w: 0.3, h: 0.1 },
      { x: 0.1, y: 0.2, w: 0.8, h: 0.6 },
      { x: 0, y: 0, w: 1, h: 1 },
    ]) {
      for (const size of [view, { width: 600, height: 500 }]) {
        const fit = fitPage({ width: 1000, height: 1500 }, { width: size.width, height: size.height - RESERVED });
        const pop = computePop(r, fit, size, RESERVED);
        expect(pop.left).toBeGreaterThanOrEqual(0);
        expect(pop.top).toBeGreaterThanOrEqual(0);
        expect(pop.left + pop.width).toBeLessThanOrEqual(size.width + 0.01);
        expect(pop.top + pop.height).toBeLessThanOrEqual(size.height - RESERVED + 0.01);
        expect(pop.scale).toBeGreaterThan(0.9);
      }
    }
  });

  it("balão grande não cobre a arte da página inteira", () => {
    const r = { x: 0.1, y: 0.1, w: 0.5, h: 0.4 };
    const pop = computePop(r, page, view, RESERVED);
    expect(pop.height).toBeLessThanOrEqual(page.height * 0.52 + r.h * page.height);
    expect(pop.width).toBeLessThanOrEqual(view.width);
  });

  it("com a página já grande, ainda salta, sem exagerar", () => {
    const big = fitPage({ width: 1000, height: 1500 }, { width: 2560, height: 2000 });
    const pop = computePop({ x: 0.4, y: 0.4, w: 0.1, h: 0.05 }, big, { width: 2560, height: 2072 }, RESERVED);
    expect(pop.scale).toBeCloseTo(1.3);
  });
});

describe("edição de regiões", () => {
  it("mover não sai da página", () => {
    expect(moveRect({ x: 0.9, y: 0.1, w: 0.2, h: 0.2 }, 0.5, -0.5)).toEqual({ x: 0.8, y: 0, w: 0.2, h: 0.2 });
  });

  it("redimensionar respeita bordas e tamanho mínimo", () => {
    const r = { x: 0.2, y: 0.2, w: 0.2, h: 0.2 };
    const se = resizeRect(r, "se", 0.1, 0.1);
    expect(se.w).toBeCloseTo(0.3);
    expect(se.h).toBeCloseTo(0.3);
    const w = resizeRect(r, "w", -0.5, 0);
    expect(w.x).toBe(0);
    expect(w.x + w.w).toBeCloseTo(0.4);
    const crushed = resizeRect(r, "n", 0, 0.9);
    expect(crushed.h).toBeCloseTo(0.01);
    expect(crushed.y + crushed.h).toBeCloseTo(0.4);
  });

  it("arraste em qualquer direção vira retângulo e clique não vira região", () => {
    expect(rectFromDrag(0.5, 0.5, 0.3, 0.2)).toEqual({ x: 0.3, y: 0.2, w: 0.2, h: 0.3 });
    expect(rectFromDrag(0.5, 0.5, 0.502, 0.502)).toBeNull();
    expect(rectFromDrag(-0.2, 0.1, 0.2, 1.5)).toEqual({ x: 0, y: 0.1, w: 0.2, h: 0.9 });
  });

  it("reordenar troca a posição e respeita os limites", () => {
    expect(moveInList(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
    expect(moveInList(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]);
    expect(moveInList(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);
  });

  it("marca o estado de revisão e a dúvida", () => {
    const added = newRegion({ x: 0, y: 0, w: 0.1, h: 0.1 }, 3, 0);
    expect(added.pageIndex).toBe(3);
    expect(added.review).toBe("added");
    expect(edited(added, { kind: "shout" }).review).toBe("added");
    const auto: GuidedRegion = { ...added, review: "auto", confidence: 0.4 };
    expect(edited(auto, { x: 0.2 }).review).toBe("adjusted");
    expect(isDoubtful(auto)).toBe(true);
    expect(isDoubtful(added)).toBe(false);
  });

  it("mover ou redimensionar descarta o contorno; mudar o tipo não", () => {
    const shaped: GuidedRegion = { ...newRegion({ x: 0, y: 0, w: 0.1, h: 0.1 }, 3, 0), review: "auto", outline: [[0.5, 0], [1, 1], [0, 1]] };
    expect(edited(shaped, { kind: "shout" }).outline).toEqual(shaped.outline);
    expect(edited(shaped, moveRect(shaped, 0.1, 0)).outline).toEqual([]);
  });
});
