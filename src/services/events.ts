import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ImportEvent } from "../domain/types";

export const EVENTS = {
  import: "import:progress",
  library: "library:changed",
  covers: "covers:progress",
} as const;

/** Assina um evento do backend; devolve a função de cancelamento (segura em StrictMode). */
export function subscribe<T>(event: string, handler: (payload: T) => void): () => void {
  let unlisten: UnlistenFn | null = null;
  let cancelled = false;
  listen<T>(event, (e) => handler(e.payload))
    .then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    })
    .catch(() => {
      /* fora do Tauri (testes): sem eventos */
    });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

export const onImportProgress = (h: (e: ImportEvent) => void) => subscribe<ImportEvent>(EVENTS.import, h);
export const onLibraryChanged = (h: () => void) => subscribe<unknown>(EVENTS.library, () => h());
export const onCoversProgress = (h: (e: { total: number; done: number }) => void) =>
  subscribe<{ total: number; done: number }>(EVENTS.covers, h);
