import { useEffect, useState } from "react";
import { MoreHorizontal, Puzzle, RefreshCw, Search, TriangleAlert } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { ConfirmDialog, Dialog } from "../../components/Dialog";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { Segmented, Select, TextField } from "../../components/Form";
import { Menu } from "../../components/Menu";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { useLibraryData } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import { EXTERNAL_RUNTIME, groupedFingerprint, languageName, type CatalogItem, type ComponentProgress, type InstalledExtension } from "../../domain/online";
import { formatBytes, formatDate } from "../../domain/progress";
import { api, errorMessage } from "../../services/api";
import { onComponentProgress } from "../../services/events";
import { Fact } from "./parts";

const CATALOG_PAGE = 100;

function Icon({ src }: { src: string | null }) {
  return src ? (
    <img src={src} alt="" className="h-10 w-10 shrink-0 rounded-md border border-border object-cover" />
  ) : (
    <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-md border border-border bg-surface text-muted">
      <Puzzle size={18} aria-hidden />
    </span>
  );
}

function Permissions({ runtime, network, hosts, storage }: { runtime: string; network: boolean; hosts: string[]; storage: boolean }) {
  if (runtime === EXTERNAL_RUNTIME) return <Fact label="Permissões">Sem isolamento: acesso aos seus arquivos e à internet</Fact>;
  return (
    <Fact label="Permissões">
      <ul className="flex flex-col gap-0.5">
        <li>{network ? `Acessar: ${hosts.join(", ")}` : "Sem acesso à internet"}</li>
        {storage && <li>Guardar uma pequena quantidade de dados próprios</li>}
      </ul>
    </Fact>
  );
}

function progressText(progress: ComponentProgress): string {
  if (progress.stage === "download") return `Baixando o componente: ${progress.percent}%`;
  return progress.stage === "unpack" ? "Preparando o componente…" : "Iniciando o componente…";
}

function CatalogDialog({ item, onClose, onChanged }: { item: CatalogItem; onClose: () => void; onChanged: () => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<ComponentProgress | null>(null);
  useEffect(() => onComponentProgress(setProgress), []);
  const action = item.updateAvailable && item.installedHere ? "update" : item.installedVersion ? null : "install";
  const { data: installed } = useLibraryData(() => api.extensions(), [], "extensions");
  const current = action === "update" ? installed?.find((extension) => extension.id === item.id)?.capabilities : undefined;
  const newAccess = current
    ? [...item.capabilities.allowedHosts.filter((host) => !current.allowedHosts.includes(host)), ...(item.capabilities.storage && !current.storage ? ["Guardar dados próprios"] : [])]
    : [];
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      if (action === "update") await api.extensionUpdate(item.id);
      else await api.extensionInstall(item.repositoryId, item.id);
      toast.success(action === "update" ? "Extensão atualizada" : "Extensão instalada", item.name);
      onChanged();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      title={item.name}
      icon={<Icon src={item.icon} />}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{action ? "Cancelar" : "Fechar"}</Button>
          {action && (
            <Button variant="primary" onClick={run} loading={busy} disabled={item.incompatible !== null}>
              {action === "update" ? "Atualizar" : "Instalar"}
            </Button>
          )}
        </>
      }
    >
      {item.description && <p className="mb-3">{item.description}</p>}
      <dl>
        <Fact label="Versão">{item.installedVersion && item.updateAvailable ? `${item.installedVersion} → ${item.version}` : item.version}</Fact>
        {item.authors.length > 0 && <Fact label="Desenvolvimento">{item.authors.join(", ")}</Fact>}
        <Fact label="Repositório">{item.repositoryName}</Fact>
        <Fact label="Idiomas">{item.languages.map(languageName).join(", ")}</Fact>
        <Fact label="Fontes">{item.sources.map((s) => s.name).join(", ")}</Fact>
        {item.size > 0 && <Fact label="Tamanho">{formatBytes(item.size)}</Fact>}
        <Permissions runtime={item.runtime} network={item.capabilities.network} hosts={item.capabilities.allowedHosts} storage={item.capabilities.storage} />
        {item.incompatible && <Fact label="Compatibilidade">{item.incompatible}</Fact>}
      </dl>
      {action === "install" && (
        <p className="mt-4 flex items-start gap-2 text-sm">
          <TriangleAlert size={16} className="mt-0.5 shrink-0 text-danger" aria-hidden />
          {item.runtime === EXTERNAL_RUNTIME
            ? "Extensões são código de terceiros. Esta roda sem isolamento: instale só se confiar em quem a publica."
            : "Extensões são código de terceiros. Ela roda isolada, sem acesso aos seus arquivos, e só com as permissões acima."}
        </p>
      )}
      {busy && progress && (
        <p role="status" className="mt-2 text-sm text-muted">
          {progressText(progress)}
        </p>
      )}
      {newAccess.length > 0 && (
        <p className="mt-4 flex items-start gap-2 text-sm">
          <TriangleAlert size={16} className="mt-0.5 shrink-0 text-danger" aria-hidden />
          Esta versão pede acesso novo: {newAccess.join(", ")}.
        </p>
      )}
      {error && (
        <p role="alert" className="mt-2 text-sm text-danger">
          {error}
        </p>
      )}
    </Dialog>
  );
}

function catalogState(item: CatalogItem): string | null {
  if (item.incompatible) return "Incompatível";
  if (item.updateAvailable && item.installedHere) return `Atualização: ${item.version}`;
  if (item.installedHere) return "Instalada";
  if (item.installedVersion) return "Instalada de outro repositório";
  return null;
}

function Catalog({ onChanged }: { onChanged: () => void }) {
  const nav = useNav();
  const [text, setText] = useState("");
  const [language, setLanguage] = useState("");
  const [sort, setSort] = useState<"name" | "repository">("name");
  const [open, setOpen] = useState<CatalogItem | null>(null);
  const [limit, setLimit] = useState(CATALOG_PAGE);
  const languages = useLibraryData(() => api.extensionCatalogLanguages(), [], "catalog-languages");
  const repositories = useLibraryData(() => api.repositories(), [], "repositories");
  const { data: items, error, reload } = useLibraryData(() => api.extensionCatalog({ text: text.trim() || undefined, language: language || undefined, sort, limit }), [text, language, sort, limit], "catalog");

  if (repositories.data?.length === 0) {
    return (
      <EmptyState icon={Puzzle} title="Nenhum repositório" actions={<Button onClick={() => nav.navigate({ name: "repositories" })}>Abrir repositórios</Button>}>
        Adicione um repositório para ver as extensões que ele oferece.
      </EmptyState>
    );
  }
  return (
    <>
      <div className="mb-3 flex flex-wrap items-end gap-3">
        <div className="relative min-w-48 flex-1">
          <TextField label="Pesquisar extensões" type="search" value={text} onChange={(e) => setText(e.target.value)} />
          <Search size={14} className="pointer-events-none absolute top-8 right-2.5 text-muted" aria-hidden />
        </div>
        <Select label="Idioma" value={language} onChange={(e) => setLanguage(e.target.value)} className="w-44">
          <option value="">Todos</option>
          {languages.data?.map((code) => (
            <option key={code} value={code}>
              {languageName(code)}
            </option>
          ))}
        </Select>
        <Select label="Ordenar por" value={sort} onChange={(e) => setSort(e.target.value as "name" | "repository")} className="w-44">
          <option value="name">Nome</option>
          <option value="repository">Repositório</option>
        </Select>
      </div>
      {error && !items && <ErrorState title="Não foi possível abrir o catálogo" message={error} />}
      {!error && !items && <Spinner label="Carregando…" />}
      {items?.length === 0 && <EmptyState icon={Search} title="Nenhuma extensão encontrada" />}
      <ul>
        {items?.map((item) => {
          const state = catalogState(item);
          return (
            <li key={`${item.repositoryId}/${item.id}`} className="border-b border-border last:border-b-0">
              <button type="button" onClick={() => setOpen(item)} className="flex w-full items-center gap-3 py-3 text-left hover:bg-hover">
                <Icon src={item.icon} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-semibold">{item.name}</span>
                  {item.description && <span className="block truncate text-sm text-muted">{item.description}</span>}
                  <span className="block truncate text-xs text-muted">
                    {[item.authors[0], item.languages.map(languageName).join(", "), item.repositoryName, item.version].filter(Boolean).join(" · ")}
                  </span>
                </span>
                {state && <span className="shrink-0 text-sm text-muted">{state}</span>}
              </button>
            </li>
          );
        })}
      </ul>
      {items?.length === limit && (
        <div className="mt-3">
          <Button onClick={() => setLimit(limit + CATALOG_PAGE)}>Carregar mais</Button>
        </div>
      )}
      {open && (
        <CatalogDialog
          item={open}
          onClose={() => setOpen(null)}
          onChanged={() => {
            reload();
            onChanged();
          }}
        />
      )}
    </>
  );
}

function InstalledDialog({ extension, onClose }: { extension: InstalledExtension; onClose: () => void }) {
  return (
    <Dialog open title={extension.name} icon={<Icon src={extension.icon} />} onClose={onClose} footer={<Button onClick={onClose}>Fechar</Button>}>
      {extension.description && <p className="mb-3">{extension.description}</p>}
      <dl>
        <Fact label="Versão">{extension.version}</Fact>
        {extension.authors.length > 0 && <Fact label="Desenvolvimento">{extension.authors.join(", ")}</Fact>}
        <Fact label="Repositório de origem">{extension.repositoryName}</Fact>
        <Fact label="Impressão digital">
          <span className="font-mono text-sm break-all">{groupedFingerprint(extension.fingerprint)}</span>
        </Fact>
        <Fact label="Fontes">
          <ul className="flex flex-col gap-0.5">
            {extension.sources.map((source) => (
              <li key={source.id}>
                {source.name} · {languageName(source.language)}
              </li>
            ))}
          </ul>
        </Fact>
        <Permissions runtime={extension.runtime} network={extension.capabilities.network} hosts={extension.capabilities.allowedHosts} storage={extension.capabilities.storage} />
        <Fact label="Instalada em">{formatDate(extension.installedAt)}</Fact>
        <Fact label="Atualizada em">{formatDate(extension.updatedAt)}</Fact>
        {extension.loadError && <Fact label="Último erro">{extension.loadError}</Fact>}
      </dl>
    </Dialog>
  );
}

function Installed({ version, onChanged, onBrowse }: { version: number; onChanged: () => void; onBrowse: () => void }) {
  const toast = useToast();
  const { data: extensions, error, reload } = useLibraryData(() => api.extensions(), [version], "extensions-versioned");
  const [details, setDetails] = useState<InstalledExtension | null>(null);
  const [removing, setRemoving] = useState<InstalledExtension | null>(null);
  const [reviewing, setReviewing] = useState<CatalogItem | null>(null);

  // Atualizar passa pela mesma revisão de instalar: versão nova e permissões antes de confirmar.
  const reviewUpdate = async (extension: InstalledExtension) => {
    try {
      const found = (await api.extensionCatalog({ text: extension.name })).find((item) => item.id === extension.id && item.installedHere);
      if (found) setReviewing(found);
      else toast.error("Não foi possível atualizar a extensão", "Ela não está mais no catálogo do repositório.");
    } catch (e) {
      toast.error("Não foi possível atualizar a extensão", errorMessage(e));
    }
  };
  const setEnabled = async (extension: InstalledExtension, enabled: boolean) => {
    try {
      await api.extensionSetEnabled(extension.id, enabled);
    } catch (e) {
      toast.error("Não foi possível alterar a extensão", errorMessage(e));
    }
    reload();
    onChanged();
  };
  const remove = async () => {
    if (!removing) return;
    try {
      await api.extensionRemove(removing.id);
      toast.success("Extensão removida", removing.name);
    } catch (e) {
      toast.error("Não foi possível remover a extensão", errorMessage(e));
    }
    setRemoving(null);
    reload();
    onChanged();
  };

  if (error && !extensions) return <ErrorState title="Não foi possível abrir as extensões" message={error} />;
  if (!extensions) return <Spinner label="Carregando…" />;
  if (extensions.length === 0) {
    return (
      <EmptyState icon={Puzzle} title="Nenhuma extensão instalada" actions={<Button onClick={onBrowse}>Ver disponíveis</Button>}>
        Extensões acrescentam fontes de leitura a Explorar.
      </EmptyState>
    );
  }
  return (
    <>
      <ul>
        {extensions.map((extension) => (
          <li key={extension.id} className="flex items-center gap-3 border-b border-border py-3 last:border-b-0">
            <Icon src={extension.icon} />
            <div className="min-w-0 flex-1">
              <p className="truncate font-semibold">
                {extension.name}
                {!extension.enabled && <span className="ml-2 text-sm font-normal text-muted">Desabilitada</span>}
              </p>
              <p className="truncate text-sm text-muted">{extension.sources.map((s) => s.name).join(", ") || "Sem fontes disponíveis"}</p>
              <p className="truncate text-xs text-muted">
                {extension.version} · {extension.repositoryName}
              </p>
              {extension.loadError && <p className="mt-1 text-sm text-danger">{extension.loadError}</p>}
            </div>
            {extension.availableVersion && (
              <Button icon={RefreshCw} onClick={() => void reviewUpdate(extension)}>
                Atualizar para {extension.availableVersion}
              </Button>
            )}
            <Menu
              label={`Ações de ${extension.name}`}
              items={[
                { label: "Detalhes e permissões", onSelect: () => setDetails(extension) },
                { label: extension.enabled ? "Desabilitar" : "Habilitar", onSelect: () => void setEnabled(extension, !extension.enabled) },
                { kind: "separator" },
                { label: "Remover", danger: true, onSelect: () => setRemoving(extension) },
              ]}
              trigger={(p) => <IconButton {...p} variant="ghost" icon={MoreHorizontal} label={`Ações de ${extension.name}`} />}
            />
          </li>
        ))}
      </ul>
      {details && <InstalledDialog extension={details} onClose={() => setDetails(null)} />}
      {reviewing && (
        <CatalogDialog
          item={reviewing}
          onClose={() => setReviewing(null)}
          onChanged={() => {
            reload();
            onChanged();
          }}
        />
      )}
      <ConfirmDialog open={removing !== null} title={`Remover ${removing?.name ?? ""}?`} confirmLabel="Remover" destructive onCancel={() => setRemoving(null)} onConfirm={() => void remove()}>
        As fontes dela saem de Explorar. Os capítulos baixados continuam na biblioteca e abrem normalmente.
      </ConfirmDialog>
    </>
  );
}

export function ExtensionsScreen() {
  const [tab, setTab] = useState<"installed" | "catalog">("installed");
  const [version, setVersion] = useState(0);
  const bump = () => setVersion((v) => v + 1);
  return (
    <div className="h-full overflow-y-auto">
      <PageHeader
        title="Extensões"
        actions={
          <Segmented
            label="Mostrar"
            hideLabel
            value={tab}
            onChange={setTab}
            options={[
              { value: "installed", label: "Instaladas" },
              { value: "catalog", label: "Disponíveis" },
            ]}
          />
        }
      />
      <div className="mx-auto max-w-4xl px-6 py-4">{tab === "installed" ? <Installed version={version} onChanged={bump} onBrowse={() => setTab("catalog")} /> : <Catalog onChanged={bump} />}</div>
    </div>
  );
}
