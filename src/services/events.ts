import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ImportEvent } from "../domain/types";

const EVENTS = {
  import: "import:progress",
  library: "library:changed",
  covers: "covers:progress",
} as const;

/** Liga um ouvinte do Tauri; devolve a função que o desliga, mesmo antes de ele ficar pronto (segura em StrictMode). */
export function cancellable(start: () => Promise<UnlistenFn>): () => void {
  let off: UnlistenFn | undefined;
  let cancelled = false;
  try {
    start()
      .then((fn) => {
        if (cancelled) fn();
        else off = fn;
      })
      .catch(() => undefined);
  } catch {
    /* fora do Tauri (testes): sem eventos */
  }
  return () => {
    cancelled = true;
    off?.();
  };
}

const subscribe = <T,>(event: string, handler: (payload: T) => void) => cancellable(() => listen<T>(event, (e) => handler(e.payload)));

export const onImportProgress = (h: (e: ImportEvent) => void) => subscribe<ImportEvent>(EVENTS.import, h);
export const onLibraryChanged = (h: () => void) => subscribe<unknown>(EVENTS.library, () => h());
export const onCoversProgress = (h: (e: { total: number; done: number }) => void) =>
  subscribe<{ total: number; done: number }>(EVENTS.covers, h);
