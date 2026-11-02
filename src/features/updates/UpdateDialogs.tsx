import { useEffect, useState } from "react";
import { ExternalLink } from "lucide-react";
import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { ProgressBar, Spinner } from "../../components/Feedback";
import { Logo } from "../../components/Logo";
import { useToast } from "../../components/Toast";
import { formatBytes } from "../../domain/progress";
import { parseNotes, shortSummary, type ParsedNotes, type ReleaseInfo, type UpdateProgress } from "../../domain/updates";
import { api, errorMessage } from "../../services/api";
import { useUpdates } from "./updates-context";

export function ReleaseNotes({ notes }: { notes: ParsedNotes }) {
  return (
    <div className="space-y-4">
      {notes.summary && <p className="text-base">{notes.summary}</p>}
      {notes.sections.map((section) => (
        <section key={section.title}>
          <h3 className="font-semibold">{section.title}</h3>
          <ul className="mt-1 list-disc space-y-1 pl-5 text-sm text-muted">
            {section.items.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

export function GithubReleaseButton({ version }: { version: string }) {
  const toast = useToast();
  return (
    <Button size="sm" icon={ExternalLink} onClick={() => void api.updatesOpenRelease(version).catch((e) => toast.error("Não foi possível abrir o navegador", errorMessage(e)))}>
      Ver no GitHub
    </Button>
  );
}

function OfferDialog() {
  const { snapshot, offerOpen, later, install } = useUpdates();
  const available = snapshot?.available;
  const summary = available ? shortSummary(parseNotes(available.notes)) : "";
  return (
    <Dialog
      open={offerOpen}
      title="Uma nova versão do Prancha está disponível!"
      onClose={later}
      size="sm"
      footer={
        <>
          <Button onClick={later}>Depois</Button>
          <Button variant="primary" onClick={install} data-autofocus>
            Atualizar agora
          </Button>
        </>
      }
    >
      <div className="space-y-3 text-base">
        <dl className="text-sm">
          <div className="flex gap-1.5">
            <dt className="text-muted">Versão instalada:</dt>
            <dd className="font-mono">v{snapshot?.currentVersion}</dd>
          </div>
          <div className="flex gap-1.5">
            <dt className="text-muted">Nova versão:</dt>
            <dd className="font-mono font-semibold">v{available?.version}</dd>
          </div>
        </dl>
        <p>Preparamos melhorias e novidades para sua experiência de leitura.</p>
        {summary && <p className="text-sm text-muted">{summary}</p>}
        {snapshot?.needsAdmin && <p className="text-sm text-muted">Este pacote instala para todos os usuários: o Windows vai pedir sua confirmação.</p>}
      </div>
    </Dialog>
  );
}

const PHASE_TITLES: Record<UpdateProgress["phase"], string> = {
  downloading: "Baixando a atualização",
  verifying: "Conferindo o arquivo",
  installing: "Preparando a instalação",
};

function ProgressDialog() {
  const { stage, cancelDownload } = useUpdates();
  if (stage.name !== "progress") return null;
  const { progress } = stage;
  const fraction = progress.phase === "downloading" && progress.total ? progress.downloaded / progress.total : null;
  return (
    <Dialog
      open
      dismissible={false}
      title={PHASE_TITLES[progress.phase]}
      onClose={() => undefined}
      size="sm"
      footer={progress.phase === "downloading" ? <Button onClick={cancelDownload}>Cancelar</Button> : undefined}
    >
      <div className="space-y-3">
        {progress.phase === "downloading" ? (
          <>
            <ProgressBar value={fraction ?? 0} label="Download da atualização" />
            <p role="status" className="font-mono text-sm">
              {fraction === null ? formatBytes(progress.downloaded) : `${Math.round(fraction * 100)}%`}
            </p>
          </>
        ) : (
          <Spinner label={progress.phase === "verifying" ? "Verificando assinatura e integridade…" : "Abrindo o instalador…"} />
        )}
        <p className="text-sm text-muted">Ao terminar, o Prancha fecha e abre de novo sozinho. Sua biblioteca e seu progresso continuam como estão.</p>
      </div>
    </Dialog>
  );
}

function FailedDialog() {
  const { stage, failedOpen, later, install } = useUpdates();
  return (
    <Dialog
      open={failedOpen}
      title="Não foi possível atualizar"
      onClose={later}
      size="sm"
      footer={
        <>
          <Button onClick={later}>Depois</Button>
          <Button variant="primary" onClick={install} data-autofocus>
            Tentar novamente
          </Button>
        </>
      }
    >
      <p role="alert" className="text-base">
        {stage.name === "failed" ? stage.message : ""}
      </p>
    </Dialog>
  );
}

function WelcomeDialog() {
  const { snapshot, welcomeOpen, closeWelcome } = useUpdates();
  const welcome = snapshot?.welcome;
  const version = welcome?.version;
  const [fetched, setFetched] = useState<ReleaseInfo | null>(null);
  const missing = welcomeOpen && !welcome?.release;
  useEffect(() => {
    if (!missing) return;
    void api
      .updatesHistory(false)
      .then((h) => setFetched(h.releases.find((r) => r.version === version) ?? null))
      .catch(() => undefined);
  }, [missing, version]);
  const release = welcome?.release ?? fetched;
  const notes = release ? parseNotes(release.notes) : null;
  const hasNotes = notes && (notes.summary || notes.sections.length > 0);
  return (
    <Dialog
      open={welcomeOpen}
      icon={<Logo />}
      title="O Prancha está atualizado!"
      description={`Você está utilizando a versão v${version}.`}
      onClose={closeWelcome}
      footer={
        <Button variant="primary" onClick={closeWelcome} data-autofocus>
          Começar a ler
        </Button>
      }
    >
      <div className="space-y-4">
        <p className="text-base font-medium">Obrigado por manter o Prancha atualizado e fazer parte da nossa evolução!</p>
        {hasNotes ? (
          <ReleaseNotes notes={notes} />
        ) : (
          version && (
            <div className="space-y-2">
              <p className="text-sm text-muted">As novidades desta versão estão na página dela no GitHub.</p>
              <GithubReleaseButton version={version} />
            </div>
          )
        )}
      </div>
    </Dialog>
  );
}

export function UpdateDialogs() {
  return (
    <>
      <WelcomeDialog />
      <OfferDialog />
      <ProgressDialog />
      <FailedDialog />
    </>
  );
}
