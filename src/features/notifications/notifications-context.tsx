import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useNav } from "../../app/navigation";
import { useSettings } from "../../app/settings-context";
import { useToast } from "../../components/Toast";
import { withRead, type AppNotification, type NotificationAction, type NotificationTarget } from "../../domain/notifications";
import { api, errorMessage } from "../../services/api";
import { onNotificationActivated, onNotificationsChanged } from "../../services/events";

/** Quantas aparecem no popup. O histórico completo fica na tela Notificações. */
export const POPUP_LIMIT = 8;

/** Alterações feitas pela interface: as listas abertas se ajustam sem consultar o banco de novo. */
export type LocalChange =
  | { type: "read"; ids: number[]; read: boolean; at: number }
  | { type: "remove"; ids: number[] }
  | { type: "allRead"; at: number }
  | { type: "clearRead" };

interface NotificationsApi {
  unread: number;
  /** Cresce quando chega uma notificação nova: listas abertas buscam só a primeira página. */
  createdVersion: number;
  recent: AppNotification[] | null;
  recentError: string | null;
  /** Carrega as recentes do popup só quando algo mudou desde a última consulta. */
  loadRecent: () => void;
  setRead: (ids: number[], read: boolean) => Promise<void>;
  markAllRead: () => Promise<void>;
  remove: (ids: number[]) => Promise<void>;
  clearRead: () => Promise<void>;
  /** Confere o destino e navega. Só marca como lida se a ação de fato acontecer. */
  run: (n: Pick<AppNotification, "id" | "readAt">, action?: NotificationAction) => Promise<boolean>;
  subscribe: (fn: (change: LocalChange) => void) => () => void;
}

const Ctx = createContext<NotificationsApi | null>(null);

export function useNotifications(): NotificationsApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("useNotifications fora do NotificationsProvider");
  return v;
}

function applyChange(items: AppNotification[], change: LocalChange): AppNotification[] {
  switch (change.type) {
    case "read":
      return withRead(items, new Set(change.ids), change.read, change.at);
    case "remove": {
      const gone = new Set(change.ids);
      return items.filter((n) => !gone.has(n.id));
    }
    case "allRead":
      return items.map((n) => (n.readAt ? n : { ...n, readAt: change.at }));
    case "clearRead":
      return items.filter((n) => !n.readAt);
  }
}

export { applyChange as applyNotificationChange };

export function NotificationsProvider({ children }: { children: ReactNode }) {
  const { loaded } = useSettings();
  const nav = useNav();
  const toast = useToast();
  const [unread, setUnread] = useState(0);
  const [createdVersion, setCreatedVersion] = useState(0);
  const [recent, setRecent] = useState<AppNotification[] | null>(null);
  const [recentError, setRecentError] = useState<string | null>(null);
  const fetchedVersion = useRef<number | null>(null);
  const listeners = useRef(new Set<(change: LocalChange) => void>());

  // A contagem vem do banco: sobrevive a reinícios e não é recalculada a cada renderização.
  useEffect(() => {
    if (!loaded) return;
    let alive = true;
    api
      .notificationsUnread()
      .then((n) => alive && setUnread(n))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [loaded]);

  useEffect(
    () =>
      onNotificationsChanged((e) => {
        if (typeof e.unread === "number") setUnread(e.unread);
        if (e.created !== undefined) setCreatedVersion((v) => v + 1);
      }),
    [],
  );

  const broadcast = useCallback((change: LocalChange) => {
    setRecent((items) => items && applyChange(items, change));
    for (const fn of listeners.current) fn(change);
  }, []);

  const subscribe = useCallback((fn: (change: LocalChange) => void) => {
    listeners.current.add(fn);
    return () => {
      listeners.current.delete(fn);
    };
  }, []);

  const loadRecent = useCallback(() => {
    if (fetchedVersion.current === createdVersion) return;
    fetchedVersion.current = createdVersion;
    api
      .notificationsList({ limit: POPUP_LIMIT })
      .then((page) => {
        setRecent(page.items);
        setUnread(page.unread);
        setRecentError(null);
      })
      .catch((e: unknown) => {
        fetchedVersion.current = null;
        setRecentError(errorMessage(e));
      });
  }, [createdVersion]);

  const setRead = useCallback(
    async (ids: number[], read: boolean) => {
      if (!ids.length) return;
      try {
        setUnread(await api.notificationsSetRead(ids, read));
        broadcast({ type: "read", ids, read, at: Date.now() });
      } catch (e) {
        toast.error(read ? "Não foi possível marcar como lida" : "Não foi possível marcar como não lida", errorMessage(e));
      }
    },
    [broadcast, toast],
  );

  const markAllRead = useCallback(async () => {
    try {
      setUnread(await api.notificationsMarkAllRead());
      broadcast({ type: "allRead", at: Date.now() });
    } catch (e) {
      toast.error("Não foi possível marcar todas como lidas", errorMessage(e));
    }
  }, [broadcast, toast]);

  const remove = useCallback(
    async (ids: number[]) => {
      if (!ids.length) return;
      try {
        setUnread(await api.notificationsRemove(ids));
        broadcast({ type: "remove", ids });
        // O popup completa a lista com as próximas na próxima abertura.
        fetchedVersion.current = null;
      } catch (e) {
        toast.error("Não foi possível remover", errorMessage(e));
      }
    },
    [broadcast, toast],
  );

  const clearRead = useCallback(async () => {
    try {
      setUnread(await api.notificationsClearRead());
      broadcast({ type: "clearRead" });
      fetchedVersion.current = null;
    } catch (e) {
      toast.error("Não foi possível limpar as notificações lidas", errorMessage(e));
    }
  }, [broadcast, toast]);

  const go = useCallback(
    (target: NotificationTarget) => {
      switch (target.kind) {
        case "series":
          nav.navigate({ name: "series", id: target.id });
          return;
        case "chapter":
          nav.navigate({ name: "chapter", id: target.id });
          return;
        case "comic":
          nav.navigate({ name: "comic", id: target.id });
          return;
        case "downloads":
          nav.navigate({ name: "downloads" });
          return;
        case "updates":
          nav.navigate({ name: "settings", section: "atualizacoes" });
          return;
        case "library":
          nav.navigate({ name: "library" });
          return;
        case "retrySync":
          nav.navigate({ name: "home" });
          return;
      }
    },
    [nav],
  );

  const run = useCallback(
    async (n: Pick<AppNotification, "id" | "readAt">, action?: NotificationAction) => {
      let target: NotificationTarget;
      try {
        target = await api.notificationTarget(n.id, action);
      } catch (e) {
        // O destino sumiu ou não está disponível: nada acontece, e a notificação continua como estava.
        toast.error("Não foi possível abrir", errorMessage(e));
        return false;
      }
      go(target);
      if (!n.readAt) await setRead([n.id], true);
      return true;
    },
    [go, setRead, toast],
  );

  // Clique numa notificação do sistema: o backend já trouxe a janela para a frente.
  const runRef = useRef(run);
  useEffect(() => {
    runRef.current = run;
  });
  useEffect(() => onNotificationActivated((e) => void runRef.current({ id: e.id, readAt: null })), []);

  const value = useMemo<NotificationsApi>(
    () => ({ unread, createdVersion, recent, recentError, loadRecent, setRead, markAllRead, remove, clearRead, run, subscribe }),
    [unread, createdVersion, recent, recentError, loadRecent, setRead, markAllRead, remove, clearRead, run, subscribe],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
