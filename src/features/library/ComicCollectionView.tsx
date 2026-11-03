import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { isSeries, itemKey, type LibraryItem } from "../../domain/library-items";
import { THUMB_WIDTHS } from "../../domain/settings";
import { useSettings } from "../../app/settings-context";
import { ComicCard, ComicRow, SeriesItemCard, SeriesItemRow, cardHeight } from "./ComicItem";

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

function Item({ item, view, width, collectionId }: { item: LibraryItem; view: "grid" | "list"; width: number; collectionId?: number }) {
  if (isSeries(item)) return view === "grid" ? <SeriesItemCard series={item} width={width} collectionId={collectionId} /> : <SeriesItemRow series={item} collectionId={collectionId} />;
  return view === "grid" ? <ComicCard comic={item} width={width} collectionId={collectionId} /> : <ComicRow comic={item} collectionId={collectionId} />;
}

interface Props {
  items: LibraryItem[];
  view: "grid" | "list";
  scrollRef: RefObject<HTMLDivElement | null>;
  collectionId?: number;
}

/** Grade ou lista virtualizada: só os itens visíveis são renderizados. */
export function ComicCollectionView({ items, view, scrollRef, collectionId }: Props) {
  const { settings } = useSettings();
  const container = useRef<HTMLDivElement>(null);
  const width = useWidth(container);
  const compact = settings["appearance.density"] === "compact";
  const gap = compact ? 12 : 20;
  const minCard = THUMB_WIDTHS[settings["appearance.thumbSize"]];
  const cols = view === "grid" ? Math.max(1, Math.floor((width + gap) / (minCard + gap))) : 1;
  const cardW = view === "grid" ? Math.floor((width - gap * (cols - 1)) / cols) : width;
  const rowH = view === "grid" ? cardHeight(cardW) + gap : 88;
  const rows = Math.ceil(items.length / cols);
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
              {items.slice(row.index * cols, row.index * cols + cols).map((item) => (
                <div role="listitem" key={itemKey(item)} style={{ width: cardW }} className={view === "list" ? "w-full" : undefined}>
                  <Item item={item} view={view} width={cardW} collectionId={collectionId} />
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
export function ComicShelf({ items }: { items: LibraryItem[] }) {
  const { settings } = useSettings();
  const w = THUMB_WIDTHS[settings["appearance.thumbSize"]];
  return (
    <div role="list" className="-mx-1 flex gap-5 overflow-x-auto px-1 pt-1 pb-3">
      {items.map((item) => (
        <div role="listitem" key={itemKey(item)} className="shrink-0">
          <Item item={item} view="grid" width={w} />
        </div>
      ))}
    </div>
  );
}
