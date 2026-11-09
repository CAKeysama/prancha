import { useEffect, useState } from "react";
import { ChartNoAxesColumn } from "lucide-react";
import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../services/api";
import { useNav } from "../../app/navigation";
import { useOnboarding } from "../onboarding/onboarding-context";
import { useTelemetry } from "./telemetry-context";
import { TelemetryDetails } from "./TelemetryDetails";

/** Espera a pessoa chegar ao app antes de perguntar, e nunca por cima de outro diálogo. */
export const INVITE_DELAY_MS = 4000;

/**
 * O primeiro contato com a telemetria: um convite, uma vez só, depois do tutorial e fora do leitor.
 * Fechar sem escolher não decide nada e o convite volta na próxima abertura; "Agora não" grava a recusa.
 */
export function TelemetryInvite() {
  const { status, setConsent } = useTelemetry();
  const { state: onboarding, wizardVisible, readerTipVisible } = useOnboarding();
  const { route } = useNav();
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [busy, setBusy] = useState(false);

  const eligible =
    !dismissed &&
    status?.consent === "unset" &&
    status.endpoint &&
    onboarding.status !== "active" &&
    !wizardVisible &&
    !readerTipVisible &&
    route.name !== "reader" &&
    route.name !== "chapter";

  useEffect(() => {
    if (!eligible || open) return;
    const timer = window.setTimeout(() => {
      if (!document.querySelector("[role='dialog']")) setOpen(true);
    }, INVITE_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [eligible, open]);

  // Nunca por cima do leitor: abrir uma HQ esconde o convite, que volta ao sair dela.
  const visible = open && route.name !== "reader" && route.name !== "chapter";

  const decide = async (granted: boolean) => {
    setBusy(true);
    try {
      await setConsent(granted);
      setOpen(false);
      setDismissed(true);
      if (granted) toast.success("Obrigado!", "Dá para desligar quando quiser em Configurações › Privacidade.");
    } catch (e) {
      toast.error("Não foi possível salvar a escolha", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={visible}
      title="Ajudar a melhorar o Prancha?"
      icon={<ChartNoAxesColumn size={20} aria-hidden />}
      onClose={() => {
        setOpen(false);
        setDismissed(true);
      }}
      size="lg"
      footer={
        <>
          <Button onClick={() => void decide(false)} disabled={busy}>
            Agora não
          </Button>
          <Button variant="primary" onClick={() => void decide(true)} loading={busy}>
            Enviar estatísticas de uso
          </Button>
        </>
      }
    >
      <TelemetryDetails compact />
      <p className="mt-3 text-sm text-muted">Fica desligado até você aceitar. Você muda de ideia quando quiser em Configurações › Privacidade.</p>
    </Dialog>
  );
}
