import { useEffect, useState } from "react";
import { Boxes, Check, MoreHorizontal, Plus, RefreshCw, ShieldAlert, TriangleAlert } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { ConfirmDialog, Dialog } from "../../components/Dialog";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { TextField } from "../../components/Form";
import { Menu } from "../../components/Menu";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { useLibraryData } from "../../app/library-events";
import { groupedFingerprint, type RecommendedList, type RepositoryInfo, type RepositoryPreview } from "../../domain/online";
import { formatDate } from "../../domain/progress";
import { api, errorMessage } from "../../services/api";
import { Fact } from "./parts";
import { sound } from "../../services/sound";

function extensionCount(count: number): string {
  return `${count} ${count === 1 ? "extensão" : "extensões"}`;
}

/** Endereço, revisão da chave e confirmação explícita: nada é confiado sem a pessoa ver a impressão digital. */
function AddRepositoryDialog({ initialAddress = "", onClose, onAdded }: { initialAddress?: string; onClose: () => void; onAdded: () => void }) {
  const toast = useToast();
  const [address, setAddress] = useState(initialAddress);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(Boolean(initialAddress));
  const [preview, setPreview] = useState<RepositoryPreview | null>(null);

  // Um recomendado já chega com o endereço: a consulta começa sozinha, e a pessoa ainda revisa e confirma a chave.
  useEffect(() => {
    if (!initialAddress) return;
    let alive = true;
    api
      .repositoryPreview(initialAddress)
      .then((found) => alive && setPreview(found))
      .catch((e) => alive && setError(errorMessage(e)))
      .finally(() => alive && setBusy(false));
    return () => {
      alive = false;
    };
  }, [initialAddress]);

  const review = async () => {
    setBusy(true);
    setError(null);
    try {
      setPreview(await api.repositoryPreview(address.trim()));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const trust = async () => {
    if (!preview) return;
    setBusy(true);
    setError(null);
    try {
      const added = await api.repositoryAdd(preview.url, preview.fingerprint);
      sound.play("add");
      toast.success("Repositório adicionado", added.name);
      onAdded();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  if (!preview) {
    return (
      <Dialog
        open
        title="Adicionar repositório"
        onClose={onClose}
        footer={
          <>
            <Button onClick={onClose}>Cancelar</Button>
            <Button variant="primary" onClick={review} loading={busy} disabled={!address.trim()}>
              Continuar
            </Button>
          </>
        }
      >
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (address.trim() && !busy) void review();
          }}
        >
          <TextField
            label="Endereço"
            type="url"
            inputMode="url"
            placeholder="https://exemplo.com/prancha-repo.json"
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            error={error}
            data-autofocus
          />
        </form>
      </Dialog>
    );
  }
  return (
    <Dialog
      open
      title="Confiar neste repositório?"
      onClose={onClose}
      footer={
        <>
          <Button onClick={() => setPreview(null)}>Voltar</Button>
          <Button variant="primary" onClick={trust} loading={busy} disabled={preview.alreadyAdded}>
            Confiar e adicionar
          </Button>
        </>
      }
    >
      <dl>
        <Fact label="Nome declarado">{preview.name}</Fact>
        {preview.description && <Fact label="Descrição">{preview.description}</Fact>}
        <Fact label="Endereço">
          <span className="font-mono text-sm break-all">{preview.url}</span>
        </Fact>
        {preview.website && (
          <Fact label="Site">
            <span className="font-mono text-sm break-all">{preview.website}</span>
          </Fact>
        )}
        <Fact label="Extensões">{preview.extensionCount}</Fact>
        {!preview.external && (
          <Fact label="Chave pública">
            <span className="font-mono text-sm break-all">{preview.publicKey}</span>
          </Fact>
        )}
        <Fact label="Impressão digital">
          <span className="font-mono text-sm break-all" data-testid="fingerprint">
            {groupedFingerprint(preview.fingerprint)}
          </span>
        </Fact>
      </dl>
      <p className="mt-4 flex items-start gap-2 text-sm">
        <TriangleAlert size={16} className="mt-0.5 shrink-0 text-danger" aria-hidden />
        As extensões deste repositório são código de terceiros. Só adicione se você reconhecer quem o publica.
      </p>
      {preview.external && (
        <p className="mt-2 flex items-start gap-2 text-sm">
          <ShieldAlert size={16} className="mt-0.5 shrink-0 text-danger" aria-hidden />
          Elas foram feitas para o Mihon e rodam sem o isolamento do Prancha, com acesso aos seus arquivos e à internet. A primeira instalação baixa o Suwayomi, um programa de terceiros de cerca de 340 MB.
        </p>
      )}
      {preview.alreadyAdded && <p className="mt-2 text-sm text-danger">Este repositório já foi adicionado.</p>}
      {error && <p className="mt-2 text-sm text-danger">{error}</p>}
    </Dialog>
  );
}

/** A lista curta mantida pelo projeto, só com repositórios que o Prancha adiciona e usa. */
function Recommended({ addedUrls, onAdd }: { addedUrls: string; onAdd: (address: string) => void }) {
  const toast = useToast();
  const [list, setList] = useState<RecommendedList | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  // A cópia do computador aparece na hora; a publicada é consultada depois, se a cópia passou de um dia.
  // `addedUrls` muda quando um repositório entra ou sai, e o “Adicionado” é recalculado.
  useEffect(() => {
    let alive = true;
    api
      .repositoriesRecommended(false)
      .then((local) => {
        if (!alive) return;
        setList(local);
        if (!local.stale) return;
        setRefreshing(true);
        return api
          .repositoriesRecommended(true)
          .then((fresh) => alive && setList(fresh))
          .finally(() => alive && setRefreshing(false));
      })
      .catch((e) => alive && setFailure(errorMessage(e)));
    return () => {
      alive = false;
    };
  }, [addedUrls]);

  const refresh = async () => {
    setRefreshing(true);
    try {
      setList(await api.repositoriesRecommended(true));
    } catch (e) {
      toast.error("Não foi possível atualizar a lista", errorMessage(e));
    } finally {
      setRefreshing(false);
    }
  };

  if (list && !list.entries.length) return null;
  return (
    <section aria-labelledby="recommended-title" className="mt-8">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <h2 id="recommended-title" className="text-lg font-semibold">
            Recomendados
          </h2>
          <p className="text-sm text-muted">Lista mantida pelo projeto Prancha. Adicionar sempre mostra a chave do repositório antes de você confiar.</p>
        </div>
        <Button size="sm" variant="ghost" icon={RefreshCw} loading={refreshing} onClick={() => void refresh()}>
          Atualizar lista
        </Button>
      </div>
      {failure && !list && <p className="mt-3 text-sm text-danger">{failure}</p>}
      {!list && !failure && <Spinner label="Carregando…" />}
      {list?.error && (
        <p role="status" className="mt-2 text-sm text-muted">
          {list.error} {list.origin === "cache" ? `Mostrando a lista salva em ${formatDate(list.fetchedAt)}.` : "Mostrando a lista que veio com o aplicativo."}
        </p>
      )}
      <ul className="mt-3 space-y-2">
        {list?.entries.map((entry) => (
          <li key={entry.id} className="flex flex-wrap items-start gap-3 rounded-lg border border-border bg-surface p-4">
            <div className="min-w-56 flex-1">
              <p className="font-semibold">{entry.name}</p>
              <p className="mt-0.5 text-sm">{entry.description}</p>
              {entry.note && <p className="mt-1 text-sm text-muted">{entry.note}</p>}
            </div>
            {entry.added ? (
              <span className="inline-flex h-8 items-center gap-1.5 text-sm text-muted">
                <Check size={14} aria-hidden />
                Adicionado
              </span>
            ) : (
              <Button icon={Plus} aria-label={`Adicionar ${entry.name}`} onClick={() => onAdd(entry.url)}>
                Adicionar
              </Button>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

function statusLine(repository: RepositoryInfo): string {
  if (!repository.enabled) return "Desabilitado";
  if (repository.status === "pending") return "Ainda não sincronizado";
  const synced = `${extensionCount(repository.extensionCount)} · Atualizado em ${formatDate(repository.lastSuccessAt)}`;
  return repository.lastSuccessAt ? synced : extensionCount(repository.extensionCount);
}

function DetailsDialog({ repository, onClose }: { repository: RepositoryInfo; onClose: () => void }) {
  return (
    <Dialog open title={repository.name} onClose={onClose} footer={<Button onClick={onClose}>Fechar</Button>}>
      <dl>
        {repository.description && <Fact label="Descrição">{repository.description}</Fact>}
        <Fact label="Endereço">
          <span className="font-mono text-sm break-all">{repository.url}</span>
        </Fact>
        {repository.website && (
          <Fact label="Site">
            <span className="font-mono text-sm break-all">{repository.website}</span>
          </Fact>
        )}
        <Fact label="Extensões">{repository.extensionCount}</Fact>
        <Fact label="Impressão digital">
          <span className="font-mono text-sm break-all">{groupedFingerprint(repository.fingerprint)}</span>
        </Fact>
        <Fact label="Última tentativa">{formatDate(repository.lastSyncAt)}</Fact>
        <Fact label="Última atualização">{formatDate(repository.lastSuccessAt)}</Fact>
        <Fact label="Adicionado em">{formatDate(repository.addedAt)}</Fact>
        {repository.lastError && <Fact label="Último erro">{repository.lastError}</Fact>}
      </dl>
    </Dialog>
  );
}

function KeyChangeDialog({ repository, onClose, onDone }: { repository: RepositoryInfo; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const accept = async () => {
    setBusy(true);
    try {
      await api.repositoryAcceptKey(repository.id, repository.pendingFingerprint ?? "");
      toast.success("Nova chave aceita", repository.name);
      onDone();
      onClose();
    } catch (e) {
      toast.error("Não foi possível aceitar a chave", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      title="A chave deste repositório mudou"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose} data-autofocus>
            Manter a chave atual
          </Button>
          <Button variant="danger" onClick={accept} loading={busy}>
            Confiar na nova chave
          </Button>
        </>
      }
    >
      <p className="flex items-start gap-2 text-sm">
        <ShieldAlert size={16} className="mt-0.5 shrink-0 text-danger" aria-hidden />
        Isso pode ser uma troca legítima ou alguém se passando pelo repositório. Confirme com quem o publica. Até lá, nada dele é atualizado.
      </p>
      <dl className="mt-3">
        <Fact label="Chave atual">
          <span className="font-mono text-sm break-all">{groupedFingerprint(repository.fingerprint)}</span>
        </Fact>
        <Fact label="Nova chave">
          <span className="font-mono text-sm break-all">{groupedFingerprint(repository.pendingFingerprint ?? "")}</span>
        </Fact>
      </dl>
    </Dialog>
  );
}

export function RepositoriesScreen() {
  const toast = useToast();
  const { data: repositories, error, reload } = useLibraryData(() => api.repositories(), [], "repositories");
  /** O endereço com que o diálogo abre: vazio pelo botão do topo, preenchido por um recomendado. */
  const [adding, setAdding] = useState<string | null>(null);
  const [details, setDetails] = useState<RepositoryInfo | null>(null);
  const [keyChange, setKeyChange] = useState<RepositoryInfo | null>(null);
  const [removing, setRemoving] = useState<RepositoryInfo | null>(null);
  const [syncing, setSyncing] = useState<Set<string>>(new Set());

  const sync = async (repository: RepositoryInfo) => {
    setSyncing((s) => new Set(s).add(repository.id));
    try {
      const after = await api.repositorySync(repository.id);
      if (after.status === "ok") toast.success("Repositório atualizado", after.name);
    } catch (e) {
      toast.error(`Não foi possível atualizar ${repository.name}`, errorMessage(e));
    } finally {
      setSyncing((s) => {
        const next = new Set(s);
        next.delete(repository.id);
        return next;
      });
      reload();
    }
  };
  const syncAll = async () => {
    setSyncing(new Set(repositories?.filter((r) => r.enabled).map((r) => r.id)));
    try {
      await api.repositoriesSyncAll();
    } catch (e) {
      toast.error("Não foi possível atualizar os repositórios", errorMessage(e));
    } finally {
      setSyncing(new Set());
      reload();
    }
  };
  const setEnabled = async (repository: RepositoryInfo, enabled: boolean) => {
    try {
      await api.repositorySetEnabled(repository.id, enabled);
    } catch (e) {
      toast.error("Não foi possível alterar o repositório", errorMessage(e));
    }
    reload();
  };
  const remove = async () => {
    if (!removing) return;
    try {
      await api.repositoryRemove(removing.id);
      sound.play("remove");
      toast.success("Repositório removido", removing.name);
    } catch (e) {
      toast.error("Não foi possível remover o repositório", errorMessage(e));
    }
    setRemoving(null);
    reload();
  };

  return (
    <div className="h-full overflow-y-auto">
      <PageHeader
        title="Repositórios"
        actions={
          <>
            {!!repositories?.length && (
              <Button icon={RefreshCw} onClick={syncAll} loading={syncing.size > 0}>
                Atualizar todos
              </Button>
            )}
            <Button variant="primary" icon={Plus} onClick={() => setAdding("")}>
              Adicionar
            </Button>
          </>
        }
      />
      <div className="mx-auto max-w-4xl px-6 py-4">
        {error && !repositories && <ErrorState title="Não foi possível abrir os repositórios" message={error} />}
        {!error && !repositories && <Spinner label="Carregando…" />}
        {repositories?.length === 0 && (
          <EmptyState icon={Boxes} title="Nenhum repositório">
            Repositórios oferecem extensões com fontes de leitura. Adicione pelo endereço ou escolha um recomendado abaixo.
          </EmptyState>
        )}
        <ul>
          {repositories?.map((repository) => (
            <li key={repository.id} className="flex items-center gap-3 border-b border-border py-3 last:border-b-0">
              <div className="min-w-0 flex-1">
                <p className="truncate font-semibold">{repository.name}</p>
                <p className="truncate text-sm text-muted">{statusLine(repository)}</p>
                <p className="truncate font-mono text-xs text-muted">{repository.url}</p>
                {repository.status === "key_changed" ? (
                  <p role="alert" className="mt-1 flex flex-wrap items-center gap-2 text-sm text-danger">
                    <ShieldAlert size={14} aria-hidden />A chave deste repositório mudou.
                    <Button size="sm" onClick={() => setKeyChange(repository)}>
                      Revisar
                    </Button>
                  </p>
                ) : (
                  repository.status === "error" && repository.lastError && <p className="mt-1 text-sm text-danger">{repository.lastError}</p>
                )}
              </div>
              {syncing.has(repository.id) && <Spinner label="Atualizando" />}
              <Menu
                label={`Ações de ${repository.name}`}
                items={[
                  { label: "Atualizar", icon: RefreshCw, onSelect: () => void sync(repository), disabled: !repository.enabled || syncing.has(repository.id) },
                  { label: "Detalhes", onSelect: () => setDetails(repository) },
                  { label: repository.enabled ? "Desabilitar" : "Habilitar", onSelect: () => void setEnabled(repository, !repository.enabled) },
                  { kind: "separator" },
                  { label: "Remover", danger: true, onSelect: () => setRemoving(repository) },
                ]}
                trigger={(p) => <IconButton {...p} variant="ghost" icon={MoreHorizontal} label={`Ações de ${repository.name}`} />}
              />
            </li>
          ))}
        </ul>
        {repositories && <Recommended addedUrls={repositories.map((r) => r.url).join("\n")} onAdd={setAdding} />}
      </div>
      {adding !== null && <AddRepositoryDialog initialAddress={adding} onClose={() => setAdding(null)} onAdded={reload} />}
      {details && <DetailsDialog repository={details} onClose={() => setDetails(null)} />}
      {keyChange && <KeyChangeDialog repository={keyChange} onClose={() => setKeyChange(null)} onDone={reload} />}
      <ConfirmDialog open={removing !== null} title={`Remover ${removing?.name ?? ""}?`} confirmLabel="Remover" destructive onCancel={() => setRemoving(null)} onConfirm={() => void remove()}>
        O catálogo dele sai da lista. As extensões já instaladas continuam funcionando, mas não recebem atualizações, e nada da sua biblioteca é apagado.
      </ConfirmDialog>
    </div>
  );
}
