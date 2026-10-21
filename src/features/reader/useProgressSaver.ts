import { useCallback, useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../../services/api";

/**
 * Persiste a posição de leitura sem gravar a cada evento: agrupa mudanças (debounce)
 * e força a gravação ao sair do leitor, trocar de HQ, ocultar ou fechar a janela.
 */
export function useProgressSaver(comicId: number, pageCount: number, onSaved?: (status: string) => void) {
  const pending = useRef<{ page: number; ratio: number } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const lastSaved = useRef<string>("");
  const onSavedRef = useRef(onSaved);
  useEffect(() => {
    onSavedRef.current = onSaved;
  });

  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    const p = pending.current;
    pending.current = null;
    if (!p || pageCount <= 0) return;
    const key = `${p.page}:${p.ratio.toFixed(3)}`;
    if (key === lastSaved.current) return;
    lastSaved.current = key;
    try {
      const status = await api.saveProgress(comicId, p.page, pageCount, p.ratio);
      onSavedRef.current?.(status);
    } catch {
      lastSaved.current = "";
    }
  }, [comicId, pageCount]);

  const schedule = useCallback(
    (page: number, ratio = 0, delay = 700) => {
      pending.current = { page, ratio };
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => void flush(), delay);
    },
    [flush],
  );

  useEffect(() => {
    const onHide = () => {
      if (document.visibilityState === "hidden") void flush();
    };
    const onUnload = () => void flush();
    document.addEventListener("visibilitychange", onHide);
    window.addEventListener("beforeunload", onUnload);
    let offClose: (() => void) | undefined;
    let cancelled = false;
    try {
      getCurrentWindow()
        .onCloseRequested(async () => {
          try {
            await flush();
          } catch {
            /* nunca impedir o fechamento */
          }
        })
        .then((fn) => {
          if (cancelled) fn();
          else offClose = fn;
        })
        .catch(() => undefined);
    } catch {
      /* fora do Tauri */
    }
    return () => {
      cancelled = true;
      document.removeEventListener("visibilitychange", onHide);
      window.removeEventListener("beforeunload", onUnload);
      offClose?.();
      void flush();
    };
  }, [flush]);

  return { schedule, flush };
}
