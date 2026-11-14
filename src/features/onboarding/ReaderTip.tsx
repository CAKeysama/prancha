import { useEffect, useId, useRef } from "react";
import { Button } from "../../components/Button";
import { Kbd } from "../../components/Feedback";
import { formatKey, type ShortcutMap } from "../../domain/shortcuts";
import { useOnboarding } from "./onboarding-context";

/**
 * Dica dos controles na primeira leitura depois da configuração inicial: cartão discreto no canto inferior,
 * acima da barra de navegação, sem cobrir a página. Aparece uma vez.
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
      data-reader-tip
      className="p-enter absolute bottom-16 left-4 z-30 w-[min(340px,calc(100vw-2rem))] rounded-lg border border-border bg-elevated shadow-overlay"
    >
      <div className="px-4 pt-3">
        <h2 id={titleId} className="text-base font-semibold">
          Controles do leitor
        </h2>
        <ul className="mt-2 space-y-1.5 text-sm text-muted">
          <li>
            <span className="text-fg">Página:</span> {keys("next")} {keys("prev")} ou os botões destacados
          </li>
          <li>
            <span className="text-fg">Zoom:</span> {keys("zoomIn")} {keys("zoomOut")}
          </li>
          <li>
            <span className="text-fg">Sair:</span> {keys("back")} ou Biblioteca, no alto. O progresso fica salvo.
          </li>
        </ul>
      </div>
      <div className="mt-3 flex justify-end px-4 pb-3">
        <Button variant="primary" data-primary onClick={onboarding.dismissReaderTip}>
          Entendi
        </Button>
      </div>
    </div>
  );
}
