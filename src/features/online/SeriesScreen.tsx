import { memo, useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { ArrowDownNarrowWide, ArrowUpNarrowWide, Bell, BookOpen, CloudOff, Download, FolderPlus, MoreHorizontal, RefreshCw, Search, Star, X } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { cn } from "../../components/cn";
import { ReadPrevious, ReadTicks, UnreadTicks } from "../../components/icons";
import { StateToggle } from "../../components/StateToggle";
import { TOOLTIP_DELAY_MS } from "../../components/Tooltip";
import { ErrorState, ProgressBar, Spinner } from "../../components/Feedback";
import { Checkbox } from "../../components/Form";
import { Menu } from "../../components/Menu";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { useLibraryVersion } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import { useScrollMemory } from "../../app/scroll-memory";
import {
  CHAPTER_STATE_LABELS,
  DOWNLOAD_STATUS_LABELS,
  READ_STATE_LABELS,
  SERIES_STATUS_LABELS,
  applyReadInfo,
  languageName,
  previousChapters,
  sortedChapters,
  type ChapterReadInfo,
  type ChapterReadState,
  type ChapterView,
  type SeriesView,
} from "../../domain/online";
import { formatDate } from "../../domain/progress";
import { ApiError, api, errorMessage } from "../../services/api";
import { onDownloadChanged } from "../../services/events";
import { useComicActions } from "../library/comic-actions";
import { searchAcrossSources } from "./ExploreScreen";
import { Fact, OnlineCover, SourceTrouble, siteAnswered } from "./parts";
import { MalSeriesFact } from "../mal/MalSeries";

let requestCounter = 0;

/** Uma obra sincronizada com a fonte há menos disto abre só com o que está gravado, sem consultar a fonte. */
export const SERIES_FRESH_MS = 5 * 60 * 1000;
const syncedAt = new Map<number, number>();
/** As últimas obras abertas, para a volta do leitor mostrar a tela na hora. */
const lastViews = new Map<number, SeriesView>();
const MAX_VIEWS = 30;

function remember(view: SeriesView) {
  lastViews.delete(view.id);
  lastViews.set(view.id, view);
  if (lastViews.size > MAX_VIEWS) lastViews.delete(lastViews.keys().next().value!);
}

/** Esquece o que foi guardado. Para os testes. */
export function clearSeriesMemory() {
  syncedAt.clear();
  lastViews.clear();
}

const ACTIVE = new Set(["queued", "running", "paused"]);
const isActive = (chapter: ChapterView) => !!chapter.download && ACTIVE.has(chapter.download.status);

function publishedDate(ms: number): string {
  return new Date(ms).toLocaleDateString("pt-BR", { dateStyle: "medium" });
}

/** Marca discreta de estado ao lado do título. Não lido não tem marca. */
function StateMark({ state }: { state: ChapterReadState }) {
  if (state === "reading")
    return (
      <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-accent-subtle px-1.5 py-px text-[11px] font-semibold text-accent-fg" data-testid="chapter-reading">
        <span className="size-1.5 rounded-full bg-accent" aria-hidden />
        {READ_STATE_LABELS.reading}
      </span>
    );
  if (state === "in_progress")
    return (
      <span className="inline-flex shrink-0 items-center rounded-full bg-attention-subtle px-1.5 py-px text-[11px] font-semibold text-attention" data-testid="chapter-in-progress">
        {READ_STATE_LABELS.in_progress}
      </span>
    );
  if (state === "read")
    return (
      <span className="inline-flex shrink-0 items-center gap-0.5 text-success" data-testid="chapter-read" title={READ_STATE_LABELS.read}>
        <ReadTicks size={15} aria-hidden />
        <span className="sr-only">{READ_STATE_LABELS.read}</span>
      </span>
    );
  return null;
}

interface RowActions {
  onSelect: (id: number, selected: boolean) => void;
  onDownload: (id: number) => void;
  onCancel: (downloadId: number) => void;
  onRead: (id: number) => void;
  onSetRead: (id: number, read: boolean) => void;
  onReadPrevious: (id: number) => void;
}

const ChapterRow = memo(function ChapterRow({ chapter, selected, busy, hasPrevious, actions }: { chapter: ChapterView; selected: boolean; busy: boolean; hasPrevious: boolean; actions: RowActions }) {
  const download = chapter.download;
  const active = isActive(chapter);
  const failed = download?.status === "failed";
  const read = chapter.readState === "read";
  const statusId = useId();
  const started = chapter.readState === "reading" || chapter.readState === "in_progress";
  const details = [
    chapter.publishedAt ? publishedDate(chapter.publishedAt) : null,
    CHAPTER_STATE_LABELS[chapter.state],
    started && chapter.progress ? `Página ${chapter.progress.pageIndex + 1} de ${chapter.progress.pageCount}` : null,
  ].filter(Boolean);
  const name = chapter.number ? `${chapter.number} ${chapter.title}` : chapter.title;
  return (
    <li
      data-testid="chapter"
      data-read-state={chapter.readState}
      data-new={chapter.isNew || undefined}
      className={cn(
        "flex items-center gap-3 border-b border-border py-2.5 last:border-b-0",
        chapter.readState === "reading" && "-mx-2 rounded-md border-transparent bg-accent-subtle px-2",
      )}
    >
      {chapter.state !== "offline" && !active && <Checkbox label={`Selecionar ${chapter.title}`} hideLabel checked={selected} onChange={(on) => actions.onSelect(chapter.id, on)} />}
      <div className="min-w-0 flex-1">
        <p className="flex min-w-0 items-center gap-2">
          <span className={cn("truncate", read ? "font-normal text-muted" : "font-medium")}>
            {chapter.number && <span className="mr-2 text-muted">{chapter.number}</span>}
            {chapter.title}
          </span>
          {chapter.isNew && (
            <span className="shrink-0 rounded-full border border-accent/40 px-1.5 py-px text-[11px] leading-4 font-semibold text-accent-fg" data-testid="chapter-new">
              Novo
            </span>
          )}
          <StateMark state={chapter.readState} />
        </p>
        <p className={cn("truncate text-sm text-muted", read && "text-muted/80")}>{details.join(" · ")}</p>
        <span id={statusId} className="sr-only">
          {READ_STATE_LABELS[chapter.readState]}
          {chapter.isNew ? ", novo" : ""}
        </span>
        {active && download && (
          <div className="mt-1.5 flex max-w-sm items-center gap-2">
            <ProgressBar value={download.pagesTotal ? download.pagesDone / download.pagesTotal : 0} label={`Download de ${chapter.title}`} />
            <span className="shrink-0 text-xs text-muted">
              {DOWNLOAD_STATUS_LABELS[download.status]}
              {download.pagesTotal > 0 && ` ${download.pagesDone}/${download.pagesTotal}`}
            </span>
          </div>
        )}
        {failed && download?.error && <p className="mt-1 text-sm text-danger">{download.error}</p>}
      </div>
      <div className="flex shrink-0 items-center gap-0.5" role="group" aria-label={`Ações de leitura de ${name}`}>
        {read ? (
          <IconButton icon={UnreadTicks} label="Marcar como não lido" tooltipDelay={TOOLTIP_DELAY_MS} aria-describedby={statusId} disabled={busy} onClick={() => actions.onSetRead(chapter.id, false)} />
        ) : (
          <IconButton icon={ReadTicks} label="Marcar como lido" tooltipDelay={TOOLTIP_DELAY_MS} aria-describedby={statusId} disabled={busy} onClick={() => actions.onSetRead(chapter.id, true)} />
        )}
        <IconButton
          icon={ReadPrevious}
          label="Marcar capítulos anteriores como lidos"
          tooltipDelay={TOOLTIP_DELAY_MS}
          aria-describedby={statusId}
          disabled={busy || !hasPrevious}
          onClick={() => actions.onReadPrevious(chapter.id)}
        />
        {/* O lugar do download existe mesmo sem botão: os ícones ficam alinhados em coluna na lista inteira. */}
        <span className="inline-flex size-8 items-center justify-center">
          {active && download ? (
            <IconButton icon={X} label="Cancelar download" tooltipDelay={TOOLTIP_DELAY_MS} aria-describedby={statusId} onClick={() => actions.onCancel(download.id)} />
          ) : chapter.state !== "offline" ? (
            <IconButton icon={failed ? RefreshCw : Download} label={failed ? "Tentar baixar de novo" : "Baixar capítulo"} tooltipDelay={TOOLTIP_DELAY_MS} aria-describedby={statusId} onClick={() => actions.onDownload(chapter.id)} />
          ) : null}
        </span>
      </div>
      <Button icon={BookOpen} onClick={() => actions.onRead(chapter.id)}>
        Ler
      </Button>
    </li>
  );
});

export function SeriesScreen({ id }: { id: number }) {
  const nav = useNav();
  const toast = useToast();
  const actions = useComicActions();
  const { version } = useLibraryVersion();
  const [view, setView] = useState<SeriesView | null>(() => lastViews.get(id) ?? null);
  const [fatal, setFatal] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(true);
  const scrollRef = useScrollMemory<HTMLDivElement>(`series:${id}`);
  useEffect(() => {
    if (view && view.id === id) remember(view);
  }, [view, id]);
  const [attempt, setAttempt] = useState(0);
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const readStored = useCallback(() => {
    api
      .seriesStored(id)
      .then((stored) => setView((current) => (current ? { ...stored, sourceError: current.sourceError, sourceProtected: current.sourceProtected } : stored)))
      .catch(() => undefined);
  }, [id]);

  // O que já se sabe aparece na hora, com ou sem internet. A fonte atualiza em seguida.
  useEffect(() => {
    let alive = true;
    let opened = false;
    const requestId = `series-${++requestCounter}`;
    // Sincronizada há pouco: o que está gravado já é o que a fonte disse. Tentar de novo sempre consulta.
    const recent = attempt === 0 && Date.now() - (syncedAt.get(id) ?? 0) < SERIES_FRESH_MS;
    api
      .seriesStored(id)
      .then((stored) => {
        if (!alive) return;
        // O gravado vale mais que a cópia da última visita, e menos que a resposta da fonte.
        if (!opened) setView(stored);
        if (recent) setRefreshing(false);
      })
      .catch((e: unknown) => alive && setFatal(errorMessage(e)));
    if (recent) {
      return () => {
        alive = false;
      };
    }
    api
      .seriesOpen(requestId, id)
      .then((fresh) => {
        if (!alive) return;
        opened = true;
        setView(fresh);
        if (!fresh.sourceError) {
          siteAnswered(fresh.sourceId);
          syncedAt.set(id, Date.now());
        }
      })
      .catch((e: unknown) => {
        if (alive && !(e instanceof ApiError && e.code === "extension_cancelled")) setFatal(errorMessage(e));
      })
      .finally(() => alive && setRefreshing(false));
    return () => {
      alive = false;
      void api.sourceCancel(requestId).catch(() => undefined);
    };
  }, [id, attempt]);

  // A abertura já leu o que está gravado: aqui, só as mudanças na biblioteca depois dela.
  const seenVersion = useRef(version);
  useEffect(() => {
    if (seenVersion.current === version) return;
    seenVersion.current = version;
    if (view) readStored();
    // Só quando a biblioteca muda: ler a cada mudança de `view` entraria em ciclo.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version, readStored]);

  useEffect(() => {
    return onDownloadChanged((event) => {
      if (event.seriesId !== id) return;
      setView((current) =>
        current && {
          ...current,
          chapters: current.chapters.map((chapter) =>
            chapter.id === event.chapterId ? { ...chapter, download: { id: event.id, status: event.status, pagesDone: event.pagesDone, pagesTotal: event.pagesTotal, error: event.error } } : chapter,
          ),
        },
      );
      if (!ACTIVE.has(event.status)) readStored();
    });
  }, [id, readStored]);

  const chapters = useMemo(() => (view ? sortedChapters(view.chapters, view.chapterSort) : []), [view]);

  const enqueueRef = useRef<(ids: number[]) => Promise<void>>(async () => undefined);
  const cancelRef = useRef<(downloadId: number) => Promise<void>>(async () => undefined);
  const enqueue = async (ids: number[]) => {
    if (ids.length === 0) return;
    try {
      await api.downloadsEnqueue(ids);
      setSelected(new Set());
      readStored();
    } catch (e) {
      toast.error("Não foi possível iniciar o download", errorMessage(e));
    }
  };
  const cancel = async (downloadId: number) => {
    try {
      await api.downloadCancel(downloadId);
    } catch (e) {
      toast.error("Não foi possível cancelar", errorMessage(e));
    }
  };
  useEffect(() => {
    enqueueRef.current = enqueue;
    cancelRef.current = cancel;
  });
  const toggleFollowing = async () => {
    if (!view) return;
    const following = !view.following;
    setView((current) => current && { ...current, following });
    try {
      const after = await api.seriesSetFollowing(id, following);
      setView((current) => current && { ...current, following: after.following });
    } catch (e) {
      setView((current) => current && { ...current, following: !following });
      toast.error("Não foi possível alterar o acompanhamento", errorMessage(e));
    }
  };

  // Marcações manuais: a lista muda na hora, e a resposta do backend confirma. Enquanto a gravação de um capítulo
  // não volta, os botões dele ficam desligados, para cliques repetidos não se cruzarem.
  const [busy, setBusy] = useState<ReadonlySet<number>>(new Set());
  const busyRef = useRef(new Set<number>());
  const apply = useCallback((infos: ChapterReadInfo[]) => setView((current) => current && { ...current, chapters: applyReadInfo(current.chapters, infos) }), []);
  const changeStates = useCallback(
    async (ids: number[], predicted: (chapter: ChapterView) => ChapterReadInfo, save: () => Promise<ChapterReadInfo[]>) => {
      if (ids.length === 0 || ids.some((chapter) => busyRef.current.has(chapter))) return;
      for (const chapter of ids) busyRef.current.add(chapter);
      setBusy(new Set(busyRef.current));
      setView((current) => {
        if (!current) return current;
        const wanted = new Set(ids);
        return { ...current, chapters: applyReadInfo(current.chapters, current.chapters.filter((chapter) => wanted.has(chapter.id)).map(predicted)) };
      });
      try {
        apply(await save());
      } catch (e) {
        toast.error("Não foi possível alterar o estado de leitura", errorMessage(e));
        readStored();
      } finally {
        for (const chapter of ids) busyRef.current.delete(chapter);
        setBusy(new Set(busyRef.current));
      }
    },
    [apply, toast, readStored],
  );
  const markedRead = (chapter: ChapterView): ChapterReadInfo => ({ id: chapter.id, seriesId: id, readState: "read", isNew: false, completedAt: chapter.completedAt ?? Date.now() });
  const markedUnread = (chapter: ChapterView): ChapterReadInfo => ({ id: chapter.id, seriesId: id, readState: "unread", isNew: chapter.isNew, completedAt: null });
  const chaptersRef = useRef<ChapterView[]>([]);
  useEffect(() => {
    chaptersRef.current = view?.chapters ?? [];
  });
  const rowActions = useMemo<RowActions>(
    () => ({
      onSelect: (chapter, on) =>
        setSelected((current) => {
          const next = new Set(current);
          if (on) next.add(chapter);
          else next.delete(chapter);
          return next;
        }),
      onDownload: (chapter) => void enqueueRef.current([chapter]),
      onCancel: (downloadId) => void cancelRef.current(downloadId),
      onRead: (chapter) => nav.navigate({ name: "chapter", id: chapter }),
      onSetRead: (chapter, read) => void changeStates([chapter], read ? markedRead : markedUnread, () => api.chaptersSetRead([chapter], read)),
      onReadPrevious: (chapter) => {
        const before = previousChapters(chaptersRef.current, chapter).map((item) => item.id);
        void changeStates(before, markedRead, () => api.chaptersMarkPreviousRead(chapter));
      },
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [changeStates, nav],
  );
  const toggleFavorite = async () => {
    if (!view) return;
    setView({ ...view, favorite: !view.favorite });
    await actions.toggleSeriesFavorite(view);
  };
  const toggleSort = async () => {
    if (!view) return;
    const sort = view.chapterSort === "asc" ? "desc" : "asc";
    setView({ ...view, chapterSort: sort });
    await api.seriesSetChapterSort(id, sort).catch(() => undefined);
  };

  if (fatal) {
    return (
      <div className="h-full overflow-y-auto">
        <PageHeader title={view?.title ?? "Obra"} onBack={nav.back} />
        <div className="p-6">
          <ErrorState title="Não foi possível abrir esta obra" message={fatal} />
        </div>
      </div>
    );
  }
  if (!view) {
    return (
      <div className="p-6">
        <Spinner label="Carregando…" />
      </div>
    );
  }

  const firstOrder = chapters.reduce((low, chapter) => Math.min(low, chapter.order), Infinity);
  const downloadable = chapters.filter((chapter) => chapter.state !== "offline" && !isActive(chapter));
  const SortIcon = view.chapterSort === "asc" ? ArrowUpNarrowWide : ArrowDownNarrowWide;
  const { resume } = view;
  const retry = () => {
    setRefreshing(true);
    setAttempt((n) => n + 1);
  };
  const searchElsewhere = () => {
    searchAcrossSources(view.title);
    nav.navigate({ name: "explore" });
  };
  return (
    <div ref={scrollRef} className="h-full overflow-y-auto">
      <PageHeader
        title={view.title}
        onBack={nav.back}
        actions={
          <>
            <StateToggle icon={Star} motion="pop" active={view.favorite} activeText="Favorito" inactiveText="Favoritar" onToggle={() => void toggleFavorite()} testId="series-favorite" />
            <Button icon={FolderPlus} onClick={() => actions.seriesCollections(view.id)}>
              Coleções
            </Button>
            <StateToggle icon={Bell} motion="ring" active={view.following} activeText="Acompanhando" inactiveText="Acompanhar" onToggle={() => void toggleFollowing()} testId="series-follow" />
            {resume && (
              <Button variant="primary" icon={BookOpen} onClick={() => nav.navigate({ name: "chapter", id: resume.chapterId })}>
                {view.readStatus === "reading" ? "Continuar" : view.readStatus === "completed" ? "Ler novamente" : "Começar a ler"}
              </Button>
            )}
            <Menu
              label={`Ações para ${view.title}`}
              items={[
                {
                  label: "Procurar em outras fontes",
                  icon: Search,
                  onSelect: searchElsewhere,
                },
                { kind: "separator" },
                ...actions.seriesMenu(view, { inDetails: true }),
              ]}
              trigger={(p) => <IconButton {...p} icon={MoreHorizontal} label={`Mais ações para ${view.title}`} />}
            />
          </>
        }
      />
      <div className="mx-auto max-w-5xl px-6 py-6">
        {view.sourceError && (
          <div role="alert" className="mb-5 flex items-start gap-3 rounded-md border border-border bg-surface px-4 py-3">
            <CloudOff size={18} className="mt-0.5 shrink-0 text-muted" aria-hidden />
            <div className="min-w-0 flex-1">
              <p className="font-semibold">A fonte não respondeu</p>
              <SourceTrouble message={view.sourceError} site={view.sourceProtected ? { seriesId: view.id } : undefined} sourceId={view.sourceId} onRetry={retry} retrying={refreshing} elsewhere={searchElsewhere}>
                {(text, actions) => (
                  <>
                    <p className="mt-0.5 mb-3 text-sm text-muted">{text} O que está aqui é o último conhecido, e os capítulos da biblioteca abrem normalmente.</p>
                    {actions}
                  </>
                )}
              </SourceTrouble>
            </div>
          </div>
        )}
        <div className="flex gap-8 max-md:flex-col">
          <div className="w-56 shrink-0 max-md:mx-auto max-md:w-44">
            <OnlineCover series={view} eager />
          </div>
          <div className="min-w-0 flex-1">
            {view.summary && <p className="mb-3 whitespace-pre-line">{view.summary}</p>}
            <dl>
              {view.author && <Fact label="Autoria">{view.author}</Fact>}
              {view.status && <Fact label="Situação">{SERIES_STATUS_LABELS[view.status] ?? view.status}</Fact>}
              {view.genres.length > 0 && <Fact label="Gêneros">{view.genres.join(", ")}</Fact>}
              <Fact label="Fonte">{view.sourceName ?? "Fonte não instalada"}</Fact>
              {view.language && <Fact label="Idioma">{languageName(view.language)}</Fact>}
              {view.lastCheckedAt && <Fact label="Última consulta">{formatDate(view.lastCheckedAt)}</Fact>}
              <MalSeriesFact seriesId={view.id} title={view.title} />
            </dl>

            <div className="mt-6 mb-1 flex flex-wrap items-center gap-2">
              <h2 className="flex-1 text-base font-semibold">
                Capítulos
                {refreshing && <Spinner size={14} className="ml-2 align-middle" />}
              </h2>
              <Button size="sm" variant="ghost" icon={SortIcon} onClick={() => void toggleSort()}>
                {view.chapterSort === "asc" ? "Do primeiro ao último" : "Do último ao primeiro"}
              </Button>
              {downloadable.length > 0 && (
                <span className="relative inline-flex">
                  <IconButton
                    icon={Download}
                    label={selected.size > 0 ? `Baixar ${selected.size} ${selected.size === 1 ? "capítulo selecionado" : "capítulos selecionados"}` : "Baixar todos os capítulos"}
                    tooltipDelay={TOOLTIP_DELAY_MS}
                    variant="secondary"
                    size="sm"
                    onClick={() => void enqueue(selected.size > 0 ? [...selected] : downloadable.map((chapter) => chapter.id))}
                  />
                  {selected.size > 0 && (
                    <span className="pointer-events-none absolute -top-1.5 -right-1.5 min-w-4 rounded-full bg-accent-emphasis px-1 text-center text-[10px] leading-4 font-semibold text-on-emphasis" aria-hidden>
                      {selected.size}
                    </span>
                  )}
                </span>
              )}
            </div>
            {chapters.length === 0 && !refreshing && <p className="py-3 text-sm text-muted">Nenhum capítulo disponível.</p>}
            <ul>
              {chapters.map((chapter) => (
                <ChapterRow
                  key={chapter.id}
                  chapter={chapter}
                  selected={selected.has(chapter.id)}
                  busy={busy.has(chapter.id)}
                  hasPrevious={chapter.order > firstOrder}
                  actions={rowActions}
                />
              ))}
            </ul>
          </div>
        </div>
      </div>
    </div>
  );
}
