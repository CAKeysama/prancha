import { ArrowUp } from "lucide-react";
import { cn } from "../../components/cn";
import { Tooltip } from "../../components/Tooltip";
import { useUpdates } from "./updates-context";

/** Aviso discreto no menu lateral. Fica enquanto houver versão nova e reabre a janela ao clicar. */
export function UpdateIndicator({ narrow }: { narrow: boolean }) {
  const { snapshot, openOffer } = useUpdates();
  const version = snapshot?.available?.version;
  if (!version) return null;
  const label = `v${version} disponível`;
  const button = (
    <button
      type="button"
      onClick={openOffer}
      aria-label={narrow ? label : undefined}
      className={cn("flex h-8 w-full items-center gap-3 rounded-md px-3 text-left text-sm font-medium text-accent-fg transition-colors hover:bg-hover", narrow && "justify-center px-0")}
    >
      <ArrowUp size={16} aria-hidden />
      {!narrow && <span className="truncate">{label}</span>}
    </button>
  );
  return narrow ? (
    <Tooltip content={label} side="bottom" className="flex w-full">
      {button}
    </Tooltip>
  ) : (
    button
  );
}
