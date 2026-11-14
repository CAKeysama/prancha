import { useDeferredValue, useMemo, useState } from "react";
import { FolderHeart, FolderPlus, MoreHorizontal, Pencil, Plus, Search, Trash2 } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { Cover } from "../../components/Cover";
import { ConfirmDialog, Dialog } from "../../components/Dialog";
import { Checkbox, TextField } from "../../components/Form";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { Menu } from "../../components/Menu";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { collectionCountLabel, foldText, isSeries, itemCountLabel, itemKey, mergeItems, type LibraryItem } from "../../domain/library-items";
import type { CollectionInfo } from "../../domain/types";
import { api, errorMessage } from "../../services/api";
import { useLibraryData, useLibraryVersion } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import { useScrollMemory } from "../../app/scroll-memory";
import { useSettings } from "../../app/settings-context";
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
    if (!name.trim() || busy) return;
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
          confirmLabel="Criar"
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
          confirmLabel="Renomear"
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
              toast.success("Coleção excluída", c.name);
              if (nav.route.name === "collection") nav.navigate({ name: "collections" });
            })
            .catch((e) => toast.error("Não foi possível excluir a coleção", errorMessage(e)));
        }}
      >
        Só a coleção é excluída. Nada sai da biblioteca e os arquivos originais não mudam.
      </ConfirmDialog>
    </>
  );
  return { dialogs, create: () => setCreating(true), rename: setRenaming, remove: setDeleting };
}

export function CollectionsScreen() {
  const nav = useNav();
  const { settings } = useSettings();
  const { data, error } = useLibraryData(() => api.collections(), [], "collections");
  const d = useCollectionDialogs();
  const scrollRef = useScrollMemory<HTMLDivElement>("collections");
  return (
    <div ref={scrollRef} className="h-full overflow-y-auto">
      <PageHeader
        title="Coleções"
        actions={
          // Sem coleções, a ação fica só no estado vazio.
          data?.length !== 0 ? (
            <Button variant="primary" icon={FolderPlus} onClick={d.create}>
              Nova coleção
            </Button>
          ) : undefined
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
                Nova coleção
              </Button>
            }
          >
            Agrupe itens da biblioteca. Um mesmo item pode estar em várias coleções.
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
                  <p className="text-sm text-muted">{collectionCountLabel(c.comicCount, settings["plugins.online"])}</p>
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
  // Com milhares de HQs, filtrar a lista não atrasa a digitação.
  const query = useDeferredValue(text);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const { data: all } = useLibraryData(() => api.listComics({ sort: "title" }), [], "comics-by-title");
  const { data: inside } = useLibraryData(() => api.listComics({ collectionId: collection.id }), [collection.id], "collection-comics");
  const series = useLibrarySeries();
  const insideIds = useMemo(() => new Set((inside ?? []).map((c) => c.id)), [inside]);
  // HQs e obras na mesma ordem por título.
  const everything = useMemo<LibraryItem[]>(
    () => mergeItems((all ?? []).filter((c) => !insideIds.has(c.id)), series.filter((s) => !s.collections.includes(collection.id)), "title"),
    [all, insideIds, series, collection.id],
  );
  const candidates = useMemo(() => {
    const q = foldText(query.trim());
    return everything.filter((item) => !q || foldText(isSeries(item) ? `${item.title} ${item.sourceName ?? ""}` : `${item.title} ${item.fileName} ${item.series ?? ""}`).includes(q));
  }, [everything, query]);
  const save = async () => {
    setBusy(true);
    try {
      const chosen = everything.filter((item) => selected.has(itemKey(item)));
      const comicIds = chosen.filter((item) => !isSeries(item)).map((item) => item.id);
      const seriesIds = chosen.filter(isSeries).map((item) => item.id);
      if (comicIds.length) await api.addToCollection(collection.id, comicIds);
      if (seriesIds.length) await api.addSeriesToCollection(collection.id, seriesIds);
      const label = itemCountLabel(chosen);
      const one = chosen.length === 1;
      const added = seriesIds.length && comicIds.length ? (one ? "adicionado" : "adicionados") : one ? "adicionada" : "adicionadas";
      toast.success(`${label} ${added} à coleção`);
      onClose();
    } catch (e) {
      toast.error("Não foi possível adicionar à coleção", errorMessage(e));
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
            {selected.size > 0 ? `Adicionar ${selected.size}` : "Adicionar"}
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
        <p className="text-sm text-muted">
          {everything.length > 0 ? "Nenhum resultado encontrado." : all.length > 0 || series.length > 0 ? "Tudo da biblioteca já está nesta coleção." : "Sua biblioteca está vazia."}
        </p>
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
  const { data } = useLibraryData(() => api.collections(), [], "collections");
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
  const add = (
    <Button variant="primary" icon={Plus} onClick={() => setAdding(true)}>
      Adicionar à coleção
    </Button>
  );
  return (
    <>
      <LibraryScreen
        key={id}
        preset={{
          title: collection.name,
          base: { collectionId: id },
          collectionId: id,
          onBack: () => nav.navigate({ name: "collections" }),
          backLabel: "Coleções",
          emptyIcon: FolderHeart,
          emptyTitle: "Esta coleção está vazia",
          emptyActions: add,
          // Vazia, o "Adicionar à coleção" fica só no estado vazio. Renomear e excluir, no menu, como nos cartões de Coleções.
          headerActions: (
            <>
              {collection.comicCount > 0 && add}
              <Menu
                label={`Ações da coleção ${collection.name}`}
                items={[
                  { label: "Renomear…", icon: Pencil, onSelect: () => d.rename(collection) },
                  { kind: "separator" },
                  { label: "Excluir coleção…", icon: Trash2, danger: true, onSelect: () => d.remove(collection) },
                ]}
                trigger={(p) => <IconButton {...p} variant="secondary" icon={MoreHorizontal} label="Mais ações" />}
              />
            </>
          ),
        }}
      />
      {adding && <AddComicsToCollection collection={collection} onClose={() => setAdding(false)} />}
      {d.dialogs}
    </>
  );
}
