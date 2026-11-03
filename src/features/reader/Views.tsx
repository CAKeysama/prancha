import { Fragment, forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";
import { cn } from "../../components/cn";
import { DEFAULT_ASPECT, spreadHeight, verticalPageWidth, webtoonWidth, type Size } from "../../domain/reader";
import type { FitMode, ReadingDirection } from "../../domain/settings";
import type { PageSrc } from "../../services/urls";
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
  pageSrc: PageSrc;
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
export function PagedView({ pageSrc, spread, sizes, fit, zoom, direction, pdf, onSize, onWheelEdge }: PagedProps) {
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
                src={pageSrc}
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
  pageSrc: PageSrc;
  /** Coluna estreita e sem espaço entre as páginas, como as tiras foram desenhadas. */
  webtoon: boolean;
  /** Aparece depois da última página. */
  footer?: ReactNode;
  /** Página em que cada capítulo começa, com o título, para marcar a passagem de um para o outro. */
  chapterStarts: ReadonlyMap<number, string>;
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
  { pageSrc, webtoon, footer, chapterStarts, count, sizes, fit, zoom, pdf, startPage, startRatio, onSize, onPosition },
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
      const w = webtoon ? webtoonWidth(zoom, size.width) : verticalPageWidth(aspect, fit, zoom, { width: Math.max(1, size.width - 24), height: Math.max(1, size.height - 12) });
      return { w, h: Math.floor(w / aspect) };
    },
    [webtoon, sizes, fit, zoom, size.width, size.height],
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
    // As setas e o espaço já rolam, sem precisar clicar na página antes.
    viewport.current?.focus({ preventScroll: true });
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
      // No fim da rolagem a última página pode ser curta demais para passar pela marca.
      if (el.scrollTop + el.clientHeight >= el.scrollHeight - 2) current = count - 1;
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
    <div ref={viewport} className="h-full w-full overflow-auto outline-none [overflow-anchor:auto]" data-reader-viewport tabIndex={-1}>
      <div className={cn("flex flex-col items-center", !webtoon && "py-1.5")} style={{ gap: webtoon ? 0 : GAP }}>
        {size.width > 0 &&
          Array.from({ length: count }, (_, i) => {
            const { w, h } = layout(i);
            const mounted = i >= range[0] && i <= range[1];
            const chapterTitle = chapterStarts.get(i);
            return (
              <Fragment key={i}>
                {chapterTitle !== undefined && (
                  <div className="flex shrink-0 items-center gap-3 py-5 text-xs text-muted" style={{ width: w }} data-testid="chapter-start">
                    <span className="h-px flex-1 bg-border" aria-hidden />
                    <span className="max-w-[70%] truncate">A partir daqui: {chapterTitle}</span>
                    <span className="h-px flex-1 bg-border" aria-hidden />
                  </div>
                )}
                <div
                  ref={(el) => {
                    pageEls.current[i] = el;
                  }}
                  data-page={i}
                  style={{ width: w, height: h }}
                  className="shrink-0"
                >
                  {mounted ? (
                    <PageView src={pageSrc} index={i} width={w} height={h} pdf={pdf} onSize={onSize} flush={webtoon} />
                  ) : (
                    <div className="h-full w-full rounded-sm bg-surface/40" aria-hidden />
                  )}
                </div>
              </Fragment>
            );
          })}
      </div>
      {size.width > 0 && footer}
    </div>
  );
});
