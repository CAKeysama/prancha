// Regras puras do leitor (testadas em tests/unit/reader.test.ts).

import type { FitMode, ReaderMode } from "./settings";

export interface Size {
  width: number;
  height: number;
}

/** Proporção padrão (largura/altura) enquanto o tamanho real da página não é conhecido. */
export const DEFAULT_ASPECT = 2 / 3;

const ZOOM_STEPS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4];
const MIN_ZOOM = ZOOM_STEPS[0]!;
const MAX_ZOOM = ZOOM_STEPS[ZOOM_STEPS.length - 1]!;

export function zoomIn(z: number): number {
  return ZOOM_STEPS.find((s) => s > z + 1e-6) ?? MAX_ZOOM;
}

export function zoomOut(z: number): number {
  return [...ZOOM_STEPS].reverse().find((s) => s < z - 1e-6) ?? MIN_ZOOM;
}

function isWide(size: Size | undefined): boolean {
  return !!size && size.width > size.height * 1.05;
}

/**
 * Agrupa as páginas em "visões". Página dupla: pares lado a lado; a capa pode ficar sozinha
 * e páginas largas (duplas já digitalizadas juntas) nunca são pareadas.
 */
export function buildSpreads(count: number, mode: ReaderMode, coverAlone: boolean, sizes: ReadonlyMap<number, Size>): number[][] {
  if (count <= 0) return [];
  if (mode !== "double") return Array.from({ length: count }, (_, i) => [i]);
  const spreads: number[][] = [];
  let i = 0;
  if (coverAlone) {
    spreads.push([0]);
    i = 1;
  }
  while (i < count) {
    const a = i;
    const b = i + 1;
    if (b < count && !isWide(sizes.get(a)) && !isWide(sizes.get(b))) {
      spreads.push([a, b]);
      i += 2;
    } else {
      spreads.push([a]);
      i += 1;
    }
  }
  return spreads;
}

export function spreadIndexOf(spreads: number[][], page: number): number {
  const idx = spreads.findIndex((s) => s.includes(page));
  return idx === -1 ? 0 : idx;
}

/**
 * Altura (em px CSS) com que as páginas de uma visão são exibidas, para o ajuste e zoom escolhidos.
 * Todas as páginas de uma visão compartilham a mesma altura.
 */
export function spreadHeight(aspects: number[], fit: FitMode, zoom: number, viewport: Size): number {
  const sum = aspects.reduce((a, b) => a + b, 0) || DEFAULT_ASPECT;
  const byWidth = viewport.width / sum;
  const base = fit === "width" ? byWidth : fit === "height" ? viewport.height : Math.min(viewport.height, byWidth);
  return Math.max(1, Math.floor(base * zoom));
}

/** Largura de uma página no modo de rolagem vertical. */
export function verticalPageWidth(aspect: number, fit: FitMode, zoom: number, viewport: Size): number {
  const base = fit === "width" ? viewport.width : Math.min(viewport.width, viewport.height * aspect);
  return Math.max(1, Math.floor(base * zoom));
}

export function clampPage(page: number, count: number): number {
  if (count <= 0) return 0;
  return Math.min(Math.max(0, Math.round(page)), count - 1);
}

export function nextMode(mode: ReaderMode): ReaderMode {
  return mode === "single" ? "double" : mode === "double" ? "vertical" : "single";
}

export const MODE_LABELS: Record<ReaderMode, string> = {
  single: "Página única",
  double: "Página dupla",
  vertical: "Rolagem vertical",
};

export const FIT_LABELS: Record<FitMode, string> = {
  page: "Ajustar à página",
  width: "Ajustar à largura",
  height: "Ajustar à altura",
};

/** Texto do indicador: "Página 3 de 20" ou "Páginas 2–3 de 20". */
export function pageIndicator(spread: number[], count: number): string {
  if (spread.length === 0 || count === 0) return "Sem páginas";
  const pages = [...spread].sort((a, b) => a - b).map((p) => p + 1);
  return pages.length === 2 ? `Páginas ${pages[0]}–${pages[1]} de ${count}` : `Página ${pages[0]} de ${count}`;
}
