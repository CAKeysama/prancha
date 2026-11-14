import { useState } from "react";
import { BookPlus, Check, CircleArrowUp, Download, Library, Mail, TriangleAlert, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { IconButton } from "../../components/Button";
import { Checkbox } from "../../components/Form";
import { cn } from "../../components/cn";
import { ACTION_LABELS, CATEGORY_LABELS, formatWhen, type AppNotification, type NotificationAction, type NotificationCategory } from "../../domain/notifications";
import { coverUrl, onlineCoverUrl } from "../../services/urls";

export const CATEGORY_ICONS: Record<NotificationCategory, LucideIcon> = {
  chapters: BookPlus,
  updates: CircleArrowUp,
  downloads: Download,
  library: Library,
  system: TriangleAlert,
};

/** A capa em miniatura identifica a obra; sem ela, o ícone da categoria. */
function Thumb({ n }: { n: AppNotification }) {
  const [failed, setFailed] = useState(false);
  const src = n.seriesId !== null && n.seriesHasCover ? onlineCoverUrl(n.seriesId) : n.comicId !== null && n.comicCoverRev !== null ? coverUrl(n.comicId, n.comicCoverRev) : null;
  const Icon = CATEGORY_ICONS[n.category] ?? TriangleAlert;
  if (src && !failed) {
    return (
      <img
        src={src}
        alt=""
        loading="lazy"
        decoding="async"
        width={32}
        height={44}
        onError={() => setFailed(true)}
        className={cn("h-11 w-8 shrink-0 rounded-sm border border-border bg-surface object-cover", n.readAt !== null && "opacity-80")}
      />
    );
  }
  return (
    <span
      aria-hidden
      className={cn(
        "flex size-8 shrink-0 items-center justify-center rounded-full border border-border bg-surface",
        n.category === "system" || n.kind.endsWith("failed") ? "text-attention" : "text-muted",
      )}
    >
      <Icon size={16} />
    </span>
  );
}

export function NotificationItem({
  n,
  now,
  variant,
  selected,
  onSelect,
  onRun,
  onToggleRead,
  onRemove,
}: {
  n: AppNotification;
  now: number;
  variant: "popup" | "page";
  selected?: boolean;
  onSelect?: (selected: boolean) => void;
  onRun: (n: AppNotification, action?: NotificationAction) => void;
  onToggleRead: (n: AppNotification) => void;
  onRemove: (n: AppNotification) => void;
}) {
  const unread = !n.readAt;
  const primary = n.actions[0];
  const secondary = variant === "page" ? n.actions.slice(1) : [];
  return (
    <li
      className={cn(
        "p-enter group relative flex items-start gap-3 rounded-md px-3 py-2.5 transition-colors hover:bg-hover",
        selected && "bg-accent-subtle hover:bg-accent-subtle",
      )}
    >
      {onSelect && (
        <span className="pt-3">
          <Checkbox label={`Selecionar “${n.title}”`} hideLabel checked={Boolean(selected)} onChange={onSelect} />
        </span>
      )}
      <Thumb n={n} />
      <div className="min-w-0 flex-1">
        <button
          type="button"
          onClick={() => onRun(n)}
          disabled={!primary}
          className="block w-full rounded-sm text-left outline-offset-2 disabled:cursor-default"
        >
          <span className="flex items-start gap-2">
            <span className={cn("min-w-0 flex-1 break-words", unread ? "font-semibold text-fg" : "font-medium text-muted")}>{n.title}</span>
            {/* Não lida: ponto, peso do título e texto para leitores de tela, não só a cor. */}
            {unread && <span aria-hidden className="mt-1.5 size-2 shrink-0 rounded-full bg-accent" />}
          </span>
          {unread && <span className="sr-only">Não lida. </span>}
          {n.body && <span className={cn("mt-0.5 block text-sm break-words", unread ? "text-fg/80" : "text-muted", variant === "popup" && "line-clamp-2")}>{n.body}</span>}
          <span className="mt-1 block text-xs text-muted">
            {CATEGORY_LABELS[n.category]} · <time dateTime={new Date(n.createdAt).toISOString()}>{formatWhen(n.createdAt, now)}</time>
            {primary && <span className="sr-only">. Ação: {ACTION_LABELS[primary]}</span>}
          </span>
        </button>
        {/* A ação principal é o clique na própria notificação; aqui ficam só as outras. */}
        {secondary.length > 0 ? (
          <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-1">
            {secondary.map((a) => (
              <button key={a} type="button" onClick={() => onRun(n, a)} className="text-sm font-medium text-accent-fg hover:underline">
                {ACTION_LABELS[a]}
              </button>
            ))}
          </div>
        ) : null}
      </div>
      <div className={cn("flex shrink-0 items-center gap-0.5 transition-opacity", variant === "popup" && "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 max-md:opacity-100")}>
        <IconButton
          size="sm"
          icon={unread ? Check : Mail}
          label={unread ? "Marcar como lida" : "Marcar como não lida"}
          onClick={() => onToggleRead(n)}
          tooltipSide="top"
        />
        <IconButton size="sm" icon={X} label="Remover notificação" onClick={() => onRemove(n)} tooltipSide="top" />
      </div>
    </li>
  );
}
