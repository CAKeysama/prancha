import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";

export type Route =
  | { name: "home" }
  | { name: "library" }
  | { name: "continue" }
  | { name: "recent" }
  | { name: "favorites" }
  | { name: "collections" }
  | { name: "collection"; id: number }
  | { name: "comic"; id: number }
  | { name: "reader"; id: number }
  | { name: "settings"; section?: string };

interface Nav {
  route: Route;
  stack: Route[];
  navigate: (r: Route, opts?: { replace?: boolean }) => void;
  back: () => void;
  canGoBack: boolean;
}

const Ctx = createContext<Nav | null>(null);

export function useNav(): Nav {
  const v = useContext(Ctx);
  if (!v) throw new Error("useNav fora do NavigationProvider");
  return v;
}

/** Rotas de primeiro nível (barra lateral) zeram o histórico. */
const TOP_LEVEL = new Set(["home", "library", "continue", "recent", "favorites", "collections", "settings"]);

export function NavigationProvider({ children, initial = { name: "home" } }: { children: ReactNode; initial?: Route }) {
  const [stack, setStack] = useState<Route[]>([initial]);
  const navigate = useCallback((r: Route, opts?: { replace?: boolean }) => {
    setStack((s) => {
      if (opts?.replace) return [...s.slice(0, -1), r];
      if (TOP_LEVEL.has(r.name)) return [r];
      return [...s.slice(-20), r];
    });
  }, []);
  const back = useCallback(() => setStack((s) => (s.length > 1 ? s.slice(0, -1) : [{ name: "home" }])), []);
  const value = useMemo<Nav>(
    () => ({ route: stack[stack.length - 1]!, stack, navigate, back, canGoBack: stack.length > 1 }),
    [stack, navigate, back],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/** Item da barra lateral a destacar: detalhes e leitor herdam a seção de onde vieram. */
export function sectionOf(stack: Route[]): Route["name"] {
  for (let i = stack.length - 1; i >= 0; i--) {
    const r = stack[i]!;
    if (r.name === "collection") return "collections";
    if (r.name !== "comic" && r.name !== "reader") return r.name;
  }
  return "library";
}
