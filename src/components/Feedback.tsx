import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "./cn";

export function Spinner({ size = 16, label, className }: { size?: number; label?: string; className?: string }) {
  return (
    <span role={label ? "status" : undefined} className={cn("inline-flex items-center gap-2", className)}>
      <svg width={size} height={size} viewBox="0 0 16 16" className="p-spin shrink-0" aria-hidden>
        <circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="2" />
        <path d="M14.5 8A6.5 6.5 0 0 0 8 1.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      </svg>
      {label && <span className="text-sm text-muted">{label}</span>}
    </span>
  );
}

export function ProgressBar({
  value,
  label,
  className,
  tone = "accent",
}: {
  value: number;
  label: string;
  className?: string;
  tone?: "accent" | "success";
}) {
  const pct = Math.round(Math.min(1, Math.max(0, value)) * 100);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      className={cn("h-1 w-full overflow-hidden rounded-full bg-border", className)}
    >
      <div
        className={cn("h-full rounded-full", tone === "success" ? "bg-success" : "bg-accent")}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

export function EmptyState({
  icon: Icon,
  title,
  children,
  actions,
  className,
}: {
  icon: LucideIcon;
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("mx-auto flex max-w-md flex-col items-center px-6 py-16 text-center", className)}>
      <Icon size={28} className="mb-3 text-muted" aria-hidden />
      <h2 className="text-lg font-semibold">{title}</h2>
      {children && <div className="mt-1.5 text-base text-muted">{children}</div>}
      {actions && <div className="mt-5 flex flex-wrap items-center justify-center gap-2">{actions}</div>}
    </div>
  );
}

export function ErrorState({ title, message, action }: { title: string; message: string; action?: ReactNode }) {
  return (
    <div role="alert" className="mx-auto max-w-lg rounded-md border border-danger/40 bg-danger-subtle px-4 py-3">
      <p className="font-semibold text-danger">{title}</p>
      <p className="mt-1 text-sm text-fg">{message}</p>
      {action && <div className="mt-3">{action}</div>}
    </div>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex min-w-5 items-center justify-center rounded border border-border bg-surface px-1.5 font-mono text-xs leading-5 text-fg shadow-[inset_0_-1px_0_var(--color-border)]">
      {children}
    </kbd>
  );
}
