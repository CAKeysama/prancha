import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { onLibraryChanged } from "../services/events";

interface LibraryVersion {
  version: number;
  bump: () => void;
}

const Ctx = createContext<LibraryVersion>({ version: 0, bump: () => undefined });

/** Contador incrementado quando o catálogo muda (eventos do backend agrupados em 250 ms). */
export function LibraryEventsProvider({ children }: { children: ReactNode }) {
  const [version, setVersion] = useState(0);
  const timer = useRef<number | undefined>(undefined);
  const bump = useCallback(() => {
    if (timer.current !== undefined) return;
    timer.current = window.setTimeout(() => {
      timer.current = undefined;
      setVersion((v) => v + 1);
    }, 250);
  }, []);
  useEffect(() => {
    const off = onLibraryChanged(bump);
    return () => {
      off();
      window.clearTimeout(timer.current);
    };
  }, [bump]);
  const value = useMemo(() => ({ version, bump }), [version, bump]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export const useLibraryVersion = () => useContext(Ctx);

/**
 * Carrega dados do backend e recarrega quando o catálogo muda. Mantém o último valor
 * durante recargas (sem piscar a tela) e expõe erro com mensagem em português.
 */
export function useLibraryData<T>(load: () => Promise<T>, deps: unknown[]): {
  data: T | null;
  error: string | null;
  loading: boolean;
  reload: () => void;
} {
  const { version } = useLibraryVersion();
  const [nonce, setNonce] = useState(0);
  const key = JSON.stringify([version, nonce, ...deps]);
  const [state, setState] = useState<{ data: T | null; error: string | null; key: string | null }>({
    data: null,
    error: null,
    key: null,
  });
  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  });
  useEffect(() => {
    let alive = true;
    loadRef
      .current()
      .then((data) => alive && setState({ data, error: null, key }))
      .catch((e: unknown) => alive && setState((s) => ({ data: s.data, error: e instanceof Error ? e.message : String(e), key })));
    return () => {
      alive = false;
    };
  }, [key]);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { data: state.data, error: state.error, loading: state.key !== key, reload };
}
