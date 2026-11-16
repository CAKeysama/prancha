import { useSyncExternalStore } from "react";

/** O app Android usa esta mesma interface, no WebView do sistema. */
export const IS_ANDROID = typeof navigator !== "undefined" && /\bAndroid\b/.test(navigator.userAgent);

/** Onde os dados ficam guardados, nos textos: "neste computador" ou "neste celular". */
export const DEVICE = IS_ANDROID ? "celular" : "computador";

/**
 * Largura de celular. A janela do desktop não fica mais estreita que 720 px (tauri.conf.json), então o layout
 * compacto só aparece no celular; o mesmo valor é a variante `max-desk:` do CSS.
 */
const COMPACT_QUERY = "(max-width: 719.98px)";

function subscribe(onChange: () => void): () => void {
  if (typeof window === "undefined" || !window.matchMedia) return () => undefined;
  const media = window.matchMedia(COMPACT_QUERY);
  media.addEventListener("change", onChange);
  return () => media.removeEventListener("change", onChange);
}

export function isCompact(): boolean {
  return typeof window !== "undefined" && Boolean(window.matchMedia?.(COMPACT_QUERY).matches);
}

/** Tela de celular em pé: navegação inferior, folhas no lugar de menus, gestos no leitor. */
export function useCompact(): boolean {
  return useSyncExternalStore(subscribe, isCompact, () => false);
}
