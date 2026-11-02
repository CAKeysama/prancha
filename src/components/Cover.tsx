import { memo, useState } from "react";
import { ImageOff } from "lucide-react";
import type { CoverKind } from "../domain/types";
import { coverUrl } from "../services/urls";
import { cn } from "./cn";

/** Hash simples e estável para variar o desenho da capa substituta. */
function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/**
 * Capa substituta gerada localmente: título + desenho geométrico discreto e o aviso
 * explícito de que a imagem original não pôde ser extraída.
 */
export function PlaceholderCover({ title, reason, className }: { title: string; reason?: string | null; className?: string }) {
  const h = hash(title);
  const shapes = Array.from({ length: 5 }, (_, i) => {
    const v = (h >>> (i * 5)) & 31;
    return { x: (v * 7) % 100, y: ((v * 13) % 100) + i * 8, r: 10 + (v % 18), kind: (v + i) % 3 };
  });
  return (
    <div
      className={cn("relative flex h-full w-full flex-col overflow-hidden bg-elevated text-fg", className)}
      title={reason ? `Capa substituta: ${reason}` : "Capa substituta"}
    >
      <svg className="absolute inset-0 h-full w-full text-border" viewBox="0 0 100 150" preserveAspectRatio="xMidYMid slice" aria-hidden>
        {shapes.map((s, i) =>
          s.kind === 0 ? (
            <circle key={i} cx={s.x} cy={s.y} r={s.r} fill="none" stroke="currentColor" strokeWidth="1.2" />
          ) : s.kind === 1 ? (
            <rect key={i} x={s.x - s.r / 2} y={s.y - s.r / 2} width={s.r} height={s.r} fill="none" stroke="currentColor" strokeWidth="1.2" />
          ) : (
            <line key={i} x1={s.x - s.r} y1={s.y} x2={s.x + s.r} y2={s.y + s.r} stroke="currentColor" strokeWidth="1.2" />
          ),
        )}
      </svg>
      <div className="relative flex flex-1 items-center px-3">
        <p className="line-clamp-4 text-center text-sm font-semibold break-words w-full">{title}</p>
      </div>
      <div className="relative flex items-center justify-center gap-1 border-t border-border bg-surface px-2 py-1 text-[11px] font-medium text-muted">
        <ImageOff size={12} aria-hidden />
        <span>Capa indisponível</span>
      </div>
    </div>
  );
}

interface CoverProps {
  comicId: number;
  title: string;
  kind: CoverKind;
  rev: number;
  error?: string | null;
  className?: string;
  /** Carregamento imediato para capas em destaque (detalhes). */
  eager?: boolean;
}

/** Capa real em cache, capa pendente (sem inventar imagem) ou capa substituta identificada. */
export const Cover = memo(function Cover({ comicId, title, kind, rev, error, className, eager }: CoverProps) {
  const [failedRev, setFailedRev] = useState<number | null>(null);
  const [loaded, setLoaded] = useState(false);
  const base = cn("relative aspect-[2/3] w-full overflow-hidden rounded-md border border-border bg-surface", className);

  if (kind === "pending") {
    return (
      <div className={base} role="img" aria-label={`Capa de ${title} sendo preparada`}>
        <div className="p-skeleton absolute inset-0 bg-elevated" />
        <span className="absolute inset-x-0 bottom-2 text-center text-[11px] text-muted">Preparando capa…</span>
      </div>
    );
  }
  if (kind === "placeholder" || failedRev === rev) {
    return (
      <div className={base} role="img" aria-label={`Capa substituta de ${title} (imagem original indisponível)`}>
        <PlaceholderCover title={title} reason={error ?? (failedRev === rev ? "a miniatura não pôde ser carregada" : null)} />
      </div>
    );
  }
  return (
    <div className={base}>
      {!loaded && <div className="p-skeleton absolute inset-0 bg-elevated" aria-hidden />}
      <img
        src={coverUrl(comicId, rev)}
        alt={`Capa de ${title}`}
        loading={eager ? "eager" : "lazy"}
        decoding="async"
        draggable={false}
        onLoad={() => setLoaded(true)}
        onError={() => setFailedRev(rev)}
        className="absolute inset-0 h-full w-full object-contain"
      />
    </div>
  );
});
