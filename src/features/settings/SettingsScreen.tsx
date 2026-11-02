import { useEffect, useMemo, useState, type ReactNode } from "react";
import { FolderOpen, GraduationCap, HardDrive, ImageMinus, RefreshCw, RotateCcw, ScanSearch, Scale, Trash2 } from "lucide-react";
import { Button } from "../../components/Button";
import { ConfirmDialog, Dialog } from "../../components/Dialog";
import { Kbd, Spinner } from "../../components/Feedback";
import { Segmented, Select, Switch, TextField } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { cn } from "../../components/cn";
import { formatBytes, formatDate } from "../../domain/progress";
import { parseNotes, type ReleaseInfo, type UpdatesHistory } from "../../domain/updates";
import { FIT_LABELS, MODE_LABELS } from "../../domain/reader";
import type { Settings } from "../../domain/settings";
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

const SECTIONS = [
  { id: "aparencia", label: "Aparência" },
  { id: "leitura", label: "Leitura" },
  { id: "biblioteca", label: "Biblioteca" },
  { id: "armazenamento", label: "Armazenamento" },
  { id: "atualizacoes", label: "Atualizações" },
  { id: "geral", label: "Geral" },
] as const;

function Section({ id, title, description, children }: { id: string; title: string; description?: string; children: ReactNode }) {
  return (
    <section id={`settings-${id}`} aria-labelledby={`settings-${id}-title`} className="scroll-mt-20 border-b border-border py-6 last:border-b-0">
      <h2 id={`settings-${id}-title`} className="text-lg font-semibold">
        {title}
      </h2>
      {description && <p className="mt-0.5 text-sm text-muted">{description}</p>}
      <div className="mt-4 space-y-5">{children}</div>
    </section>
  );
}

function SettingRow({ label, description, children }: { label: string; description?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-2">
      <div className="min-w-56 flex-1">
        <p className="font-medium">{label}</p>
        {description && <p className="text-sm text-muted">{description}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function SegmentedRow<T extends string>({ label, description, ...segmented }: { label: string; description?: ReactNode; value: T; onChange: (v: T) => void; options: Array<{ value: T; label: string }> }) {
  return (
    <SettingRow label={label} description={description}>
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
      <SettingRow label="Pasta de dados do aplicativo" description={info ? <span className="font-mono text-xs break-all select-text">{info.dataDir}</span> : "Carregando…"}>
        <Button icon={FolderOpen} onClick={() => void api.openDataDir().catch((e) => toast.error("Não foi possível abrir a pasta", errorMessage(e)))}>
          Abrir pasta
        </Button>
      </SettingRow>
      {info && (
        <dl className="grid grid-cols-3 gap-3 max-sm:grid-cols-1">
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
      )}
      {covers && <Spinner label={`Recriando capas: ${covers.done} de ${covers.total}`} />}
      <SettingRow label="Limpar miniaturas em cache" description="Remove as capas geradas. Elas são recriadas automaticamente a partir dos arquivos.">
        <Button icon={ImageMinus} loading={busy === "covers"} onClick={() => setConfirm("covers")}>
          Limpar miniaturas
        </Button>
      </SettingRow>
      <SettingRow label="Limpar cache de páginas" description="Remove as páginas extraídas de arquivos CBR e CB7. Elas são extraídas de novo ao abrir a HQ.">
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
      <SettingRow label="Versão instalada" description={snapshot?.updatedAt ? `Atualizada em ${formatDate(snapshot.updatedAt)}.` : undefined}>
        <span className="font-mono text-base">{snapshot ? `v${snapshot.currentVersion}` : "…"}</span>
      </SettingRow>
      <SettingRow
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
      {history?.stale && <p className="text-sm text-muted">{releases.length ? "Sem conexão: estas são as notas guardadas neste computador." : "Não foi possível carregar as versões agora."}</p>}
      {releases.length > 0 && (
        <ul aria-label="Versões publicadas" className="divide-y divide-border rounded-md border border-border bg-surface">
          {releases.map((release) => (
            <ReleaseRow key={release.version} release={release} installed={release.version === snapshot?.currentVersion} />
          ))}
        </ul>
      )}
    </Section>
  );
}

export function SettingsScreen() {
  const { settings, effectiveTheme } = useSettings();
  const save = useSave();
  const toast = useToast();
  const onboarding = useOnboarding();
  const nav = useNav();
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [licensesOpen, setLicensesOpen] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [active, setActive] = useState<string>("aparencia");
  useEffect(() => void api.appInfo().then(setAppInfo).catch(() => undefined), []);

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
    <div className="h-full overflow-y-auto" id="settings-scroll">
      <PageHeader title="Configurações" />
      <div className="mx-auto flex max-w-5xl gap-8 px-6">
        <nav aria-label="Seções das configurações" className="sticky top-20 h-fit w-44 shrink-0 py-6 max-md:hidden">
          <ul className="space-y-0.5">
            {SECTIONS.map((s) => (
              <li key={s.id}>
                <a
                  href={`#settings-${s.id}`}
                  onClick={(e) => {
                    e.preventDefault();
                    setActive(s.id);
                    document.getElementById(`settings-${s.id}`)?.scrollIntoView({ behavior: "smooth" });
                  }}
                  aria-current={active === s.id ? "true" : undefined}
                  className={cn("block rounded-md px-3 py-1.5 text-base", active === s.id ? "bg-hover font-semibold text-fg" : "text-muted hover:bg-hover hover:text-fg")}
                >
                  {s.label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
        <div className="min-w-0 flex-1">
          <Section id="aparencia" title="Aparência">
            <SegmentedRow
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
              label="Densidade da biblioteca"
              value={settings["appearance.density"]}
              onChange={(v) => save("appearance.density", v)}
              options={[
                { value: "comfortable", label: "Confortável" },
                { value: "compact", label: "Compacta" },
              ]}
            />
            <SegmentedRow
              label="Visualização da biblioteca"
              value={settings["library.view"]}
              onChange={(v) => save("library.view", v)}
              options={[
                { value: "grid", label: "Grade" },
                { value: "list", label: "Lista" },
              ]}
            />
            <SegmentedRow
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

          <Section id="leitura" title="Leitura">
            <SettingRow label="Modo de página">
              <Select label="Modo de página" hideLabel value={settings["reader.mode"]} onChange={(e) => save("reader.mode", e.target.value as Settings["reader.mode"])} className="w-48">
                {(["single", "double", "vertical"] as const).map((m) => (
                  <option key={m} value={m}>
                    {MODE_LABELS[m]}
                  </option>
                ))}
              </Select>
            </SettingRow>
            <SegmentedRow
              label="Direção de leitura"
              description="Direita para a esquerda é o padrão de mangás."
              value={settings["reader.direction"]}
              onChange={(v) => save("reader.direction", v)}
              options={[
                { value: "ltr", label: "Esquerda → direita" },
                { value: "rtl", label: "Direita → esquerda" },
              ]}
            />
            <SettingRow label="Ajuste padrão da imagem">
              <Select label="Ajuste padrão da imagem" hideLabel value={settings["reader.fit"]} onChange={(e) => save("reader.fit", e.target.value as Settings["reader.fit"])} className="w-48">
                {(["page", "width", "height"] as const).map((f) => (
                  <option key={f} value={f}>
                    {FIT_LABELS[f]}
                  </option>
                ))}
              </Select>
            </SettingRow>
            <Switch
              label="Capa sozinha na página dupla"
              description="Mostra a primeira página isolada para que os pares fiquem alinhados como no impresso."
              checked={settings["reader.coverAlone"]}
              onChange={(v) => save("reader.coverAlone", v)}
            />
            <SegmentedRow
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
              label="Ao avançar depois da última página"
              value={settings["reader.end"]}
              onChange={(v) => save("reader.end", v)}
              options={[
                { value: "stay", label: "Permanecer no leitor" },
                { value: "library", label: "Voltar à biblioteca" },
              ]}
            />
            <SegmentedRow
              label="Controles do leitor"
              description="Ocultos, reaparecem ao mover o mouse ou pelo botão “Mostrar controles”."
              value={settings["reader.controls"]}
              onChange={(v) => save("reader.controls", v)}
              options={[
                { value: "autohide", label: "Ocultar sozinhos" },
                { value: "always", label: "Sempre visíveis" },
              ]}
            />
            <ShortcutEditor />
          </Section>

          <Section id="biblioteca" title="Biblioteca">
            <SettingRow label="Arquivos duplicados" description="Duplicatas são identificadas pelo conteúdo, não pelo nome do arquivo.">
              <Select label="Arquivos duplicados" hideLabel value={settings["library.duplicates"]} onChange={(e) => save("library.duplicates", e.target.value as Settings["library.duplicates"])} className="w-56">
                <option value="skip">Ignorar e avisar</option>
                <option value="allow">Importar mesmo assim</option>
              </Select>
            </SettingRow>
            <SettingRow label="Ordenação padrão da biblioteca">
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
            <SettingRow label="Verificar arquivos" description="Procura HQs cujos arquivos foram movidos ou excluídos e marca-as como ausentes.">
              <Button icon={ScanSearch} loading={verifying} onClick={() => void verify()}>
                Verificar arquivos agora
              </Button>
            </SettingRow>
            <SettingRow label="Atualizar capas" description="Recria todas as miniaturas a partir dos arquivos originais.">
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

          <StorageSection />

          <UpdatesSection />

          <Section id="geral" title="Geral">
            <SettingRow label="Idioma" description="No momento, a interface está disponível apenas em português do Brasil.">
              <span className="text-base">Português (Brasil)</span>
            </SettingRow>
            <SettingRow label="Tutorial de primeira execução" description="Mostra novamente o passo a passo para adicionar e ler uma HQ.">
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
            <SettingRow label="Licenças de terceiros">
              <Button icon={Scale} onClick={() => setLicensesOpen(true)}>
                Ver licenças
              </Button>
            </SettingRow>
            <div className="rounded-md border border-border bg-surface p-4">
              <p className="flex items-center gap-2 font-semibold">
                <HardDrive size={16} aria-hidden /> Sobre o {APP_NAME}
              </p>
              <p className="mt-1 text-sm text-muted">{APP_DESCRIPTION}. Funciona no seu computador, sem conta. A internet só é usada para procurar e baixar atualizações.</p>
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
          </Section>
        </div>
      </div>
      <LicensesDialog open={licensesOpen} onClose={() => setLicensesOpen(false)} />
    </div>
  );
}
