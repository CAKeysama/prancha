import { useMemo, useState } from "react";
import { FolderHeart, FolderPlus, MoreHorizontal, Pencil, Plus, Search, Trash2 } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { Cover } from "../../components/Cover";
import { ConfirmDialog, Dialog } from "../../components/Dialog";
import { Checkbox, TextField } from "../../components/Form";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { Menu } from "../../components/Menu";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { foldText, isSeries, itemKey, type LibraryItem } from "../../domain/library-items";
import { comicCountLabel } from "../../domain/progress";
import type { CollectionInfo } from "../../domain/types";
import { api, errorMessage } from "../../services/api";
import { useLibraryData, useLibraryVersion } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import { OnlineCover } from "../online/parts";
import { LibraryScreen } from "./LibraryScreen";
import { useLibrarySeries } from "./use-library-series";

function NameDialog({
  title,
  initial,
  confirmLabel,
  onSubmit,
  onClose,
}: {
  title: string;
  initial: string;
  confirmLabel: string;
  onSubmit: (name: string) => Promise<void>;
  onClose: () => void;
}) {
  const [name, setName] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSubmit(name);
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
      title={title}
      onClose={onClose}
      size="sm"
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button variant="primary" onClick={submit} loading={busy} disabled={!name.trim()}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <TextField label="Nome da coleção" value={name} onChange={(e) => setName(e.target.value)} error={error} data-autofocus maxLength={80} />
      </form>
    </Dialog>
  );
}

function useCollectionDialogs() {
  const toast = useToast();
  const { bump } = useLibraryVersion();
  const [creating, setCreating] = useState(false);
  const [renaming, setRenaming] = useState<CollectionInfo | null>(null);
  const [deleting, setDeleting] = useState<CollectionInfo | null>(null);
  const nav = useNav();
  const dialogs = (
    <>
      {creating && (
        <NameDialog
          title="Nova coleção"
          initial=""
          confirmLabel="Criar coleção"
          onClose={() => setCreating(false)}
          onSubmit={async (name) => {
            const id = await api.createCollection(name);
            bump();
            toast.success("Coleção criada", name.trim());
            nav.navigate({ name: "collection", id });
          }}
        />
      )}
      {renaming && (
        <NameDialog
          title="Renomear coleção"
          initial={renaming.name}
          confirmLabel="Salvar nome"
          onClose={() => setRenaming(null)}
          onSubmit={async (name) => {
            await api.renameCollection(renaming.id, name);
            bump();
            toast.success("Coleção renomeada");
          }}
        />
      )}
      <ConfirmDialog
        open={deleting !== null}
        destructive
        title={`Excluir a coleção “${deleting?.name ?? ""}”?`}
        confirmLabel="Excluir coleção"
        onCancel={() => setDeleting(null)}
        onConfirm={() => {
          const c = deleting!;
          setDeleting(null);
          void api
            .deleteCollection(c.id)
            .then(() => {
              bump();
              toast.success("Coleção excluída", "As HQs continuam na biblioteca.");
              if (nav.route.name === "collection") nav.navigate({ name: "collections" });
            })
            .catch((e) => toast.error("Não foi possível excluir a coleção", errorMessage(e)));
        }}
      >
        Apenas o agrupamento será excluído. As HQs continuam na biblioteca e os arquivos originais não são alterados.
      </ConfirmDialog>
    </>
  );
  return { dialogs, create: () => setCreating(true), rename: setRenaming, remove: setDeleting };
}

export function CollectionsScreen() {
  const nav = useNav();
  const { data, error } = useLibraryData(() => api.collections(), []);
  const d = useCollectionDialogs();
  return (
    <div className="h-full overflow-y-auto">
      <PageHeader
        title="Coleções"
        subtitle={data ? `${data.length} ${data.length === 1 ? "coleção" : "coleções"}` : undefined}
        actions={
          <Button variant="primary" icon={FolderPlus} onClick={d.create}>
            Nova coleção
          </Button>
        }
      />
      <div className="px-6 py-5">
        {error && <ErrorState title="Não foi possível carregar as coleções" message={error} />}
        {!data && !error && <Spinner label="Carregando coleções…" />}
        {data && data.length === 0 && (
          <EmptyState
            icon={FolderHeart}
            title="Nenhuma coleção ainda"
            actions={
              <Button variant="primary" icon={FolderPlus} onClick={d.create}>
                Criar a primeira coleção
              </Button>
            }
          >
            Coleções agrupam HQs da biblioteca, como “Mangás” ou “Lendo com a família”. Uma HQ pode estar em várias coleções.
          </EmptyState>
        )}
        {data && data.length > 0 && (
          <ul className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
            {data.map((c) => (
              <li key={c.id} className="group relative rounded-lg border border-border bg-surface hover:border-border-control">
                <button type="button" className="block w-full p-3 text-left" onClick={() => nav.navigate({ name: "collection", id: c.id })} aria-label={`Abrir coleção ${c.name}`}>
                  <div className="grid grid-cols-4 gap-1.5" aria-hidden>
                    {Array.from({ length: 4 }, (_, i) => {
                      const p = c.preview[i];
                      if (p?.online) return <OnlineCover key={`obra-${p.id}`} series={{ id: p.id, title: p.title, hasCover: p.coverKind !== "placeholder" }} className="rounded-sm" />;
                      return p ? (
                        <Cover key={p.id} comicId={p.id} title={p.title} kind={p.coverKind} rev={p.coverRev} className="rounded-sm" />
                      ) : (
                        <div key={i} className="aspect-[2/3] rounded-sm border border-dashed border-border" />
                      );
                    })}
                  </div>
                  <p className="mt-3 truncate pr-8 font-semibold">{c.name}</p>
                  <p className="text-sm text-muted">{comicCountLabel(c.comicCount)}</p>
                </button>
                <div className="absolute right-2 bottom-3">
                  <Menu
                    label={`Ações da coleção ${c.name}`}
                    items={[
                      { label: "Renomear…", icon: Pencil, onSelect: () => d.rename(c) },
                      { kind: "separator" },
                      { label: "Excluir coleção…", icon: Trash2, danger: true, onSelect: () => d.remove(c) },
                    ]}
                    trigger={(p) => <IconButton {...p} icon={MoreHorizontal} label={`Mais ações para a coleção ${c.name}`} size="sm" />}
                  />
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
      {d.dialogs}
    </div>
  );
}

function AddComicsToCollection({ collection, onClose }: { collection: CollectionInfo; onClose: () => void }) {
  const toast = useToast();
  const [text, setText] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const { data: all } = useLibraryData(() => api.listComics({ sort: "title" }), []);
  const { data: inside } = useLibraryData(() => api.listComics({ collectionId: collection.id }), [collection.id]);
  const series = useLibrarySeries();
  const insideIds = useMemo(() => new Set((inside ?? []).map((c) => c.id)), [inside]);
  const everything = useMemo<LibraryItem[]>(() => [...(all ?? []).filter((c) => !insideIds.has(c.id)), ...series.filter((s) => !s.collections.includes(collection.id))], [all, insideIds, series, collection.id]);
  const candidates = useMemo(() => {
    const q = foldText(text.trim());
    return everything.filter((item) => !q || foldText(isSeries(item) ? `${item.title} ${item.sourceName ?? ""}` : `${item.title} ${item.fileName} ${item.series ?? ""}`).includes(q));
  }, [everything, text]);
  const save = async () => {
    setBusy(true);
    try {
      const chosen = everything.filter((item) => selected.has(itemKey(item)));
      await api.addToCollection(collection.id, chosen.filter((item) => !isSeries(item)).map((item) => item.id));
      await api.addSeriesToCollection(collection.id, chosen.filter(isSeries).map((item) => item.id));
      toast.success(`${selected.size} ${selected.size === 1 ? "HQ adicionada" : "HQs adicionadas"} à coleção`);
      onClose();
    } catch (e) {
      toast.error("Não foi possível adicionar", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      title={`Adicionar a “${collection.name}”`}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button variant="primary" onClick={save} loading={busy} disabled={selected.size === 0}>
            Adicionar {selected.size > 0 ? selected.size : ""} à coleção
          </Button>
        </>
      }
    >
      <div className="relative mb-3">
        <Search size={15} aria-hidden className="pointer-events-none absolute top-[34px] left-2.5 text-muted" />
        <TextField label="Pesquisar na biblioteca" value={text} onChange={(e) => setText(e.target.value)} className="[&_input]:pl-8" data-autofocus />
      </div>
      {!all ? (
        <Spinner label="Carregando…" />
      ) : candidates.length === 0 ? (
        <p className="text-sm text-muted">Nenhuma HQ disponível para adicionar.</p>
      ) : (
        <div className="max-h-80 space-y-2 overflow-y-auto pr-1">
          {candidates.map((item) => (
            <Checkbox
              key={itemKey(item)}
              label={item.title}
              description={isSeries(item) ? (item.sourceName ?? undefined) : (item.series ?? item.fileName)}
              checked={selected.has(itemKey(item))}
              onChange={(v) =>
                setSelected((s) => {
                  const n = new Set(s);
                  if (v) n.add(itemKey(item));
                  else n.delete(itemKey(item));
                  return n;
                })
              }
            />
          ))}
        </div>
      )}
    </Dialog>
  );
}

export function CollectionDetailScreen({ id }: { id: number }) {
  const nav = useNav();
  const { data } = useLibraryData(() => api.collections(), []);
  const collection = data?.find((c) => c.id === id);
  const d = useCollectionDialogs();
  const [adding, setAdding] = useState(false);
  if (data && !collection) {
    return (
      <div className="p-6">
        <ErrorState title="Coleção não encontrada" message="Ela pode ter sido excluída." action={<Button onClick={() => nav.navigate({ name: "collections" })}>Ver coleções</Button>} />
      </div>
    );
  }
  if (!collection) return <Spinner label="Carregando coleção…" className="p-6" />;
  return (
    <>
      <LibraryScreen
        key={id}
        preset={{
          title: collection.name,
          subtitle: `Coleção · ${comicCountLabel(collection.comicCount)}`,
          base: { collectionId: id },
          collectionId: id,
          onBack: () => nav.navigate({ name: "collections" }),
          backLabel: "Coleções",
          emptyIcon: FolderHeart,
          emptyTitle: "Esta coleção está vazia",
          emptyText: "Adicione HQs da biblioteca a esta coleção. Elas continuam também na biblioteca principal.",
          emptyActions: (
            <Button variant="primary" icon={Plus} onClick={() => setAdding(true)}>
              Adicionar à coleção
            </Button>
          ),
          headerActions: (
            <>
              <Button icon={Pencil} onClick={() => d.rename(collection)}>
                Renomear
              </Button>
              <Button variant="danger" icon={Trash2} onClick={() => d.remove(collection)}>
                Excluir coleção
              </Button>
              <Button variant="primary" icon={Plus} onClick={() => setAdding(true)}>
                Adicionar à coleção
              </Button>
            </>
          ),
        }}
      />
      {adding && <AddComicsToCollection collection={collection} onClose={() => setAdding(false)} />}
      {d.dialogs}
    </>
  );
}
