import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { ComicSummary } from "../../domain/types";
import { THUMB_WIDTHS } from "../../domain/settings";
import { useSettings } from "../../app/settings-context";
import { ComicCard, ComicRow, cardHeight } from "./ComicItem";

function useWidth(ref: RefObject<HTMLElement | null>): number {
  const [w, setW] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setW(el.clientWidth);
    const ro = new ResizeObserver(() => setW(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return w;
}

interface Props {
  comics: ComicSummary[];
  view: "grid" | "list";
  scrollRef: RefObject<HTMLDivElement | null>;
  collectionId?: number;
}

/** Grade ou lista virtualizada: só os itens visíveis são renderizados. */
export function ComicCollectionView({ comics, view, scrollRef, collectionId }: Props) {
  const { settings } = useSettings();
  const container = useRef<HTMLDivElement>(null);
  const width = useWidth(container);
  const compact = settings["appearance.density"] === "compact";
  const gap = compact ? 12 : 20;
  const minCard = THUMB_WIDTHS[settings["appearance.thumbSize"]];
  const cols = view === "grid" ? Math.max(1, Math.floor((width + gap) / (minCard + gap))) : 1;
  const cardW = view === "grid" ? Math.floor((width - gap * (cols - 1)) / cols) : width;
  const rowH = view === "grid" ? cardHeight(cardW) + gap : 88;
  const rows = Math.ceil(comics.length / cols);
  const [margin, setMargin] = useState(0);

  useLayoutEffect(() => {
    if (container.current && scrollRef.current) {
      const c = container.current.getBoundingClientRect().top;
      const s = scrollRef.current.getBoundingClientRect().top;
      setMargin(c - s + scrollRef.current.scrollTop);
    }
  }, [scrollRef, width]);

  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowH,
    overscan: 3,
    scrollMargin: margin,
    initialRect: { width, height: 800 },
  });

  useLayoutEffect(() => {
    virtualizer.measure();
  }, [rowH, cols, virtualizer]);

  return (
    <div ref={container} role="list" aria-label="Quadrinhos" className={view === "list" ? "rounded-md border border-border" : ""}>
      {width > 0 && (
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {virtualizer.getVirtualItems().map((row) => (
            <div
              key={row.key}
              className="absolute inset-x-0 flex items-start"
              style={{ transform: `translateY(${row.start - margin}px)`, height: rowH, gap }}
            >
              {comics.slice(row.index * cols, row.index * cols + cols).map((c) => (
                <div role="listitem" key={c.id} style={{ width: cardW }} className={view === "list" ? "w-full" : undefined}>
                  {view === "grid" ? (
                    <ComicCard comic={c} width={cardW} collectionId={collectionId} />
                  ) : (
                    <ComicRow comic={c} collectionId={collectionId} />
                  )}
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** Faixa horizontal simples (tela inicial), sem virtualização: no máximo 12 itens. */
export function ComicShelf({ comics }: { comics: ComicSummary[] }) {
  const { settings } = useSettings();
  const w = THUMB_WIDTHS[settings["appearance.thumbSize"]];
  return (
    <div role="list" className="-mx-1 flex gap-5 overflow-x-auto px-1 pt-1 pb-3">
      {comics.map((c) => (
        <div role="listitem" key={c.id} className="shrink-0">
          <ComicCard comic={c} width={w} />
        </div>
      ))}
    </div>
  );
}
