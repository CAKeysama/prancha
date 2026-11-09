import { memo, useId, type ReactNode } from "react";
import { BookOpen, CircleCheck, CircleDashed, FileWarning, MoreHorizontal, Star } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { Cover } from "../../components/Cover";
import { ProgressBar } from "../../components/Feedback";
import { Menu, type MenuEntry } from "../../components/Menu";
import { cn } from "../../components/cn";
import { NewCount } from "../../components/NewCount";
import { seriesProgressLabel, seriesRatio } from "../../domain/library-items";
import type { LibrarySeries } from "../../domain/online";
import { STATUS_LABELS, formatDate, progressLabel, progressRatio, readActionLabel, subtitle } from "../../domain/progress";
import { FORMAT_LABELS, type ComicSummary, type ReadStatus } from "../../domain/types";
import { OnlineCover } from "../online/parts";
import { useComicActions } from "./comic-actions";

interface StatusProps {
  status: ReadStatus;
  label: string;
  ratio: number | null;
  title: string;
}

function StatusLine({ status, label, ratio, title, compact, onResume }: StatusProps & { compact?: boolean; onResume?: (() => void) | undefined }) {
  const Icon = status === "completed" ? CircleCheck : status === "reading" ? BookOpen : CircleDashed;
  // Uma linha só: em andamento mostra onde parou (o estado fica implícito e no leitor de tela).
  const reading = status === "reading";
  return (
    <div className="min-w-0">
      {reading && onResume ? (
        // Na obra, onde parou é também o atalho para continuar dali: o mesmo que o botão "Continuar".
        <button
          type="button"
          onClick={onResume}
          className="flex max-w-full items-center gap-1.5 rounded-sm text-left text-xs text-fg hover:underline"
          aria-label={`Continuar ${title}: ${label}`}
          data-testid="resume-chapter"
        >
          <Icon size={13} aria-hidden className="shrink-0 text-accent" />
          <span className="truncate">{label}</span>
        </button>
      ) : (
        <p className={cn("flex items-center gap-1.5 text-xs", status === "completed" ? "font-medium text-success" : reading ? "text-fg" : "text-muted")}>
          <Icon size={13} aria-hidden className={cn("shrink-0", reading && "text-accent")} />
          {reading && <span className="sr-only">{STATUS_LABELS.reading}: </span>}
          <span className="truncate">{reading ? label : STATUS_LABELS[status]}</span>
        </p>
      )}
      {reading && !compact && ratio !== null && <ProgressBar value={ratio} label={`Progresso de ${title}`} className="mt-1.5" />}
    </div>
  );
}

function Flags({ missing, favorite, newCount = 0 }: { missing: boolean; favorite: boolean; newCount?: number }) {
  return (
    <div className="pointer-events-none absolute top-1.5 right-1.5 left-1.5 flex items-start justify-between gap-1">
      {missing ? (
        <span className="inline-flex items-center gap-1 rounded bg-danger-emphasis px-1.5 py-0.5 text-[11px] font-semibold text-on-emphasis">
          <FileWarning size={12} aria-hidden /> Arquivo ausente
        </span>
      ) : newCount > 0 ? (
        <NewCount count={newCount} className="shadow-overlay" />
      ) : (
        <span />
      )}
      {favorite && (
        <span className="inline-flex size-6 items-center justify-center rounded-full bg-bg/85 text-attention" title="Favorito">
          <Star size={13} fill="currentColor" aria-hidden />
          <span className="sr-only">Favorito</span>
        </span>
      )}
    </div>
  );
}

function MoreMenu({ title, items }: { title: string; items: MenuEntry[] }) {
  return <Menu label={`Ações para ${title}`} items={items} trigger={(p) => <IconButton {...p} icon={MoreHorizontal} label={`Mais ações para ${title}`} size="sm" />} />;
}

/** O que muda entre uma HQ e uma obra de fonte: o desenho do cartão e da linha é o mesmo. */
interface ItemFrame {
  title: string;
  sub: string | null;
  /** O atributo que identifica o item na tela. */
  ids: Record<string, number>;
  cover: ReactNode;
  status: StatusProps;
  favorite: boolean;
  onOpen: () => void;
  /** Sem leitura possível (arquivo ausente), o botão não aparece. */
  onRead: (() => void) | null;
  menu: MenuEntry[];
  /** Capítulos novos da obra. As HQs não têm. */
  newCount?: number;
  /** Clicar em onde parou abre o capítulo no ponto salvo. */
  onResume?: () => void;
}

function CardFrame({ title, sub, ids, cover, status, favorite, missing, onOpen, onRead, menu, width, highlight, newCount, onResume }: ItemFrame & { width: number; missing?: boolean; highlight?: boolean }) {
  const titleId = useId();
  const coverHeight = width * 1.5;
  return (
    <article aria-labelledby={titleId} className="group relative" style={{ width }} {...ids}>
      <button type="button" onClick={onOpen} className={cn("block w-full rounded-md text-left", highlight && "ring-2 ring-accent ring-offset-2 ring-offset-bg")} aria-label={`Ver detalhes de ${title}`}>
        <div className="relative">
          {cover}
          <Flags missing={missing ?? false} favorite={favorite} newCount={newCount} />
        </div>
      </button>
      {onRead && (
        <div className="absolute inset-x-0 flex justify-center opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100" style={{ top: coverHeight - 40 }}>
          <Button size="sm" variant="primary" icon={BookOpen} onClick={onRead} aria-label={`${readActionLabel(status)}: ${title}`}>
            {status.status === "reading" ? "Continuar" : "Ler"}
          </Button>
        </div>
      )}
      <div className="mt-2 flex items-start gap-1">
        <div className="min-w-0 flex-1">
          <h3 id={titleId} className="line-clamp-2 text-sm leading-5 font-semibold break-words" title={title}>
            {title}
          </h3>
          <p className="h-[18px] truncate text-xs text-muted">{sub}</p>
        </div>
        <div className="-mr-1 shrink-0">
          <MoreMenu title={title} items={menu} />
        </div>
      </div>
      <div className="mt-1">
        <StatusLine {...status} onResume={onResume} />
      </div>
    </article>
  );
}

function RowFrame({ title, sub, ids, cover, status, favorite, onOpen, onRead, menu, note, kind, addedAt, newCount = 0 }: ItemFrame & { note?: ReactNode; kind: string; addedAt: number }) {
  return (
    <div className="group flex h-[88px] items-center gap-3 border-b border-border px-3" {...ids}>
      <button type="button" onClick={onOpen} className="flex min-w-0 flex-1 items-center gap-3 rounded-md py-1 text-left" aria-label={`Ver detalhes de ${title}`}>
        <div className="w-12 shrink-0">{cover}</div>
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-1.5 truncate font-semibold">
            <span className="truncate">{title}</span>
            {favorite && (
              <>
                <Star size={13} fill="currentColor" className="shrink-0 text-attention" aria-hidden />
                <span className="sr-only">Favorito</span>
              </>
            )}
            <NewCount count={newCount} />
          </p>
          <p className="truncate text-sm text-muted">{sub}</p>
          {note}
        </div>
        <div className="hidden w-24 shrink-0 text-sm text-muted md:block">{kind}</div>
        <div className="w-44 shrink-0 max-sm:hidden">
          <StatusLine {...status} compact />
          {status.status === "reading" && status.ratio !== null && <ProgressBar value={status.ratio} label={`Progresso de ${title}`} className="mt-1" />}
        </div>
        <div className="hidden w-36 shrink-0 text-xs text-muted lg:block">
          <span className="sr-only">Adicionado em </span>
          {formatDate(addedAt)}
        </div>
      </button>
      <div className="flex shrink-0 items-center gap-1">
        {onRead && (
          <Button size="sm" icon={BookOpen} onClick={onRead} aria-label={`${readActionLabel(status)}: ${title}`}>
            {status.status === "reading" ? "Continuar" : "Ler"}
          </Button>
        )}
        <MoreMenu title={title} items={menu} />
      </div>
    </div>
  );
}

const comicStatus = (comic: ComicSummary): StatusProps => ({ status: comic.status, label: progressLabel(comic), ratio: progressRatio(comic), title: comic.title });
const seriesStatus = (series: LibrarySeries): StatusProps => ({ status: series.readStatus, label: seriesProgressLabel(series), ratio: seriesRatio(series), title: series.title });

interface CardProps {
  comic: ComicSummary;
  width: number;
  collectionId?: number;
  highlight?: boolean;
}

/** Cartão da grade. Áreas distintas: abrir detalhes, ler e menu de ações. */
export const ComicCard = memo(function ComicCard({ comic, width, collectionId, highlight }: CardProps) {
  const actions = useComicActions();
  return (
    <CardFrame
      title={comic.title}
      sub={subtitle(comic)}
      ids={{ "data-comic-id": comic.id }}
      width={width}
      highlight={highlight}
      missing={comic.missing}
      favorite={comic.favorite}
      cover={<Cover comicId={comic.id} title={comic.title} kind={comic.coverKind} rev={comic.coverRev} error={comic.coverError} className="transition-[border-color] group-hover:border-border-control" />}
      status={comicStatus(comic)}
      onOpen={() => actions.openDetails(comic)}
      onRead={comic.missing ? null : () => actions.read(comic)}
      menu={actions.menuFor(comic, { collectionId })}
    />
  );
});

/** Altura estimada do cartão para virtualização. */
export function cardHeight(width: number): number {
  return width * 1.5 + 8 + 40 + 18 + 4 + 18 + 18 + 10;
}

export const ComicRow = memo(function ComicRow({ comic, collectionId }: { comic: ComicSummary; collectionId?: number }) {
  const actions = useComicActions();
  return (
    <RowFrame
      title={comic.title}
      sub={subtitle(comic) ?? comic.fileName}
      ids={{ "data-comic-id": comic.id }}
      favorite={comic.favorite}
      cover={<Cover comicId={comic.id} title={comic.title} kind={comic.coverKind} rev={comic.coverRev} error={comic.coverError} className="rounded" />}
      note={
        comic.missing && (
          <p className="mt-0.5 inline-flex items-center gap-1 text-xs font-medium text-danger">
            <FileWarning size={12} aria-hidden /> Arquivo ausente
          </p>
        )
      }
      kind={FORMAT_LABELS[comic.format]}
      addedAt={comic.addedAt}
      status={comicStatus(comic)}
      onOpen={() => actions.openDetails(comic)}
      onRead={comic.missing ? null : () => actions.read(comic)}
      menu={actions.menuFor(comic, { collectionId })}
    />
  );
});

export const SeriesItemCard = memo(function SeriesItemCard({ series, width, collectionId }: { series: LibrarySeries; width: number; collectionId?: number }) {
  const actions = useComicActions();
  return (
    <CardFrame
      title={series.title}
      sub={series.sourceName}
      ids={{ "data-series-id": series.id }}
      width={width}
      favorite={series.favorite}
      cover={<OnlineCover series={series} />}
      status={seriesStatus(series)}
      newCount={series.newCount}
      onResume={() => actions.readSeries(series)}
      onOpen={() => actions.openSeries(series)}
      onRead={() => actions.readSeries(series)}
      menu={actions.seriesMenu(series, { collectionId })}
    />
  );
});

export const SeriesItemRow = memo(function SeriesItemRow({ series, collectionId }: { series: LibrarySeries; collectionId?: number }) {
  const actions = useComicActions();
  return (
    <RowFrame
      title={series.title}
      sub={series.sourceName}
      ids={{ "data-series-id": series.id }}
      favorite={series.favorite}
      cover={<OnlineCover series={series} className="rounded" />}
      kind="Online"
      addedAt={series.addedAt}
      newCount={series.newCount}
      status={seriesStatus(series)}
      onOpen={() => actions.openSeries(series)}
      onRead={() => actions.readSeries(series)}
      menu={actions.seriesMenu(series, { collectionId })}
    />
  );
});
