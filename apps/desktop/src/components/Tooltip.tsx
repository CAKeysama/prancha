import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "./cn";

/** Atraso dos botões só com ícone das ações de capítulo: o tooltip explica, mas não aparece antes de a pessoa parar ali. */
export const TOOLTIP_DELAY_MS = 2000;

/** Atraso padrão dos demais tooltips do app. */
const DEFAULT_HOVER_DELAY_MS = 450;

/**
 * Um temporizador para o app inteiro: só um tooltip pode estar esperando para aparecer. Passar o cursor por uma
 * fileira de ícones não cria um temporizador por botão, e entrar num novo cancela o anterior.
 */
const pending: { timer: number | undefined; owner: symbol | null } = { timer: undefined, owner: null };

function schedule(owner: symbol, delay: number, run: () => void) {
  window.clearTimeout(pending.timer);
  pending.owner = owner;
  pending.timer = window.setTimeout(() => {
    pending.owner = null;
    pending.timer = undefined;
    run();
  }, delay);
}

function cancel(owner: symbol) {
  if (pending.owner !== owner) return;
  window.clearTimeout(pending.timer);
  pending.owner = null;
  pending.timer = undefined;
}

interface TooltipProps {
  content: string;
  side?: "top" | "bottom";
  /** Espera antes de aparecer, no hover e no foco por teclado. Sem ele, o hover usa o padrão do app e o foco mostra na hora. */
  delay?: number;
  className?: string;
  children: ReactNode;
}

/**
 * Tooltip leve: aparece no hover (após o atraso) e no foco por teclado, e some ao sair.
 * Não substitui o nome acessível do controle, que continua no próprio botão (`aria-label`), e por isso não é
 * anunciado de novo por leitores de tela. Toque não abre tooltip: em telas sem hover, o nome acessível basta.
 */
export function Tooltip({ content, side = "bottom", delay, className, children }: TooltipProps) {
  const [pos, setPos] = useState<{ x: number; y: number; side: "top" | "bottom" } | null>(null);
  const [owner] = useState(() => Symbol("tooltip"));
  const wrapper = useRef<HTMLSpanElement>(null);
  const id = useId();

  const show = useCallback(
    (wait: number) => {
      schedule(owner, wait, () => {
        const el = wrapper.current;
        if (!el || !el.isConnected) return;
        const r = el.getBoundingClientRect();
        const s = side === "bottom" && r.bottom + 36 > window.innerHeight ? "top" : side === "top" && r.top < 36 ? "bottom" : side;
        setPos({ x: r.left + r.width / 2, y: s === "bottom" ? r.bottom + 6 : r.top - 6, side: s });
      });
    },
    [side, owner],
  );
  const hide = useCallback(() => {
    cancel(owner);
    setPos(null);
  }, [owner]);
  useEffect(() => () => cancel(owner), [owner]);
  useEffect(() => {
    if (!pos) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setPos(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pos]);

  return (
    <span
      ref={wrapper}
      className={cn("inline-flex", className)}
      onPointerEnter={(e) => {
        if (e.pointerType === "mouse" || e.pointerType === "pen") show(delay ?? DEFAULT_HOVER_DELAY_MS);
      }}
      onPointerLeave={hide}
      onPointerDown={hide}
      onFocus={(e) => {
        if (e.target instanceof Element && e.target.matches(":focus-visible")) show(delay ?? 0);
      }}
      onBlur={hide}
    >
      {children}
      {pos &&
        createPortal(
          <div
            id={id}
            role="tooltip"
            className="pointer-events-none fixed z-[100] max-w-xs rounded-md bg-fg px-2 py-1 text-xs font-medium text-bg shadow-overlay"
            style={{
              left: Math.min(Math.max(pos.x, 80), window.innerWidth - 80),
              top: pos.y,
              transform: `translate(-50%, ${pos.side === "top" ? "-100%" : "0"})`,
            }}
          >
            {content}
          </div>,
          document.body,
        )}
    </span>
  );
}
