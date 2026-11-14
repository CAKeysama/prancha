import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { ImportCounts, ImportEvent, ImportItem } from "../../domain/types";
import { api, errorMessage } from "../../services/api";
import { cancellable, onImportProgress } from "../../services/events";
import { useToast } from "../../components/Toast";
import { sound } from "../../services/sound";

export interface ImportJob {
  id: number;
  phase: ImportEvent["phase"];
  counts: ImportCounts;
  current: string | null;
  items: ImportItem[];
  startedAt: number;
}

interface ImportApi {
  jobs: ImportJob[];
  /** Abre o seletor nativo. Devolve o id da importação, ou `null` se o usuário cancelou. */
  pickAndImport: (mode: "files" | "folder") => Promise<number | null>;
  importPaths: (paths: string[]) => Promise<number | null>;
  cancel: (jobId: number) => void;
  dismiss: (jobId: number) => void;
  /** Resolve quando a importação termina, com os resultados por item. */
  waitFor: (jobId: number) => Promise<ImportJob>;
  dragging: boolean;
}

const Ctx = createContext<ImportApi | null>(null);

export function useImport(): ImportApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("useImport fora do ImportProvider");
  return v;
}

const EMPTY_COUNTS: ImportCounts = { total: 0, processed: 0, added: 0, skipped: 0, failed: 0 };

const newJob = (id: number): ImportJob => ({ id, phase: "scanning", counts: EMPTY_COUNTS, current: null, items: [], startedAt: Date.now() });

export function ImportProvider({ children }: { children: ReactNode }) {
  const [jobs, setJobs] = useState<ImportJob[]>([]);
  const [dragging, setDragging] = useState(false);
  const waiters = useRef(new Map<number, Array<(j: ImportJob) => void>>());
  const finished = useRef(new Map<number, ImportJob>());
  const toast = useToast();

  useEffect(
    () =>
      onImportProgress((e) => {
        setJobs((list) => {
          const existing = list.find((j) => j.id === e.jobId);
          const base = existing ?? newJob(e.jobId);
          const job: ImportJob = {
            ...base,
            phase: e.phase,
            counts: e.counts,
            current: e.phase === "processing" ? (e.current ?? base.current) : null,
            items: e.item ? [...base.items, e.item] : base.items,
          };
          if (e.phase === "done" || e.phase === "cancelled") {
            finished.current.set(job.id, job);
            const ws = waiters.current.get(job.id) ?? [];
            waiters.current.delete(job.id);
            queueMicrotask(() => ws.forEach((w) => w(job)));
          }
          return existing ? list.map((j) => (j.id === job.id ? job : j)) : [...list, job];
        });
        if (e.phase === "done" && e.counts.added > 0) sound.play("add");
      }),
    [],
  );

  const importPaths = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return null;
      try {
        const id = await api.startImport(paths);
        setJobs((list) => (list.some((j) => j.id === id) ? list : [...list, newJob(id)]));
        return id;
      } catch (e) {
        toast.error("Não foi possível iniciar a importação", errorMessage(e));
        return null;
      }
    },
    [toast],
  );

  const pickAndImport = useCallback(
    async (mode: "files" | "folder") => {
      try {
        const paths = await api.pickImportPaths(mode);
        if (!paths || paths.length === 0) return null;
        return await importPaths(paths);
      } catch (e) {
        toast.error("Não foi possível abrir o seletor de arquivos", errorMessage(e));
        return null;
      }
    },
    [importPaths, toast],
  );

  const waitFor = useCallback((jobId: number) => {
    const done = finished.current.get(jobId);
    if (done) return Promise.resolve(done);
    return new Promise<ImportJob>((resolve) => {
      const list = waiters.current.get(jobId) ?? [];
      list.push(resolve);
      waiters.current.set(jobId, list);
    });
  }, []);

  const cancel = useCallback((jobId: number) => void api.cancelImport(jobId), []);
  const dismiss = useCallback((jobId: number) => setJobs((list) => list.filter((j) => j.id !== jobId)), []);

  // Arrastar e soltar arquivos/pastas sobre a janela.
  useEffect(
    () =>
      cancellable(() =>
        getCurrentWebview().onDragDropEvent((event) => {
          const p = event.payload;
          if (p.type === "enter" || p.type === "over") setDragging(true);
          else if (p.type === "leave") setDragging(false);
          else if (p.type === "drop") {
            setDragging(false);
            void importPaths(p.paths);
          }
        }),
      ),
    [importPaths],
  );

  const value = useMemo(
    () => ({ jobs, pickAndImport, importPaths, cancel, dismiss, waitFor, dragging }),
    [jobs, pickAndImport, importPaths, cancel, dismiss, waitFor, dragging],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
