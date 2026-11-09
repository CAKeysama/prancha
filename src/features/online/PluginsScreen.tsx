import { useState } from "react";
import { Compass } from "lucide-react";
import { useLibraryVersion } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import { useSettings } from "../../app/settings-context";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/Dialog";
import { Segmented, Switch } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { RATING_STATE_HELP, SUSPECT_BEHAVIOR_OPTIONS, type SuspectBehavior } from "../../domain/rating";
import { api, errorMessage } from "../../services/api";

/** O que não é o leitor e a biblioteca fica aqui, para quem quiser. */
export function PluginsScreen() {
  const nav = useNav();
  const toast = useToast();
  const { settings, update } = useSettings();
  const { bump } = useLibraryVersion();
  const installed = settings["plugins.online"];
  const [removing, setRemoving] = useState(false);
  const [confirmingAdult, setConfirmingAdult] = useState(false);

  const install = async () => {
    await update("plugins.online", true);
    nav.navigate({ name: "explore" });
  };
  const remove = async () => {
    setRemoving(false);
    await update("plugins.online", false);
    await api.onlineStop().catch((e: unknown) => toast.error("Não foi possível parar as fontes", errorMessage(e)));
  };

  const setAdult = async (allowed: boolean) => {
    setConfirmingAdult(false);
    await update("content.adult", allowed);
    bump();
  };
  const setSuspect = async (behavior: SuspectBehavior) => {
    await update("content.suspect", behavior);
    bump();
  };
  const filterOn = !settings["content.adult"];

  return (
    <div className="h-full overflow-y-auto">
      <PageHeader title="Plugins" />
      <div className="mx-auto max-w-3xl px-6 py-6">
        <section aria-label="Fontes de leitura" className="flex items-start gap-4 rounded-lg border border-border bg-surface p-4">
          <Compass size={24} className="mt-0.5 shrink-0 text-muted" aria-hidden />
          <div className="min-w-0 flex-1">
            <h2 className="font-semibold">Fontes de leitura</h2>
            <p className="mt-0.5 text-sm text-muted">Procure obras em repositórios de extensões e leia capítulo por capítulo, sem baixar. Extensões são código de terceiros.</p>
          </div>
          {installed ? (
            <Button onClick={() => setRemoving(true)}>Remover</Button>
          ) : (
            <Button variant="primary" onClick={() => void install()}>
              Instalar
            </Button>
          )}
        </section>
        {installed && (
          <section aria-label="Conteúdo +18" className="mt-3 rounded-lg border border-border bg-surface p-4">
            <Switch
              label="Esconder conteúdo +18"
              description="Obras confirmadas como +18, e fontes e extensões adultas ou sem classificação, ficam de fora de todas as telas."
              checked={filterOn}
              onChange={(hide) => (hide ? void setAdult(false) : setConfirmingAdult(true))}
            />
            {filterOn && (
              <div className="mt-4 border-t border-border pt-4">
                <Segmented label="Obras suspeitas ou sem informações" value={settings["content.suspect"]} options={SUSPECT_BEHAVIOR_OPTIONS} onChange={(behavior) => void setSuspect(behavior)} size="sm" />
                <p className="mt-2 text-sm text-muted">
                  Suspeita: {RATING_STATE_HELP.suspect.toLowerCase()} Sem informações: {RATING_STATE_HELP.insufficient.toLowerCase()} Com aviso, a capa leva um selo discreto. A classificação de cada obra pode ser conferida e corrigida na página dela.
                </p>
              </div>
            )}
          </section>
        )}
      </div>
      <ConfirmDialog open={confirmingAdult} title="Mostrar conteúdo +18?" confirmLabel="Tenho 18 anos ou mais" onCancel={() => setConfirmingAdult(false)} onConfirm={() => void setAdult(true)}>
        O que é adulto, suspeito ou não tem classificação passa a aparecer. A classificação junta o que cada repositório informa com o que o Prancha deduz dos metadados, e pode falhar.
      </ConfirmDialog>
      <ConfirmDialog open={removing} title="Remover Fontes de leitura?" confirmLabel="Remover" destructive onCancel={() => setRemoving(false)} onConfirm={() => void remove()}>
        Explorar, Downloads, Extensões e Repositórios saem do menu. O que está na biblioteca, as extensões e o progresso ficam guardados e voltam se você instalar de novo.
      </ConfirmDialog>
    </div>
  );
}
