// Leitura Guiada: regras puras do balão destacado e da edição das regiões, sem depender da interface.

import type { Size } from "./reader";

export type RegionKind = "speech" | "thought" | "shout" | "whisper" | "offscreen" | "narration" | "caption" | "note" | "other";

export const KIND_LABELS: Record<RegionKind, string> = {
  speech: "Fala",
  thought: "Pensamento",
  shout: "Grito",
  whisper: "Sussurro",
  offscreen: "Fala fora de quadro",
  narration: "Narração",
  caption: "Legenda",
  note: "Nota explicativa",
  other: "Outro texto",
};

export const REGION_KINDS = Object.keys(KIND_LABELS) as RegionKind[];

/** Coordenadas normalizadas (0–1) em relação à página. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface GuidedRegion extends Rect {
  id: string;
  pageIndex: number;
  kind: RegionKind;
  order: number;
  /** `null` em regiões desenhadas à mão. */
  confidence: number | null;
  review: "auto" | "adjusted" | "added" | "confirmed";
  /** Contorno do balão em frações da região; vazio quando a forma não é conhecida. */
  outline: [number, number][];
}

export interface GuidedPage {
  pageIndex: number;
  source: "auto" | "user";
  regions: GuidedRegion[];
  panels: Rect[];
  needsReview: boolean;
}

/** Igual ao limite do backend: abaixo disso a região é duvidosa. */
export const LOW_CONFIDENCE = 0.55;

export const isDoubtful = (r: GuidedRegion) => r.confidence !== null && r.confidence < LOW_CONFIDENCE;

/** Tamanho da página inteira dentro da área disponível. */
export function fitPage(natural: Size, view: Size): Size {
  const s = Math.min(view.width / natural.width, view.height / natural.height);
  return { width: natural.width * s, height: natural.height * s };
}

/** Onde a página fica na tela: centrada na área acima dos controles. */
export function pageOrigin(page: Size, view: Size, reserved: number) {
  return { x: (view.width - page.width) / 2, y: Math.max(0, (view.height - reserved - page.height) / 2) };
}

/** Largura de página em que o letreiro de um quadrinho fica confortável de ler. */
const READING_PAGE_WIDTH = 1250;
/** Sempre cresce pelo menos isto, para o balão saltar mesmo com a página já grande. */
const MIN_POP = 1.3;
/** Fração da página que o balão pode cobrir: a arte em volta continua à vista. */
const MAX_COVER_WIDTH = 0.85;
const MAX_COVER_HEIGHT = 0.5;
const POP_MARGIN = 8;

export interface Pop {
  /** Posição e tamanho do balão destacado. */
  left: number;
  top: number;
  width: number;
  height: number;
  scale: number;
  /** Deslocamento até o lugar dele na página, onde tem escala 1/`scale`. */
  fromX: number;
  fromY: number;
}

/**
 * Balão destacado sobre a página: maior, centrado no próprio lugar e sempre inteiro na tela.
 * `page` é a página ajustada à tela; `reserved` é a faixa inferior ocupada pelos controles.
 */
export function computePop(region: Rect, page: Size, view: Size, reserved: number): Pop {
  const origin = pageOrigin(page, view, reserved);
  const [baseLeft, baseTop] = [origin.x + region.x * page.width, origin.y + region.y * page.height];
  const [baseW, baseH] = [region.w * page.width, region.h * page.height];
  const freeW = view.width - POP_MARGIN * 2;
  const freeH = view.height - reserved - POP_MARGIN * 2;
  const wanted = Math.min(READING_PAGE_WIDTH / page.width, (page.width * MAX_COVER_WIDTH) / baseW, (page.height * MAX_COVER_HEIGHT) / baseH);
  const scale = Math.min(Math.max(MIN_POP, wanted), freeW / baseW, freeH / baseH);
  const [width, height] = [baseW * scale, baseH * scale];
  const left = Math.max(POP_MARGIN, Math.min(view.width - POP_MARGIN - width, baseLeft + baseW / 2 - width / 2));
  const top = Math.max(POP_MARGIN, Math.min(view.height - reserved - POP_MARGIN - height, baseTop + baseH / 2 - height / 2));
  return { left, top, width, height, scale, fromX: baseLeft - left, fromY: baseTop - top };
}

// ---- Edição ----

const MIN_SIDE = 0.01;

export type Handle = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";
export const HANDLES: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export function moveRect(r: Rect, dx: number, dy: number): Rect {
  return { ...r, x: clamp(r.x + dx, 0, 1 - r.w), y: clamp(r.y + dy, 0, 1 - r.h) };
}

export function resizeRect(r: Rect, handle: Handle, dx: number, dy: number): Rect {
  let [x0, y0, x1, y1] = [r.x, r.y, r.x + r.w, r.y + r.h];
  if (handle.includes("w")) x0 = clamp(x0 + dx, 0, x1 - MIN_SIDE);
  if (handle.includes("e")) x1 = clamp(x1 + dx, x0 + MIN_SIDE, 1);
  if (handle.includes("n")) y0 = clamp(y0 + dy, 0, y1 - MIN_SIDE);
  if (handle.includes("s")) y1 = clamp(y1 + dy, y0 + MIN_SIDE, 1);
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** Retângulo de um arraste em qualquer direção, ou `null` se foi só um clique. */
export function rectFromDrag(ax: number, ay: number, bx: number, by: number): Rect | null {
  const [x0, x1] = [clamp(Math.min(ax, bx), 0, 1), clamp(Math.max(ax, bx), 0, 1)];
  const [y0, y1] = [clamp(Math.min(ay, by), 0, 1), clamp(Math.max(ay, by), 0, 1)];
  if (x1 - x0 < MIN_SIDE || y1 - y0 < MIN_SIDE) return null;
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

export function moveInList<T>(list: readonly T[], index: number, delta: number): T[] {
  const target = index + delta;
  if (target < 0 || target >= list.length) return [...list];
  const out = [...list];
  const [item] = out.splice(index, 1);
  out.splice(target, 0, item!);
  return out;
}

export function newRegion(rect: Rect, pageIndex: number, order: number): GuidedRegion {
  const id = `u${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
  return { ...rect, id, pageIndex, kind: "speech", order, confidence: null, review: "added", outline: [] };
}

/** Região criada à mão continua "added"; as demais passam a "adjusted" ao mudar. */
export function edited(region: GuidedRegion, change: Partial<GuidedRegion>): GuidedRegion {
  // Movida ou redimensionada, a região não coincide mais com o balão detectado.
  const outline = "x" in change || "y" in change || "w" in change || "h" in change ? [] : region.outline;
  return { ...region, ...change, outline, review: region.review === "added" ? "added" : "adjusted" };
}
