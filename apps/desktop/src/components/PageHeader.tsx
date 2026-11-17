import { createContext, useContext, type ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { Button, IconButton } from "./Button";
import { useNav } from "../app/navigation";
import { useCompact } from "../app/platform";

/** No celular, o que a navegação põe no canto do cabeçalho das telas principais (o sino das notificações). */
export const HeaderAccessory = createContext<ReactNode>(null);

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
  const accessory = useContext(HeaderAccessory);
  const nav = useNav();
  const phone = useCompact();
  // No celular, uma tela aberta por cima de outra (de "Mais", do sino) ganha a seta de voltar, como no Android.
  const back = onBack ?? (phone && nav.canGoBack ? nav.back : undefined);
  return (
    <header className="sticky top-0 z-20 border-b border-border bg-bg/95 backdrop-blur-sm">
      <div className="flex min-h-14 flex-wrap items-center gap-x-4 gap-y-2 px-6 py-2.5 max-desk:gap-x-2 max-desk:px-4">
        {back &&
          (phone ? (
            <IconButton icon={ArrowLeft} label={backLabel} onClick={back} className="-ml-2" />
          ) : (
            <Button variant="ghost" size="sm" icon={ArrowLeft} onClick={back} className="-ml-2">
              {backLabel}
            </Button>
          ))}
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-xl font-semibold">{title}</h1>
          {subtitle && <p className="truncate text-sm text-muted">{subtitle}</p>}
        </div>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
        {accessory}
      </div>
      {children && <div className="px-6 pb-3 max-desk:px-4">{children}</div>}
    </header>
  );
}
