import { useLayoutEffect, useState } from "react";

// Onde cada tela estava rolada, enquanto o app está aberto. A tela que volta abre no mesmo ponto.
const positions = new Map<string, number>();

/** Por quanto tempo a tela que volta insiste em chegar ao ponto, enquanto o conteúdo termina de crescer. */
const RESTORE_WINDOW_MS = 1500;

/**
 * Guarda e devolve a rolagem do elemento com `ref`, por `key`. Uma chave nova começa do topo. Se o conteúdo ainda
 * não tem altura para chegar ao ponto guardado, tenta de novo quando ele cresce, até a pessoa rolar ou o tempo acabar.
 */
export function useScrollMemory<T extends HTMLElement>(key: string): (element: T | null) => void {
  // Uma ref de função: a tela pode desenhar o elemento rolável só depois de carregar.
  const [element, setElement] = useState<T | null>(null);
  useLayoutEffect(() => {
    if (!element) return;
    const target = positions.get(key) ?? 0;
    let restoring = target > 0;
    const apply = () => {
      if (!restoring) return;
      element.scrollTo(0, target);
      if (Math.abs(element.scrollTop - target) < 2) restoring = false;
    };
    const stop = () => {
      restoring = false;
    };
    if (restoring) apply();
    else element.scrollTo(0, 0);
    const observer = new ResizeObserver(apply);
    if (element.firstElementChild) observer.observe(element.firstElementChild);
    for (const child of element.children) observer.observe(child);
    const timer = window.setTimeout(stop, RESTORE_WINDOW_MS);
    // Enquanto tenta chegar ao ponto, a rolagem que o navegador corta não vale como a posição da pessoa.
    const save = () => {
      if (!restoring) positions.set(key, element.scrollTop);
    };
    element.addEventListener("scroll", save, { passive: true });
    element.addEventListener("wheel", stop, { passive: true });
    element.addEventListener("touchstart", stop, { passive: true });
    element.addEventListener("keydown", stop);
    element.addEventListener("pointerdown", stop);
    return () => {
      save();
      observer.disconnect();
      window.clearTimeout(timer);
      element.removeEventListener("scroll", save);
      element.removeEventListener("wheel", stop);
      element.removeEventListener("touchstart", stop);
      element.removeEventListener("keydown", stop);
      element.removeEventListener("pointerdown", stop);
    };
  }, [key, element]);
  return setElement;
}

/** Esquece as posições. Para os testes. */
export function clearScrollMemory() {
  positions.clear();
}
