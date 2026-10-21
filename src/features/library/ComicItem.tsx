import { memo, useId } from "react";
import { BookOpen, CircleCheck, CircleDashed, FileWarning, MoreHorizontal, Star } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { Cover } from "../../components/Cover";
import { ProgressBar } from "../../components/Feedback";
import { Menu } from "../../components/Menu";
import { cn } from "../../components/cn";
import { STATUS_LABELS, formatDate, progressLabel, progressRatio, readActionLabel, subtitle } from "../../domain/progress";
import { FORMAT_LABELS, type ComicSummary } from "../../domain/types";
import { useComicActions } from "./comic-actions";

export function StatusLine({ comic, compact }: { comic: ComicSummary; compact?: boolean }) {
  const ratio = progressRatio(comic);
  const Icon = comic.status === "completed" ? CircleCheck : comic.status === "reading" ? BookOpen : CircleDashed;
  // Uma linha só: em andamento mostra onde parou (o estado fica implícito e no leitor de tela).
  const reading = comic.status === "reading";
  return (
    <div className="min-w-0">
      <p
        className={cn(
          "flex items-center gap-1.5 text-xs",
          comic.status === "completed" ? "font-medium text-success" : reading ? "text-fg" : "text-muted",
        )}
      >
        <Icon size={13} aria-hidden className={cn("shrink-0", reading && "text-accent")} />
        {reading && <span className="sr-only">{STATUS_LABELS.reading}: </span>}
        <span className="truncate">{reading ? progressLabel(comic) : STATUS_LABELS[comic.status]}</span>
      </p>
      {reading && !compact && ratio !== null && <ProgressBar value={ratio} label={`Progresso de ${comic.title}`} className="mt-1.5" />}
    </div>
  );
}

function Flags({ comic }: { comic: ComicSummary }) {
  return (
    <div className="pointer-events-none absolute top-1.5 right-1.5 left-1.5 flex items-start justify-between gap-1">
      {comic.missing ? (
        <span className="inline-flex items-center gap-1 rounded bg-danger-emphasis px-1.5 py-0.5 text-[11px] font-semibold text-on-emphasis">
          <FileWarning size={12} aria-hidden /> Arquivo ausente
        </span>
      ) : (
        <span />
      )}
      {comic.favorite && (
        <span className="inline-flex size-6 items-center justify-center rounded-full bg-bg/85 text-attention" title="Favorito">
          <Star size={13} fill="currentColor" aria-hidden />
          <span className="sr-only">Favorito</span>
        </span>
      )}
    </div>
  );
}

function MoreMenu({ comic, collectionId }: { comic: ComicSummary; collectionId?: number }) {
  const actions = useComicActions();
  return (
    <Menu
      label={`Ações para ${comic.title}`}
      items={actions.menuFor(comic, { collectionId })}
      trigger={(p) => <IconButton {...p} icon={MoreHorizontal} label={`Mais ações para ${comic.title}`} size="sm" />}
    />
  );
}

interface CardProps {
  comic: ComicSummary;
  width: number;
  collectionId?: number;
  highlight?: boolean;
}

/** Cartão da grade. Áreas distintas: abrir detalhes, ler e menu de ações. */
export const ComicCard = memo(function ComicCard({ comic, width, collectionId, highlight }: CardProps) {
  const actions = useComicActions();
  const titleId = useId();
  const sub = subtitle(comic);
  const coverHeight = width * 1.5;
  return (
    <article aria-labelledby={titleId} className="group relative" style={{ width }} data-comic-id={comic.id}>
      <button
        type="button"
        onClick={() => actions.openDetails(comic)}
        className={cn("block w-full rounded-md text-left", highlight && "ring-2 ring-accent ring-offset-2 ring-offset-bg")}
        aria-label={`Ver detalhes de ${comic.title}`}
      >
        <div className="relative">
          <Cover comicId={comic.id} title={comic.title} kind={comic.coverKind} rev={comic.coverRev} error={comic.coverError} className="transition-[border-color] group-hover:border-border-control" />
          <Flags comic={comic} />
        </div>
      </button>
      {!comic.missing && (
        <div
          className="absolute inset-x-0 flex justify-center opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100"
          style={{ top: coverHeight - 40 }}
        >
          <Button size="sm" variant="primary" icon={BookOpen} onClick={() => actions.read(comic)} aria-label={`${readActionLabel(comic)}: ${comic.title}`}>
            {comic.status === "reading" ? "Continuar" : "Ler"}
          </Button>
        </div>
      )}
      <div className="mt-2 flex items-start gap-1">
        <div className="min-w-0 flex-1">
          <h3 id={titleId} className="line-clamp-2 text-sm leading-5 font-semibold break-words" title={comic.title}>
            {comic.title}
          </h3>
          <p className="h-[18px] truncate text-xs text-muted">{sub}</p>
        </div>
        <div className="-mr-1 shrink-0">
          <MoreMenu comic={comic} collectionId={collectionId} />
        </div>
      </div>
      <div className="mt-1">
        <StatusLine comic={comic} />
      </div>
    </article>
  );
});

/** Altura estimada do cartão para virtualização. */
export function cardHeight(width: number): number {
  return width * 1.5 + 8 + 40 + 18 + 4 + 18 + 18 + 10;
}

export const ComicRow = memo(function ComicRow({ comic, collectionId }: { comic: ComicSummary; collectionId?: number }) {
  const actions = useComicActions();
  const sub = subtitle(comic);
  const ratio = progressRatio(comic);
  return (
    <div className="group flex h-[88px] items-center gap-3 border-b border-border px-3" data-comic-id={comic.id}>
      <button
        type="button"
        onClick={() => actions.openDetails(comic)}
        className="flex min-w-0 flex-1 items-center gap-3 rounded-md py-1 text-left"
        aria-label={`Ver detalhes de ${comic.title}`}
      >
        <div className="w-12 shrink-0">
          <Cover comicId={comic.id} title={comic.title} kind={comic.coverKind} rev={comic.coverRev} error={comic.coverError} className="rounded" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-1.5 truncate font-semibold">
            <span className="truncate">{comic.title}</span>
            {comic.favorite && (
              <>
                <Star size={13} fill="currentColor" className="shrink-0 text-attention" aria-hidden />
                <span className="sr-only">Favorito</span>
              </>
            )}
          </p>
          <p className="truncate text-sm text-muted">{sub ?? comic.fileName}</p>
          {comic.missing && (
            <p className="mt-0.5 inline-flex items-center gap-1 text-xs font-medium text-danger">
              <FileWarning size={12} aria-hidden /> Arquivo ausente
            </p>
          )}
        </div>
        <div className="hidden w-24 shrink-0 text-sm text-muted md:block">{FORMAT_LABELS[comic.format]}</div>
        <div className="w-44 shrink-0 max-sm:hidden">
          <StatusLine comic={comic} compact />
          {comic.status === "reading" && ratio !== null && <ProgressBar value={ratio} label={`Progresso de ${comic.title}`} className="mt-1" />}
        </div>
        <div className="hidden w-36 shrink-0 text-xs text-muted lg:block">
          <span className="sr-only">Adicionado em </span>
          {formatDate(comic.addedAt)}
        </div>
      </button>
      <div className="flex shrink-0 items-center gap-1">
        {!comic.missing && (
          <Button size="sm" icon={BookOpen} onClick={() => actions.read(comic)} aria-label={`${readActionLabel(comic)}: ${comic.title}`}>
            {comic.status === "reading" ? "Continuar" : "Ler"}
          </Button>
        )}
        <MoreMenu comic={comic} collectionId={collectionId} />
      </div>
    </div>
  );
});
