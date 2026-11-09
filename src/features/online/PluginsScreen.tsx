import { useState } from "react";
import { Compass } from "lucide-react";
import { useNav } from "../../app/navigation";
import { useSettings } from "../../app/settings-context";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/Dialog";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { api, errorMessage } from "../../services/api";

/** O que não é o leitor e a biblioteca fica aqui, para quem quiser. */
export function PluginsScreen() {
  const nav = useNav();
  const toast = useToast();
  const { settings, update } = useSettings();
  const installed = settings["plugins.online"];
  const [removing, setRemoving] = useState(false);

  const install = async () => {
    await update("plugins.online", true);
    nav.navigate({ name: "explore" });
  };
  const remove = async () => {
    setRemoving(false);
    await update("plugins.online", false);
    await api.onlineStop().catch((e: unknown) => toast.error("Não foi possível parar as fontes", errorMessage(e)));
  };

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
      </div>
      <ConfirmDialog open={removing} title="Remover Fontes de leitura?" confirmLabel="Remover" destructive onCancel={() => setRemoving(false)} onConfirm={() => void remove()}>
        Explorar, Downloads, Extensões e Repositórios saem do menu. O que está na biblioteca, as extensões e o progresso ficam guardados e voltam se você instalar de novo.
      </ConfirmDialog>
    </div>
  );
}
