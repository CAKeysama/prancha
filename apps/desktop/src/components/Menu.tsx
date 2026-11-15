import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { LucideIcon } from "lucide-react";
import { cn } from "./cn";

export type MenuEntry =
  | {
      kind?: "item";
      label: string;
      icon?: LucideIcon;
      onSelect: () => void;
      danger?: boolean;
      disabled?: boolean;
      hint?: string;
    }
  | { kind: "separator" };

interface MenuProps {
  /** Renderiza o botão que abre o menu; recebe as props de acessibilidade. */
  trigger: (props: {
    ref: (el: HTMLButtonElement | null) => void;
    onClick: () => void;
    "aria-haspopup": "menu";
    "aria-expanded": boolean;
    "aria-controls": string | undefined;
  }) => ReactNode;
  items: MenuEntry[];
  label: string;
  align?: "start" | "end";
}

/** Remove separadores no início, no fim e repetidos. */
function cleanSeparators(items: MenuEntry[]): MenuEntry[] {
  const out: MenuEntry[] = [];
  for (const item of items) {
    if (item.kind === "separator" && (out.length === 0 || out[out.length - 1]!.kind === "separator")) continue;
    out.push(item);
  }
  while (out.length && out[out.length - 1]!.kind === "separator") out.pop();
  return out;
}

/** Menu suspenso acessível (setas, Home/End, Esc, clique fora). */
export function Menu({ trigger, items: rawItems, label, align = "end" }: MenuProps) {
  const items = cleanSeparators(rawItems);
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number; up: boolean } | null>(null);
  const [triggerNode, setTriggerNode] = useState<HTMLButtonElement | null>(null);
  const menuEl = useRef<HTMLDivElement>(null);
  const id = useId();

  const close = useCallback(
    (refocus = true) => {
      setOpen(false);
      if (refocus) triggerNode?.focus();
    },
    [triggerNode],
  );

  /** Posição calculada no clique, a partir do botão que abriu o menu. */
  const toggle = () => {
    if (open || !triggerNode) {
      setOpen(false);
      return;
    }
    const r = triggerNode.getBoundingClientRect();
    const height = Math.min(items.length * 34 + 12, 420);
    const up = r.bottom + height > window.innerHeight - 8 && r.top > height;
    const width = 240;
    const left = align === "end" ? Math.max(8, r.right - width) : Math.min(r.left, window.innerWidth - width - 8);
    setPos({ left, top: up ? r.top - 4 : r.bottom + 4, up });
    setOpen(true);
  };

  useEffect(() => {
    if (!open) return;
    const first = menuEl.current?.querySelector<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])');
    first?.focus();
    const onDown = (e: MouseEvent) => {
      if (!menuEl.current?.contains(e.target as Node) && !triggerNode?.contains(e.target as Node)) close(false);
    };
    const onScroll = () => close(false);
    document.addEventListener("mousedown", onDown);
    window.addEventListener("resize", onScroll);
    return () => {
      document.removeEventListener("mousedown", onDown);
      window.removeEventListener("resize", onScroll);
    };
  }, [open, close, triggerNode]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    const nodes = Array.from(menuEl.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])') ?? []);
    const idx = nodes.indexOf(document.activeElement as HTMLElement);
    const focus = (i: number) => nodes[(i + nodes.length) % nodes.length]?.focus();
    if (e.key === "ArrowDown") focus(idx + 1);
    else if (e.key === "ArrowUp") focus(idx - 1);
    else if (e.key === "Home") focus(0);
    else if (e.key === "End") focus(nodes.length - 1);
    else if (e.key === "Escape") close();
    else if (e.key === "Tab") close(false);
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  return (
    <>
      {trigger({
        ref: setTriggerNode,
        onClick: toggle,
        "aria-haspopup": "menu",
        "aria-expanded": open,
        "aria-controls": open ? id : undefined,
      })}
      {open &&
        pos &&
        createPortal(
          <div
            ref={menuEl}
            id={id}
            role="menu"
            aria-label={label}
            onKeyDown={onKeyDown}
            className="p-enter fixed z-[60] w-60 overflow-y-auto rounded-lg border border-border bg-elevated py-1.5 shadow-overlay"
            style={{
              left: pos.left,
              top: pos.top,
              transform: pos.up ? "translateY(-100%)" : undefined,
              maxHeight: 420,
            }}
          >
            {items.map((item, i) =>
              item.kind === "separator" ? (
                <div key={`sep-${i}`} role="separator" className="my-1.5 h-px bg-border" />
              ) : (
                <button
                  key={item.label}
                  type="button"
                  role="menuitem"
                  aria-disabled={item.disabled || undefined}
                  tabIndex={-1}
                  onClick={() => {
                    if (item.disabled) return;
                    close();
                    item.onSelect();
                  }}
                  className={cn(
                    "flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-base outline-none",
                    "focus-visible:bg-hover hover:bg-hover focus:bg-hover",
                    item.danger ? "text-danger" : "text-fg",
                    item.disabled && "cursor-not-allowed opacity-50",
                  )}
                >
                  {item.icon && <item.icon size={16} aria-hidden className={item.danger ? "" : "text-muted"} />}
                  <span className="flex-1 truncate">{item.label}</span>
                  {item.hint && <span className="text-xs text-muted">{item.hint}</span>}
                </button>
              ),
            )}
          </div>,
          document.body,
        )}
    </>
  );
}
