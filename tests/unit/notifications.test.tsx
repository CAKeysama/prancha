// Central de notificações: regras puras e interface com o backend simulado por `mockIPC`.
// Deduplicação, agrupamento, persistência e retenção são cobertos pelos testes Rust (`notifications.rs`).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { App } from "../../src/app/App";
import { panelPosition } from "../../src/features/notifications/NotificationBell";
import {
  badgeLabel,
  formatWhen,
  groupByPeriod,
  mergeNewest,
  muteUntil,
  periodOf,
  visibleCategories,
  withRead,
  type AppNotification,
  type NotificationQuery,
} from "../../src/domain/notifications";
import { sanitizeSettings } from "../../src/domain/settings";

const HOUR = 60 * 60 * 1000;
const DAY = 24 * HOUR;

function note(id: number, patch: Partial<AppNotification> = {}): AppNotification {
  return {
    id,
    category: "chapters",
    kind: "chapters_new",
    title: `Notificação ${id}`,
    body: `Texto ${id}`,
    seriesId: 1,
    seriesHasCover: false,
    chapterId: 10 + id,
    chapterIds: [10 + id],
    comicId: null,
    comicCoverRev: null,
    actions: ["open_chapter", "open_series"],
    count: 1,
    createdAt: Date.now() - id * 1000,
    updatedAt: Date.now(),
    readAt: null,
    ...patch,
  };
}

describe("regras da central", () => {
  it("a badge esconde o zero e limita em 99+", () => {
    expect(badgeLabel(0)).toBeNull();
    expect(badgeLabel(-3)).toBeNull();
    expect(badgeLabel(1)).toBe("1");
    expect(badgeLabel(5)).toBe("5");
    expect(badgeLabel(99)).toBe("99");
    expect(badgeLabel(100)).toBe("99+");
  });

  it("agrupa por período do calendário, mantendo a ordem", () => {
    const now = new Date(2026, 10, 12, 15, 0).getTime();
    const at = (d: number, h: number) => new Date(2026, 10, d, h, 0).getTime();
    expect(periodOf(at(12, 0), now)).toBe("today");
    expect(periodOf(at(11, 23), now)).toBe("yesterday");
    expect(periodOf(at(6, 12), now)).toBe("week");
    expect(periodOf(at(5, 12), now)).toBe("older");
    const items = [at(12, 9), at(12, 1), at(11, 8), at(8, 8), at(1, 8)].map((createdAt, i) => ({ id: i, createdAt }));
    expect(groupByPeriod(items, now).map((g) => [g.period, g.items.length])).toEqual([
      ["today", 2],
      ["yesterday", 1],
      ["week", 1],
      ["older", 1],
    ]);
    expect(formatWhen(at(11, 8), now)).toMatch(/^Ontem, 08:00$/);
  });

  it("aplica leituras e páginas novas sem buscar tudo de novo", () => {
    const items = [note(1), note(2)];
    const read = withRead(items, new Set([2]), true, 99);
    expect(read.map((n) => n.readAt)).toEqual([null, 99]);
    expect(withRead(read, new Set([2]), false, 100)[1]!.readAt).toBeNull();
    const merged = mergeNewest(items, [note(3, { createdAt: Date.now() + 5000 }), { ...note(1), title: "Atualizada" }]);
    expect(merged.map((n) => n.id)).toEqual([3, 1, 2]);
    expect(merged[1]!.title).toBe("Atualizada");
  });

  it("só mostra filtros de categorias que podem ter eventos", () => {
    expect(visibleCategories(true)).toEqual(["chapters", "updates", "downloads", "library", "system"]);
    expect(visibleCategories(false)).toEqual(["updates", "library"]);
  });

  it("o popup fica dentro da janela em qualquer tamanho", () => {
    const anchor = { left: 200, right: 232, top: 12, bottom: 44, width: 32, height: 32 } as DOMRect;
    const wide = panelPosition(anchor, { width: 1280, height: 800 });
    expect(wide.left).toBe(240);
    expect(wide.width).toBe(380);
    const small = panelPosition(anchor, { width: 400, height: 300 });
    expect(small.left).toBeGreaterThanOrEqual(8);
    expect(small.left + small.width).toBeLessThanOrEqual(392);
    expect(small.top + small.maxHeight).toBeLessThanOrEqual(292);
  });

  it("silencia por um tempo e guarda as preferências com padrões seguros", () => {
    const now = new Date(2026, 10, 12, 22, 0).getTime();
    expect(muteUntil("1h", now)).toBe(now + HOUR);
    expect(new Date(muteUntil("tomorrow", now)).getHours()).toBe(8);
    const s = sanitizeSettings({ "notifications.native": false, "notifications.nativeMutedUntil": "amanhã", "notifications.chapters": "sim" });
    expect(s["notifications.native"]).toBe(false);
    expect(s["notifications.nativeMutedUntil"]).toBeNull();
    expect(s["notifications.chapters"]).toBe(true);
    expect(s["notifications.nativeDownloads"]).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Interface
// ---------------------------------------------------------------------------

let store: AppNotification[];
let settings: Record<string, unknown>;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;
let targets: Record<number, unknown>;

const unreadCount = () => store.filter((n) => !n.readAt).length;

function list(query: NotificationQuery) {
  let items = [...store].sort((a, b) => b.createdAt - a.createdAt || b.id - a.id);
  if (query.state === "unread") items = items.filter((n) => !n.readAt);
  if (query.state === "read") items = items.filter((n) => n.readAt);
  if (query.category) items = items.filter((n) => n.category === query.category);
  if (query.before) {
    const b = query.before;
    items = items.filter((n) => n.createdAt < b.createdAt || (n.createdAt === b.createdAt && n.id < b.id));
  }
  const limit = query.limit ?? 30;
  const page = items.slice(0, limit);
  const last = page[page.length - 1];
  return { items: page, nextCursor: items.length > limit && last ? { createdAt: last.createdAt, id: last.id } : null, unread: unreadCount() };
}

function setup() {
  mockWindows("main");
  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      const ids = (args.ids as number[] | undefined) ?? [];
      switch (cmd) {
        case "get_settings":
          return settings;
        case "set_setting":
          settings[args.key as string] = args.value;
          return null;
        case "get_home":
          return { total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] };
        case "list_comics":
        case "pending_pdf_covers":
        case "list_collections":
        case "series_library":
          return [];
        case "notifications_unread":
          return unreadCount();
        case "notifications_list":
          return list(args.query as NotificationQuery);
        case "notifications_set_read":
          store = store.map((n) => (ids.includes(n.id) ? { ...n, readAt: args.read ? (n.readAt ?? Date.now()) : null } : n));
          return unreadCount();
        case "notifications_mark_all_read":
          store = store.map((n) => ({ ...n, readAt: n.readAt ?? Date.now() }));
          return 0;
        case "notifications_remove":
          store = store.filter((n) => !ids.includes(n.id));
          return unreadCount();
        case "notifications_clear_read":
          store = store.filter((n) => !n.readAt);
          return unreadCount();
        case "notification_target": {
          const t = targets[args.id as number];
          if (t instanceof Error) throw { code: "notification_target_missing", message: t.message };
          return t ?? { kind: "library" };
        }
        case "get_comic":
          throw { code: "not_found", message: "O item não foi encontrado na biblioteca." };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}

beforeEach(() => {
  store = [];
  targets = {};
  settings = { onboarding: { status: "completed", step: 4, comicId: null }, "plugins.online": true };
  calls = [];
  localStorage.clear();
  setup();
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

const bell = () => screen.findByRole("button", { name: /^Notificações/ });
const badge = () => screen.queryByTestId("notification-badge");

async function openPopup(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await bell());
  return screen.findByRole("dialog", { name: "Notificações" });
}

describe("sino e badge", () => {
  it("sem não lidas, só o sino", async () => {
    store = [note(1, { readAt: 1 })];
    render(<App />);
    expect(await bell()).toHaveAccessibleName("Notificações");
    await waitFor(() => expect(calls.some((c) => c.cmd === "notifications_unread")).toBe(true));
    expect(badge()).toBeNull();
  });

  it("mostra a quantidade real de não lidas, guardada no banco", async () => {
    store = [note(1)];
    const { unmount } = render(<App />);
    await waitFor(() => expect(badge()).toHaveTextContent("1"));
    expect(await bell()).toHaveAccessibleName("Notificações, 1 não lida");
    unmount();
    // Reiniciar: a contagem volta do banco.
    store = Array.from({ length: 5 }, (_, i) => note(i + 1));
    render(<App />);
    await waitFor(() => expect(badge()).toHaveTextContent("5"));
  });

  it("acima de 99 mostra 99+", async () => {
    store = Array.from({ length: 120 }, (_, i) => note(i + 1));
    render(<App />);
    await waitFor(() => expect(badge()).toHaveTextContent("99+"));
  });

  it("atualiza com o evento do backend, em qualquer tela, sem recarregar", async () => {
    const user = userEvent.setup();
    render(<App />);
    await bell();
    expect(badge()).toBeNull();
    await user.click(screen.getByRole("button", { name: "Configurações" }));
    store = [note(1), note(2)];
    await act(() => emit("notifications:changed", { unread: 2, created: 2 }));
    await waitFor(() => expect(badge()).toHaveTextContent("2"));
    await act(() => emit("notifications:changed", { unread: 0 }));
    await waitFor(() => expect(badge()).toBeNull());
  });
});

describe("popup", () => {
  it("abre, fecha com Esc e com clique fora", async () => {
    store = [note(1)];
    const user = userEvent.setup();
    render(<App />);
    let popup = await openPopup(user);
    expect(within(popup).getByRole("heading", { name: "Notificações" })).toBeInTheDocument();
    expect(await within(popup).findByText("Notificação 1")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Notificações" })).not.toBeInTheDocument());
    expect(await bell()).toHaveFocus();
    popup = await openPopup(user);
    // Interações internas não fecham.
    await user.click(within(popup).getByRole("heading", { name: "Notificações" }));
    expect(screen.getByRole("dialog", { name: "Notificações" })).toBeInTheDocument();
    await user.click(document.querySelector("main")!);
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Notificações" })).not.toBeInTheDocument());
  });

  it("não consulta de novo a cada abertura sem novidade", async () => {
    store = [note(1)];
    const user = userEvent.setup();
    render(<App />);
    await openPopup(user);
    await screen.findByText("Notificação 1");
    await user.keyboard("{Escape}");
    await openPopup(user);
    expect(calls.filter((c) => c.cmd === "notifications_list")).toHaveLength(1);
    await act(() => emit("notifications:changed", { unread: 1, created: 9 }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "notifications_list")).toHaveLength(2));
  });

  it("estado vazio", async () => {
    const user = userEvent.setup();
    render(<App />);
    const popup = await openPopup(user);
    expect(await within(popup).findByText("Nenhuma notificação")).toBeInTheDocument();
    expect(within(popup).getByRole("button", { name: "Marcar todas como lidas" })).toBeDisabled();
  });

  it("abrir executa a ação, marca como lida e fecha", async () => {
    store = [note(1, { category: "updates", kind: "update_available", actions: ["open_updates"] })];
    targets[1] = { kind: "updates" };
    const user = userEvent.setup();
    render(<App />);
    const popup = await openPopup(user);
    await user.click(await within(popup).findByRole("button", { name: /Notificação 1/ }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Notificações" })).not.toBeInTheDocument());
    expect(await screen.findByRole("heading", { name: "Atualizações", level: 2 })).toBeInTheDocument();
    expect(calls.find((c) => c.cmd === "notifications_set_read")?.args).toEqual({ ids: [1], read: true });
    await waitFor(() => expect(badge()).toBeNull());
  });

  it("destino inexistente: avisa e não marca como lida", async () => {
    store = [note(1)];
    targets[1] = new Error("Este capítulo não está mais disponível na fonte.");
    const user = userEvent.setup();
    render(<App />);
    const popup = await openPopup(user);
    await user.click(await within(popup).findByRole("button", { name: /Notificação 1/ }));
    expect(await screen.findByText("Este capítulo não está mais disponível na fonte.")).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === "notifications_set_read")).toBe(false);
    expect(screen.getByRole("dialog", { name: "Notificações" })).toBeInTheDocument();
    expect(badge()).toHaveTextContent("1");
  });

  it("marca individualmente, remove e marca todas, com a badge na hora", async () => {
    store = [note(1), note(2), note(3)];
    const user = userEvent.setup();
    render(<App />);
    const popup = await openPopup(user);
    await within(popup).findByText("Notificação 1");
    const item = (title: string) => within(popup).getByText(title).closest("li")!;
    await user.click(within(item("Notificação 1")).getByRole("button", { name: "Marcar como lida" }));
    await waitFor(() => expect(badge()).toHaveTextContent("2"));
    await user.click(within(item("Notificação 1")).getByRole("button", { name: "Marcar como não lida" }));
    await waitFor(() => expect(badge()).toHaveTextContent("3"));
    await user.click(within(item("Notificação 2")).getByRole("button", { name: "Remover notificação" }));
    await waitFor(() => expect(within(popup).queryByText("Notificação 2")).not.toBeInTheDocument());
    expect(badge()).toHaveTextContent("2");
    await user.click(within(popup).getByRole("button", { name: "Marcar todas como lidas" }));
    await waitFor(() => expect(badge()).toBeNull());
    expect(within(popup).getByText("Nenhuma não lida")).toBeInTheDocument();
  });

  it("leva à central completa", async () => {
    const user = userEvent.setup();
    render(<App />);
    const popup = await openPopup(user);
    await user.click(within(popup).getByRole("button", { name: "Ver todas as notificações" }));
    expect(await screen.findByRole("heading", { name: "Notificações", level: 1 })).toBeInTheDocument();
  });
});

describe("tela Notificações", () => {
  async function openCenter(user: ReturnType<typeof userEvent.setup>) {
    const popup = await openPopup(user);
    await user.click(within(popup).getByRole("button", { name: "Ver todas as notificações" }));
    return screen.findByRole("heading", { name: "Notificações", level: 1 });
  }

  it("lista em ordem, agrupa por período e pagina", async () => {
    const now = Date.now();
    store = [
      ...Array.from({ length: 31 }, (_, i) => note(i + 1, { createdAt: now - i * 1000 })),
      note(100, { createdAt: now - 10 * DAY, title: "Bem antiga" }),
    ];
    const user = userEvent.setup();
    render(<App />);
    await openCenter(user);
    expect(await screen.findByText("Notificação 1")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Hoje" })).toBeInTheDocument();
    expect(screen.queryByText("Bem antiga")).not.toBeInTheDocument();
    const titles = within(screen.getByRole("main")).getAllByRole("listitem").map((li) => li.textContent ?? "");
    expect(titles[0]).toContain("Notificação 1");
    expect(titles[1]).toContain("Notificação 2");
    await user.click(screen.getByRole("button", { name: "Carregar mais" }));
    expect(await screen.findByText("Bem antiga")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Anteriores" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Carregar mais" })).not.toBeInTheDocument();
    const lists = calls.filter((c) => c.cmd === "notifications_list").map((c) => (c.args.query as NotificationQuery).before);
    expect(lists.at(-1)).toEqual({ createdAt: now - 29 * 1000, id: 30 });
  });

  it("filtra por estado e por categoria, e explica filtro sem resultado", async () => {
    store = [note(1), note(2, { readAt: 5, title: "Já lida" }), note(3, { category: "updates", kind: "update_available", title: "Versão nova", actions: ["open_updates"] })];
    const user = userEvent.setup();
    render(<App />);
    await openCenter(user);
    await screen.findByText("Notificação 1");
    await user.click(screen.getByRole("radio", { name: "Não lidas" }));
    await waitFor(() => expect(screen.queryByText("Já lida")).not.toBeInTheDocument());
    expect(screen.getByText("Versão nova")).toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: "Lidas" }));
    expect(await screen.findByText("Já lida")).toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: "Todas" }));
    const chips = screen.getByRole("group", { name: "Categoria" });
    await user.click(within(chips).getByRole("button", { name: "Atualizações" }));
    await waitFor(() => expect(screen.queryByText("Notificação 1")).not.toBeInTheDocument());
    expect(screen.getByText("Versão nova")).toBeInTheDocument();
    await user.click(within(chips).getByRole("button", { name: "Downloads" }));
    expect(await screen.findByText("Nenhuma notificação em Downloads")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Ver todas" }));
    expect(await screen.findByText("Notificação 1")).toBeInTheDocument();
  });

  it("estado vazio sem filtro", async () => {
    const user = userEvent.setup();
    render(<App />);
    await openCenter(user);
    expect(await screen.findByRole("heading", { name: "Nenhuma notificação" })).toBeInTheDocument();
  });

  it("seleção múltipla: marca e remove as escolhidas; limpar lidas pede confirmação", async () => {
    store = [note(1), note(2), note(3)];
    const user = userEvent.setup();
    render(<App />);
    await openCenter(user);
    await screen.findByText("Notificação 1");
    await user.click(screen.getByRole("checkbox", { name: "Selecionar “Notificação 1”" }));
    await user.click(screen.getByRole("checkbox", { name: "Selecionar “Notificação 2”" }));
    const bar = screen.getByRole("toolbar", { name: "Ações nas selecionadas" });
    await user.click(within(bar).getByRole("button", { name: "Marcar como lidas" }));
    await waitFor(() => expect(badge()).toHaveTextContent("1"));
    await user.click(within(bar).getByRole("button", { name: "Remover" }));
    await waitFor(() => expect(screen.queryByText("Notificação 1")).not.toBeInTheDocument());
    expect(screen.queryByText("Notificação 2")).not.toBeInTheDocument();
    expect(screen.getByText("Notificação 3")).toBeInTheDocument();

    await user.click(within(screen.getByText("Notificação 3").closest("li")!).getByRole("button", { name: "Marcar como lida" }));
    await user.click(await screen.findByRole("button", { name: "Limpar lidas" }));
    const confirm = await screen.findByRole("dialog", { name: "Limpar notificações lidas?" });
    await user.click(within(confirm).getByRole("button", { name: "Limpar lidas" }));
    expect(await screen.findByRole("heading", { name: "Nenhuma notificação" })).toBeInTheDocument();
  });

  it("executa ações secundárias", async () => {
    store = [note(1)];
    targets[1] = { kind: "library" };
    const user = userEvent.setup();
    render(<App />);
    await openCenter(user);
    await user.click(await screen.findByRole("button", { name: "Abrir obra" }));
    expect(calls.find((c) => c.cmd === "notification_target")?.args).toEqual({ id: 1, action: "open_series" });
    expect(await screen.findByRole("heading", { name: "Minha biblioteca", level: 1 })).toBeInTheDocument();
  });

  it("uma notificação nova aparece no topo sem perder o que foi carregado", async () => {
    store = [note(1)];
    const user = userEvent.setup();
    render(<App />);
    await openCenter(user);
    await screen.findByText("Notificação 1");
    store = [note(50, { createdAt: Date.now() + 1000, title: "Chegou agora" }), ...store];
    await act(() => emit("notifications:changed", { unread: 2, created: 50 }));
    expect(await screen.findByText("Chegou agora")).toBeInTheDocument();
    expect(screen.getByText("Notificação 1")).toBeInTheDocument();
  });

  it("clique numa notificação do sistema executa a ação", async () => {
    store = [note(1)];
    targets[1] = { kind: "downloads" };
    render(<App />);
    await bell();
    await act(() => emit("notifications:activate", { id: 1 }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "notification_target")).toBe(true));
    expect(await screen.findByRole("heading", { name: "Downloads", level: 1 })).toBeInTheDocument();
  });
});

describe("preferências", () => {
  it("liga e desliga categorias e separa as notificações do sistema", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    await user.click(within(await screen.findByRole("navigation", { name: "Categorias das configurações" })).getByRole("button", { name: "Notificações" }));
    await user.click(await screen.findByRole("switch", { name: "Capítulos novos" }));
    expect(settings["notifications.chapters"]).toBe(false);
    await user.click(screen.getByRole("switch", { name: "Mostrar notificações do sistema" }));
    expect(settings["notifications.native"]).toBe(false);
    // Sem as do sistema, a central continua com seus próprios interruptores.
    expect(screen.getByRole("switch", { name: "Falhas de download" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("button", { name: "Enviar notificação de teste" })).toBeDisabled();
    await user.click(screen.getByRole("switch", { name: "Mostrar notificações do sistema" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Silenciar temporariamente" }), "1h");
    expect(typeof settings["notifications.nativeMutedUntil"]).toBe("number");
    expect(await screen.findByText(/Silenciadas até/)).toBeInTheDocument();
    await user.selectOptions(screen.getByRole("combobox", { name: "Silenciar temporariamente" }), "off");
    expect(settings["notifications.nativeMutedUntil"]).toBeNull();
  });

  it("permissão negada avisa sem desligar a central", async () => {
    const user = userEvent.setup();
    render(<App />);
    clearMocks();
    mockWindows("main");
    const base = (cmd: string, args: Record<string, unknown>) => {
      if (cmd === "notifications_native_test") return { permission: "denied" };
      if (cmd === "set_setting") settings[args.key as string] = args.value;
      return null;
    };
    mockIPC((cmd, payload) => base(cmd, (payload ?? {}) as Record<string, unknown>), { shouldMockEvents: true });
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    await user.click(within(await screen.findByRole("navigation", { name: "Categorias das configurações" })).getByRole("button", { name: "Notificações" }));
    await user.click(await screen.findByRole("button", { name: "Enviar notificação de teste" }));
    expect(await screen.findByText("O sistema não permite notificações do Prancha")).toBeInTheDocument();
    expect(settings["notifications.native"]).toBeUndefined();
  });

  it("o atalho do popup abre direto as preferências", async () => {
    const user = userEvent.setup();
    render(<App />);
    const popup = await openPopup(user);
    await user.click(within(popup).getByRole("button", { name: "Preferências de notificação" }));
    expect(await screen.findByRole("switch", { name: "Capítulos novos" })).toBeInTheDocument();
  });
});
