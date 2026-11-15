import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ComponentProgress, DownloadView } from "../domain/online";
import type { ImportEvent } from "../domain/types";

const EVENTS = {
  import: "import:progress",
  library: "library:changed",
  covers: "covers:progress",
  downloads: "downloads:changed",
  component: "component:progress",
  notifications: "notifications:changed",
  notificationActivate: "notifications:activate",
  mal: "mal:changed",
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
export const onDownloadChanged = (h: (e: DownloadView) => void) => subscribe<DownloadView>(EVENTS.downloads, h);
export const onComponentProgress = (h: (e: ComponentProgress) => void) => subscribe<ComponentProgress>(EVENTS.component, h);
export const onCoversProgress = (h: (e: { total: number; done: number }) => void) =>
  subscribe<{ total: number; done: number }>(EVENTS.covers, h);
/** `created` traz o id de uma notificação nova; sem ele, a mudança veio da própria interface. */
export const onNotificationsChanged = (h: (e: { unread: number; created?: number }) => void) =>
  subscribe<{ unread: number; created?: number }>(EVENTS.notifications, h);
/** Clique numa notificação do sistema operacional. */
export const onNotificationActivated = (h: (e: { id: number }) => void) => subscribe<{ id: number }>(EVENTS.notificationActivate, h);

export const onMalChanged = (h: () => void) => subscribe<unknown>(EVENTS.mal, () => h());
