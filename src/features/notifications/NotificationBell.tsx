import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Bell, CheckCheck, Settings } from "lucide-react";
import { useNav } from "../../app/navigation";
import { useSettings } from "../../app/settings-context";
import { Button, IconButton } from "../../components/Button";
import { Spinner } from "../../components/Feedback";
import { cn } from "../../components/cn";
import { badgeLabel, emptyNotificationsHint, unreadLabel, type AppNotification, type NotificationAction } from "../../domain/notifications";
import { NotificationItem } from "./NotificationItem";
import { useNotifications } from "./notifications-context";

const PANEL_WIDTH = 380;
const MARGIN = 8;

/** Posição do painel junto ao sino, sempre dentro da janela, qualquer que seja o tamanho dela. */
export function panelPosition(anchor: DOMRect, viewport: { width: number; height: number }) {
  const width = Math.min(PANEL_WIDTH, viewport.width - MARGIN * 2);
  // O sino fica no menu lateral: o painel abre ao lado dele, sem cobrir o próprio sino.
  let left = anchor.right + MARGIN;
  if (left + width > viewport.width - MARGIN) left = Math.max(MARGIN, anchor.right - width);
  left = Math.min(Math.max(MARGIN, left), viewport.width - width - MARGIN);
  const top = Math.max(MARGIN, Math.min(anchor.top, viewport.height - MARGIN - 240));
  const maxHeight = Math.max(160, Math.min(560, viewport.height - top - MARGIN));
  return { left, top, width, maxHeight };
}

export function NotificationBell({ current }: { narrow?: boolean; current: boolean }) {
  const { unread } = useNotifications();
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  const label = badgeLabel(unread);
  const close = useCallback((refocus: boolean) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  }, []);
  return (
    <>
      <button
        ref={button}
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? panelId : undefined}
        aria-current={current ? "page" : undefined}
        aria-label={unread > 0 ? `Notificações, ${unreadLabel(unread)}` : "Notificações"}
        className={cn(
          "relative inline-flex size-8 shrink-0 items-center justify-center rounded-md text-muted transition-colors hover:bg-hover hover:text-fg",
          (open || current) && "bg-hover text-fg",
        )}
      >
        <Bell size={18} aria-hidden />
        {label && (
          <span
            key={label}
            aria-hidden
            data-testid="notification-badge"
            className="p-badge absolute -top-1 -right-1 inline-flex h-4 min-w-4 items-center justify-center rounded-full border border-surface bg-accent-emphasis px-1 text-[10px] leading-none font-semibold text-on-emphasis tabular-nums"
          >
            {label}
          </span>
        )}
      </button>
      {open && <NotificationPopup id={panelId} anchor={button} onClose={close} />}
    </>
  );
}

function NotificationPopup({ id, anchor, onClose }: { id: string; anchor: RefObject<HTMLButtonElement | null>; onClose: (refocus: boolean) => void }) {
  const { unread, recent, recentError, loadRecent, markAllRead, setRead, remove, run } = useNotifications();
  const nav = useNav();
  const { settings } = useSettings();
  const panel = useRef<HTMLDivElement>(null);
  const measure = () => (anchor.current ? panelPosition(anchor.current.getBoundingClientRect(), { width: window.innerWidth, height: window.innerHeight }) : null);
  // Já nasce no lugar: sem o primeiro quadro escondido, o foco do efeito abaixo pega no painel.
  const [pos, setPos] = useState(measure);
  // O popup monta a cada abertura: a hora das notificações é a do momento em que ele abriu.
  const [now] = useState(() => Date.now());

  useLayoutEffect(() => {
    const place = () => anchor.current && setPos(panelPosition(anchor.current.getBoundingClientRect(), { width: window.innerWidth, height: window.innerHeight }));
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [anchor]);

  // Só consulta o banco se chegou algo novo desde a última abertura.
  useEffect(() => {
    loadRecent();
  }, [loadRecent]);

  useEffect(() => {
    panel.current?.focus();
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (panel.current?.contains(t) || anchor.current?.contains(t)) return;
      // Tooltips e diálogos abertos a partir do painel ficam fora dele no DOM.
      if (t instanceof Element && t.closest("[role='tooltip'], [role='dialog'], [role='alertdialog']")) return;
      onClose(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose(true);
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [anchor, onClose]);

  const open = async (n: AppNotification, action?: NotificationAction) => {
    if (await run(n, action)) onClose(false);
  };

  return createPortal(
    <div
      ref={panel}
      id={id}
      role="dialog"
      aria-label="Notificações"
      tabIndex={-1}
      className="p-enter fixed z-[60] flex flex-col overflow-hidden rounded-lg border border-border bg-elevated shadow-overlay outline-none"
      style={pos ? { left: pos.left, top: pos.top, width: pos.width, maxHeight: pos.maxHeight } : { visibility: "hidden" }}
    >
      <div className="flex items-center gap-2 border-b border-border px-3 py-2.5">
        <div className="min-w-0 flex-1">
          <h2 className="text-base font-semibold">Notificações</h2>
          <p className="text-xs text-muted" aria-live="polite">
            {unreadLabel(unread)}
          </p>
        </div>
        <IconButton size="sm" icon={CheckCheck} label="Marcar todas como lidas" disabled={unread === 0} onClick={() => void markAllRead()} />
        <IconButton
          size="sm"
          icon={Settings}
          label="Preferências de notificação"
          onClick={() => {
            onClose(false);
            nav.navigate({ name: "settings", section: "notificacoes" });
          }}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-1">
        {recentError ? (
          <div role="alert" className="flex flex-col items-center gap-2 px-6 py-6 text-center">
            <p className="text-sm font-medium text-danger">Não foi possível carregar as notificações</p>
            <p className="text-sm text-muted">{recentError}</p>
            <Button size="sm" onClick={loadRecent}>
              Tentar novamente
            </Button>
          </div>
        ) : recent === null ? (
          <div className="flex justify-center py-8">
            <Spinner label="Carregando…" />
          </div>
        ) : recent.length === 0 ? (
          <div className="flex flex-col items-center px-6 py-8 text-center">
            <span className="mb-3 flex size-10 items-center justify-center rounded-full border border-border bg-surface text-muted">
              <Bell size={18} aria-hidden />
            </span>
            <p className="font-medium">Nenhuma notificação</p>
            <p className="mt-0.5 text-sm text-muted">{emptyNotificationsHint(settings["plugins.online"])}</p>
          </div>
        ) : (
          <ul aria-label="Notificações recentes" className="flex flex-col">
            {recent.map((n) => (
              <NotificationItem
                key={n.id}
                n={n}
                now={now}
                variant="popup"
                onRun={(item, action) => void open(item, action)}
                onToggleRead={(item) => void setRead([item.id], !item.readAt)}
                onRemove={(item) => void remove([item.id])}
              />
            ))}
          </ul>
        )}
      </div>
      <div className="border-t border-border p-1.5">
        <Button
          variant="ghost"
          size="sm"
          className="w-full"
          onClick={() => {
            onClose(false);
            nav.navigate({ name: "notifications" });
          }}
        >
          Ver todas
        </Button>
      </div>
    </div>,
    document.body,
  );
}
