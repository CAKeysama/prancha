import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";
import { DEFAULT_ASPECT, spreadHeight, verticalPageWidth, type Size } from "../../domain/reader";
import type { FitMode, ReadingDirection } from "../../domain/settings";
import { PageView } from "./PageView";

export function useViewportSize(ref: React.RefObject<HTMLElement | null>): Size {
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const update = () => setSize({ width: el.clientWidth, height: el.clientHeight });
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return size;
}

const aspectOf = (s: Size | undefined) => (s && s.height > 0 ? s.width / s.height : DEFAULT_ASPECT);

interface PagedProps {
  comicId: number;
  spread: number[];
  sizes: ReadonlyMap<number, Size>;
  fit: FitMode;
  zoom: number;
  direction: ReadingDirection;
  pdf: PDFDocumentProxy | null;
  onSize: (i: number, s: Size) => void;
  /** Roda do mouse: devolve true quando consumiu o evento para virar a página. */
  onWheelEdge: (delta: number, atStart: boolean, atEnd: boolean) => boolean;
}

/** Página única ou dupla. O conteúdo maior que a janela pode ser rolado. */
export function PagedView({ comicId, spread, sizes, fit, zoom, direction, pdf, onSize, onWheelEdge }: PagedProps) {
  const viewport = useRef<HTMLDivElement>(null);
  const size = useViewportSize(viewport);
  const aspects = spread.map((i) => aspectOf(sizes.get(i)));
  const padding = 16;
  const inner = { width: Math.max(1, size.width - padding * 2), height: Math.max(1, size.height - padding * 2) };
  const h = spreadHeight(aspects, fit, zoom, inner);
  const ordered = direction === "rtl" ? [...spread].reverse() : spread;
  const key = spread.join("-");

  // Ao virar a página, volta ao início (canto superior do lado de leitura).
  useLayoutEffect(() => {
    const el = viewport.current;
    if (!el) return;
    el.scrollTop = 0;
    el.scrollLeft = direction === "rtl" ? el.scrollWidth : 0;
  }, [key, direction]);

  const onWheelEdgeRef = useRef(onWheelEdge);
  useEffect(() => {
    onWheelEdgeRef.current = onWheelEdge;
  });
  useEffect(() => {
    const el = viewport.current;
    if (!el) return;
    const handler = (e: WheelEvent) => {
      if (e.ctrlKey) return; // zoom tratado no leitor
      const atStart = el.scrollTop <= 1;
      const atEnd = el.scrollTop + el.clientHeight >= el.scrollHeight - 1;
      if (onWheelEdgeRef.current(e.deltaY, atStart, atEnd)) e.preventDefault();
    };
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  }, []);

  return (
    <div ref={viewport} className="h-full w-full overflow-auto" data-reader-viewport tabIndex={-1}>
      <div className="flex min-h-full min-w-full" style={{ padding }}>
        <div className="m-auto flex items-center" key={key}>
          {size.width > 0 &&
            ordered.map((i) => (
              <PageView
                key={i}
                comicId={comicId}
                index={i}
                width={Math.floor(h * aspectOf(sizes.get(i)))}
                height={h}
                pdf={pdf}
                onSize={onSize}
              />
            ))}
        </div>
      </div>
    </div>
  );
}

interface VerticalProps {
  comicId: number;
  count: number;
  sizes: ReadonlyMap<number, Size>;
  fit: FitMode;
  zoom: number;
  pdf: PDFDocumentProxy | null;
  startPage: number;
  startRatio: number;
  onSize: (i: number, s: Size) => void;
  /** Página atual e posição relativa dentro dela (0–1). */
  onPosition: (page: number, ratio: number) => void;
}

export interface VerticalHandle {
  scrollToPage: (page: number, ratio?: number) => void;
}

const GAP = 6;

/** Rolagem contínua. Só monta as páginas próximas da área visível. */
export const VerticalView = forwardRef<VerticalHandle, VerticalProps>(function VerticalView(
  { comicId, count, sizes, fit, zoom, pdf, startPage, startRatio, onSize, onPosition },
  ref,
) {
  const viewport = useRef<HTMLDivElement>(null);
  const size = useViewportSize(viewport);
  const pageEls = useRef<Array<HTMLDivElement | null>>([]);
  const [range, setRange] = useState<[number, number]>([Math.max(0, startPage - 2), startPage + 3]);
  const restored = useRef(false);

  const layout = useCallback(
    (i: number) => {
      const aspect = aspectOf(sizes.get(i));
      const w = verticalPageWidth(aspect, fit, zoom, { width: Math.max(1, size.width - 24), height: Math.max(1, size.height - 12) });
      return { w, h: Math.floor(w / aspect) };
    },
    [sizes, fit, zoom, size.width, size.height],
  );

  const scrollToPage = useCallback((page: number, ratio = 0) => {
    const el = viewport.current;
    const target = pageEls.current[page];
    if (!el || !target) return;
    el.scrollTop = target.offsetTop + ratio * target.offsetHeight;
  }, []);

  useImperativeHandle(ref, () => ({ scrollToPage }), [scrollToPage]);

  // Restaura a posição salva assim que o layout tem tamanho.
  useLayoutEffect(() => {
    if (restored.current || size.width === 0) return;
    restored.current = true;
    scrollToPage(startPage, startRatio);
  }, [size.width, startPage, startRatio, scrollToPage]);

  const onPositionRef = useRef(onPosition);
  useEffect(() => {
    onPositionRef.current = onPosition;
  });

  useEffect(() => {
    const el = viewport.current;
    if (!el) return;
    let frame = 0;
    const measure = () => {
      frame = 0;
      const probe = el.scrollTop + el.clientHeight * 0.25;
      let current = 0;
      for (let i = 0; i < pageEls.current.length; i++) {
        const p = pageEls.current[i];
        if (!p) continue;
        if (p.offsetTop <= probe) current = i;
        else break;
      }
      const p = pageEls.current[current];
      const ratio = p && p.offsetHeight > 0 ? Math.min(1, Math.max(0, (el.scrollTop - p.offsetTop) / p.offsetHeight)) : 0;
      const visibleEnd = el.scrollTop + el.clientHeight;
      let last = current;
      while (last + 1 < pageEls.current.length && (pageEls.current[last + 1]?.offsetTop ?? Infinity) < visibleEnd) last++;
      setRange((r) => {
        const next: [number, number] = [Math.max(0, current - 2), Math.min(count - 1, last + 3)];
        return r[0] === next[0] && r[1] === next[1] ? r : next;
      });
      if (restored.current) onPositionRef.current(current, ratio);
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(measure);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      el.removeEventListener("scroll", onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [count]);

  return (
    <div ref={viewport} className="h-full w-full overflow-auto [overflow-anchor:auto]" data-reader-viewport tabIndex={-1}>
      <div className="flex flex-col items-center py-1.5" style={{ gap: GAP }}>
        {size.width > 0 &&
          Array.from({ length: count }, (_, i) => {
            const { w, h } = layout(i);
            const mounted = i >= range[0] && i <= range[1];
            return (
              <div
                key={i}
                ref={(el) => {
                  pageEls.current[i] = el;
                }}
                data-page={i}
                style={{ width: w, height: h }}
                className="shrink-0"
              >
                {mounted ? (
                  <PageView comicId={comicId} index={i} width={w} height={h} pdf={pdf} onSize={onSize} />
                ) : (
                  <div className="h-full w-full rounded-sm bg-surface/40" aria-hidden />
                )}
              </div>
            );
          })}
      </div>
    </div>
  );
});
