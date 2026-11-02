import { forwardRef, useId, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes, type TextareaHTMLAttributes } from "react";
import { Check } from "lucide-react";
import { cn } from "./cn";

const CONTROL =
  "w-full rounded-md border border-border-control bg-bg px-2.5 text-base text-fg placeholder:text-muted " +
  "focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-0 focus-visible:border-accent " +
  "disabled:cursor-not-allowed disabled:opacity-60";

interface FieldProps {
  label: string;
  hint?: ReactNode;
  error?: string | null;
  badge?: ReactNode;
}

export const TextField = forwardRef<HTMLInputElement, FieldProps & InputHTMLAttributes<HTMLInputElement>>(
  function TextField({ label, hint, error, badge, className, id, ...rest }, ref) {
    const auto = useId();
    const inputId = id ?? auto;
    return (
      <div className={className}>
        <label htmlFor={inputId} className="mb-1 flex items-center gap-2 text-sm font-medium">
          {label}
          {badge}
        </label>
        <input
          ref={ref}
          id={inputId}
          aria-invalid={error ? true : undefined}
          aria-describedby={hint || error ? `${inputId}-hint` : undefined}
          className={cn(CONTROL, "h-8", error && "border-danger")}
          {...rest}
        />
        {(error || hint) && (
          <p id={`${inputId}-hint`} className={cn("mt-1 text-xs", error ? "text-danger" : "text-muted")}>
            {error ?? hint}
          </p>
        )}
      </div>
    );
  },
);

export function TextArea({ label, hint, badge, className, id, ...rest }: FieldProps & TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const auto = useId();
  const inputId = id ?? auto;
  return (
    <div className={className}>
      <label htmlFor={inputId} className="mb-1 flex items-center gap-2 text-sm font-medium">
        {label}
        {badge}
      </label>
      <textarea id={inputId} className={cn(CONTROL, "min-h-20 py-1.5")} {...rest} />
      {hint && <p className="mt-1 text-xs text-muted">{hint}</p>}
    </div>
  );
}

export function Select({
  label,
  hint,
  className,
  id,
  children,
  hideLabel,
  ...rest
}: FieldProps & SelectHTMLAttributes<HTMLSelectElement> & { hideLabel?: boolean }) {
  const auto = useId();
  const inputId = id ?? auto;
  return (
    <div className={className}>
      <label htmlFor={inputId} className={hideLabel ? "sr-only" : "mb-1 block text-sm font-medium"}>
        {label}
      </label>
      <select id={inputId} className={cn(CONTROL, "h-8 pr-7")} {...rest}>
        {children}
      </select>
      {hint && <p className="mt-1 text-xs text-muted">{hint}</p>}
    </div>
  );
}

export function Checkbox({
  label,
  description,
  checked,
  onChange,
  disabled,
  hideLabel,
}: {
  label: string;
  description?: ReactNode;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  hideLabel?: boolean;
}) {
  const id = useId();
  return (
    <div className="flex items-start gap-2.5">
      <span className="relative mt-0.5 inline-flex size-4 shrink-0">
        <input
          id={id}
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={(e) => onChange(e.target.checked)}
          aria-describedby={description ? `${id}-d` : undefined}
          className="peer size-4 appearance-none rounded-sm border border-border-control bg-bg checked:border-accent-emphasis checked:bg-accent-emphasis focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent disabled:opacity-50"
        />
        <Check
          size={12}
          strokeWidth={3}
          aria-hidden
          className="pointer-events-none absolute top-0.5 left-0.5 hidden text-on-emphasis peer-checked:block"
        />
      </span>
      <label htmlFor={id} className={hideLabel ? "sr-only" : "text-base leading-5"}>
        {label}
        {description && (
          <span id={`${id}-d`} className="mt-0.5 block text-sm text-muted">
            {description}
          </span>
        )}
      </label>
    </div>
  );
}

interface SegmentOption<T extends string> {
  value: T;
  label: string;
  icon?: React.ComponentType<{ size?: number; "aria-hidden"?: boolean }>;
}

/** Grupo de opções exclusivas (radiogroup) com aparência de controle segmentado. */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  hideLabel,
  size = "md",
  compactLabels,
}: {
  label: string;
  value: T;
  options: SegmentOption<T>[];
  onChange: (v: T) => void;
  hideLabel?: boolean;
  size?: "sm" | "md";
  /** Em janelas estreitas, mostra só os ícones (o texto continua para leitores de tela). */
  compactLabels?: boolean;
}) {
  const id = useId();
  return (
    <div>
      <span id={id} className={hideLabel ? "sr-only" : "mb-1 block text-sm font-medium"}>
        {label}
      </span>
      <div
        role="radiogroup"
        aria-labelledby={id}
        className={cn("inline-flex rounded-md border border-border bg-surface p-0.5", compactLabels ? "flex-nowrap" : "flex-wrap")}
        onKeyDown={(e) => {
          const i = options.findIndex((o) => o.value === value);
          let next = -1;
          if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (i + 1) % options.length;
          if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (i - 1 + options.length) % options.length;
          if (next >= 0) {
            e.preventDefault();
            onChange(options[next]!.value);
            const buttons = (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[role="radio"]');
            buttons[next]?.focus();
          }
        }}
      >
        {options.map((o) => {
          const selected = o.value === value;
          const Icon = o.icon;
          return (
            <button
              key={o.value}
              type="button"
              role="radio"
              aria-checked={selected}
              tabIndex={selected ? 0 : -1}
              onClick={() => onChange(o.value)}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-[5px] px-2.5 font-medium transition-colors",
                size === "sm" ? "h-6 text-sm" : "h-7 text-base",
                selected ? "bg-elevated text-fg shadow-[0_0_0_1px_var(--color-border)]" : "text-muted hover:text-fg",
              )}
            >
              {Icon && <Icon size={14} aria-hidden />}
              <span className={compactLabels && Icon ? "max-md:sr-only" : undefined}>{o.label}</span>
              {selected && <span className="sr-only"> (selecionado)</span>}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function Switch({
  label,
  description,
  checked,
  onChange,
}: {
  label: string;
  description?: ReactNode;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  const id = useId();
  return (
    <div className="flex items-start justify-between gap-4">
      <div>
        <label htmlFor={id} className="text-base font-medium">
          {label}
        </label>
        {description && (
          <p id={`${id}-d`} className="text-sm text-muted">
            {description}
          </p>
        )}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-describedby={description ? `${id}-d` : undefined}
        onClick={() => onChange(!checked)}
        className={cn(
          "relative mt-0.5 inline-flex h-5 w-9 shrink-0 items-center rounded-full border transition-colors",
          checked ? "border-transparent bg-accent-emphasis" : "border-border-control bg-surface",
        )}
      >
        <span
          className={cn(
            "inline-block size-3.5 rounded-full transition-transform",
            checked ? "translate-x-4.5 bg-on-emphasis" : "translate-x-0.5 bg-muted",
          )}
        />
        <span className="sr-only">{checked ? "Ativado" : "Desativado"}</span>
      </button>
    </div>
  );
}
