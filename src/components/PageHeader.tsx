import type { ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { Button } from "./Button";

/** Cabeçalho contextual das telas (fixo no topo da área de conteúdo). */
export function PageHeader({
  title,
  subtitle,
  actions,
  onBack,
  backLabel = "Voltar",
  children,
}: {
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
  onBack?: () => void;
  backLabel?: string;
  children?: ReactNode;
}) {
  return (
    <header className="sticky top-0 z-20 border-b border-border bg-bg/95 backdrop-blur-sm">
      <div className="flex min-h-14 flex-wrap items-center gap-x-4 gap-y-2 px-6 py-2.5">
        {onBack && (
          <Button variant="ghost" size="sm" icon={ArrowLeft} onClick={onBack} className="-ml-2">
            {backLabel}
          </Button>
        )}
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-xl font-semibold">{title}</h1>
          {subtitle && <p className="truncate text-sm text-muted">{subtitle}</p>}
        </div>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {children && <div className="px-6 pb-3">{children}</div>}
    </header>
  );
}
