import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import { cn } from "./cn";
import { Spinner } from "./Feedback";
import { Tooltip } from "./Tooltip";

type Variant = "primary" | "secondary" | "danger" | "ghost";
type Size = "sm" | "md";

const VARIANTS: Record<Variant, string> = {
  primary:
    "bg-accent-emphasis text-on-emphasis border-transparent hover:brightness-110 active:brightness-95 disabled:opacity-60",
  secondary: "bg-surface text-fg border-border hover:bg-hover active:bg-active disabled:text-muted",
  danger:
    "bg-surface text-danger border-border hover:bg-danger-emphasis hover:text-on-emphasis hover:border-transparent disabled:opacity-60",
  ghost: "bg-transparent text-fg border-transparent hover:bg-hover active:bg-active disabled:text-muted",
};

const SIZES: Record<Size, string> = {
  sm: "h-7 px-2.5 text-sm gap-1.5",
  md: "h-8 px-3 text-base gap-2",
};

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  icon?: LucideIcon;
  trailingIcon?: LucideIcon;
  loading?: boolean;
  children?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", icon: Icon, trailingIcon: Trailing, loading, className, children, disabled, type, ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type ?? "button"}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-md border font-medium whitespace-nowrap",
        // Resposta no instante do clique (pointer-down), não só ao soltar.
        "transition-[background-color,color,border-color,filter,transform] duration-100 active:scale-[0.98] disabled:cursor-not-allowed disabled:active:scale-100",
        VARIANTS[variant],
        SIZES[size],
        className,
      )}
      {...rest}
    >
      {loading ? <Spinner size={14} /> : Icon ? <Icon size={size === "sm" ? 14 : 16} aria-hidden /> : null}
      {children}
      {Trailing ? <Trailing size={14} aria-hidden className="text-muted" /> : null}
    </button>
  );
});

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  icon: LucideIcon;
  /** Nome acessível e texto do tooltip. Obrigatório. */
  label: string;
  shortcut?: string;
  variant?: "ghost" | "secondary";
  size?: Size;
  pressed?: boolean;
  tooltipSide?: "top" | "bottom";
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { icon: Icon, label, shortcut, variant = "ghost", size = "md", pressed, className, tooltipSide = "bottom", type, ...rest },
  ref,
) {
  return (
    <Tooltip content={shortcut ? `${label} (${shortcut})` : label} side={tooltipSide}>
      <button
        ref={ref}
        type={type ?? "button"}
        aria-label={label}
        aria-pressed={pressed}
        className={cn(
          "inline-flex shrink-0 items-center justify-center rounded-md border text-muted hover:text-fg",
          "transition-colors duration-100 disabled:cursor-not-allowed disabled:opacity-50",
          size === "sm" ? "size-7" : "size-8",
          VARIANTS[variant],
          pressed && "bg-accent-subtle text-accent-fg hover:text-accent-fg",
          className,
        )}
        {...rest}
      >
        <Icon size={size === "sm" ? 15 : 17} aria-hidden />
      </button>
    </Tooltip>
  );
});
