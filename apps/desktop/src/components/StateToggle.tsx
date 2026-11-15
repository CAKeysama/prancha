import { useState } from "react";
import type { LucideIcon } from "lucide-react";
import { Button } from "./Button";
import { cn } from "./cn";

interface StateToggleProps {
  icon: LucideIcon;
  active: boolean;
  /** Texto com o estado: "Favorito" / "Favoritar", "Acompanhando" / "Acompanhar". */
  activeText: string;
  inactiveText: string;
  /** `pop` cresce e volta (estrela). `ring` balança (sino). */
  motion: "pop" | "ring";
  onToggle: () => void;
  disabled?: boolean;
  testId?: string;
}

/**
 * Botão de estado com texto (favorito, acompanhamento). Ativo, o ícone fica preenchido em amarelo.
 * A animação roda uma vez, só quando a pessoa liga; desligar só troca a cor, com transição.
 * Com `prefers-reduced-motion`, nada se mexe (ver index.css).
 */
export function StateToggle({ icon, active, activeText, inactiveText, motion, onToggle, disabled, testId }: StateToggleProps) {
  const [animating, setAnimating] = useState(false);
  return (
    <Button
      icon={icon}
      aria-pressed={active}
      data-testid={testId}
      data-active={active || undefined}
      disabled={disabled}
      iconFilled={active}
      iconClassName={cn("transition-colors duration-200", active ? "text-attention" : "text-muted", animating && (motion === "pop" ? "p-pop" : "p-ring"))}
      onIconAnimationEnd={() => setAnimating(false)}
      onClick={() => {
        if (!active) setAnimating(true);
        onToggle();
      }}
    >
      {active ? activeText : inactiveText}
    </Button>
  );
}
