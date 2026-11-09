import { useCallback, useEffect, useRef, useState } from "react";
import { Bell, CheckCheck, Mail, Trash2, X } from "lucide-react";
import { useNav } from "../../app/navigation";
import { useSettings } from "../../app/settings-context";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/Dialog";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { Checkbox, Segmented } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { cn } from "../../components/cn";
import {
  CATEGORY_LABELS,
  PERIOD_LABELS,
  groupByPeriod,
  mergeNewest,
  unreadLabel,
  visibleCategories,
  type AppNotification,
  type NotificationCategory,
  type NotificationCursor,
  type NotificationStateFilter,
} from "../../domain/notifications";
import { api, errorMessage } from "../../services/api";
import { CATEGORY_ICONS, NotificationItem } from "./NotificationItem";
import { applyNotificationChange, useNotifications } from "./notifications-context";

export const PAGE_SIZE = 30;

const STATE_OPTIONS: Array<{ value: NotificationStateFilter; label: string }> = [
  { value: "all", label: "Todas" },
  { value: "unread", label: "Não lidas" },
  { value: "read", label: "Lidas" },
];

function emptyText(state: NotificationStateFilter, category: NotificationCategory | null): { title: string; text: string } {
  const where = category ? ` em ${CATEGORY_LABELS[category]}` : "";
  if (state === "unread") return { title: `Nenhuma notificação não lida${where}`, text: "Você está em dia." };
  if (state === "read") return { title: `Nenhuma notificação lida${where}`, text: "As notificações que você abrir ou marcar como lidas aparecem aqui." };
  return { title: `Nenhuma notificação${where}`, text: "Quando houver algo dessa categoria, aparece aqui." };
}

export function NotificationsScreen() {
  const { settings } = useSettings();
  const nav = useNav();
  const { unread, createdVersion, setRead, markAllRead, remove, clearRead, run, subscribe } = useNotifications();
  const [state, setState] = useState<NotificationStateFilter>("all");
  const [category, setCategory] = useState<NotificationCategory | null>(null);
  const [items, setItems] = useState<AppNotification[] | null>(null);
  const [cursor, setCursor] = useState<NotificationCursor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [confirmClear, setConfirmClear] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const filterKey = `${state}|${category ?? ""}`;
  const loadedKey = useRef<string | null>(null);
  const categories = visibleCategories(settings["plugins.online"]);

  const query = useCallback(
    (before?: NotificationCursor) =>
      api.notificationsList({ state: state === "all" ? undefined : state, category: category ?? undefined, before, limit: PAGE_SIZE }),
    [state, category],
  );

  // Primeira página a cada filtro. Notificações novas entram no topo sem perder o que já foi carregado.
  useEffect(() => {
    let alive = true;
    const fresh = loadedKey.current !== filterKey;
    if (fresh) {
      setItems(null);
      setSelected(new Set());
    }
    query()
      .then((page) => {
        if (!alive) return;
        setNow(Date.now());
        setError(null);
        if (fresh) {
          setItems(page.items);
          setCursor(page.nextCursor);
        } else {
          setItems((current) => mergeNewest(current ?? [], page.items));
        }
        loadedKey.current = filterKey;
      })
      .catch((e: unknown) => alive && setError(errorMessage(e)));
    return () => {
      alive = false;
    };
  }, [query, filterKey, createdVersion]);

  // Mudanças feitas pelo popup também valem aqui.
  useEffect(
    () =>
      subscribe((change) => {
        setItems((current) => current && applyNotificationChange(current, change));
        if (change.type === "remove") setSelected((s) => new Set([...s].filter((id) => !change.ids.includes(id))));
      }),
    [subscribe],
  );

  const more = async () => {
    if (!cursor) return;
    setLoadingMore(true);
    try {
      const page = await query(cursor);
      setItems((current) => [...(current ?? []), ...page.items.filter((n) => !(current ?? []).some((c) => c.id === n.id))]);
      setCursor(page.nextCursor);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoadingMore(false);
    }
  };

  const ids = [...selected];
  const toggle = (id: number, on: boolean) =>
    setSelected((s) => {
      const next = new Set(s);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });
  const allSelected = Boolean(items?.length) && items!.every((n) => selected.has(n.id));
  const hasRead = items?.some((n) => n.readAt) ?? false;
  const unfiltered = state === "all" && category === null;

  const header = (
    <PageHeader
      title="Notificações"
      subtitle={unreadLabel(unread)}
      actions={
        <>
          <Button icon={CheckCheck} disabled={unread === 0} onClick={() => void markAllRead()}>
            Marcar todas como lidas
          </Button>
          {hasRead && (
            <Button icon={Trash2} onClick={() => setConfirmClear(true)}>
              Limpar lidas
            </Button>
          )}
        </>
      }
    >
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <Segmented label="Estado" hideLabel size="sm" value={state} options={STATE_OPTIONS} onChange={setState} />
        <div role="group" aria-label="Categoria" className="flex flex-wrap gap-1.5">
          <CategoryChip label="Todas as categorias" active={category === null} onClick={() => setCategory(null)} />
          {categories.map((c) => (
            <CategoryChip key={c} label={CATEGORY_LABELS[c]} icon={CATEGORY_ICONS[c]} active={category === c} onClick={() => setCategory(c)} />
          ))}
        </div>
      </div>
    </PageHeader>
  );

  let body;
  if (error && !items) {
    body = (
      <div className="p-6">
        <ErrorState title="Não foi possível carregar as notificações" message={error} />
      </div>
    );
  } else if (!items) {
    body = (
      <div className="flex justify-center py-16">
        <Spinner label="Carregando notificações…" />
      </div>
    );
  } else if (items.length === 0) {
    const empty = emptyText(state, category);
    body = unfiltered ? (
      <EmptyState icon={Bell} title="Nenhuma notificação">
        Avisos sobre capítulos novos das obras que você acompanha, downloads e atualizações do Prancha aparecem aqui.
      </EmptyState>
    ) : (
      <EmptyState
        icon={Bell}
        title={empty.title}
        actions={
          <Button
            onClick={() => {
              setState("all");
              setCategory(null);
            }}
          >
            Ver todas
          </Button>
        }
      >
        {empty.text}
      </EmptyState>
    );
  } else {
    body = (
      <div className="mx-auto max-w-3xl px-6 py-4">
        <div className="mb-2 flex min-h-8 flex-wrap items-center gap-2 px-3">
          <Checkbox
            label={selected.size ? `${selected.size} selecionada${selected.size > 1 ? "s" : ""}` : "Selecionar todas"}
            checked={allSelected}
            onChange={(on) => setSelected(on ? new Set(items.map((n) => n.id)) : new Set())}
          />
          {selected.size > 0 && (
            <div role="toolbar" aria-label="Ações nas selecionadas" className="ml-auto flex flex-wrap gap-1.5">
              <Button size="sm" icon={CheckCheck} onClick={() => void setRead(ids, true)}>
                Marcar como lidas
              </Button>
              <Button size="sm" icon={Mail} onClick={() => void setRead(ids, false)}>
                Marcar como não lidas
              </Button>
              <Button size="sm" variant="danger" icon={Trash2} onClick={() => void remove(ids).then(() => setSelected(new Set()))}>
                Remover
              </Button>
              <Button size="sm" variant="ghost" icon={X} onClick={() => setSelected(new Set())}>
                Limpar seleção
              </Button>
            </div>
          )}
        </div>
        {groupByPeriod(items, now).map((group) => (
          <section key={group.period} aria-labelledby={`notifications-${group.period}`} className="mb-4">
            <h2 id={`notifications-${group.period}`} className="px-3 pb-1 text-xs font-semibold tracking-wide text-muted uppercase">
              {PERIOD_LABELS[group.period]}
            </h2>
            <ul className="flex flex-col">
              {group.items.map((n) => (
                <NotificationItem
                  key={n.id}
                  n={n}
                  now={now}
                  variant="page"
                  selected={selected.has(n.id)}
                  onSelect={(on) => toggle(n.id, on)}
                  onRun={(item, action) => void run(item, action)}
                  onToggleRead={(item) => void setRead([item.id], !item.readAt)}
                  onRemove={(item) => void remove([item.id])}
                />
              ))}
            </ul>
          </section>
        ))}
        {error && <p role="alert" className="px-3 text-sm text-danger">{error}</p>}
        {cursor && (
          <div className="flex justify-center py-3">
            <Button loading={loadingMore} onClick={() => void more()}>
              Carregar mais
            </Button>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      {header}
      {body}
      <ConfirmDialog
        open={confirmClear}
        title="Limpar notificações lidas?"
        confirmLabel="Limpar lidas"
        destructive
        onCancel={() => setConfirmClear(false)}
        onConfirm={() => {
          setConfirmClear(false);
          void clearRead();
        }}
      >
        As notificações já lidas saem da central. As não lidas continuam, e nada muda nas obras, nos capítulos ou no progresso.
      </ConfirmDialog>
      {/* Atalho para as preferências, como nas outras telas com opções próprias. */}
      <p className="pb-6 text-center text-sm text-muted">
        <button type="button" className="font-medium text-accent-fg hover:underline" onClick={() => nav.navigate({ name: "settings", section: "notificacoes" })}>
          Escolher o que aparece aqui
        </button>
      </p>
    </div>
  );
}

function CategoryChip({ label, icon: Icon, active, onClick }: { label: string; icon?: typeof Bell; active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-sm font-medium transition-colors",
        active ? "border-accent-emphasis bg-accent-subtle text-accent-fg" : "border-border bg-surface text-muted hover:bg-hover hover:text-fg",
      )}
    >
      {Icon && <Icon size={14} aria-hidden />}
      {label}
    </button>
  );
}
