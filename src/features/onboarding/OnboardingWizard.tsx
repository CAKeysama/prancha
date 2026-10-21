import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { BookOpen, CircleAlert, FolderPlus, Library, Upload } from "lucide-react";
import { Button } from "../../components/Button";
import { Spinner } from "../../components/Feedback";
import { cn } from "../../components/cn";
import type { ComicSummary, ImportItem } from "../../domain/types";
import { api } from "../../services/api";
import { APP_NAME, SUPPORTED_FORMATS_TEXT } from "../../app/config";
import { useNav } from "../../app/navigation";
import { useImport, type ImportJob } from "../library/import-context";
import { ComicCard } from "../library/ComicItem";
import { useOnboarding } from "./onboarding-context";

const TOTAL_STEPS = 4;

function StepDots({ step }: { step: number }) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-xs font-medium text-muted">
        Etapa {step} de {TOTAL_STEPS}
      </span>
      <div className="flex gap-1" aria-hidden>
        {Array.from({ length: TOTAL_STEPS }, (_, i) => (
          <span key={i} className={cn("h-1.5 rounded-full transition-all", i + 1 === step ? "w-5 bg-accent" : i + 1 < step ? "w-1.5 bg-accent" : "w-1.5 bg-border")} />
        ))}
      </div>
    </div>
  );
}

/** Acompanha a posição de um elemento real da interface para destacá-lo. */
function useTargetRect(selector: string | null): DOMRect | null {
  const [rect, setRect] = useState<DOMRect | null>(null);
  useEffect(() => {
    if (!selector) return;
    let timer = 0;
    let last = "";
    const tick = () => {
      const el = Array.from(document.querySelectorAll<HTMLElement>(selector)).find((e) => e.offsetParent !== null);
      const r = el?.getBoundingClientRect() ?? null;
      const key = r ? `${r.x},${r.y},${r.width},${r.height}` : "";
      if (key !== last) {
        last = key;
        setRect(r);
      }
      timer = window.setTimeout(tick, 250);
    };
    timer = window.setTimeout(tick, 0);
    return () => window.clearTimeout(timer);
  }, [selector]);
  return selector ? rect : null;
}

function Card({ step, title, children, actions, onSkip, style, modal }: { step: number; title: string; children: ReactNode; actions: ReactNode; onSkip: () => void; style?: React.CSSProperties; modal?: boolean }) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("[data-primary]")?.focus();
  }, [step]);
  return (
    <div
      ref={ref}
      role="dialog"
      aria-modal={modal || undefined}
      aria-labelledby={titleId}
      data-onboarding-step={step}
      className="p-enter fixed z-[90] w-[min(440px,calc(100vw-2rem))] rounded-lg border border-border bg-elevated shadow-overlay"
      style={style}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onSkip();
        }
      }}
    >
      <div className="px-5 pt-4">
        <StepDots step={step} />
        <h2 id={titleId} className="mt-3 text-xl font-semibold">
          {title}
        </h2>
        <div className="mt-2 text-base text-muted">{children}</div>
      </div>
      <div className="mt-4 flex flex-wrap items-center justify-between gap-2 border-t border-border px-5 py-3">
        <button type="button" onClick={onSkip} className="text-sm font-medium text-muted hover:text-fg hover:underline">
          Pular tutorial
        </button>
        <div className="flex flex-wrap gap-2">{actions}</div>
      </div>
    </div>
  );
}

function resultOf(job: ImportJob): { comicId: number | null; failure: ImportItem | null } {
  const ok = job.items.find((i) => (i.status === "added" || i.status === "relinked" || i.status === "duplicate" || i.status === "skipped") && i.comicId);
  return { comicId: ok?.comicId ?? null, failure: ok ? null : (job.items.find((i) => i.status === "failed") ?? null) };
}

function StepAdd() {
  const onboarding = useOnboarding();
  const imports = useImport();
  const nav = useNav();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "info" | "error"; text: string } | null>(null);
  const [enteredAt] = useState(() => Date.now());
  const rect = useTargetRect('[data-tour="add-comics"]');

  useEffect(() => {
    if (nav.route.name !== "home" && nav.route.name !== "library") nav.navigate({ name: "home" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Também avança quando a importação foi iniciada pelo botão real destacado ou por arrastar e soltar.
  const finished = imports.jobs.filter((j) => (j.phase === "done" || j.phase === "cancelled") && j.startedAt >= enteredAt - 1000);
  const lastFinished = finished[finished.length - 1];
  const external = !busy && lastFinished ? resultOf(lastFinished) : null;
  const externalComic = external?.comicId ?? null;
  const { goTo } = onboarding;
  useEffect(() => {
    if (externalComic !== null) goTo(3, externalComic);
  }, [externalComic, goTo]);
  const shown =
    message ??
    (external?.failure ? { tone: "error" as const, text: `${external.failure.name}: ${external.failure.message ?? "não foi possível importar"}` } : null);

  const pick = async (mode: "files" | "folder") => {
    setMessage(null);
    setBusy(true);
    try {
      const jobId = await imports.pickAndImport(mode);
      if (jobId === null) {
        setMessage({ tone: "info", text: "Nenhum arquivo foi selecionado. Você pode tentar de novo ou fazer isso depois." });
        return;
      }
      const job = await imports.waitFor(jobId);
      const { comicId, failure } = resultOf(job);
      if (comicId) onboarding.goTo(3, comicId);
      else
        setMessage({
          tone: "error",
          text: failure
            ? `Não foi possível adicionar “${failure.name}”: ${failure.message ?? "erro desconhecido"}. Escolha outro arquivo.`
            : "Nenhum quadrinho foi encontrado no que você selecionou. Escolha outro arquivo.",
        });
    } finally {
      setBusy(false);
    }
  };

  const style: React.CSSProperties = rect
    ? { top: Math.min(rect.bottom + 16, window.innerHeight - 330), left: Math.max(16, Math.min(rect.right - 440, window.innerWidth - 456)) }
    : { top: "18vh", left: "50%", transform: "translateX(-50%)" };

  return (
    <>
      {rect && (
        <div
          aria-hidden
          className="pointer-events-none fixed z-[85] rounded-lg ring-2 ring-accent transition-all"
          style={{ left: rect.left - 6, top: rect.top - 6, width: rect.width + 12, height: rect.height + 12, boxShadow: "0 0 0 9999px var(--color-overlay)" }}
        />
      )}
      <Card
        step={2}
        title="Vamos adicionar sua primeira HQ"
        onSkip={onboarding.skip}
        style={style}
        actions={
          <>
            <Button onClick={onboarding.skip} disabled={busy}>
              Fazer depois
            </Button>
            <Button variant="primary" icon={Upload} onClick={() => void pick("files")} loading={busy} data-primary>
              Selecionar arquivo
            </Button>
          </>
        }
      >
        <p>
          Escolha um arquivo de quadrinhos no seu computador. O {APP_NAME} vai identificar o formato, preparar a capa e adicionar o item à sua
          biblioteca.
        </p>
        <p className="mt-2 text-sm">{SUPPORTED_FORMATS_TEXT}</p>
        <button type="button" disabled={busy} onClick={() => void pick("folder")} className="mt-2 inline-flex items-center gap-1.5 text-sm font-medium text-accent-fg hover:underline disabled:opacity-50">
          <FolderPlus size={14} aria-hidden /> Prefiro selecionar uma pasta com imagens
        </button>
        {busy && (
          <div className="mt-3">
            <Spinner label="Aguardando a escolha e a importação…" />
          </div>
        )}
        {shown && (
          <p role={shown.tone === "error" ? "alert" : "status"} className={cn("mt-3 flex gap-1.5 rounded-md border px-3 py-2 text-sm", shown.tone === "error" ? "border-danger/40 bg-danger-subtle text-fg" : "border-border bg-surface text-fg")}>
            {shown.tone === "error" && <CircleAlert size={15} className="mt-0.5 shrink-0 text-danger" aria-hidden />}
            {shown.text}
          </p>
        )}
      </Card>
    </>
  );
}

function StepImported({ comicId }: { comicId: number | null }) {
  const onboarding = useOnboarding();
  const nav = useNav();
  const [comic, setComic] = useState<ComicSummary | null>(null);
  const [fetchFailed, setFetchFailed] = useState(false);
  const missing = comicId === null || fetchFailed;
  useEffect(() => {
    if (comicId === null) return;
    let alive = true;
    api
      .comic(comicId)
      .then((c) => alive && setComic(c))
      .catch(() => alive && setFetchFailed(true));
    return () => {
      alive = false;
    };
  }, [comicId]);

  return (
    <>
      <div className="fixed inset-0 z-[85] bg-overlay" aria-hidden />
      <Card
        step={3}
        modal
        title="Sua HQ está na biblioteca"
        onSkip={onboarding.skip}
        style={{ top: "10vh", left: "50%", transform: "translateX(-50%)" }}
        actions={
          <>
            <Button
              icon={Library}
              onClick={() => {
                onboarding.goTo(4);
                nav.navigate({ name: "library" });
              }}
            >
              Ir para a biblioteca
            </Button>
            <Button
              variant="primary"
              icon={BookOpen}
              data-primary
              disabled={!comic || comic.missing}
              onClick={() => {
                if (!comic) return;
                onboarding.goTo(4);
                nav.navigate({ name: "reader", id: comic.id });
              }}
            >
              Abrir HQ
            </Button>
          </>
        }
      >
        <p>Cada item possui uma capa e um indicador de leitura. Quando você retomar uma HQ, o aplicativo vai guardar sua última posição para facilitar a próxima sessão.</p>
        {missing && (
          <div className="mt-3 rounded-md border border-border bg-surface p-3 text-sm">
            A HQ importada não está mais na biblioteca.{" "}
            <button type="button" className="font-medium text-accent-fg hover:underline" onClick={() => onboarding.goTo(2, null)}>
              Adicionar outra HQ
            </button>
          </div>
        )}
        {comic && (
          <div className="mt-4 flex gap-4 rounded-md border border-border bg-bg p-3">
            <div className="pointer-events-none" aria-hidden>
              <ComicCard comic={comic} width={112} />
            </div>
            <ol className="space-y-2.5 text-sm text-fg">
              <li>
                <span className="font-semibold">1 · Capa</span>
                <span className="block text-muted">{comic.coverKind === "placeholder" ? "Substituta: a original não pôde ser extraída." : comic.coverKind === "pending" ? "Sendo preparada." : "Extraída do próprio arquivo."}</span>
              </li>
              <li>
                <span className="font-semibold">2 · Título</span>
                <span className="block text-muted">Editável nos detalhes.</span>
              </li>
              <li>
                <span className="font-semibold">3 · Estado e páginas</span>
                <span className="block text-muted">
                  Não iniciado{comic.pageCount ? ` · ${comic.pageCount} páginas` : ""}. Ao ler, mostra onde você parou.
                </span>
              </li>
            </ol>
          </div>
        )}
        {!comic && !missing && (
          <div className="mt-3">
            <Spinner label="Carregando a HQ…" />
          </div>
        )}
      </Card>
    </>
  );
}

export function OnboardingWizard() {
  const onboarding = useOnboarding();
  const nav = useNav();
  if (!onboarding.wizardVisible || nav.route.name === "reader") return null;
  const { step, comicId } = onboarding.state;
  return createPortal(
    step === 1 ? (
      <>
        <div className="fixed inset-0 z-[85] bg-overlay" aria-hidden />
        <Card
          step={1}
          modal
          title={`Bem-vindo ao ${APP_NAME}`}
          onSkip={onboarding.skip}
          style={{ top: "18vh", left: "50%", transform: "translateX(-50%)" }}
          actions={
            <Button variant="primary" data-primary onClick={() => onboarding.goTo(2)}>
              Começar
            </Button>
          }
        >
          <p>Sua biblioteca de quadrinhos fica organizada em um só lugar. Adicione seus arquivos, leia no seu ritmo e retome de onde parou.</p>
        </Card>
      </>
    ) : step === 2 ? (
      <StepAdd />
    ) : (
      <StepImported comicId={comicId} />
    ),
    document.body,
  );
}
