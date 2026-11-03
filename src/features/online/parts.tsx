import { useState, type ReactNode } from "react";
import { ShieldCheck } from "lucide-react";
import { Button } from "../../components/Button";
import { PlaceholderCover } from "../../components/Cover";
import { useToast } from "../../components/Toast";
import { cn } from "../../components/cn";
import type { SeriesCard } from "../../domain/online";
import { api, errorMessage, type VerifiableSite } from "../../services/api";
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
export function OnlineCover({ series, eager, className }: { series: Pick<SeriesCard, "id" | "title" | "hasCover">; eager?: boolean; className?: string }) {
  const [failed, setFailed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  if (!series.hasCover || failed) {
    return (
      <div className={cn(COVER_BOX, className)} role="img" aria-label={`Capa substituta de ${series.title}`}>
        <PlaceholderCover title={series.title} />
      </div>
    );
  }
  return (
    <div className={cn(COVER_BOX, className)}>
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

/** Os sites que já liberaram nesta sessão: se ainda recusam, repetir a verificação não resolve. */
const released = new Set<string>();

/**
 * Abre o site numa janela para a pessoa passar pela verificação à mão.
 * Quando a janela fecha, `onDone` tenta de novo.
 */
export function VerifySiteButton({ site, onDone, size }: { site: VerifiableSite; onDone: () => void; size?: "sm" }) {
  const toast = useToast();
  const key = JSON.stringify(site);
  const [waiting, setWaiting] = useState(false);
  const [refusedAnyway, setRefusedAnyway] = useState(() => released.has(key));
  const verify = async () => {
    setWaiting(true);
    try {
      if (await api.sourceVerify(site)) {
        released.add(key);
        setRefusedAnyway(true);
      }
      onDone();
    } catch (e) {
      toast.error("Não foi possível abrir o site", errorMessage(e));
    } finally {
      setWaiting(false);
    }
  };
  return (
    <>
      {refusedAnyway && !waiting && <p className="text-sm text-muted">O site liberou a verificação e mesmo assim recusou o Prancha. Procure esta obra em outra fonte.</p>}
      <Button variant="primary" size={size} icon={ShieldCheck} loading={waiting} onClick={() => void verify()}>
        {waiting ? "Aguardando a verificação…" : "Verificar no site"}
      </Button>
    </>
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
