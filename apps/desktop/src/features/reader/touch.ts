import { useEffect, useRef, type RefObject } from "react";

/** Gestos de toque do leitor no celular. Só eventos de toque: o mouse e o teclado do desktop não passam por aqui. */
export interface TouchGestures {
  /** Toque simples. `x` vai de 0 (borda esquerda) a 1 (borda direita) da área de leitura. */
  onTap?: (x: number) => void;
  /** Toques que não esperam um possível segundo toque (as bordas, que viram a página na hora). */
  immediate?: (x: number) => boolean;
  /** Dois toques rápidos no mesmo ponto, em coordenadas da área de leitura. */
  onDoubleTap?: (x: number, y: number) => void;
  /** Arrastar para o lado: `-1` para a esquerda, `1` para a direita. Só quando `canSwipe` deixa. */
  onSwipe?: (direction: -1 | 1) => void;
  canSwipe?: () => boolean;
  /** Pinça em andamento e ao soltar: escala desde o começo do gesto e o centro dele na área de leitura. */
  onPinch?: (scale: number, x: number, y: number) => void;
  onPinchEnd?: (scale: number, x: number, y: number) => void;
}

const TAP_SLOP = 10;
const TAP_MS = 300;
const DOUBLE_TAP_MS = 280;
const DOUBLE_TAP_SLOP = 32;
const SWIPE_MIN = 56;

/** Liga os gestos na área de leitura. Os callbacks podem mudar a cada render: vale sempre o mais recente. */
export function useTouchGestures(ref: RefObject<HTMLElement | null>, gestures: TouchGestures | undefined) {
  const latest = useRef(gestures);
  useEffect(() => {
    latest.current = gestures;
  });
  const enabled = gestures !== undefined;

  useEffect(() => {
    const el = ref.current;
    if (!el || !enabled) return;
    let start: { x: number; y: number; t: number } | null = null;
    let pinch: { dist: number; scale: number; cx: number; cy: number } | null = null;
    let multi = false;
    let lastTap: { x: number; y: number; t: number } | null = null;
    let tapTimer = 0;

    const local = (clientX: number, clientY: number) => {
      const r = el.getBoundingClientRect();
      return { x: clientX - r.left, y: clientY - r.top, w: r.width };
    };
    const distance = (a: Touch, b: Touch) => Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY);

    const onStart = (e: TouchEvent) => {
      if (e.touches.length === 1) {
        const t = e.touches[0]!;
        start = { x: t.clientX, y: t.clientY, t: Date.now() };
        multi = false;
      } else if (e.touches.length === 2) {
        multi = true;
        start = null;
        const [a, b] = [e.touches[0]!, e.touches[1]!];
        const c = local((a.clientX + b.clientX) / 2, (a.clientY + b.clientY) / 2);
        pinch = { dist: distance(a, b), scale: 1, cx: c.x, cy: c.y };
      }
    };

    const onMove = (e: TouchEvent) => {
      if (e.touches.length === 2 && pinch && latest.current?.onPinch) {
        // A pinça é do leitor: sem isso o WebView tentaria ampliar a página inteira.
        e.preventDefault();
        const [a, b] = [e.touches[0]!, e.touches[1]!];
        pinch.scale = distance(a, b) / Math.max(1, pinch.dist);
        latest.current.onPinch(pinch.scale, pinch.cx, pinch.cy);
      }
    };

    const onEnd = (e: TouchEvent) => {
      const g = latest.current;
      if (pinch && e.touches.length < 2) {
        const p = pinch;
        pinch = null;
        g?.onPinchEnd?.(p.scale, p.cx, p.cy);
        return;
      }
      if (!start || multi || e.touches.length > 0 || !g) return;
      const t = e.changedTouches[0]!;
      const dx = t.clientX - start.x;
      const dy = t.clientY - start.y;
      const dt = Date.now() - start.t;
      start = null;
      if (Math.abs(dx) >= SWIPE_MIN && Math.abs(dx) > Math.abs(dy) * 1.5 && dt < 700) {
        if (g.onSwipe && (g.canSwipe?.() ?? true)) g.onSwipe(dx < 0 ? -1 : 1);
        return;
      }
      if (Math.abs(dx) > TAP_SLOP || Math.abs(dy) > TAP_SLOP || dt > TAP_MS) return;
      const p = local(t.clientX, t.clientY);
      const now = Date.now();
      if (g.onDoubleTap && lastTap && now - lastTap.t < DOUBLE_TAP_MS && Math.hypot(p.x - lastTap.x, p.y - lastTap.y) < DOUBLE_TAP_SLOP) {
        window.clearTimeout(tapTimer);
        lastTap = null;
        // O toque gera também um clique: ele não pode cair num botão que apareceu embaixo do dedo.
        e.preventDefault();
        g.onDoubleTap(p.x, p.y);
        return;
      }
      const ratio = p.w > 0 ? p.x / p.w : 0.5;
      if (!g.onDoubleTap || g.immediate?.(ratio)) {
        lastTap = null;
        g.onTap?.(ratio);
        return;
      }
      lastTap = { x: p.x, y: p.y, t: now };
      window.clearTimeout(tapTimer);
      tapTimer = window.setTimeout(() => {
        lastTap = null;
        latest.current?.onTap?.(ratio);
      }, DOUBLE_TAP_MS);
    };

    el.addEventListener("touchstart", onStart, { passive: true });
    el.addEventListener("touchmove", onMove, { passive: false });
    el.addEventListener("touchend", onEnd, { passive: false });
    el.addEventListener("touchcancel", onEnd, { passive: false });
    return () => {
      window.clearTimeout(tapTimer);
      el.removeEventListener("touchstart", onStart);
      el.removeEventListener("touchmove", onMove);
      el.removeEventListener("touchend", onEnd);
      el.removeEventListener("touchcancel", onEnd);
    };
  }, [ref, enabled]);
}
