import { useCallback, useEffect, useMemo, useState } from "react";
import { ArrowDownNarrowWide, ArrowUpNarrowWide, Bell, BellOff, BookOpen, CloudOff, Download, RefreshCw, X } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { ErrorState, ProgressBar, Spinner } from "../../components/Feedback";
import { Checkbox } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { useLibraryVersion } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import {
  CHAPTER_STATE_LABELS,
  DOWNLOAD_STATUS_LABELS,
  SERIES_STATUS_LABELS,
  languageName,
  sortedChapters,
  type ChapterView,
  type SeriesView,
} from "../../domain/online";
import { formatDate } from "../../domain/progress";
import { ApiError, api, errorMessage } from "../../services/api";
import { onDownloadChanged } from "../../services/events";
import { Fact, OnlineCover } from "./parts";

let requestCounter = 0;

const ACTIVE = new Set(["queued", "running", "paused"]);
const isActive = (chapter: ChapterView) => !!chapter.download && ACTIVE.has(chapter.download.status);

function publishedDate(ms: number): string {
  return new Date(ms).toLocaleDateString("pt-BR", { dateStyle: "medium" });
}

function ChapterRow({
  chapter,
  selected,
  onSelect,
  onDownload,
  onCancel,
  onRead,
}: {
  chapter: ChapterView;
  selected: boolean;
  onSelect: (selected: boolean) => void;
  onDownload: () => void;
  onCancel: (id: number) => void;
  onRead: (comicId: number) => void;
}) {
  const download = chapter.download;
  const active = isActive(chapter);
  const failed = download?.status === "failed";
  const details = [
    chapter.publishedAt ? publishedDate(chapter.publishedAt) : null,
    CHAPTER_STATE_LABELS[chapter.state],
    chapter.progress ? `Página ${chapter.progress.pageIndex + 1} de ${chapter.progress.pageCount}` : null,
  ].filter(Boolean);
  return (
    <li data-testid="chapter" className="flex items-center gap-3 border-b border-border py-2.5 last:border-b-0">
      {chapter.state !== "offline" && !active && <Checkbox label={`Selecionar ${chapter.title}`} hideLabel checked={selected} onChange={onSelect} />}
      <div className="min-w-0 flex-1">
        <p className="truncate font-medium">
          {chapter.number && <span className="mr-2 text-muted">{chapter.number}</span>}
          {chapter.title}
        </p>
        <p className="truncate text-sm text-muted">{details.join(" · ")}</p>
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
      {chapter.comicId !== null && (
        <Button icon={BookOpen} onClick={() => onRead(chapter.comicId!)}>
          Ler
        </Button>
      )}
      {active && download && <IconButton icon={X} label={`Cancelar download de ${chapter.title}`} variant="ghost" onClick={() => onCancel(download.id)} />}
      {chapter.state !== "offline" && !active && (
        <Button icon={failed ? RefreshCw : Download} onClick={onDownload}>
          {failed ? "Tentar de novo" : "Baixar"}
        </Button>
      )}
    </li>
  );
}

export function SeriesScreen({ id }: { id: number }) {
  const nav = useNav();
  const toast = useToast();
  const { version } = useLibraryVersion();
  const [view, setView] = useState<SeriesView | null>(null);
  const [fatal, setFatal] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(true);
  const [attempt, setAttempt] = useState(0);
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const readStored = useCallback(() => {
    api
      .seriesStored(id)
      .then((stored) => setView((current) => (current ? { ...stored, sourceError: current.sourceError } : stored)))
      .catch(() => undefined);
  }, [id]);

  // O que já se sabe aparece na hora, com ou sem internet. A fonte atualiza em seguida.
  useEffect(() => {
    let alive = true;
    const requestId = `series-${++requestCounter}`;
    api
      .seriesStored(id)
      .then((stored) => alive && setView((current) => current ?? stored))
      .catch((e: unknown) => alive && setFatal(errorMessage(e)));
    api
      .seriesOpen(requestId, id)
      .then((fresh) => alive && setView(fresh))
      .catch((e: unknown) => {
        if (alive && !(e instanceof ApiError && e.code === "extension_cancelled")) setFatal(errorMessage(e));
      })
      .finally(() => alive && setRefreshing(false));
    return () => {
      alive = false;
      void api.sourceCancel(requestId).catch(() => undefined);
    };
  }, [id, attempt]);

  useEffect(() => {
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
  const toggleFollowing = async () => {
    if (!view) return;
    try {
      const after = await api.seriesSetFollowing(id, !view.following);
      setView({ ...view, following: after.following });
    } catch (e) {
      toast.error("Não foi possível alterar o acompanhamento", errorMessage(e));
    }
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

  const downloadable = chapters.filter((chapter) => chapter.state !== "offline" && !isActive(chapter));
  const SortIcon = view.chapterSort === "asc" ? ArrowUpNarrowWide : ArrowDownNarrowWide;
  return (
    <div className="h-full overflow-y-auto">
      <PageHeader
        title={view.title}
        onBack={nav.back}
        actions={
          <Button icon={view.following ? BellOff : Bell} onClick={() => void toggleFollowing()} aria-pressed={view.following}>
            {view.following ? "Acompanhando" : "Acompanhar"}
          </Button>
        }
      />
      <div className="mx-auto max-w-5xl px-6 py-6">
        {view.sourceError && (
          <div role="alert" className="mb-5 flex flex-wrap items-start gap-3 rounded-md border border-border bg-surface px-4 py-3">
            <CloudOff size={18} className="mt-0.5 shrink-0 text-muted" aria-hidden />
            <div className="min-w-0 flex-1">
              <p className="font-semibold">A fonte não respondeu</p>
              <p className="mt-0.5 text-sm text-muted">{view.sourceError} O que está aqui é o último conhecido, e os capítulos da biblioteca abrem normalmente.</p>
            </div>
            <Button
              icon={RefreshCw}
              onClick={() => {
                setRefreshing(true);
                setAttempt((n) => n + 1);
              }}
              loading={refreshing}
            >
              Tentar de novo
            </Button>
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
                <Button size="sm" icon={Download} onClick={() => void enqueue(selected.size > 0 ? [...selected] : downloadable.map((chapter) => chapter.id))}>
                  {selected.size > 0 ? `Baixar ${selected.size}` : "Baixar todos"}
                </Button>
              )}
            </div>
            {chapters.length === 0 && !refreshing && <p className="py-3 text-sm text-muted">Nenhum capítulo disponível.</p>}
            <ul>
              {chapters.map((chapter) => (
                <ChapterRow
                  key={chapter.id}
                  chapter={chapter}
                  selected={selected.has(chapter.id)}
                  onSelect={(on) =>
                    setSelected((current) => {
                      const next = new Set(current);
                      if (on) next.add(chapter.id);
                      else next.delete(chapter.id);
                      return next;
                    })
                  }
                  onDownload={() => void enqueue([chapter.id])}
                  onCancel={(downloadId) => void cancel(downloadId)}
                  onRead={(comicId) => nav.navigate({ name: "reader", id: comicId })}
                />
              ))}
            </ul>
          </div>
        </div>
      </div>
    </div>
  );
}
