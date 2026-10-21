import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  BookOpen,
  CircleCheck,
  CircleDashed,
  FolderOpen,
  FolderPlus,
  Info,
  Pencil,
  Plus,
  Star,
  StarOff,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "../../components/Button";
import { ConfirmDialog, Dialog } from "../../components/Dialog";
import { Checkbox, TextArea, TextField } from "../../components/Form";
import type { MenuEntry } from "../../components/Menu";
import { Spinner } from "../../components/Feedback";
import { useToast } from "../../components/Toast";
import { readActionLabel } from "../../domain/progress";
import type { CollectionInfo, ComicDetails, ComicSummary, FieldSource, MetadataPatch } from "../../domain/types";
import { api, errorMessage } from "../../services/api";
import { useNav } from "../../app/navigation";
import { useLibraryVersion } from "../../app/library-events";

interface ComicActionsApi {
  read: (c: ComicSummary) => void;
  openDetails: (c: ComicSummary) => void;
  toggleFavorite: (c: ComicSummary) => Promise<void>;
  setStatus: (c: ComicSummary, status: "unread" | "completed") => Promise<void>;
  edit: (id: number) => void;
  collections: (id: number) => void;
  remove: (c: ComicSummary) => void;
  menuFor: (c: ComicSummary, opts?: { collectionId?: number; inDetails?: boolean }) => MenuEntry[];
}

const Ctx = createContext<ComicActionsApi | null>(null);

export function useComicActions(): ComicActionsApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("useComicActions fora do provedor");
  return v;
}

export function SourceBadge({ source }: { source?: FieldSource }) {
  if (!source) return null;
  const text = source === "user" ? "editado" : source === "file" ? "do arquivo" : "do nome do arquivo";
  return (
    <span
      className={
        "rounded-full border px-1.5 text-[11px] leading-4 font-normal " +
        (source === "user" ? "border-accent/50 text-accent-fg" : "border-border text-muted")
      }
      title={source === "user" ? "Valor editado manualmente" : "Valor extraído automaticamente"}
    >
      {text}
    </span>
  );
}

// ---------------------------------------------------------------------------

function EditMetadataDialog({ id, onClose }: { id: number; onClose: () => void }) {
  const [details, setDetails] = useState<ComicDetails | null>(null);
  const [form, setForm] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .comic(id)
      .then((d) => {
        setDetails(d);
        setForm({
          title: d.title,
          series: d.series ?? "",
          issueNumber: d.issueNumber ?? "",
          volume: d.volume ?? "",
          year: d.year ? String(d.year) : "",
          writer: d.writer ?? "",
          artist: d.artist ?? "",
          publisher: d.publisher ?? "",
          summary: d.summaryText ?? "",
          tags: d.tags.join(", "),
        });
      })
      .catch((e) => setError(errorMessage(e)));
  }, [id]);

  const set = (k: string) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
    setForm((f) => ({ ...f, [k]: e.target.value }));
  const opt = (v: string | undefined) => (v && v.trim() ? v.trim() : null);

  const save = async () => {
    if (!form.title?.trim()) {
      setError("O título não pode ficar vazio.");
      return;
    }
    const yearText = form.year?.trim() ?? "";
    const year = yearText ? Number(yearText) : null;
    if (yearText && (!Number.isInteger(year) || year! < 1800 || year! > 2200)) {
      setError("Informe um ano entre 1800 e 2200.");
      return;
    }
    const patch: MetadataPatch = {
      title: form.title.trim(),
      series: opt(form.series),
      issueNumber: opt(form.issueNumber),
      volume: opt(form.volume),
      year,
      writer: opt(form.writer),
      artist: opt(form.artist),
      publisher: opt(form.publisher),
      summary: opt(form.summary),
      tags: (form.tags ?? "")
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean),
    };
    setSaving(true);
    setError(null);
    try {
      await api.updateMetadata(id, patch);
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const src = details?.fieldSources ?? {};
  return (
    <Dialog
      open
      onClose={onClose}
      title="Editar informações"
      description={details ? details.fileName : undefined}
      size="lg"
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button variant="primary" onClick={save} loading={saving} disabled={!details}>
            Salvar alterações
          </Button>
        </>
      }
    >
      {!details && !error && <Spinner label="Carregando informações…" />}
      {error && (
        <p role="alert" className="mb-3 rounded-md border border-danger/40 bg-danger-subtle px-3 py-2 text-sm text-danger">
          {error}
        </p>
      )}
      {details && (
        <form
          className="grid grid-cols-1 gap-3 sm:grid-cols-6"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <TextField
            className="sm:col-span-6"
            label="Título"
            value={form.title}
            onChange={set("title")}
            required
            data-autofocus
            badge={<SourceBadge source={src.title} />}
          />
          <TextField className="sm:col-span-3" label="Série" value={form.series} onChange={set("series")} badge={<SourceBadge source={src.series} />} />
          <TextField className="sm:col-span-1" label="Número" value={form.issueNumber} onChange={set("issueNumber")} badge={<SourceBadge source={src.issueNumber} />} />
          <TextField className="sm:col-span-1" label="Volume" value={form.volume} onChange={set("volume")} badge={<SourceBadge source={src.volume} />} />
          <TextField className="sm:col-span-1" label="Ano" inputMode="numeric" value={form.year} onChange={set("year")} badge={<SourceBadge source={src.year} />} />
          <TextField className="sm:col-span-3" label="Autor (roteiro)" value={form.writer} onChange={set("writer")} badge={<SourceBadge source={src.writer} />} />
          <TextField className="sm:col-span-3" label="Artista" value={form.artist} onChange={set("artist")} badge={<SourceBadge source={src.artist} />} />
          <TextField className="sm:col-span-3" label="Editora" value={form.publisher} onChange={set("publisher")} badge={<SourceBadge source={src.publisher} />} />
          <TextField
            className="sm:col-span-3"
            label="Tags"
            hint="Separe as tags por vírgula."
            value={form.tags}
            onChange={set("tags")}
            badge={<SourceBadge source={src.tags} />}
          />
          <TextArea className="sm:col-span-6" label="Sinopse" value={form.summary} onChange={set("summary")} badge={<SourceBadge source={src.summary} />} />
          <button type="submit" className="hidden" aria-hidden tabIndex={-1} />
        </form>
      )}
    </Dialog>
  );
}

function CollectionsDialog({ id, onClose }: { id: number; onClose: () => void }) {
  const [all, setAll] = useState<CollectionInfo[] | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [title, setTitle] = useState("");
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    Promise.all([api.collections(), api.comic(id)])
      .then(([cols, d]) => {
        if (!alive) return;
        setAll(cols);
        setTitle(d.title);
        setSelected(new Set(d.collections.map((c) => c.id)));
      })
      .catch((e) => alive && setError(errorMessage(e)));
    return () => {
      alive = false;
    };
  }, [id]);

  const create = async () => {
    setError(null);
    try {
      const newId = await api.createCollection(newName);
      setNewName("");
      setAll(await api.collections());
      setSelected((s) => new Set(s).add(newId));
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      await api.setComicCollections(id, [...selected]);
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open
      onClose={onClose}
      title="Coleções"
      description={title ? `Escolha as coleções de “${title}”.` : undefined}
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button variant="primary" onClick={save} loading={saving} disabled={!all}>
            Salvar coleções
          </Button>
        </>
      }
    >
      {error && (
        <p role="alert" className="mb-3 text-sm text-danger">
          {error}
        </p>
      )}
      {!all ? (
        <Spinner label="Carregando coleções…" />
      ) : (
        <div className="space-y-4">
          {all.length === 0 ? (
            <p className="text-sm text-muted">Você ainda não tem coleções. Crie a primeira abaixo.</p>
          ) : (
            <fieldset className="space-y-2.5">
              <legend className="sr-only">Coleções disponíveis</legend>
              {all.map((c) => (
                <Checkbox
                  key={c.id}
                  label={c.name}
                  description={`${c.comicCount} ${c.comicCount === 1 ? "HQ" : "HQs"}`}
                  checked={selected.has(c.id)}
                  onChange={(v) =>
                    setSelected((s) => {
                      const n = new Set(s);
                      if (v) n.add(c.id);
                      else n.delete(c.id);
                      return n;
                    })
                  }
                />
              ))}
            </fieldset>
          )}
          <form
            className="flex items-end gap-2 border-t border-border pt-4"
            onSubmit={(e) => {
              e.preventDefault();
              void create();
            }}
          >
            <TextField className="flex-1" label="Nova coleção" placeholder="Ex.: Mangás favoritos" value={newName} onChange={(e) => setNewName(e.target.value)} />
            <Button type="submit" icon={Plus} disabled={!newName.trim()}>
              Criar coleção
            </Button>
          </form>
        </div>
      )}
    </Dialog>
  );
}

function RemoveDialog({ comic, onClose, onRemoved }: { comic: ComicSummary; onClose: () => void; onRemoved: () => void }) {
  const toast = useToast();
  const [alsoTrash, setAlsoTrash] = useState(false);
  const [busy, setBusy] = useState(false);
  const confirm = async () => {
    setBusy(true);
    try {
      const r = await api.removeComic(comic.id, alsoTrash);
      toast.success(
        "Removida da biblioteca",
        r.trashed ? "O arquivo original foi movido para a Lixeira." : "O arquivo original continua no seu computador.",
      );
      onRemoved();
      onClose();
    } catch (e) {
      toast.error("Não foi possível remover", errorMessage(e));
      setBusy(false);
    }
  };
  return (
    <ConfirmDialog
      open
      destructive
      title={`Remover “${comic.title}” da biblioteca?`}
      confirmLabel={alsoTrash ? "Remover e mover para a Lixeira" : "Remover da biblioteca"}
      busy={busy}
      onConfirm={confirm}
      onCancel={onClose}
    >
      <p>O item sai do catálogo junto com o progresso de leitura, os favoritos e as coleções.</p>
      <p className="mt-2 text-muted">
        {alsoTrash
          ? "O arquivo original também será movido para a Lixeira do sistema, de onde ainda pode ser restaurado."
          : "O arquivo original não será apagado do seu computador."}
      </p>
      {!comic.missing && (
        <div className="mt-4 rounded-md border border-border bg-surface p-3">
          <Checkbox
            label="Também mover o arquivo original para a Lixeira"
            description={comic.format === "folder" ? "A pasta inteira com as imagens será movida." : comic.fileName}
            checked={alsoTrash}
            onChange={setAlsoTrash}
          />
        </div>
      )}
    </ConfirmDialog>
  );
}

// ---------------------------------------------------------------------------

export function ComicActionsProvider({ children }: { children: ReactNode }) {
  const nav = useNav();
  const toast = useToast();
  const { bump } = useLibraryVersion();
  const [editing, setEditing] = useState<number | null>(null);
  const [collecting, setCollecting] = useState<number | null>(null);
  const [removing, setRemoving] = useState<ComicSummary | null>(null);

  const read = useCallback((c: ComicSummary) => nav.navigate({ name: "reader", id: c.id }), [nav]);
  const openDetails = useCallback((c: ComicSummary) => nav.navigate({ name: "comic", id: c.id }), [nav]);

  const toggleFavorite = useCallback(
    async (c: ComicSummary) => {
      try {
        await api.setFavorite(c.id, !c.favorite);
      } catch (e) {
        toast.error("Não foi possível atualizar os favoritos", errorMessage(e));
      }
    },
    [toast],
  );

  const setStatus = useCallback(
    async (c: ComicSummary, status: "unread" | "completed") => {
      try {
        await api.setReadStatus(c.id, status);
      } catch (e) {
        toast.error("Não foi possível alterar o estado de leitura", errorMessage(e));
      }
    },
    [toast],
  );

  const menuFor = useCallback(
    (c: ComicSummary, opts?: { collectionId?: number; inDetails?: boolean }): MenuEntry[] => {
      const items: MenuEntry[] = [];
      if (!opts?.inDetails) {
        items.push({ label: readActionLabel(c), icon: BookOpen, onSelect: () => read(c), disabled: c.missing });
        items.push({ label: "Ver detalhes", icon: Info, onSelect: () => openDetails(c) });
        items.push({ kind: "separator" });
      }
      items.push({ label: c.favorite ? "Remover dos favoritos" : "Adicionar aos favoritos", icon: c.favorite ? StarOff : Star, onSelect: () => void toggleFavorite(c) });
      items.push({ label: "Editar informações", icon: Pencil, onSelect: () => setEditing(c.id) });
      items.push({ label: "Organizar em coleções…", icon: FolderPlus, onSelect: () => setCollecting(c.id) });
      if (opts?.collectionId !== undefined) {
        const cid = opts.collectionId;
        items.push({
          label: "Remover desta coleção",
          icon: X,
          onSelect: () =>
            void api
              .removeFromCollection(cid, c.id)
              .then(() => toast.success("Removida da coleção", c.title))
              .catch((e) => toast.error("Não foi possível remover da coleção", errorMessage(e))),
        });
      }
      items.push({ kind: "separator" });
      if (c.status !== "completed") items.push({ label: "Marcar como concluída", icon: CircleCheck, onSelect: () => void setStatus(c, "completed") });
      if (c.status !== "unread") items.push({ label: "Marcar como não lida", icon: CircleDashed, onSelect: () => void setStatus(c, "unread") });
      if (!c.missing)
        items.push({
          label: "Mostrar na pasta",
          icon: FolderOpen,
          onSelect: () => void api.revealComic(c.id).catch((e) => toast.error("Não foi possível abrir a pasta", errorMessage(e))),
        });
      items.push({ kind: "separator" });
      items.push({ label: "Remover da biblioteca…", icon: Trash2, danger: true, onSelect: () => setRemoving(c) });
      return items;
    },
    [read, openDetails, toggleFavorite, setStatus, toast],
  );

  const value = useMemo<ComicActionsApi>(
    () => ({
      read,
      openDetails,
      toggleFavorite,
      setStatus,
      edit: setEditing,
      collections: setCollecting,
      remove: setRemoving,
      menuFor,
    }),
    [read, openDetails, toggleFavorite, setStatus, menuFor],
  );

  return (
    <Ctx.Provider value={value}>
      {children}
      {editing !== null && <EditMetadataDialog id={editing} onClose={() => setEditing(null)} />}
      {collecting !== null && <CollectionsDialog id={collecting} onClose={() => setCollecting(null)} />}
      {removing && (
        <RemoveDialog
          comic={removing}
          onClose={() => setRemoving(null)}
          onRemoved={() => {
            bump();
            if (nav.route.name === "comic" && nav.route.id === removing.id) nav.back();
          }}
        />
      )}
    </Ctx.Provider>
  );
}
