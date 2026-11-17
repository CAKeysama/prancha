import { Blocks, Boxes, ChevronRight, Compass, Download, House, Library, Menu as MenuIcon, Puzzle, Settings } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { cn } from "../components/cn";
import { PageHeader } from "../components/PageHeader";
import { useNav, type Route } from "./navigation";
import { useSettings } from "./settings-context";

// Navegação do celular (design aprovado da Fase 2): quatro destinos embaixo, ao alcance do polegar. Os itens da
// barra lateral do desktop continuam todos aqui: os da biblioteca viram abas dentro dela (LibraryTabs) e os
// utilitários vão para "Mais".

type Tab = "home" | "library" | "explore" | "more";

const LIBRARY_ROUTES = new Set<Route["name"]>(["library", "continue", "recent", "favorites", "collections", "collection", "comic"]);
const MORE_ROUTES = new Set<Route["name"]>(["more", "downloads", "extensions", "repositories", "plugins", "settings"]);

/** O destino aceso: telas internas herdam o da seção de onde vieram. */
export function tabOf(stack: Route[]): Tab | null {
  for (let i = stack.length - 1; i >= 0; i--) {
    const name = stack[i]!.name;
    if (name === "home") return "home";
    if (LIBRARY_ROUTES.has(name)) return "library";
    if (name === "explore" || name === "series") return "explore";
    if (MORE_ROUTES.has(name)) return "more";
  }
  return null;
}

/** Telas de detalhe ficam com a tela inteira, como no Android. */
export function showsBottomNav(route: Route): boolean {
  return route.name !== "comic" && route.name !== "series";
}

const TABS: Array<{ tab: Tab; route: Route; label: string; icon: LucideIcon }> = [
  { tab: "home", route: { name: "home" }, label: "Início", icon: House },
  { tab: "library", route: { name: "library" }, label: "Biblioteca", icon: Library },
  { tab: "explore", route: { name: "explore" }, label: "Explorar", icon: Compass },
  { tab: "more", route: { name: "more" }, label: "Mais", icon: MenuIcon },
];

export function BottomNav() {
  const nav = useNav();
  const { settings } = useSettings();
  const current = tabOf(nav.stack);
  const tabs = TABS.filter((t) => t.tab !== "explore" || settings["plugins.online"]);
  return (
    <nav aria-label="Navegação principal" className="shrink-0 border-t border-border bg-surface">
      <ul className="flex h-16">
        {tabs.map(({ tab, route, label, icon: Icon }) => {
          const on = tab === current;
          return (
            <li key={tab} className="flex-1">
              <button
                type="button"
                onClick={() => nav.navigate(route)}
                aria-current={on ? "page" : undefined}
                className="flex h-full w-full flex-col items-center justify-center gap-1"
              >
                <span className={cn("grid h-8 w-16 place-items-center rounded-full transition-colors", on ? "bg-accent-subtle text-fg" : "text-muted")}>
                  <Icon size={22} strokeWidth={on ? 2.2 : 1.8} aria-hidden />
                </span>
                <span className={cn("text-xs", on ? "font-semibold text-fg" : "font-medium text-muted")}>{label}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

const LIBRARY_TABS: Array<{ route: Route; label: string }> = [
  { route: { name: "library" }, label: "Tudo" },
  { route: { name: "continue" }, label: "Continuar lendo" },
  { route: { name: "recent" }, label: "Recentes" },
  { route: { name: "favorites" }, label: "Favoritos" },
  { route: { name: "collections" }, label: "Coleções" },
];

/** As seções da biblioteca, que no desktop ficam na barra lateral, em abas roláveis. */
export function LibraryTabs({ current }: { current: Route["name"] }) {
  const nav = useNav();
  return (
    <div role="tablist" aria-label="Seções da biblioteca" className="-mx-4 flex gap-2 overflow-x-auto px-4 pb-1 [scrollbar-width:none]">
      {LIBRARY_TABS.map(({ route, label }) => {
        const on = route.name === current;
        return (
          <button
            key={route.name}
            type="button"
            role="tab"
            aria-selected={on}
            onClick={() => nav.navigate(route)}
            className={cn(
              "h-9 shrink-0 rounded-lg border px-3 text-sm font-medium whitespace-nowrap transition-colors",
              on ? "border-transparent bg-accent-subtle text-fg" : "border-border-control/60 text-fg",
            )}
          >
            {label}
          </button>
        );
      })}
    </div>
  );
}

function MoreRow({ icon: Icon, label, onClick }: { icon: LucideIcon; label: string; onClick: () => void }) {
  return (
    <li>
      <button type="button" onClick={onClick} className="flex min-h-14 w-full items-center gap-4 px-4 text-left text-base active:bg-active">
        <Icon size={22} strokeWidth={1.8} className="shrink-0 text-muted" aria-hidden />
        <span className="flex-1">{label}</span>
        <ChevronRight size={18} className="text-muted" aria-hidden />
      </button>
    </li>
  );
}

/** "Mais": os destinos que no desktop ficam no rodapé e no bloco online da barra lateral. */
export function MoreScreen() {
  const nav = useNav();
  const { settings } = useSettings();
  const online = settings["plugins.online"];
  return (
    <div className="h-full overflow-y-auto">
      <PageHeader title="Mais" />
      <ul className="py-2">
        {online && <MoreRow icon={Download} label="Downloads" onClick={() => nav.navigate({ name: "downloads" }, { push: true })} />}
        {online && <MoreRow icon={Puzzle} label="Extensões" onClick={() => nav.navigate({ name: "extensions" }, { push: true })} />}
        {online && <MoreRow icon={Boxes} label="Repositórios" onClick={() => nav.navigate({ name: "repositories" }, { push: true })} />}
        <MoreRow icon={Blocks} label="Plugins" onClick={() => nav.navigate({ name: "plugins" }, { push: true })} />
        <MoreRow icon={Settings} label="Configurações" onClick={() => nav.navigate({ name: "settings" }, { push: true })} />
      </ul>
    </div>
  );
}
