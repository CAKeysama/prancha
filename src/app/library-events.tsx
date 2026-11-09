import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { onLibraryChanged } from "../services/events";
import { fetchInto, peek, subscribe } from "../services/query-cache";

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

/** Leituras locais repetidas neste intervalo, na mesma versão da biblioteca, usam o valor guardado sem consultar. */
const LOCAL_FRESH_MS = 2000;

/**
 * Carrega dados do backend e recarrega quando o catálogo muda. `name` identifica a consulta no cache do app:
 * a tela que volta mostra na hora o último valor e confere em segundo plano. Mantém o último valor durante
 * recargas (sem piscar a tela) e expõe erro com mensagem em português.
 */
export function useLibraryData<T>(
  load: () => Promise<T>,
  deps: unknown[],
  name: string,
): {
  data: T | null;
  error: string | null;
  loading: boolean;
  reload: () => void;
} {
  const { version } = useLibraryVersion();
  const [nonce, setNonce] = useState(0);
  const key = `${name}:${JSON.stringify(deps)}`;
  const snapshot = useSyncExternalStore(
    useCallback((listener: () => void) => subscribe(key, listener), [key]),
    () => peek<T>(key),
  );
  // Enquanto a chave nova não tem valor, segue mostrando o da anterior.
  const [shownKey, setShownKey] = useState(key);
  if (snapshot.data !== undefined && shownKey !== key) setShownKey(key);
  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  });
  const handledNonce = useRef(nonce);
  useEffect(() => {
    const current = peek<T>(key);
    const forced = handledNonce.current !== nonce;
    handledNonce.current = nonce;
    if (!forced && current.version === version && Date.now() - current.updatedAt < LOCAL_FRESH_MS) return;
    fetchInto(key, () => loadRef.current(), version).catch(() => undefined);
  }, [key, version, nonce]);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  const data = snapshot.data !== undefined ? snapshot.data : peek<T>(shownKey).data;
  return { data: data ?? null, error: snapshot.error, loading: snapshot.loading || (snapshot.version !== version && snapshot.error === null), reload };
}
