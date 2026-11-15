import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { CircleAlert, CircleCheck, Info, X } from "lucide-react";
import { cn } from "./cn";

type Tone = "info" | "success" | "error";
interface ToastItem {
  id: number;
  tone: Tone;
  title: string;
  message?: string;
  action?: { label: string; onClick: () => void };
}

interface ToastApi {
  show: (t: Omit<ToastItem, "id">) => void;
  success: (title: string, message?: string) => void;
  error: (title: string, message?: string) => void;
  info: (title: string, message?: string) => void;
}

const Ctx = createContext<ToastApi | null>(null);

export function useToast(): ToastApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("useToast fora do ToastProvider");
  return v;
}

const ICONS = { info: Info, success: CircleCheck, error: CircleAlert };

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const next = useRef(1);
  const timers = useRef(new Set<number>());
  // Um aviso que ainda ia sumir não mexe mais no estado depois que o provedor sai da tela.
  useEffect(() => {
    const pending = timers.current;
    return () => {
      pending.forEach((timer) => window.clearTimeout(timer));
      pending.clear();
    };
  }, []);
  const dismiss = useCallback((id: number) => setItems((v) => v.filter((t) => t.id !== id)), []);
  const show = useCallback(
    (t: Omit<ToastItem, "id">) => {
      const id = next.current++;
      setItems((v) => [...v.slice(-3), { ...t, id }]);
      // Erros ficam mais tempo na tela para dar tempo de ler.
      const timer = window.setTimeout(
        () => {
          timers.current.delete(timer);
          dismiss(id);
        },
        t.tone === "error" ? 9000 : 4500,
      );
      timers.current.add(timer);
    },
    [dismiss],
  );
  const api = useMemo<ToastApi>(
    () => ({
      show,
      success: (title, message) => show({ tone: "success", title, message }),
      error: (title, message) => show({ tone: "error", title, message }),
      info: (title, message) => show({ tone: "info", title, message }),
    }),
    [show],
  );
  return (
    <Ctx.Provider value={api}>
      {children}
      <div className="pointer-events-none fixed bottom-4 left-1/2 z-[70] flex w-[min(420px,calc(100vw-2rem))] -translate-x-1/2 flex-col gap-2">
        {items.map((t) => {
          const Icon = ICONS[t.tone];
          return (
            <div
              key={t.id}
              role={t.tone === "error" ? "alert" : "status"}
              className="p-enter pointer-events-auto flex gap-3 rounded-lg border border-border bg-elevated px-3 py-2.5 shadow-overlay"
            >
              <Icon
                size={18}
                aria-hidden
                className={cn(
                  "mt-0.5 shrink-0",
                  t.tone === "success" ? "text-success" : t.tone === "error" ? "text-danger" : "text-accent",
                )}
              />
              <div className="min-w-0 flex-1">
                <p className="font-medium">{t.title}</p>
                {t.message && <p className="mt-0.5 text-sm break-words text-muted">{t.message}</p>}
                {t.action && (
                  <button
                    type="button"
                    className="mt-1 text-sm font-medium text-accent-fg hover:underline"
                    onClick={() => {
                      t.action?.onClick();
                      dismiss(t.id);
                    }}
                  >
                    {t.action.label}
                  </button>
                )}
              </div>
              <button
                type="button"
                aria-label="Fechar notificação"
                onClick={() => dismiss(t.id)}
                className="-mr-1 inline-flex size-6 shrink-0 items-center justify-center rounded text-muted hover:bg-hover hover:text-fg"
              >
                <X size={14} aria-hidden />
              </button>
            </div>
          );
        })}
      </div>
    </Ctx.Provider>
  );
}
