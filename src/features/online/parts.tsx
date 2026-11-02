import { useState, type ReactNode } from "react";
import { PlaceholderCover } from "../../components/Cover";
import { cn } from "../../components/cn";
import type { SeriesCard } from "../../domain/online";
import { onlineCoverUrl } from "../../services/urls";

export function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[140px_1fr] gap-3 border-b border-border py-2 last:border-b-0 max-sm:grid-cols-1 max-sm:gap-0.5">
      <dt className="text-sm text-muted">{label}</dt>
      <dd className="min-w-0 text-base break-words">{children}</dd>
    </div>
  );
}

const COVER_BOX = "relative aspect-[2/3] w-full overflow-hidden rounded-md border border-border bg-surface";

/** A imagem vem pelo protocolo do app: quem fala com o servidor da fonte é o backend. */
export function OnlineCover({ series, eager }: { series: SeriesCard; eager?: boolean }) {
  const [failed, setFailed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  if (!series.hasCover || failed) {
    return (
      <div className={COVER_BOX} role="img" aria-label={`Capa substituta de ${series.title}`}>
        <PlaceholderCover title={series.title} />
      </div>
    );
  }
  return (
    <div className={COVER_BOX}>
      {!loaded && <div className="p-skeleton absolute inset-0 bg-elevated" aria-hidden />}
      <img
        src={onlineCoverUrl(series.id)}
        alt={`Capa de ${series.title}`}
        loading={eager ? "eager" : "lazy"}
        decoding="async"
        draggable={false}
        onLoad={() => setLoaded(true)}
        onError={() => setFailed(true)}
        className="absolute inset-0 h-full w-full object-contain"
      />
    </div>
  );
}

export const SERIES_GRID = "grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-4";

export function SeriesGrid({ items, onOpen, showSource }: { items: SeriesCard[]; onOpen: (series: SeriesCard) => void; showSource?: boolean }) {
  return (
    <ul className={SERIES_GRID}>
      {items.map((series) => (
        <li key={series.id}>
          <button type="button" onClick={() => onOpen(series)} className={cn("group block w-full rounded-md text-left focus-visible:outline-2 focus-visible:outline-accent")}>
            <OnlineCover series={series} />
            <span className="mt-1.5 line-clamp-2 text-sm font-medium group-hover:underline">{series.title}</span>
            {showSource && series.sourceName && <span className="block truncate text-xs text-muted">{series.sourceName}</span>}
          </button>
        </li>
      ))}
    </ul>
  );
}

export function GridSkeleton({ count = 4 }: { count?: number }) {
  return (
    <div className={SERIES_GRID} aria-hidden>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="p-skeleton aspect-[2/3] rounded-md bg-elevated" />
      ))}
    </div>
  );
}
