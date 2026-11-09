import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { BellRing, ChartColumn, ChartNoAxesColumn, ChevronRight, ExternalLink, FolderOpen, GraduationCap, HardDrive, ImageMinus, MessageSquarePlus, RefreshCw, RotateCcw, ScanSearch, Scale, Search, SearchX, Trash2, X } from "lucide-react";
import { Button } from "../../components/Button";
import { ConfirmDialog, Dialog } from "../../components/Dialog";
import { EmptyState, Kbd, Spinner } from "../../components/Feedback";
import { Segmented, Select, Switch, TextArea, TextField } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { cn } from "../../components/cn";
import { formatBytes, formatDate } from "../../domain/progress";
import { parseNotes, type ReleaseInfo, type UpdatesHistory } from "../../domain/updates";
import { FIT_LABELS, MODE_LABELS } from "../../domain/reader";
import { muteUntil, type MuteChoice } from "../../domain/notifications";
import type { Settings } from "../../domain/settings";
import { PLACE_LABELS, SETTINGS_CATEGORIES, searchSettings, type SettingEntry, type SettingsCategory } from "../../domain/settings-search";
import {
  DEFAULT_SHORTCUTS,
  SHORTCUT_ACTIONS,
  SHORTCUT_LABELS,
  conflictFor,
  formatKey,
  keyFromEvent,
  type ShortcutAction,
} from "../../domain/shortcuts";
import type { AppInfo, SortKey, StorageInfo } from "../../domain/types";
import { api, errorMessage } from "../../services/api";
import { onCoversProgress } from "../../services/events";
import { useSettings } from "../../app/settings-context";
import { useNav } from "../../app/navigation";
import { useOnboarding } from "../onboarding/onboarding-context";
import { GithubReleaseButton, ReleaseNotes } from "../updates/UpdateDialogs";
import { useUpdates } from "../updates/updates-context";
import { APP_DESCRIPTION, APP_NAME } from "../../app/config";
import licenses from "../../generated/licenses.json";
import { useTelemetry } from "../telemetry/telemetry-context";
import { TelemetryDetails } from "../telemetry/TelemetryDetails";

/** A opção escolhida na pesquisa, destacada por um instante. */
const Highlight = createContext<string | null>(null);

/** Âncora de uma opção: o índice da pesquisa (`domain/settings-search.ts`) aponta para estes `id`s. */
function Anchor({ id, children }: { id: string; children: ReactNode }) {
  const on = useContext(Highlight) === id;
  return (
    <div id={`setting-${id}`} className={cn("-mx-3 rounded-md px-3 py-1 transition-colors", on ? "bg-accent-subtle duration-150" : "duration-700")}>
      {children}
    </div>
  );
}

function Section({ id, title, description, children }: { id: string; title: string; description?: string; children: ReactNode }) {
  return (
    <section aria-labelledby={`settings-${id}-title`} className="py-6">
      <h2 id={`settings-${id}-title`} className="text-lg font-semibold">
        {title}
      </h2>
      {description && <p className="mt-0.5 text-sm text-muted">{description}</p>}
      <div className="mt-3 space-y-3">{children}</div>
    </section>
  );
}

function SettingRow({ id, label, description, children }: { id: string; label: string; description?: ReactNode; children: ReactNode }) {
  return (
    <Anchor id={id}>
      <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-2">
        <div className="min-w-56 flex-1">
          <p className="font-medium">{label}</p>
          {description && <p className="text-sm text-muted">{description}</p>}
        </div>
        <div className="shrink-0">{children}</div>
      </div>
    </Anchor>
  );
}

function SegmentedRow<T extends string>({
  id,
  label,
  description,
  ...segmented
}: {
  id: string;
  label: string;
  description?: ReactNode;
  value: T;
  onChange: (v: T) => void;
  options: Array<{ value: T; label: string }>;
}) {
  return (
    <SettingRow id={id} label={label} description={description}>
      <Segmented label={label} hideLabel {...segmented} />
    </SettingRow>
  );
}

function useSave() {
  const { update } = useSettings();
  const toast = useToast();
  return <K extends keyof Settings>(key: K, value: Settings[K]) =>
    void update(key, value).catch((e) => toast.error("Não foi possível salvar a preferência", errorMessage(e)));
}

function ShortcutEditor() {
  const { settings } = useSettings();
  const save = useSave();
  const map = settings["reader.shortcuts"];
  const [recording, setRecording] = useState<ShortcutAction | null>(null);
  const [warning, setWarning] = useState<string | null>(null);

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(null);
        return;
      }
      const key = keyFromEvent(e);
      if (!key) return;
      const conflict = conflictFor(map, key, recording);
      const nextMap = { ...map, [recording]: [key, ...map[recording].filter((k) => k !== key)].slice(0, 3) };
      if (conflict) {
        nextMap[conflict] = map[conflict].filter((k) => k !== key);
        setWarning(`A tecla ${formatKey(key)} foi removida de “${SHORTCUT_LABELS[conflict]}”.`);
      } else setWarning(null);
      save("reader.shortcuts", nextMap);
      setRecording(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording, map, save]);

  return (
    <div>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="font-medium">Atalhos de teclado</p>
          <p className="text-sm text-muted">Valem dentro do leitor. Esc cancela a gravação.</p>
        </div>
        <Button size="sm" icon={RotateCcw} onClick={() => save("reader.shortcuts", DEFAULT_SHORTCUTS)}>
          Restaurar atalhos padrão
        </Button>
      </div>
      {warning && (
        <p role="status" className="mt-2 text-sm text-attention">
          {warning}
        </p>
      )}
      <table className="mt-3 w-full overflow-hidden rounded-md border border-border text-left">
        <thead className="bg-surface text-xs text-muted">
          <tr>
            <th className="px-3 py-2 font-medium">Ação</th>
            <th className="px-3 py-2 font-medium">Teclas</th>
            <th className="px-3 py-2">
              <span className="sr-only">Alterar</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {SHORTCUT_ACTIONS.map((a) => (
            <tr key={a} className="border-t border-border">
              <td className="px-3 py-1.5">{SHORTCUT_LABELS[a]}</td>
              <td className="px-3 py-1.5">
                <span className="flex flex-wrap gap-1">
                  {recording === a ? (
                    <span className="text-sm text-accent-fg">Pressione uma tecla…</span>
                  ) : map[a].length ? (
                    map[a].map((k) => <Kbd key={k}>{formatKey(k)}</Kbd>)
                  ) : (
                    <span className="text-sm text-muted">sem atalho</span>
                  )}
                </span>
              </td>
              <td className="px-3 py-1.5 text-right">
                <Button size="sm" variant="ghost" onClick={() => setRecording(recording === a ? null : a)} aria-label={`Alterar atalho de ${SHORTCUT_LABELS[a]}`}>
                  {recording === a ? "Cancelar" : "Alterar"}
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function LicensesDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [q, setQ] = useState("");
  const all = useMemo(
    () => [
      ...licenses.js.map((l) => ({ ...l, kind: "Interface (npm)" })),
      ...licenses.rust.map((l) => ({ ...l, kind: "Nativo (Rust)" })),
    ],
    [],
  );
  const filtered = all.filter((l) => !q || `${l.name} ${l.license}`.toLowerCase().includes(q.toLowerCase()));
  return (
    <Dialog
      open={open}
      onClose={onClose}
      size="lg"
      title="Licenças de terceiros"
      description={`${all.length} componentes de código aberto distribuídos com o ${APP_NAME}.`}
      footer={<Button onClick={onClose}>Fechar</Button>}
    >
      <TextField label="Filtrar" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Nome ou licença" className="mb-3" />
      <div className="max-h-72 overflow-y-auto rounded-md border border-border">
        <table className="w-full text-left text-sm">
          <thead className="sticky top-0 bg-surface text-xs text-muted">
            <tr>
              <th className="px-3 py-1.5 font-medium">Componente</th>
              <th className="px-3 py-1.5 font-medium">Versão</th>
              <th className="px-3 py-1.5 font-medium">Licença</th>
              <th className="px-3 py-1.5 font-medium">Origem</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((l) => (
              <tr key={`${l.kind}-${l.name}-${l.version}`} className="border-t border-border">
                <td className="px-3 py-1 font-mono text-xs">{l.name}</td>
                <td className="px-3 py-1 font-mono text-xs text-muted">{l.version}</td>
                <td className="px-3 py-1">{l.license}</td>
                <td className="px-3 py-1 text-muted">{l.kind}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <h3 className="mt-5 font-semibold">UnRAR (leitura de arquivos CBR)</h3>
      <p className="text-sm text-muted">O código-fonte do UnRAR, de Alexander Roshal, é usado sem modificações para ler arquivos RAR. Termos:</p>
      <pre className="mt-2 max-h-56 overflow-auto rounded-md border border-border bg-surface p-3 font-mono text-xs whitespace-pre-wrap select-text">{licenses.unrar}</pre>
    </Dialog>
  );
}

function StorageSection() {
  const toast = useToast();
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [confirm, setConfirm] = useState<null | "covers" | "pages">(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [covers, setCovers] = useState<{ total: number; done: number } | null>(null);
  const refresh = () => void api.storageInfo().then(setInfo).catch(() => undefined);
  useEffect(refresh, []);
  useEffect(
    () =>
      onCoversProgress((p) => {
        setCovers(p.done >= p.total ? null : p);
        if (p.done >= p.total) refresh();
      }),
    [],
  );
  const clear = async (what: "covers" | "pages") => {
    setConfirm(null);
    setBusy(what);
    try {
      const freed = what === "covers" ? await api.clearCoverCache() : await api.clearPageCache();
      toast.success(what === "covers" ? "Miniaturas removidas" : "Cache de páginas limpo", `${formatBytes(freed)} liberados. ${what === "covers" ? "As capas estão sendo recriadas." : ""}`);
      refresh();
    } catch (e) {
      toast.error("Não foi possível limpar o cache", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  return (
    <Section id="armazenamento" title="Armazenamento" description="Limpar caches não apaga suas HQs nem o progresso.">
      <SettingRow id="pasta" label="Pasta de dados do aplicativo" description={info ? <span className="font-mono text-xs break-all select-text">{info.dataDir}</span> : "Carregando…"}>
        <Button icon={FolderOpen} onClick={() => void api.openDataDir().catch((e) => toast.error("Não foi possível abrir a pasta", errorMessage(e)))}>
          Abrir pasta
        </Button>
      </SettingRow>
      <Anchor id="uso">
        <p className="font-medium">Espaço usado</p>
        {info ? (
          <dl className="mt-2 grid grid-cols-3 gap-3 max-sm:grid-cols-1">
            {[
              ["Catálogo (banco de dados)", info.databaseBytes],
              ["Miniaturas de capas", info.coversBytes],
              ["Cache de páginas (CBR/CB7)", info.pagesBytes],
            ].map(([label, bytes]) => (
              <div key={label as string} className="rounded-md border border-border bg-surface px-3 py-2">
                <dt className="text-xs text-muted">{label}</dt>
                <dd className="font-mono text-base">{formatBytes(bytes as number)}</dd>
              </div>
            ))}
          </dl>
        ) : (
          <p className="text-sm text-muted">Carregando…</p>
        )}
      </Anchor>
      {covers && <Spinner label={`Recriando capas: ${covers.done} de ${covers.total}`} />}
      <SettingRow id="limpar-miniaturas" label="Limpar miniaturas em cache" description="Remove as capas geradas. Elas são recriadas automaticamente a partir dos arquivos.">
        <Button icon={ImageMinus} loading={busy === "covers"} onClick={() => setConfirm("covers")}>
          Limpar miniaturas
        </Button>
      </SettingRow>
      <SettingRow id="limpar-paginas" label="Limpar cache de páginas" description="Remove as páginas extraídas de arquivos CBR e CB7. Elas são extraídas de novo ao abrir a HQ.">
        <Button icon={Trash2} loading={busy === "pages"} onClick={() => setConfirm("pages")}>
          Limpar cache de páginas
        </Button>
      </SettingRow>
      <ConfirmDialog
        open={confirm !== null}
        title={confirm === "covers" ? "Limpar as miniaturas?" : "Limpar o cache de páginas?"}
        confirmLabel={confirm === "covers" ? "Limpar miniaturas" : "Limpar cache"}
        onCancel={() => setConfirm(null)}
        onConfirm={() => void clear(confirm!)}
      >
        {confirm === "covers"
          ? "As capas serão recriadas em segundo plano. Suas HQs, favoritos, coleções e progresso não serão alterados."
          : "As próximas aberturas de arquivos CBR e CB7 podem demorar um pouco mais. Nada mais é alterado."}
      </ConfirmDialog>
    </Section>
  );
}

function ReleaseRow({ release, installed }: { release: ReleaseInfo; installed: boolean }) {
  const notes = parseNotes(release.notes);
  const date = notes.released ?? (release.date ? formatDate(Date.parse(release.date)) : "");
  return (
    <li>
      <details className="group">
        <summary className="flex cursor-pointer items-center gap-2 px-3 py-2 hover:bg-hover">
          <span className="font-mono text-sm font-semibold">v{release.version}</span>
          {installed && <span className="rounded-full bg-accent-subtle px-2 text-xs font-medium text-accent-fg">instalada</span>}
          <span className="ml-auto text-sm text-muted">{date}</span>
        </summary>
        <div className="space-y-3 border-t border-border px-3 py-3">
          <ReleaseNotes notes={notes} />
          <GithubReleaseButton version={release.version} />
        </div>
      </details>
    </li>
  );
}

function UpdatesSection() {
  const { snapshot, check, openOffer } = useUpdates();
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [history, setHistory] = useState<UpdatesHistory | null>(null);
  const loadHistory = (force: boolean) => void api.updatesHistory(force).then(setHistory).catch(() => undefined);
  useEffect(() => loadHistory(false), []);
  const verify = async () => {
    setChecking(true);
    setResult(null);
    try {
      const next = await check();
      setResult(next.available ? null : "O Prancha está na versão mais recente.");
      loadHistory(true);
    } catch (e) {
      setResult(errorMessage(e));
    } finally {
      setChecking(false);
    }
  };
  const available = snapshot?.available;
  const releases = history?.releases ?? [];
  return (
    <Section id="atualizacoes" title="Atualizações">
      <SettingRow id="versao" label="Versão instalada" description={snapshot?.updatedAt ? `Atualizada em ${formatDate(snapshot.updatedAt)}.` : undefined}>
        <span className="font-mono text-base">{snapshot ? `v${snapshot.currentVersion}` : "…"}</span>
      </SettingRow>
      <SettingRow
        id="procurar"
        label={available ? `Nova versão: v${available.version}` : "Procurar nova versão"}
        description={
          <span role="status">{result ?? (!available && snapshot?.lastCheckAt ? `Última verificação em ${formatDate(snapshot.lastCheckAt)}.` : "")}</span>
        }
      >
        <div className="flex flex-wrap gap-2">
          <Button icon={RefreshCw} loading={checking} onClick={() => void verify()}>
            Verificar atualizações
          </Button>
          {available && (
            <Button variant="primary" onClick={openOffer}>
              Atualizar agora
            </Button>
          )}
        </div>
      </SettingRow>
      <Anchor id="historico">
        <p className="font-medium">Versões publicadas</p>
        {!history && <p className="text-sm text-muted">Carregando…</p>}
        {history?.stale && <p className="text-sm text-muted">{releases.length ? "Sem conexão: estas são as notas guardadas neste computador." : "Não foi possível carregar as versões agora."}</p>}
        {releases.length > 0 && (
          <ul aria-label="Versões publicadas" className="mt-2 divide-y divide-border rounded-md border border-border bg-surface">
            {releases.map((release) => (
              <ReleaseRow key={release.version} release={release} installed={release.version === snapshot?.currentVersion} />
            ))}
          </ul>
        )}
      </Anchor>
    </Section>
  );
}

function SearchField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="relative">
      <Search size={15} aria-hidden className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted" />
      <label htmlFor="settings-search" className="sr-only">
        Pesquisar nas configurações
      </label>
      <input
        id="settings-search"
        type="search"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value) {
            e.stopPropagation();
            onChange("");
          }
        }}
        placeholder="Pesquisar"
        autoComplete="off"
        spellCheck={false}
        className="h-8 w-full rounded-md border border-border-control bg-bg pr-8 pl-8 text-base placeholder:text-muted focus-visible:border-accent focus-visible:outline-2 focus-visible:outline-offset-0 focus-visible:outline-accent [&::-webkit-search-cancel-button]:hidden"
      />
      {value && (
        <button
          type="button"
          aria-label="Apagar a pesquisa"
          onClick={() => onChange("")}
          className="absolute top-1/2 right-1.5 inline-flex size-6 -translate-y-1/2 items-center justify-center rounded text-muted hover:bg-hover hover:text-fg"
        >
          <X size={14} aria-hidden />
        </button>
      )}
    </div>
  );
}

function SearchResults({ query, results, onChoose, onClear }: { query: string; results: SettingEntry[]; onChoose: (entry: SettingEntry) => void; onClear: () => void }) {
  const count = results.length === 1 ? "1 opção encontrada" : `${results.length} opções encontradas`;
  return (
    <section aria-label="Resultados da pesquisa" className="py-6">
      <p role="status" className={cn("text-sm text-muted", !results.length && "sr-only")}>
        {results.length ? count : "Nenhuma opção encontrada"}
      </p>
      {results.length > 0 ? (
        <ul className="mt-2 divide-y divide-border overflow-hidden rounded-md border border-border">
          {results.map((entry) => (
            <li key={entry.id}>
              <button type="button" onClick={() => onChoose(entry)} className="flex w-full items-center gap-3 px-3 py-2.5 text-left hover:bg-hover focus-visible:-outline-offset-2">
                <span className="min-w-0 flex-1">
                  <span className="block font-medium">{entry.title}</span>
                  <span className="block text-sm text-muted">{entry.description}</span>
                </span>
                <span className="shrink-0 rounded-full border border-border px-2 text-xs text-muted">{PLACE_LABELS[entry.place]}</span>
                <ChevronRight size={16} aria-hidden className="shrink-0 text-muted" />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <EmptyState icon={SearchX} title={`Nada encontrado para “${query.trim()}”`} actions={<Button onClick={onClear}>Limpar pesquisa</Button>}>
          Tente outra palavra, como “tema”, “atalhos” ou “cache”.
        </EmptyState>
      )}
    </section>
  );
}

interface Draft {
  title: string;
  text: string;
}
const EMPTY_DRAFT: Draft = { title: "", text: "" };
const TITLE_MAX = 100;
const TEXT_MAX = 1500;

/** O texto fica com a tela de configurações: fechar ou falhar não apaga o que foi escrito. */
function SuggestionDialog({ draft, onChange, onClose, onSent }: { draft: Draft; onChange: (d: Draft) => void; onClose: () => void; onSent: () => void }) {
  const toast = useToast();
  const [errors, setErrors] = useState<{ title?: string; text?: string }>({});
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const form = useRef<HTMLFormElement>(null);
  const send = async () => {
    const next = { title: draft.title.trim() ? undefined : "Escreva um título.", text: draft.text.trim() ? undefined : "Descreva a sua ideia." };
    setErrors(next);
    if (next.title || next.text) return;
    setBusy(true);
    setFailure(null);
    try {
      await api.openImprovementForm(draft.title, draft.text);
      toast.success("Formulário aberto no navegador", "Confira o texto e envie a sugestão pelo GitHub.");
      onSent();
    } catch (e) {
      setFailure(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    // Depois da validação, o foco vai para o primeiro campo vazio.
    form.current?.querySelector<HTMLElement>("[aria-invalid='true']")?.focus();
  }, [errors]);
  return (
    <Dialog
      open
      title="Sugerir uma melhoria"
      description="A sugestão abre no GitHub já preenchida. Lá, você confere e envia; é preciso ter uma conta, e ela fica pública. Não inclua dados pessoais."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button variant="primary" icon={ExternalLink} loading={busy} onClick={() => void send()}>
            Abrir no GitHub
          </Button>
        </>
      }
    >
      <form
        ref={form}
        noValidate
        className="space-y-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (!busy) void send();
        }}
      >
        <TextField
          label="Título"
          value={draft.title}
          maxLength={TITLE_MAX}
          onChange={(e) => onChange({ ...draft, title: e.target.value })}
          error={errors.title}
          hint="Uma frase curta, como “Marcar páginas favoritas”."
          data-autofocus
        />
        <TextArea
          label="Descrição"
          value={draft.text}
          maxLength={TEXT_MAX}
          rows={6}
          onChange={(e) => onChange({ ...draft, text: e.target.value })}
          error={errors.text}
          hint={`O que você gostaria que o Prancha fizesse. ${draft.text.length} de ${TEXT_MAX} caracteres.`}
        />
      </form>
      {failure && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {failure}
        </p>
      )}
    </Dialog>
  );
}

function NotificationsSection() {
  const { settings } = useSettings();
  const save = useSave();
  const toast = useToast();
  const [testing, setTesting] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const native = settings["notifications.native"];
  const mutedUntil = settings["notifications.nativeMutedUntil"];
  const muted = mutedUntil !== null && mutedUntil > now;
  useEffect(() => {
    if (!muted) return;
    // Quando o silêncio acaba, o texto volta sozinho.
    const timer = window.setTimeout(() => setNow(Date.now()), Math.min(mutedUntil - now + 50, 2 ** 31 - 1));
    return () => window.clearTimeout(timer);
  }, [muted, mutedUntil, now]);
  const test = async () => {
    setTesting(true);
    try {
      const r = await api.notificationsNativeTest();
      if (r.permission === "granted") toast.success("Notificação de teste enviada", "Se ela não apareceu, confira as notificações do Prancha nas configurações do sistema.");
      else toast.error("O sistema não permite notificações do Prancha", "Libere nas configurações de notificação do sistema operacional. A central continua funcionando.");
    } catch (e) {
      toast.error("Não foi possível enviar a notificação de teste", errorMessage(e));
    } finally {
      setTesting(false);
    }
  };
  const row = (id: string, key: keyof Settings, label: string, description: string) => (
    <Anchor id={id}>
      <Switch label={label} description={description} checked={Boolean(settings[key])} onChange={(v) => save(key, v as never)} />
    </Anchor>
  );
  return (
    <>
      <Section id="notificacoes" title="Notificações" description="O que entra na central de notificações. Desligar um tipo não apaga as que já estão lá.">
        {row("notif-capitulos", "notifications.chapters", "Capítulos novos", "Quando uma obra que você acompanha recebe capítulos. Capítulos que chegam juntos viram uma notificação só.")}
        {row("notif-atualizacoes", "notifications.updates", "Atualizações do aplicativo", "Nova versão disponível, atualização concluída ou que não terminou.")}
        {row("notif-downloads", "notifications.downloadsCompleted", "Downloads concluídos", "Capítulos baixados de uma mesma obra são agrupados.")}
        {row("notif-falhas", "notifications.downloadsFailed", "Falhas de download", "Capítulos que não puderam ser baixados.")}
        {row("notif-sincronizacao", "notifications.sync", "Problemas de sincronização", "Obras acompanhadas cujas fontes não responderam ao procurar capítulos novos.")}
        {row("notif-biblioteca", "notifications.library", "Avisos da biblioteca", "Importações com arquivos recusados e HQs cujo arquivo foi movido ou excluído.")}
      </Section>
      <Section id="notificacoes-sistema" title="Notificações do sistema operacional" description="Avisos fora da janela, só para o que é importante e só com o Prancha em segundo plano. A central guarda tudo mesmo com elas desligadas.">
        <Anchor id="notif-nativas">
          <Switch
            label="Mostrar notificações do sistema"
            description="Capítulos novos, nova versão disponível e problemas de sincronização."
            checked={native}
            onChange={(v) => save("notifications.native", v)}
          />
        </Anchor>
        <Anchor id="notif-nativas-downloads">
          <Switch
            label="Também para downloads concluídos"
            description={native ? "Desligado por padrão: um lote de downloads já vira um aviso só na central." : "Ligue as notificações do sistema para usar esta opção."}
            checked={native && settings["notifications.nativeDownloads"]}
            onChange={(v) => native && save("notifications.nativeDownloads", v)}
          />
        </Anchor>
        <SettingRow
          id="notif-silenciar"
          label="Silenciar temporariamente"
          description={muted ? `Silenciadas até ${new Date(mutedUntil).toLocaleString("pt-BR", { weekday: "short", hour: "2-digit", minute: "2-digit" })}. A central continua recebendo tudo.` : "Pausa só as notificações do sistema."}
        >
          <Select
            label="Silenciar temporariamente"
            hideLabel
            className="w-48"
            disabled={!native}
            value={muted ? "muted" : "off"}
            onChange={(e) => {
              const v = e.target.value;
              if (v === "muted") return;
              save("notifications.nativeMutedUntil", v === "off" ? null : muteUntil(v as MuteChoice, Date.now()));
              setNow(Date.now());
            }}
          >
            <option value="off">Não silenciar</option>
            {muted && <option value="muted">Silenciadas</option>}
            <option value="1h">Por 1 hora</option>
            <option value="8h">Por 8 horas</option>
            <option value="tomorrow">Até amanhã às 8h</option>
          </Select>
        </SettingRow>
        <SettingRow id="notif-teste" label="Testar" description="Mostra um aviso agora, para conferir se o sistema permite notificações do Prancha.">
          <Button icon={BellRing} loading={testing} disabled={!native} onClick={() => void test()}>
            Enviar notificação de teste
          </Button>
        </SettingRow>
      </Section>
    </>
  );
}

function PrivacySection() {
  const { status, setConsent, resetId, forget } = useTelemetry();
  const toast = useToast();
  const [busy, setBusy] = useState<"consent" | "reset" | "forget" | null>(null);
  const [confirmForget, setConfirmForget] = useState(false);
  const on = status?.consent === "granted";

  const run = async (kind: "consent" | "reset" | "forget", action: () => Promise<unknown>, done?: string) => {
    setBusy(kind);
    try {
      await action();
      if (done) toast.success(done);
    } catch (e) {
      toast.error("Não foi possível concluir", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const state = !status
    ? "Carregando…"
    : on
      ? `Ligada desde ${status.decidedAt ? formatDate(status.decidedAt) : "agora"}.`
      : status.consent === "denied"
        ? `Desligada por você${status.decidedAt ? ` em ${formatDate(status.decidedAt)}` : ""}. Nada é enviado.`
        : "Desligada. Nada é enviado até você aceitar.";

  return (
    <>
      <Section id="privacidade" title="Privacidade" description="O Prancha funciona no seu computador, sem conta. A telemetria é opcional e começa desligada.">
        <Anchor id="telemetria">
          <div className="rounded-md border border-border bg-surface p-4">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div className="min-w-56 flex-1">
                <p className="flex items-center gap-2 font-medium">
                  <ChartNoAxesColumn size={16} aria-hidden /> Estatísticas de uso
                  <span
                    className={cn("rounded-full px-2 py-0.5 text-xs font-semibold", on ? "bg-success-subtle text-success" : "bg-hover text-muted")}
                    data-testid="telemetry-state"
                  >
                    {on ? "Ligada" : "Desligada"}
                  </span>
                </p>
                <p className="mt-0.5 text-sm text-muted">{state}</p>
              </div>
              <Switch label="Enviar estatísticas de uso" checked={on} onChange={(v) => void run("consent", () => setConsent(v), v ? "Telemetria ligada" : "Telemetria desligada")} />
            </div>
            <div className="mt-4 border-t border-border pt-4">
              <TelemetryDetails />
            </div>
            {status && !status.endpoint && <p className="mt-3 text-sm text-attention">Esta é uma versão de desenvolvimento: ela não envia nada, mesmo ligada.</p>}
            {on && status && (
              <p className="mt-3 text-sm text-muted">
                {status.lastSentAt ? `Último envio em ${formatDate(status.lastSentAt)}.` : "Nenhum envio ainda."} {status.pendingEvents > 0 ? `${status.pendingEvents} registro(s) esperando o próximo envio.` : ""}
              </p>
            )}
          </div>
        </Anchor>
        <SettingRow
          id="telemetria-identificador"
          label="Identificador da instalação"
          description={on && status?.installId ? <span className="font-mono text-xs">{status.installId}</span> : "Só existe com a telemetria ligada. É aleatório e não tem relação com você ou com este computador."}
        >
          <Button icon={RefreshCw} disabled={!on || busy !== null} loading={busy === "reset"} onClick={() => void run("reset", resetId, "Identificador trocado")}>
            Gerar outro
          </Button>
        </SettingRow>
        <SettingRow
          id="telemetria-apagar"
          label="Apagar os dados enviados"
          description={status?.forgetPending ? "O último pedido não chegou ao servidor. Tente de novo." : "Desliga a telemetria e pede ao servidor que apague tudo que esta instalação enviou."}
        >
          <Button variant="danger" icon={Trash2} disabled={!(on || status?.forgetPending) || busy !== null} loading={busy === "forget"} onClick={() => setConfirmForget(true)}>
            {status?.forgetPending ? "Tentar de novo" : "Apagar dados"}
          </Button>
        </SettingRow>
      </Section>
      <ConfirmDialog
        open={confirmForget}
        title="Apagar os dados enviados?"
        confirmLabel="Apagar"
        destructive
        busy={busy === "forget"}
        onCancel={() => setConfirmForget(false)}
        onConfirm={() => {
          setConfirmForget(false);
          void run("forget", forget, "Dados apagados no servidor");
        }}
      >
        A telemetria será desligada, o identificador deste computador será apagado e o servidor apagará tudo que esta instalação enviou. Se o servidor não responder, o pedido fica guardado para você tentar de novo.
      </ConfirmDialog>
    </>
  );
}

export function SettingsScreen({ section }: { section?: string } = {}) {
  const { settings, effectiveTheme } = useSettings();
  const save = useSave();
  const toast = useToast();
  const onboarding = useOnboarding();
  const nav = useNav();
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [licensesOpen, setLicensesOpen] = useState(false);
  const [suggesting, setSuggesting] = useState(false);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [verifying, setVerifying] = useState(false);
  const [category, setCategory] = useState<SettingsCategory>(() => SETTINGS_CATEGORIES.find((c) => c.id === section)?.id ?? "aparencia");
  const [query, setQuery] = useState("");
  const [target, setTarget] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const results = useMemo(() => searchSettings(query), [query]);
  const searching = query.trim().length > 0;
  useEffect(() => void api.appInfo().then(setAppInfo).catch(() => undefined), []);

  // Só a área das configurações rola até a opção escolhida; a janela fica parada.
  useEffect(() => {
    if (!target) return;
    const box = scroller.current;
    const row = document.getElementById(`setting-${target}`);
    if (box && row) {
      const header = box.querySelector("header")?.offsetHeight ?? 0;
      const top = row.getBoundingClientRect().top - box.getBoundingClientRect().top + box.scrollTop - header - 16;
      box.scrollTo({ top, behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
      // Num grupo de opções, o foco vai para a escolhida, como faria o Tab.
      (row.querySelector<HTMLElement>("[role='radio'][aria-checked='true']") ?? row.querySelector<HTMLElement>("button, input, select, textarea"))?.focus({ preventScroll: true });
    }
    const timer = window.setTimeout(() => setTarget(null), 1600);
    return () => window.clearTimeout(timer);
  }, [target]);

  const toTop = () => {
    if (scroller.current) scroller.current.scrollTop = 0;
  };
  const openCategory = (id: SettingsCategory) => {
    setQuery("");
    setTarget(null);
    setCategory(id);
    toTop();
  };
  const choose = (entry: SettingEntry) => {
    setQuery("");
    if (entry.place === "plugins") {
      nav.navigate({ name: "plugins" });
      return;
    }
    setCategory(entry.place);
    setTarget(entry.id);
  };

  const verify = async () => {
    setVerifying(true);
    try {
      const r = await api.verifyLibrary();
      toast.success("Verificação concluída", r.missing || r.restored ? `${r.missing} arquivo(s) ausente(s), ${r.restored} reencontrado(s).` : "Todos os arquivos estão no lugar.");
    } catch (e) {
      toast.error("Não foi possível verificar", errorMessage(e));
    } finally {
      setVerifying(false);
    }
  };

  return (
    <div ref={scroller} className="h-full overflow-y-auto">
      <PageHeader title="Configurações" />
      <div className="mx-auto flex max-w-5xl gap-8 px-6 max-md:flex-col max-md:gap-0">
        <aside className="w-52 shrink-0 py-6 max-md:w-full max-md:pb-0 md:sticky md:top-14 md:self-start">
          <SearchField
            value={query}
            onChange={(v) => {
              setQuery(v);
              toTop();
            }}
          />
          <nav aria-label="Categorias das configurações" className="mt-3">
            <ul className="flex flex-col gap-0.5 max-md:flex-row max-md:flex-wrap max-md:gap-1">
              {SETTINGS_CATEGORIES.map((c) => {
                const current = !searching && c.id === category;
                return (
                  <li key={c.id}>
                    <button
                      type="button"
                      onClick={() => openCategory(c.id)}
                      aria-current={current ? "true" : undefined}
                      className={cn(
                        "block w-full rounded-md px-3 py-1.5 text-left text-base max-md:w-auto max-md:border",
                        current ? "bg-hover font-semibold text-fg max-md:border-border-control" : "text-muted hover:bg-hover hover:text-fg max-md:border-border",
                      )}
                    >
                      {c.label}
                    </button>
                  </li>
                );
              })}
            </ul>
          </nav>
        </aside>
        <div className="min-w-0 flex-1">
          {searching ? (
            <SearchResults query={query} results={results} onChoose={choose} onClear={() => setQuery("")} />
          ) : (
            <Highlight.Provider value={target}>
              {category === "aparencia" && (
                <Section id="aparencia" title="Aparência">
                  <SegmentedRow
                    id="tema"
                    label="Tema"
                    description={settings["appearance.theme"] === "system" ? `Seguindo o sistema (agora: ${effectiveTheme === "dark" ? "escuro" : "claro"}).` : undefined}
                    value={settings["appearance.theme"]}
                    onChange={(v) => save("appearance.theme", v)}
                    options={[
                      { value: "dark", label: "Escuro" },
                      { value: "light", label: "Claro" },
                      { value: "system", label: "Seguir o sistema" },
                    ]}
                  />
                  <SegmentedRow
                    id="densidade"
                    label="Densidade da biblioteca"
                    value={settings["appearance.density"]}
                    onChange={(v) => save("appearance.density", v)}
                    options={[
                      { value: "comfortable", label: "Confortável" },
                      { value: "compact", label: "Compacta" },
                    ]}
                  />
                  <SegmentedRow
                    id="visualizacao"
                    label="Visualização da biblioteca"
                    value={settings["library.view"]}
                    onChange={(v) => save("library.view", v)}
                    options={[
                      { value: "grid", label: "Grade" },
                      { value: "list", label: "Lista" },
                    ]}
                  />
                  <SegmentedRow
                    id="miniaturas"
                    label="Tamanho das miniaturas"
                    value={settings["appearance.thumbSize"]}
                    onChange={(v) => save("appearance.thumbSize", v)}
                    options={[
                      { value: "small", label: "Pequeno" },
                      { value: "medium", label: "Médio" },
                      { value: "large", label: "Grande" },
                    ]}
                  />
                </Section>
              )}

              {category === "leitura" && (
                <Section id="leitura" title="Leitura">
                  <SettingRow id="modo" label="Modo de página">
                    <Select label="Modo de página" hideLabel value={settings["reader.mode"]} onChange={(e) => save("reader.mode", e.target.value as Settings["reader.mode"])} className="w-48">
                      {(["single", "double", "vertical", "webtoon"] as const).map((m) => (
                        <option key={m} value={m}>
                          {MODE_LABELS[m]}
                        </option>
                      ))}
                    </Select>
                  </SettingRow>
                  <SegmentedRow
                    id="direcao"
                    label="Direção de leitura"
                    description="Direita para a esquerda é o padrão de mangás."
                    value={settings["reader.direction"]}
                    onChange={(v) => save("reader.direction", v)}
                    options={[
                      { value: "ltr", label: "Esquerda → direita" },
                      { value: "rtl", label: "Direita → esquerda" },
                    ]}
                  />
                  <SettingRow id="ajuste" label="Ajuste padrão da imagem">
                    <Select label="Ajuste padrão da imagem" hideLabel value={settings["reader.fit"]} onChange={(e) => save("reader.fit", e.target.value as Settings["reader.fit"])} className="w-48">
                      {(["page", "width", "height"] as const).map((f) => (
                        <option key={f} value={f}>
                          {FIT_LABELS[f]}
                        </option>
                      ))}
                    </Select>
                  </SettingRow>
                  <Anchor id="capa-sozinha">
                    <Switch
                      label="Capa sozinha na página dupla"
                      description="Mostra a primeira página isolada para que os pares fiquem alinhados como no impresso."
                      checked={settings["reader.coverAlone"]}
                      onChange={(v) => save("reader.coverAlone", v)}
                    />
                  </Anchor>
                  <SegmentedRow
                    id="roda"
                    label="Roda do mouse"
                    description="Em página única ou dupla, ao chegar ao fim da página."
                    value={settings["reader.wheel"]}
                    onChange={(v) => save("reader.wheel", v)}
                    options={[
                      { value: "page", label: "Vira a página" },
                      { value: "scroll", label: "Só rola" },
                    ]}
                  />
                  <SegmentedRow
                    id="fim"
                    label="Ao avançar depois da última página"
                    value={settings["reader.end"]}
                    onChange={(v) => save("reader.end", v)}
                    options={[
                      { value: "stay", label: "Permanecer no leitor" },
                      { value: "library", label: "Voltar à biblioteca" },
                    ]}
                  />
                  <SegmentedRow
                    id="controles"
                    label="Controles do leitor"
                    description="Ocultos, reaparecem ao mover o mouse ou pelo botão “Mostrar controles”."
                    value={settings["reader.controls"]}
                    onChange={(v) => save("reader.controls", v)}
                    options={[
                      { value: "autohide", label: "Ocultar sozinhos" },
                      { value: "always", label: "Sempre visíveis" },
                    ]}
                  />
                  <Anchor id="atalhos">
                    <ShortcutEditor />
                  </Anchor>
                </Section>
              )}

              {category === "biblioteca" && (
                <Section id="biblioteca" title="Biblioteca">
                  <SettingRow id="duplicados" label="Arquivos duplicados" description="Duplicatas são identificadas pelo conteúdo, não pelo nome do arquivo.">
                    <Select label="Arquivos duplicados" hideLabel value={settings["library.duplicates"]} onChange={(e) => save("library.duplicates", e.target.value as Settings["library.duplicates"])} className="w-56">
                      <option value="skip">Ignorar e avisar</option>
                      <option value="allow">Importar mesmo assim</option>
                    </Select>
                  </SettingRow>
                  <SettingRow id="ordenacao" label="Ordenação padrão da biblioteca">
                    <div className="flex gap-2">
                      <Select label="Ordenação padrão" hideLabel value={settings["library.sort"]} onChange={(e) => save("library.sort", e.target.value as SortKey)} className="w-44">
                        <option value="title">Título</option>
                        <option value="added">Data de adição</option>
                        <option value="opened">Último acesso</option>
                        <option value="progress">Progresso</option>
                      </Select>
                      <Select label="Direção da ordenação" hideLabel value={settings["library.sortDesc"] ? "desc" : "asc"} onChange={(e) => save("library.sortDesc", e.target.value === "desc")} className="w-36">
                        <option value="asc">Crescente</option>
                        <option value="desc">Decrescente</option>
                      </Select>
                    </div>
                  </SettingRow>
                  <SettingRow id="verificar" label="Verificar arquivos" description="Procura HQs cujos arquivos foram movidos ou excluídos e marca-as como ausentes.">
                    <Button icon={ScanSearch} loading={verifying} onClick={() => void verify()}>
                      Verificar arquivos agora
                    </Button>
                  </SettingRow>
                  <SettingRow id="capas" label="Atualizar capas" description="Recria todas as miniaturas a partir dos arquivos originais.">
                    <Button
                      icon={RefreshCw}
                      onClick={() =>
                        void api
                          .regenerateCovers()
                          .then(() => toast.info("Recriando capas em segundo plano"))
                          .catch((e) => toast.error("Não foi possível recriar as capas", errorMessage(e)))
                      }
                    >
                      Recriar todas as capas
                    </Button>
                  </SettingRow>
                </Section>
              )}

              {category === "notificacoes" && <NotificationsSection />}

              {category === "armazenamento" && <StorageSection />}

              {category === "atualizacoes" && <UpdatesSection />}

              {category === "privacidade" && <PrivacySection />}

              {category === "geral" && (
                <Section id="geral" title="Geral">
                  <SettingRow id="idioma" label="Idioma" description="No momento, a interface está disponível apenas em português do Brasil.">
                    <span className="text-base">Português (Brasil)</span>
                  </SettingRow>
                  <SettingRow id="tutorial" label="Tutorial de primeira execução" description="Mostra novamente o passo a passo para adicionar e ler uma HQ.">
                    <Button
                      icon={GraduationCap}
                      onClick={() => {
                        onboarding.restart();
                        nav.navigate({ name: "home" });
                      }}
                    >
                      Abrir tutorial
                    </Button>
                  </SettingRow>
                  <SettingRow id="sugerir" label="Sugerir uma melhoria" description="Escreva sua ideia aqui; ela abre no GitHub já preenchida para você enviar. É preciso ter uma conta, e a sugestão fica pública.">
                    <Button icon={MessageSquarePlus} onClick={() => setSuggesting(true)}>
                      Escrever sugestão
                    </Button>
                  </SettingRow>
                  <SettingRow
                    id="visao-geral"
                    label="Visão geral do projeto"
                    description="Versões publicadas, downloads dos instaladores e as verificações automáticas do Prancha, com dados públicos do GitHub. Abre no navegador; nada do seu uso é enviado."
                  >
                    <Button
                      icon={ChartColumn}
                      onClick={() => void api.openProjectOverview().catch((e) => toast.error("Não foi possível abrir o navegador", errorMessage(e)))}
                    >
                      Ver visão geral
                    </Button>
                  </SettingRow>
                  <SettingRow id="licencas" label="Licenças de terceiros">
                    <Button icon={Scale} onClick={() => setLicensesOpen(true)}>
                      Ver licenças
                    </Button>
                  </SettingRow>
                  <Anchor id="sobre">
                    <div className="rounded-md border border-border bg-surface p-4">
                      <p className="flex items-center gap-2 font-semibold">
                        <HardDrive size={16} aria-hidden /> Sobre o {APP_NAME}
                      </p>
                      <p className="mt-1 text-sm text-muted">
                        {APP_DESCRIPTION}. Funciona no seu computador, sem conta. A internet é usada para procurar atualizações, com o plugin Fontes de leitura para repositórios e fontes online e, só se você ligar, para a telemetria (Configurações › Privacidade).
                      </p>
                      <dl className="mt-3 grid grid-cols-[160px_1fr] gap-y-1 text-sm">
                        <dt className="text-muted">Versão</dt>
                        <dd className="font-mono">{appInfo?.version ?? "…"}</dd>
                        <dt className="text-muted">Tauri</dt>
                        <dd className="font-mono">{appInfo?.tauriVersion ?? "…"}</dd>
                        <dt className="text-muted">Esquema do banco</dt>
                        <dd className="font-mono">{appInfo ? `v${appInfo.schemaVersion}` : "…"}</dd>
                        <dt className="text-muted">Formatos</dt>
                        <dd>CBZ, CBR, CB7, CBT, PDF e pastas de imagens (JPEG, PNG, WebP, GIF, BMP, AVIF)</dd>
                        <dt className="text-muted">Componentes principais</dt>
                        <dd>Tauri, React, SQLite, PDF.js, UnRAR, sevenz-rust2, zip, image</dd>
                      </dl>
                    </div>
                  </Anchor>
                </Section>
              )}
            </Highlight.Provider>
          )}
        </div>
      </div>
      <LicensesDialog open={licensesOpen} onClose={() => setLicensesOpen(false)} />
      {suggesting && (
        <SuggestionDialog
          draft={draft}
          onChange={setDraft}
          onClose={() => setSuggesting(false)}
          onSent={() => {
            setDraft(EMPTY_DRAFT);
            setSuggesting(false);
          }}
        />
      )}
    </div>
  );
}
