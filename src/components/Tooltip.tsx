import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "./cn";

interface TooltipProps {
  content: string;
  side?: "top" | "bottom";
  className?: string;
  children: ReactNode;
}

/**
 * Tooltip leve: aparece no hover (após um breve atraso) e no foco por teclado.
 * Não substitui o nome acessível do controle, que continua no próprio botão.
 */
export function Tooltip({ content, side = "bottom", className, children }: TooltipProps) {
  const [pos, setPos] = useState<{ x: number; y: number; side: "top" | "bottom" } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const wrapper = useRef<HTMLSpanElement>(null);

  const show = useCallback(
    (delay: number) => {
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => {
        const el = wrapper.current;
        if (!el) return;
        const r = el.getBoundingClientRect();
        const s = side === "bottom" && r.bottom + 36 > window.innerHeight ? "top" : side === "top" && r.top < 36 ? "bottom" : side;
        setPos({ x: r.left + r.width / 2, y: s === "bottom" ? r.bottom + 6 : r.top - 6, side: s });
      }, delay);
    },
    [side],
  );
  const hide = useCallback(() => {
    window.clearTimeout(timer.current);
    setPos(null);
  }, []);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  return (
    <span
      ref={wrapper}
      className={cn("inline-flex", className)}
      onMouseEnter={() => show(450)}
      onMouseLeave={hide}
      onMouseDown={hide}
      onFocus={(e) => {
        if (e.target instanceof Element && e.target.matches(":focus-visible")) show(0);
      }}
      onBlur={hide}
    >
      {children}
      {pos &&
        createPortal(
          <div
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
