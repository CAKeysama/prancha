import { useEffect, useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { Button, IconButton } from "./Button";
import { cn } from "./cn";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export interface DialogProps {
  open: boolean;
  title: string;
  description?: ReactNode;
  onClose: () => void;
  /** Diálogos de confirmação destrutiva podem impedir fechamento acidental. */
  dismissible?: boolean;
  children?: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg";
  initialFocus?: string;
}

/** Diálogo modal acessível: foco preso, Esc fecha (quando permitido), foco volta ao gatilho. */
export function Dialog({ open, title, description, onClose, dismissible = true, children, footer, size = "md", initialFocus }: DialogProps) {
  const titleId = useId();
  const descId = useId();
  const panel = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  useEffect(() => {
    onCloseRef.current = onClose;
  });

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    const node = panel.current;
    const first =
      (initialFocus ? node?.querySelector<HTMLElement>(initialFocus) : null) ??
      node?.querySelector<HTMLElement>("[data-autofocus]") ??
      node?.querySelector<HTMLElement>(FOCUSABLE);
    first?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && dismissible) {
        e.stopPropagation();
        onCloseRef.current();
      }
      if (e.key === "Tab" && node) {
        const items = Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => el.offsetParent !== null);
        if (items.length === 0) return;
        const firstEl = items[0]!;
        const lastEl = items[items.length - 1]!;
        if (e.shiftKey && document.activeElement === firstEl) {
          e.preventDefault();
          lastEl.focus();
        } else if (!e.shiftKey && document.activeElement === lastEl) {
          e.preventDefault();
          firstEl.focus();
        }
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      previous?.focus?.();
    };
  }, [open, dismissible, initialFocus]);

  if (!open) return null;
  return createPortal(
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-overlay px-4 py-[8vh]">
      <div
        className="fixed inset-0"
        aria-hidden
        onMouseDown={() => {
          if (dismissible) onClose();
        }}
      />
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descId : undefined}
        className={cn(
          "p-enter relative w-full rounded-lg border border-border bg-elevated shadow-overlay",
          size === "sm" ? "max-w-md" : size === "lg" ? "max-w-3xl" : "max-w-xl",
        )}
      >
        <div className="flex items-start gap-3 border-b border-border px-4 py-3">
          <div className="min-w-0 flex-1">
            <h2 id={titleId} className="text-lg font-semibold break-words">
              {title}
            </h2>
            {description && (
              <div id={descId} className="mt-0.5 text-sm text-muted">
                {description}
              </div>
            )}
          </div>
          {dismissible && <IconButton icon={X} label="Fechar" onClick={onClose} size="sm" />}
        </div>
        {children && <div className="max-h-[65vh] overflow-y-auto px-4 py-4">{children}</div>}
        {footer && <div className="flex flex-wrap justify-end gap-2 border-t border-border px-4 py-3">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  children?: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  destructive?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  open,
  title,
  children,
  confirmLabel,
  cancelLabel = "Cancelar",
  destructive,
  busy,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Dialog
      open={open}
      title={title}
      onClose={onCancel}
      size="sm"
      footer={
        <>
          <Button onClick={onCancel} data-autofocus={destructive ? true : undefined}>
            {cancelLabel}
          </Button>
          <Button variant={destructive ? "danger" : "primary"} onClick={onConfirm} loading={busy}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      <div className="text-base text-fg">{children}</div>
    </Dialog>
  );
}
