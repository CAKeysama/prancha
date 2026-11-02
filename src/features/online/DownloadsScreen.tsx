import { useCallback, useEffect, useRef, useState } from "react";
import { BookOpen, Download, Pause, Play, RefreshCw, X } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { EmptyState, ErrorState, ProgressBar, Spinner } from "../../components/Feedback";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { useNav } from "../../app/navigation";
import { DOWNLOAD_STATUS_LABELS, type DownloadView } from "../../domain/online";
import { formatBytes } from "../../domain/progress";
import { api, errorMessage } from "../../services/api";
import { onDownloadChanged } from "../../services/events";

const ACTIVE = new Set(["queued", "running", "paused"]);

export function DownloadsScreen() {
  const nav = useNav();
  const toast = useToast();
  const [items, setItems] = useState<DownloadView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [speeds, setSpeeds] = useState<Record<number, number>>({});
  const samples = useRef(new Map<number, { bytes: number; at: number }>());

  const load = useCallback(() => {
    api
      .downloads(200)
      .then((list) => {
        setItems(list);
        setError(null);
      })
      .catch((e: unknown) => setError(errorMessage(e)));
  }, []);

  useEffect(() => {
    load();
    return onDownloadChanged((event) => {
      // A velocidade é a diferença entre dois avisos: o backend só conta os bytes.
      const now = performance.now();
      const last = samples.current.get(event.id);
      if (event.status !== "running") samples.current.delete(event.id);
      else if (!last || event.bytes > last.bytes) {
        samples.current.set(event.id, { bytes: event.bytes, at: now });
        if (last) setSpeeds((all) => ({ ...all, [event.id]: ((event.bytes - last.bytes) * 1000) / Math.max(1, now - last.at) }));
      }
      setItems((list) => {
        if (!list) return list;
        return list.some((item) => item.id === event.id) ? list.map((item) => (item.id === event.id ? event : item)) : [event, ...list];
      });
    });
  }, [load]);

  const act = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (e) {
      toast.error("Não foi possível concluir a ação", errorMessage(e));
    }
    load();
  };

  const header = (
    <PageHeader
      title="Downloads"
      actions={
        items?.some((item) => !ACTIVE.has(item.status)) ? (
          <Button variant="ghost" onClick={() => void act(() => api.downloadsClearFinished())}>
            Limpar histórico
          </Button>
        ) : undefined
      }
    />
  );

  return (
    <div className="h-full overflow-y-auto">
      {header}
      <div className="mx-auto max-w-4xl px-6 py-4">
        {error && !items && <ErrorState title="Não foi possível abrir os downloads" message={error} />}
        {!error && !items && <Spinner label="Carregando…" />}
        {items?.length === 0 && (
          <EmptyState icon={Download} title="Nenhum download" actions={<Button onClick={() => nav.navigate({ name: "explore" })}>Explorar</Button>}>
            Os capítulos que você baixar entram na biblioteca e abrem sem internet.
          </EmptyState>
        )}
        <ul>
          {items?.map((item) => {
            const active = ACTIVE.has(item.status);
            const speed = speeds[item.id];
            const facts = [
              DOWNLOAD_STATUS_LABELS[item.status],
              item.pagesTotal > 0 ? `${item.pagesDone} de ${item.pagesTotal} páginas` : null,
              item.bytes > 0 ? formatBytes(item.bytes) : null,
              item.status === "running" && speed ? `${formatBytes(speed)}/s` : null,
            ].filter(Boolean);
            return (
              <li key={item.id} data-testid="download" className="flex items-center gap-3 border-b border-border py-3 last:border-b-0">
                <div className="min-w-0 flex-1">
                  <button type="button" onClick={() => nav.navigate({ name: "series", id: item.seriesId })} className="block max-w-full truncate text-left font-semibold hover:underline">
                    {item.seriesTitle}
                  </button>
                  <p className="truncate text-sm">
                    {item.chapterTitle}
                    {item.sourceName && <span className="text-muted"> · {item.sourceName}</span>}
                  </p>
                  <p className="truncate text-sm text-muted">{facts.join(" · ")}</p>
                  {active && <ProgressBar value={item.pagesTotal ? item.pagesDone / item.pagesTotal : 0} label={`Download de ${item.chapterTitle}`} className="mt-1.5 max-w-md" />}
                  {item.error && <p className="mt-1 text-sm text-danger">{item.error}</p>}
                </div>
                {item.status === "completed" && item.comicId !== null && (
                  <Button icon={BookOpen} onClick={() => nav.navigate({ name: "reader", id: item.comicId! })}>
                    Ler
                  </Button>
                )}
                {(item.status === "failed" || item.status === "cancelled") && (
                  <Button icon={RefreshCw} onClick={() => void act(() => api.downloadRetry(item.id))}>
                    Tentar de novo
                  </Button>
                )}
                {(item.status === "running" || item.status === "queued") && <IconButton icon={Pause} label={`Pausar ${item.chapterTitle}`} onClick={() => void act(() => api.downloadPause(item.id))} />}
                {item.status === "paused" && <IconButton icon={Play} label={`Retomar ${item.chapterTitle}`} onClick={() => void act(() => api.downloadResume(item.id))} />}
                {active && <IconButton icon={X} label={`Cancelar ${item.chapterTitle}`} variant="ghost" onClick={() => void act(() => api.downloadCancel(item.id))} />}
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
