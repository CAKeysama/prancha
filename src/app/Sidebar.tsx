import { useEffect, useState } from "react";
import { BookOpen, FolderHeart, History, House, Library, Settings, Star } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { cn } from "../components/cn";
import { Logo } from "../components/Logo";
import { Tooltip } from "../components/Tooltip";
import { UpdateIndicator } from "../features/updates/UpdateIndicator";
import { APP_NAME } from "./config";
import { useNav, type Route } from "./navigation";

const ITEMS: Array<{ route: Route; label: string; icon: LucideIcon }> = [
  { route: { name: "home" }, label: "Início", icon: House },
  { route: { name: "library" }, label: "Minha biblioteca", icon: Library },
  { route: { name: "continue" }, label: "Continuar lendo", icon: BookOpen },
  { route: { name: "recent" }, label: "Recentes", icon: History },
  { route: { name: "favorites" }, label: "Favoritos", icon: Star },
  { route: { name: "collections" }, label: "Coleções", icon: FolderHeart },
];

function useNarrow(breakpoint = 960): boolean {
  const [narrow, setNarrow] = useState(() => window.innerWidth < breakpoint);
  useEffect(() => {
    const on = () => setNarrow(window.innerWidth < breakpoint);
    window.addEventListener("resize", on);
    return () => window.removeEventListener("resize", on);
  }, [breakpoint]);
  return narrow;
}

export function Sidebar({ current }: { current: Route["name"] }) {
  const nav = useNav();
  const narrow = useNarrow();
  const item = (r: Route, label: string, Icon: LucideIcon) => {
    const selected = current === r.name;
    const button = (
      <button
        type="button"
        onClick={() => nav.navigate(r)}
        aria-current={selected ? "page" : undefined}
        aria-label={narrow ? label : undefined}
        className={cn(
          "relative flex h-9 w-full items-center gap-3 rounded-md px-3 text-left text-base transition-colors",
          selected ? "bg-elevated font-semibold text-fg" : "text-muted hover:bg-hover hover:text-fg",
          narrow && "justify-center px-0",
        )}
      >
        {/* Barra lateral de seleção: o estado não depende só da cor. */}
        <span className={cn("absolute top-1.5 bottom-1.5 -left-2 w-1 rounded-full", selected ? "bg-accent" : "bg-transparent")} aria-hidden />
        <Icon size={18} aria-hidden className={selected ? "text-fg" : undefined} />
        {!narrow && <span className="truncate">{label}</span>}
      </button>
    );
    return narrow ? (
      <Tooltip content={label} side="bottom" className="flex w-full">
        {button}
      </Tooltip>
    ) : (
      button
    );
  };
  return (
    <nav aria-label="Navegação principal" className={cn("flex h-full shrink-0 flex-col border-r border-border bg-surface px-3 py-3", narrow ? "w-16" : "w-60")}>
      <div className={cn("mb-4 flex items-center gap-2.5 px-2", narrow && "justify-center px-0")}>
        <Logo />
        {!narrow && <span className="text-lg font-semibold tracking-tight">{APP_NAME}</span>}
      </div>
      <ul className="flex flex-col gap-0.5">
        {ITEMS.map((i) => (
          <li key={i.route.name}>{item(i.route, i.label, i.icon)}</li>
        ))}
      </ul>
      <div className="mt-auto border-t border-border pt-3">
        <UpdateIndicator narrow={narrow} />
        {item({ name: "settings" }, "Configurações", Settings)}
      </div>
    </nav>
  );
}
