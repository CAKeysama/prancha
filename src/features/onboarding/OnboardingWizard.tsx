import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ArrowLeft, ArrowRight, Check, CircleAlert, FileUp, FolderPlus } from "lucide-react";
import { Button } from "../../components/Button";
import { ChoiceCards, Segmented, Switch, type ChoiceOption } from "../../components/Form";
import { Spinner } from "../../components/Feedback";
import { Logo } from "../../components/Logo";
import { cn } from "../../components/cn";
import { APP_NAME, SUPPORTED_FORMATS_TEXT } from "../../app/config";
import { useNav } from "../../app/navigation";
import { useSettings } from "../../app/settings-context";
import { FIT_LABELS, MODE_LABELS } from "../../domain/reader";
import { ONBOARDING_STEPS, type FitMode, type ReaderMode, type ReadingDirection, type Settings } from "../../domain/settings";
import { sound } from "../../services/sound";
import { FontChoice, SoundControls } from "../settings/PreferenceControls";
import { useImport, type ImportJob } from "../library/import-context";
import { useOnboarding } from "./onboarding-context";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Etapas depois das boas-vindas, as que aparecem no indicador de progresso. */
const COUNTED = ONBOARDING_STEPS.length - 1;

/** Configuração inicial: ocupa a janela inteira, uma escolha por etapa, tudo aplicado na hora e salvo a cada passo. */
export function OnboardingWizard() {
  const onboarding = useOnboarding();
  const nav = useNav();
  // O leitor nunca fica atrás dela: quem reabre pelas Configurações volta ao Início antes.
  if (!onboarding.wizardVisible || nav.route.name === "reader" || nav.route.name === "chapter") return null;
  return createPortal(<Wizard />, document.body);
}

function Wizard() {
  const onboarding = useOnboarding();
  const nav = useNav();
  const step = onboarding.state.step;
  const name = ONBOARDING_STEPS[step]!;
  const [direction, setDirection] = useState<"forward" | "back">("forward");
  const [leaving, setLeaving] = useState(false);
  const [imported, setImported] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const first = useRef(true);

  // A cada etapa o foco vai para o título, para quem usa leitor de tela ouvir onde está; na abertura, para o botão principal.
  useEffect(() => {
    const node = root.current;
    if (!node) return;
    if (first.current) {
      first.current = false;
      node.querySelector<HTMLElement>("[data-primary]")?.focus();
    } else node.querySelector<HTMLElement>("[data-step-title]")?.focus();
  }, [step]);

  // O app atrás fica inerte (nem foco, nem leitor de tela) e o Tab circula só pela configuração.
  useEffect(() => {
    const app = document.getElementById("root");
    app?.setAttribute("inert", "");
    return () => app?.removeAttribute("inert");
  }, []);
  useEffect(() => {
    const node = root.current;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab" || !node) return;
      const items = Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => el.offsetParent !== null);
      if (items.length === 0) return;
      const firstEl = items[0]!;
      const lastEl = items[items.length - 1]!;
      if (!node.contains(document.activeElement)) {
        e.preventDefault();
        firstEl.focus();
      } else if (e.shiftKey && document.activeElement === firstEl) {
        e.preventDefault();
        lastEl.focus();
      } else if (!e.shiftKey && document.activeElement === lastEl) {
        e.preventDefault();
        firstEl.focus();
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, []);

  const go = (next: number) => {
    setDirection(next > step ? "forward" : "back");
    onboarding.goTo(next);
  };

  const finish = () => {
    if (leaving) return;
    setLeaving(true);
    sound.play("complete");
    if (imported > 0) nav.navigate({ name: "library" });
    else if (nav.route.name !== "home") nav.navigate({ name: "home" });
  };
  // A animação de saída termina e só então a configuração some; o tempo extra cobre quem desliga animações.
  useEffect(() => {
    if (!leaving) return;
    const t = window.setTimeout(onboarding.complete, 300);
    return () => window.clearTimeout(t);
  }, [leaving, onboarding.complete]);

  const last = step === ONBOARDING_STEPS.length - 1;

  return (
    <div
      ref={root}
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      data-onboarding={name}
      className={cn("fixed inset-0 z-[65] flex flex-col bg-bg", leaving ? "p-onboarding-out pointer-events-none" : "p-enter")}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <header className="flex h-14 shrink-0 items-center justify-between px-6">
        <div className={cn("flex items-center gap-2 transition-opacity", name === "welcome" && "opacity-0")} aria-hidden>
          <Logo />
          <span className="font-semibold">{APP_NAME}</span>
        </div>
        {!last && (
          <Button variant="ghost" onClick={onboarding.skip}>
            Pular configuração
          </Button>
        )}
      </header>

      <main className="min-h-0 flex-1 overflow-y-auto">
        <div key={step} className={cn("mx-auto flex min-h-full w-full max-w-5xl px-6 py-6", direction === "back" ? "p-step-back" : "p-step-forward")}>
          {name === "welcome" && <WelcomeStep titleId={titleId} onStart={() => go(1)} />}
          {name === "appearance" && <AppearanceStep titleId={titleId} />}
          {name === "type" && <TypeStep titleId={titleId} />}
          {name === "sound" && <SoundStep titleId={titleId} />}
          {name === "reading" && <ReadingStep titleId={titleId} />}
          {name === "start" && <StartStep titleId={titleId} onImported={(n) => setImported((c) => c + n)} />}
        </div>
      </main>

      {name !== "welcome" && (
        <footer className="flex h-16 shrink-0 items-center gap-4 px-6">
          <Progress step={step} />
          <div className="ml-auto flex gap-2">
            <Button onClick={() => go(step - 1)}>
              Voltar
            </Button>
            {last ? (
              <Button variant="primary" data-primary onClick={finish}>
                Entrar no {APP_NAME}
              </Button>
            ) : (
              <Button variant="primary" data-primary onClick={() => go(step + 1)}>
                Continuar
              </Button>
            )}
          </div>
        </footer>
      )}
    </div>
  );
}

function Progress({ step }: { step: number }) {
  return (
    <div className="flex items-center gap-3">
      <div className="flex gap-1" aria-hidden>
        {Array.from({ length: COUNTED }, (_, i) => (
          <span key={i} className={cn("h-1 w-6 rounded-full transition-colors", i < step ? "bg-accent" : "bg-border")} />
        ))}
      </div>
      <span className="text-sm text-muted tabular-nums">
        {step} de {COUNTED}
      </span>
    </div>
  );
}

/** Título e linha de apoio de cada etapa; o título recebe o foco ao chegar. */
function StepHeading({ titleId, title, children }: { titleId: string; title: string; children?: ReactNode }) {
  return (
    <div className="mb-6">
      <h2 id={titleId} data-step-title tabIndex={-1} className="text-2xl font-semibold outline-none">
        {title}
      </h2>
      {children && <p className="mt-1 text-base text-muted">{children}</p>}
    </div>
  );
}

/** Controles à esquerda, prévia à direita (some em janelas estreitas). */
function Split({ children, preview }: { children: ReactNode; preview: ReactNode }) {
  return (
    <div className="my-auto grid w-full items-center gap-10 min-[900px]:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
      <div className="min-w-0">{children}</div>
      <div className="hidden min-[900px]:block">{preview}</div>
    </div>
  );
}

function WelcomeStep({ titleId, onStart }: { titleId: string; onStart: () => void }) {
  return (
    <div className="m-auto flex max-w-md flex-col items-center text-center">
      <svg viewBox="0 0 1024 1024" className="size-24" aria-hidden>
        <rect x="64" y="64" width="896" height="896" rx="200" fill="var(--color-accent-emphasis)" className="p-panel-in" />
        <rect x="232" y="224" width="250" height="576" rx="36" fill="#fff" className="p-panel-in" style={{ animationDelay: "140ms" }} />
        <rect x="542" y="224" width="250" height="258" rx="36" fill="#fff" className="p-panel-in" style={{ animationDelay: "240ms" }} />
        <rect x="542" y="542" width="250" height="258" rx="36" fill="#fff" fillOpacity="0.72" className="p-panel-in" style={{ animationDelay: "340ms" }} />
      </svg>
      <h2 id={titleId} data-step-title tabIndex={-1} className="mt-8 text-2xl font-semibold outline-none">
        Boas-vindas ao {APP_NAME}
      </h2>
      <p className="mt-2 text-base text-muted">Seus quadrinhos, mangás e manhwas num lugar só, para ler no seu ritmo e voltar de onde parou.</p>
      <Button variant="primary" data-primary className="mt-8" onClick={onStart}>
        Começar
      </Button>
      <p className="mt-3 text-sm text-muted">Leva menos de um minuto. Tudo muda depois em Configurações.</p>
    </div>
  );
}

/* ---------- Aparência ---------- */

function ThemeSwatch({ theme, className, style }: { theme: "dark" | "light"; className?: string; style?: React.CSSProperties }) {
  return (
    <span data-theme-preview={theme} aria-hidden className={cn("flex h-16 overflow-hidden rounded-md border border-border bg-bg", className)} style={style}>
      <span className="flex w-1/3 flex-col gap-1 border-r border-border bg-surface p-1.5">
        <span className="h-1.5 w-3/4 rounded-full bg-accent-emphasis" />
        <span className="h-1.5 w-2/3 rounded-full bg-border" />
        <span className="h-1.5 w-1/2 rounded-full bg-border" />
      </span>
      <span className="grid flex-1 grid-cols-3 gap-1 p-1.5">
        <span className="rounded-sm bg-border" />
        <span className="rounded-sm bg-accent-subtle" />
        <span className="rounded-sm bg-border" />
      </span>
    </span>
  );
}

const THEME_OPTIONS: ChoiceOption<Settings["appearance.theme"]>[] = [
  { value: "dark", label: "Escuro", visual: <ThemeSwatch theme="dark" /> },
  { value: "light", label: "Claro", visual: <ThemeSwatch theme="light" /> },
  {
    value: "system",
    label: "Seguir o sistema",
    visual: (
      <span className="relative block h-16">
        <ThemeSwatch theme="dark" className="absolute inset-0" />
        <ThemeSwatch theme="light" className="absolute inset-0" style={{ clipPath: "polygon(100% 0, 100% 100%, 0 100%)" }} />
      </span>
    ),
  },
];

function AppearanceStep({ titleId }: { titleId: string }) {
  const { settings, update } = useSettings();
  return (
    <Split preview={<LibraryPreview />}>
      <StepHeading titleId={titleId} title="Aparência">
        Muda na hora, no app inteiro.
      </StepHeading>
      <div className="space-y-6">
        <ChoiceCards label="Tema" value={settings["appearance.theme"]} options={THEME_OPTIONS} onChange={(v) => void update("appearance.theme", v)} className="grid-cols-3" />
        <Segmented
          label="Tamanho das miniaturas"
          value={settings["appearance.thumbSize"]}
          onChange={(v) => void update("appearance.thumbSize", v)}
          options={[
            { value: "small", label: "Pequeno" },
            { value: "medium", label: "Médio" },
            { value: "large", label: "Grande" },
          ]}
        />
      </div>
    </Split>
  );
}

/** Miniatura do app com as escolhas atuais: tema, fonte e tamanho das capas. Só formas e os nomes reais do menu. */
function LibraryPreview() {
  const { settings } = useSettings();
  const size = settings["appearance.thumbSize"];
  const cols = size === "small" ? 5 : size === "medium" ? 4 : 3;
  const fills = ["bg-elevated", "bg-accent-subtle", "bg-surface", "bg-elevated", "bg-surface"];
  return (
    <div aria-hidden className="pointer-events-none flex h-[360px] overflow-hidden rounded-xl border border-border bg-bg shadow-overlay">
      <div className="w-40 shrink-0 space-y-0.5 border-r border-border bg-surface p-2 text-sm">
        {["Início", "Minha biblioteca", "Continuar lendo", "Recentes", "Favoritos", "Coleções"].map((label, i) => (
          <div key={label} className={cn("truncate rounded-md px-2 py-1", i === 1 ? "bg-hover font-medium text-fg" : "text-muted")}>
            {label}
          </div>
        ))}
      </div>
      <div className="min-w-0 flex-1 p-4">
        <div className="text-lg font-semibold">Minha biblioteca</div>
        <div className="mt-3 grid gap-3 transition-all" style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}>
          {Array.from({ length: cols * 2 }, (_, i) => (
            <div key={i}>
              <div className={cn("aspect-[2/3] rounded-md border border-border", fills[i % fills.length])} />
              <div className="mt-1.5 h-1.5 w-4/5 rounded-full bg-border" />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/* ---------- Fonte ---------- */

function TypeStep({ titleId }: { titleId: string }) {
  const { settings, update } = useSettings();
  return (
    <Split preview={<LibraryPreview />}>
      <StepHeading titleId={titleId} title="Fonte da interface">
        Só nos menus e textos. As páginas das obras não mudam.
      </StepHeading>
      <FontChoice value={settings["appearance.font"]} onChange={(v) => void update("appearance.font", v)} />
    </Split>
  );
}

/* ---------- Sons ---------- */

function SoundStep({ titleId }: { titleId: string }) {
  const { settings, update, isSaved } = useSettings();
  // Quem nunca escolheu encontra os sons ligados aqui, com o controle à vista. Uma escolha já salva nunca é trocada.
  const proposed = useRef(false);
  useEffect(() => {
    if (proposed.current || isSaved("sound.enabled")) return;
    proposed.current = true;
    void update("sound.enabled", true);
  }, [isSaved, update]);
  return (
    <div className="m-auto w-full max-w-xl">
      <StepHeading titleId={titleId} title="Sons">
        Curtos e discretos, só quando algo acontece de fato. Na leitura, só ao trocar de capítulo.
      </StepHeading>
      <SoundControls
        enabled={settings["sound.enabled"]}
        volume={settings["sound.volume"]}
        onEnabled={(v) => void update("sound.enabled", v)}
        onVolume={(v) => void update("sound.volume", v)}
        samples={["add", "complete", "chapter", "notify"]}
      />
    </div>
  );
}

/* ---------- Leitura ---------- */

function ModeDiagram({ mode }: { mode: ReaderMode }) {
  const page = "rounded-[2px] bg-muted/50";
  return (
    <span aria-hidden className="flex h-12 items-center justify-center gap-0.5 overflow-hidden rounded-md bg-reader">
      {mode === "single" && <span className={cn(page, "h-9 w-6")} />}
      {mode === "double" && (
        <>
          <span className={cn(page, "h-9 w-6")} />
          <span className={cn(page, "h-9 w-6")} />
        </>
      )}
      {mode === "vertical" && (
        <span className="flex flex-col gap-1 self-start pt-1.5">
          <span className={cn(page, "h-7 w-5")} />
          <span className={cn(page, "h-7 w-5")} />
        </span>
      )}
      {mode === "webtoon" && (
        <span className="flex flex-col self-start pt-1.5">
          <span className={cn(page, "h-7 w-4 rounded-b-none")} />
          <span className={cn(page, "h-7 w-4 rounded-t-none")} />
        </span>
      )}
    </span>
  );
}

const MODE_DESCRIPTIONS: Record<ReaderMode, string> = {
  single: "Uma página por vez",
  double: "Como um livro aberto",
  vertical: "Páginas em sequência",
  webtoon: "Uma faixa contínua",
};

function ReadingStep({ titleId }: { titleId: string }) {
  const { settings, update } = useSettings();
  const mode = settings["reader.mode"];
  const paged = mode === "single" || mode === "double";
  return (
    <Split preview={<ReaderPreview mode={mode} direction={settings["reader.direction"]} fit={settings["reader.fit"]} />}>
      <StepHeading titleId={titleId} title="Leitura">
        O ponto de partida. No leitor, tudo isso muda a qualquer momento.
      </StepHeading>
      <div className="space-y-6">
        <ChoiceCards
          label="Modo de página"
          value={mode}
          onChange={(v) => void update("reader.mode", v)}
          className="grid-cols-2"
          options={(["single", "double", "vertical", "webtoon"] as const).map((m) => ({ value: m, label: MODE_LABELS[m], description: MODE_DESCRIPTIONS[m], visual: <ModeDiagram mode={m} /> }))}
        />
        {paged && (
          <div>
            <Segmented
              label="Direção de leitura"
              value={settings["reader.direction"]}
              onChange={(v) => void update("reader.direction", v)}
              options={[
                { value: "ltr", label: "Esquerda → direita" },
                { value: "rtl", label: "Direita → esquerda" },
              ]}
            />
            <p className="mt-1 text-sm text-muted">Mangás costumam ir da direita para a esquerda.</p>
          </div>
        )}
        <Segmented
          label="Ajuste da imagem"
          value={settings["reader.fit"]}
          onChange={(v) => void update("reader.fit", v)}
          options={(["page", "width", "height"] as const).map((f) => ({ value: f, label: FIT_LABELS[f] }))}
        />
      </div>
    </Split>
  );
}

/** Um leitor em miniatura que segue o modo, a direção e o ajuste escolhidos. */
function ReaderPreview({ mode, direction, fit }: { mode: ReaderMode; direction: ReadingDirection; fit: FitMode }) {
  const continuous = mode === "vertical" || mode === "webtoon";
  const wide = fit === "width";
  const page = "rounded-[3px] bg-muted/45";
  return (
    <div aria-hidden className="pointer-events-none relative flex h-[360px] items-start justify-center overflow-hidden rounded-xl border border-border bg-reader shadow-overlay">
      {mode === "single" && (
        <div className={cn(page, "absolute left-1/2 aspect-[2/3] -translate-x-1/2", wide ? "top-4 w-[78%]" : "top-[5%] h-[82%]")} />
      )}
      {mode === "double" && (
        <div className={cn("absolute left-1/2 flex -translate-x-1/2 gap-0.5", wide ? "top-4 w-[92%]" : "top-[5%] h-[82%]")}>
          <div className={cn(page, "aspect-[2/3]", wide ? "flex-1" : "h-full")} />
          <div className={cn(page, "aspect-[2/3]", wide ? "flex-1" : "h-full")} />
        </div>
      )}
      {continuous && (
        <div className={cn("flex flex-col pt-4", mode === "vertical" ? "gap-2" : "gap-0", wide ? "w-[78%]" : "w-[44%]")}>
          {[1, 2, 3].map((n) => (
            <div key={n} className={cn(page, "aspect-[2/3]", mode === "webtoon" && "rounded-none first:rounded-t-[3px]")} />
          ))}
        </div>
      )}
      {!continuous && (
        <span className="absolute bottom-4 left-1/2 -translate-x-1/2 text-muted">{direction === "rtl" ? <ArrowLeft size={18} /> : <ArrowRight size={18} />}</span>
      )}
    </div>
  );
}

/* ---------- Começar ---------- */

function summary(job: ImportJob): { tone: "ok" | "error" | "info"; text: string } {
  const added = job.items.filter((i) => i.status === "added" || i.status === "relinked").length;
  const already = job.items.filter((i) => i.status === "duplicate" || i.status === "skipped").length;
  const failed = job.items.filter((i) => i.status === "failed");
  if (added > 0) {
    const tail = failed.length > 0 ? ` ${failed.length} não ${failed.length === 1 ? "pôde ser adicionado" : "puderam ser adicionados"}.` : "";
    return { tone: "ok", text: `${added === 1 ? "1 HQ adicionada" : `${added} HQs adicionadas`} à biblioteca.${tail}` };
  }
  if (already > 0 && failed.length === 0) return { tone: "info", text: already === 1 ? "Essa HQ já estava na biblioteca." : "Essas HQs já estavam na biblioteca." };
  if (failed.length > 0) {
    const f = failed[0]!;
    return { tone: "error", text: `Não foi possível adicionar “${f.name}”: ${f.message ?? "formato não reconhecido"}.` };
  }
  if (job.phase === "cancelled") return { tone: "info", text: "Importação cancelada." };
  return { tone: "error", text: `Nenhuma HQ encontrada no que você escolheu. Formatos aceitos: ${SUPPORTED_FORMATS_TEXT}.` };
}

function StartStep({ titleId, onImported }: { titleId: string; onImported: (count: number) => void }) {
  const { settings, update } = useSettings();
  const imports = useImport();
  const [enteredAt] = useState(() => Date.now());
  const [picking, setPicking] = useState(false);
  const reported = useRef(new Set<number>());

  // Vale o seletor daqui e também arrastar arquivos para a janela.
  const mine = imports.jobs.filter((j) => j.startedAt >= enteredAt - 1000);
  const running = mine.find((j) => j.phase === "scanning" || j.phase === "processing");
  const done = mine.filter((j) => j.phase === "done" || j.phase === "cancelled");
  const latest = done[done.length - 1];

  useEffect(() => {
    for (const job of done) {
      if (reported.current.has(job.id)) continue;
      reported.current.add(job.id);
      const added = job.items.filter((i) => i.status === "added" || i.status === "relinked").length;
      if (added > 0) onImported(added);
    }
  }, [done, onImported]);

  const pick = async (mode: "files" | "folder") => {
    setPicking(true);
    try {
      await imports.pickAndImport(mode);
    } finally {
      setPicking(false);
    }
  };
  const result = latest && !running ? summary(latest) : null;

  return (
    <div className="m-auto w-full max-w-xl">
      <StepHeading titleId={titleId} title="Tudo pronto">
        Traga seus arquivos agora ou explore primeiro. Nada aqui é obrigatório.
      </StepHeading>
      <div className="space-y-8">
        <section aria-labelledby={`${titleId}-files`}>
          <h3 id={`${titleId}-files`} className="text-base font-medium">
            Adicionar HQs
          </h3>
          <p className="mt-0.5 text-sm text-muted">{SUPPORTED_FORMATS_TEXT}. Também dá para arrastar para a janela.</p>
          <div className="mt-3 flex flex-wrap gap-2">
            <Button icon={FileUp} disabled={picking || !!running} onClick={() => void pick("files")}>
              Escolher arquivos
            </Button>
            <Button icon={FolderPlus} disabled={picking || !!running} onClick={() => void pick("folder")}>
              Escolher pasta
            </Button>
          </div>
          <div role="status" className="mt-3 min-h-6 text-sm">
            {running ? (
              <span className="inline-flex items-center gap-2 text-muted">
                <Spinner />
                {running.counts.total > 0 ? `Adicionando ${running.counts.processed} de ${running.counts.total}…` : "Procurando arquivos…"}
              </span>
            ) : result ? (
              <span className="flex items-start gap-1.5">
                {result.tone === "ok" && <Check size={16} className="mt-0.5 shrink-0 text-success" aria-hidden />}
                {result.tone === "error" && <CircleAlert size={16} className="mt-0.5 shrink-0 text-danger" aria-hidden />}
                <span className={result.tone === "info" ? "text-muted" : "text-fg"}>{result.text}</span>
              </span>
            ) : null}
          </div>
        </section>
        <section aria-label="Fontes de leitura">
          {settings["plugins.online"] ? (
            <p className="flex items-start gap-1.5 text-base">
              <Check size={16} className="mt-1 shrink-0 text-success" aria-hidden />
              <span>
                <span className="font-medium">Fontes de leitura ativadas.</span> <span className="text-muted">Explorar está no menu.</span>
              </span>
            </p>
          ) : (
            <Switch
              label="Fontes de leitura"
              description="Procure obras em repositórios de extensões e leia online. Extensões são código de terceiros: instale só as de quem você confia."
              checked={false}
              onChange={(v) => v && void update("plugins.online", true)}
            />
          )}
        </section>
      </div>
    </div>
  );
}
