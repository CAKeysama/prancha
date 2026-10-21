import { useEffect, useId, useRef } from "react";
import { Button } from "../../components/Button";
import { Kbd } from "../../components/Feedback";
import { formatKey, type ShortcutMap } from "../../domain/shortcuts";
import { useOnboarding } from "./onboarding-context";

/**
 * Etapa 4 do tutorial: cartão discreto no canto inferior, acima da barra de navegação,
 * sem cobrir a página. Os controles reais citados ficam destacados.
 */
export function ReaderTip({ shortcuts }: { shortcuts: ShortcutMap }) {
  const onboarding = useOnboarding();
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("[data-primary]")?.focus();
  }, []);
  const keys = (a: keyof ShortcutMap) => shortcuts[a].slice(0, 2).map((k) => <Kbd key={k}>{formatKey(k)}</Kbd>);
  return (
    <div
      ref={ref}
      role="dialog"
      aria-labelledby={titleId}
      data-onboarding-step={4}
      className="p-enter absolute bottom-16 left-4 z-30 w-[min(360px,calc(100vw-2rem))] rounded-lg border border-accent/60 bg-elevated shadow-overlay"
    >
      <div className="px-4 pt-3">
        <p className="text-xs font-medium text-muted">Etapa 4 de 4</p>
        <h2 id={titleId} className="mt-1 text-lg font-semibold">
          Fazendo a primeira leitura
        </h2>
        <ul className="mt-2 space-y-2 text-sm">
          <li>
            <span className="font-medium">Avançar e voltar:</span> {keys("next")} / {keys("prev")} ou os botões <span className="font-medium">Próxima página</span> e{" "}
            <span className="font-medium">Página anterior</span> (destacados).
          </li>
          <li>
            <span className="font-medium">Zoom:</span> {keys("zoomIn")} / {keys("zoomOut")} ou os botões de zoom na barra superior.
          </li>
          <li>
            <span className="font-medium">Sair do leitor:</span> botão <span className="font-medium">Biblioteca</span> no canto superior esquerdo ou {keys("back")}. Seu
            progresso é salvo automaticamente.
          </li>
        </ul>
      </div>
      <div className="mt-3 flex justify-end border-t border-border px-4 py-2.5">
        <Button variant="primary" data-primary onClick={onboarding.complete}>
          Entendi
        </Button>
      </div>
    </div>
  );
}
