import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { TelemetryStatus } from "../../domain/telemetry";
import { api } from "../../services/api";
import { installGlobalErrorReporting, setTelemetryEnabled } from "../../services/telemetry";

interface TelemetryApi {
  /** `null` enquanto carrega ou se o backend não respondeu: a tela mostra a telemetria como desligada. */
  status: TelemetryStatus | null;
  setConsent: (granted: boolean) => Promise<TelemetryStatus>;
  resetId: () => Promise<TelemetryStatus>;
  forget: () => Promise<TelemetryStatus>;
  refresh: () => Promise<void>;
}

const Ctx = createContext<TelemetryApi | null>(null);

export function useTelemetry(): TelemetryApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("useTelemetry fora do provedor");
  return v;
}

/** Estado do consentimento. Nada aqui segura a abertura do app: tudo roda depois da primeira tela. */
export function TelemetryProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<TelemetryStatus | null>(null);
  const ready = useRef(false);

  const apply = useCallback((s: TelemetryStatus) => {
    setTelemetryEnabled(s.consent === "granted");
    setStatus(s);
    return s;
  }, []);

  const refresh = useCallback(async () => {
    try {
      apply(await api.telemetryStatus());
    } catch {
      setTelemetryEnabled(false);
    }
  }, [apply]);

  useEffect(() => {
    api
      .telemetryStatus()
      .then(apply)
      .catch(() => setTelemetryEnabled(false));
    if (!ready.current) {
      ready.current = true;
      // A primeira tela já está desenhada: é o fim da inicialização. O backend só conta com consentimento.
      api.telemetryReady().catch(() => undefined);
    }
    return installGlobalErrorReporting();
  }, [apply]);

  const setConsent = useCallback(async (granted: boolean) => {
    // Desliga do lado da interface antes de esperar o backend: nada novo sai a partir daqui.
    if (!granted) setTelemetryEnabled(false);
    return apply(await api.telemetrySetConsent(granted));
  }, [apply]);
  const resetId = useCallback(async () => apply(await api.telemetryResetId()), [apply]);
  const forget = useCallback(async () => {
    setTelemetryEnabled(false);
    try {
      return apply(await api.telemetryForget());
    } finally {
      void refresh();
    }
  }, [apply, refresh]);

  const value = useMemo(() => ({ status, setConsent, resetId, forget, refresh }), [status, setConsent, resetId, forget, refresh]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
