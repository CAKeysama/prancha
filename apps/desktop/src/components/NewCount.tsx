import { cn } from "./cn";

/** Quantos capítulos novos a obra tem. Só aparece acima de zero, e só com o número. */
export function NewCount({ count, className }: { count: number; className?: string }) {
  if (count <= 0) return null;
  return (
    <span
      className={cn("inline-flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-accent-emphasis px-1.5 text-[11px] leading-none font-semibold text-on-emphasis tabular-nums", className)}
      data-testid="new-count"
      title={count === 1 ? "1 capítulo novo" : `${count} capítulos novos`}
    >
      <span aria-hidden>{count > 99 ? "99+" : count}</span>
      <span className="sr-only">{count === 1 ? "1 capítulo novo" : `${count} capítulos novos`}</span>
    </span>
  );
}
