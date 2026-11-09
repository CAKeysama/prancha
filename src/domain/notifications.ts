// Tipos e regras puras da central de notificações: badge, períodos, datas e textos das ações.

export type NotificationCategory = "chapters" | "updates" | "system" | "downloads" | "library";

export type NotificationAction =
  | "open_series"
  | "open_chapter"
  | "open_comic"
  | "open_downloads"
  | "open_updates"
  | "open_library"
  | "retry_sync";

export interface AppNotification {
  id: number;
  category: NotificationCategory;
  kind: string;
  title: string;
  body: string;
  seriesId: number | null;
  seriesHasCover: boolean;
  chapterId: number | null;
  chapterIds: number[];
  comicId: number | null;
  comicCoverRev: number | null;
  /** A primeira é a principal, executada ao clicar na notificação. */
  actions: NotificationAction[];
  count: number;
  createdAt: number;
  updatedAt: number;
  readAt: number | null;
}

export interface NotificationCursor {
  createdAt: number;
  id: number;
}

export interface NotificationPage {
  items: AppNotification[];
  nextCursor: NotificationCursor | null;
  unread: number;
}

export type NotificationStateFilter = "all" | "unread" | "read";

export interface NotificationQuery {
  state?: Exclude<NotificationStateFilter, "all">;
  category?: NotificationCategory;
  before?: NotificationCursor;
  limit?: number;
}

/** O destino já conferido pelo backend. */
export type NotificationTarget =
  | { kind: "series"; id: number }
  | { kind: "chapter"; id: number }
  | { kind: "comic"; id: number }
  | { kind: "downloads" }
  | { kind: "updates" }
  | { kind: "library" }
  | { kind: "retrySync" };

/** Texto da badge: nada com zero, o número até 99 e "99+" depois. */
export function badgeLabel(unread: number): string | null {
  if (!Number.isFinite(unread) || unread <= 0) return null;
  return unread > 99 ? "99+" : String(Math.floor(unread));
}

export function unreadLabel(unread: number): string {
  if (unread <= 0) return "Nenhuma não lida";
  return unread === 1 ? "1 não lida" : `${unread} não lidas`;
}

export type Period = "today" | "yesterday" | "week" | "older";

export const PERIOD_LABELS: Record<Period, string> = {
  today: "Hoje",
  yesterday: "Ontem",
  week: "Esta semana",
  older: "Anteriores",
};

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** Período pelo dia do calendário local. "Esta semana" são os sete dias antes de ontem. */
export function periodOf(ms: number, now: number): Period {
  const today = startOfDay(now);
  if (ms >= today) return "today";
  const yesterday = startOfDay(today - 1);
  if (ms >= yesterday) return "yesterday";
  const week = startOfDay(today - 6 * 24 * 60 * 60 * 1000);
  if (ms >= week) return "week";
  return "older";
}

/** Agrupa mantendo a ordem recebida (da mais recente para a mais antiga). */
export function groupByPeriod<T extends { createdAt: number }>(items: readonly T[], now: number): Array<{ period: Period; items: T[] }> {
  const groups: Array<{ period: Period; items: T[] }> = [];
  for (const item of items) {
    const period = periodOf(item.createdAt, now);
    const last = groups[groups.length - 1];
    if (last && last.period === period) last.items.push(item);
    else groups.push({ period, items: [item] });
  }
  return groups;
}

const time = new Intl.DateTimeFormat("pt-BR", { hour: "2-digit", minute: "2-digit" });
const day = new Intl.DateTimeFormat("pt-BR", { day: "numeric", month: "short" });
const dayYear = new Intl.DateTimeFormat("pt-BR", { day: "numeric", month: "short", year: "numeric" });
const weekday = new Intl.DateTimeFormat("pt-BR", { weekday: "long" });

/** Data curta: o horário hoje, "Ontem" e o dia da semana na semana, a data depois. */
export function formatWhen(ms: number, now: number): string {
  const period = periodOf(ms, now);
  if (period === "today") return time.format(ms);
  if (period === "yesterday") return `Ontem, ${time.format(ms)}`;
  if (period === "week") return weekday.format(ms);
  return new Date(ms).getFullYear() === new Date(now).getFullYear() ? day.format(ms) : dayYear.format(ms);
}

export const ACTION_LABELS: Record<NotificationAction, string> = {
  open_series: "Abrir obra",
  open_chapter: "Abrir capítulo",
  open_comic: "Abrir HQ",
  open_downloads: "Abrir downloads",
  open_updates: "Ver atualização",
  open_library: "Abrir biblioteca",
  retry_sync: "Verificar de novo",
};

export const CATEGORY_LABELS: Record<NotificationCategory, string> = {
  chapters: "Capítulos novos",
  updates: "Atualizações",
  downloads: "Downloads",
  library: "Biblioteca",
  system: "Sistema",
};

/** Categorias que dependem das fontes de leitura: sem o plugin, não há eventos delas. */
export const ONLINE_CATEGORIES: ReadonlySet<NotificationCategory> = new Set(["chapters", "downloads", "system"]);

export function visibleCategories(online: boolean): NotificationCategory[] {
  const all: NotificationCategory[] = ["chapters", "updates", "downloads", "library", "system"];
  return online ? all : all.filter((c) => !ONLINE_CATEGORIES.has(c));
}

/** Silêncio temporário das notificações do sistema: até quando, a partir de agora. */
export type MuteChoice = "1h" | "8h" | "tomorrow";

export function muteUntil(choice: MuteChoice, now: number): number {
  if (choice === "1h") return now + 60 * 60 * 1000;
  if (choice === "8h") return now + 8 * 60 * 60 * 1000;
  const d = new Date(now);
  d.setDate(d.getDate() + 1);
  d.setHours(8, 0, 0, 0);
  return d.getTime();
}

/** Aplica uma leitura localmente, sem buscar a lista de novo. */
export function withRead(items: readonly AppNotification[], ids: ReadonlySet<number>, read: boolean, now: number): AppNotification[] {
  return items.map((n) => (ids.has(n.id) ? { ...n, readAt: read ? (n.readAt ?? now) : null } : n));
}

/** Junta uma página nova com o que já estava carregado: ids repetidos são substituídos e a ordem é refeita. */
export function mergeNewest(current: readonly AppNotification[], fresh: readonly AppNotification[]): AppNotification[] {
  const byId = new Map<number, AppNotification>();
  for (const n of current) byId.set(n.id, n);
  for (const n of fresh) byId.set(n.id, n);
  return [...byId.values()].sort((a, b) => b.createdAt - a.createdAt || b.id - a.id);
}
