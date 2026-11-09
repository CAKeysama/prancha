import { useState, type ReactNode } from "react";
import { RefreshCw, Search, ShieldCheck } from "lucide-react";
import { Button } from "../../components/Button";
import { PlaceholderCover } from "../../components/Cover";
import { NewCount } from "../../components/NewCount";
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

type CoverSeries = Pick<SeriesCard, "id" | "title" | "hasCover">;

/** A imagem vem pelo protocolo do app: quem fala com o servidor da fonte é o backend. */
export function OnlineCover({ series, eager, className }: { series: CoverSeries; eager?: boolean; className?: string }) {
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

/** As fontes que liberaram a verificação e seguem recusando nesta sessão: repetir não resolve. */
const refusedSites = new Set<string>();

/** A fonte voltou a responder: se pedir a verificação outra vez, vale tentar. */
export function siteAnswered(sourceId: string) {
  refusedSites.delete(sourceId);
}

const REFUSED_MESSAGE = "Este site recusa o Prancha, mesmo depois da verificação. Verificar de novo não resolve.";

/**
 * O que dizer e oferecer quando uma fonte não abre, sem repetir o que já falhou.
 * Com `site`, o site pediu uma verificação: a janela abre para a pessoa passar à mão e, ao fechar, `onRetry` tenta de novo.
 */
export function SourceTrouble({
  message,
  site,
  sourceId,
  onRetry,
  retrying,
  elsewhere,
  size,
  center,
  children,
}: {
  message: string;
  site?: VerifiableSite | undefined;
  /** Vale para a fonte inteira quando informado, e não só para a obra ou o capítulo em `site`. */
  sourceId?: string;
  onRetry: () => void;
  retrying?: boolean;
  elsewhere?: (() => void) | undefined;
  size?: "sm";
  center?: boolean;
  children: (text: string, actions: ReactNode) => ReactNode;
}) {
  const toast = useToast();
  const [waiting, setWaiting] = useState(false);
  const key = sourceId ?? JSON.stringify(site);
  const refused = !!site && refusedSites.has(key);
  const verify = async () => {
    if (!site) return;
    setWaiting(true);
    try {
      if (await api.sourceVerify(site)) refusedSites.add(key);
      onRetry();
    } catch (e) {
      toast.error("Não foi possível abrir o site", errorMessage(e));
    } finally {
      setWaiting(false);
    }
  };
  const actions = (
    <div className={cn("flex flex-wrap gap-2", center && "justify-center")}>
      {site && !refused && (
        <Button variant="primary" size={size} icon={ShieldCheck} loading={waiting} onClick={() => void verify()}>
          {waiting ? "Aguardando a verificação…" : "Verificar no site"}
        </Button>
      )}
      {elsewhere && (
        <Button variant={refused ? "primary" : "secondary"} size={size} icon={Search} onClick={elsewhere}>
          Procurar em outras fontes
        </Button>
      )}
      <Button size={size} icon={RefreshCw} loading={retrying} onClick={onRetry}>
        Tentar de novo
      </Button>
    </div>
  );
  return <>{children(refused ? REFUSED_MESSAGE : message, actions)}</>;
}

export const SERIES_GRID = "grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-4";

export function SeriesGrid({ items, onOpen, showSource }: { items: SeriesCard[]; onOpen: (series: SeriesCard) => void; showSource?: boolean }) {
  return (
    <ul className={SERIES_GRID}>
      {items.map((series) => (
        <li key={series.id}>
          <button type="button" onClick={() => onOpen(series)} className={cn("group block w-full rounded-md text-left focus-visible:outline-2 focus-visible:outline-accent")}>
            <div className="relative">
              <OnlineCover series={series} />
              {series.newCount > 0 && (
                <span className="pointer-events-none absolute top-1.5 left-1.5">
                  <NewCount count={series.newCount} className="shadow-overlay" />
                </span>
              )}
            </div>
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
