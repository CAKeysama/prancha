import { useState, type ReactNode } from "react";
import { BookOpen, FileSearch, FileWarning, FolderPlus, MoreHorizontal, Pencil, RotateCcw, Star, Trash2 } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { Cover } from "../../components/Cover";
import { ConfirmDialog } from "../../components/Dialog";
import { ErrorState, ProgressBar, Spinner } from "../../components/Feedback";
import { Menu } from "../../components/Menu";
import { PageHeader } from "../../components/PageHeader";
import { useToast } from "../../components/Toast";
import { STATUS_LABELS, formatBytes, formatDate, progressLabel, progressRatio, readActionLabel } from "../../domain/progress";
import { FORMAT_LABELS, type ComicDetails, type FieldSource } from "../../domain/types";
import { api, errorMessage } from "../../services/api";
import { useLibraryData, useLibraryVersion } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import { SourceBadge, useComicActions } from "./comic-actions";

const COVER_KIND_TEXT: Record<string, string> = {
  metadata: "Capa indicada nos metadados do arquivo (ComicInfo).",
  format: "Imagem de capa identificada pelo nome dentro do arquivo.",
  first_page: "Miniatura gerada a partir da primeira página válida.",
  placeholder: "Capa substituta: a imagem original não pôde ser extraída.",
  pending: "A capa está sendo preparada.",
};

function Row({ label, children, source }: { label: string; children: ReactNode; source?: FieldSource }) {
  return (
    <div className="grid grid-cols-[140px_1fr] gap-3 border-b border-border py-2 last:border-b-0 max-sm:grid-cols-1 max-sm:gap-0.5">
      <dt className="flex items-center gap-1.5 text-sm text-muted">
        {label}
        {source === "user" && <SourceBadge source={source} />}
      </dt>
      <dd className="min-w-0 text-base break-words">{children}</dd>
    </div>
  );
}

function MissingBanner({ comic }: { comic: ComicDetails }) {
  const toast = useToast();
  const { bump } = useLibraryVersion();
  const [busy, setBusy] = useState(false);
  const [mismatch, setMismatch] = useState<string | null>(null);
  const relocate = async (force: boolean, path?: string) => {
    setBusy(true);
    try {
      const r = await api.relocateComic(comic.id, force, path);
      if (!r.picked) return;
      if (!r.matched && !r.updated) {
        setMismatch(r.path);
        return;
      }
      setMismatch(null);
      toast.success("Arquivo localizado", r.matched ? "O conteúdo confere com o original. O progresso foi mantido." : "A HQ agora aponta para o novo arquivo.");
      bump();
    } catch (e) {
      toast.error("Não foi possível usar esse arquivo", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div role="alert" className="mb-5 flex flex-wrap items-start gap-3 rounded-md border border-danger/50 bg-danger-subtle px-4 py-3">
      <FileWarning size={18} className="mt-0.5 shrink-0 text-danger" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="font-semibold">Arquivo original não encontrado</p>
        <p className="mt-0.5 text-sm">
          O {comic.format === "folder" ? "diretório" : "arquivo"} não está mais em <span className="font-mono text-xs break-all">{comic.path}</span>. Ele
          pode ter sido movido, renomeado ou estar em um disco desconectado. As informações e o progresso continuam salvos.
        </p>
      </div>
      <Button icon={FileSearch} onClick={() => void relocate(false)} loading={busy}>
        Localizar arquivo…
      </Button>
      <ConfirmDialog
        open={mismatch !== null}
        title="O conteúdo é diferente do original"
        confirmLabel="Usar este arquivo mesmo assim"
        onCancel={() => setMismatch(null)}
        onConfirm={() => void relocate(true, mismatch ?? undefined)}
      >
        O arquivo escolhido não é idêntico ao que foi importado. Se continuar, a HQ passará a usar o novo arquivo, a capa será refeita e o
        progresso salvo será mantido.
      </ConfirmDialog>
    </div>
  );
}

export function DetailsScreen({ id }: { id: number }) {
  const nav = useNav();
  const actions = useComicActions();
  const toast = useToast();
  const { data: comic, error } = useLibraryData(() => api.comic(id), [id]);
  const [confirmReset, setConfirmReset] = useState(false);

  if (error && !comic) {
    return (
      <div className="h-full overflow-y-auto">
        <PageHeader title="Detalhes" onBack={nav.back} />
        <div className="p-6">
          <ErrorState title="Não foi possível abrir os detalhes" message={error} />
        </div>
      </div>
    );
  }
  if (!comic) {
    return (
      <div className="p-6">
        <Spinner label="Carregando detalhes…" />
      </div>
    );
  }
  const ratio = progressRatio(comic);
  const src = comic.fieldSources;
  return (
    <div className="h-full overflow-y-auto">
      <PageHeader
        title={comic.title}
        onBack={nav.back}
        actions={
          <Menu
            label="Mais ações"
            items={[
              ...(comic.status !== "unread" ? [{ label: "Reiniciar progresso", icon: RotateCcw, onSelect: () => setConfirmReset(true) }] : []),
              ...actions.menuFor(comic, { inDetails: true }),
            ]}
            trigger={(p) => <IconButton {...p} variant="secondary" icon={MoreHorizontal} label="Mais ações" />}
          />
        }
      />
      <div className="mx-auto max-w-5xl px-6 py-6">
        {comic.missing && <MissingBanner comic={comic} />}
        <div className="flex gap-8 max-md:flex-col">
          <div className="w-64 shrink-0 max-md:mx-auto max-md:w-48">
            <Cover comicId={comic.id} title={comic.title} kind={comic.coverKind} rev={comic.coverRev} error={comic.coverError} eager />
            {(comic.coverKind === "placeholder" || comic.coverKind === "pending") && (
              <p className="mt-2 text-xs text-muted">
                {COVER_KIND_TEXT[comic.coverKind]}
                {comic.coverKind === "placeholder" && comic.coverError && ` Motivo: ${comic.coverError}.`}
              </p>
            )}
          </div>
          <div className="min-w-0 flex-1">
            <div className="rounded-md border border-border bg-surface p-4">
              {comic.status === "reading" && <p className="text-sm text-muted">{STATUS_LABELS.reading}</p>}
              <p className="text-lg font-semibold">{progressLabel(comic)}</p>
              {ratio !== null && <ProgressBar value={ratio} label="Progresso da leitura" className="mt-2" tone={comic.status === "completed" ? "success" : "accent"} />}
              <div className="mt-4 flex flex-wrap gap-2">
                <Button variant="primary" icon={BookOpen} onClick={() => actions.read(comic)} disabled={comic.missing}>
                  {readActionLabel(comic)}
                </Button>
                <Button icon={Star} onClick={() => void actions.toggleFavorite(comic)} aria-pressed={comic.favorite}>
                  {comic.favorite ? "Remover dos favoritos" : "Favoritar"}
                </Button>
                <Button icon={Pencil} onClick={() => actions.edit(comic.id)}>
                  Editar informações
                </Button>
                <Button icon={FolderPlus} onClick={() => actions.collections(comic.id)}>
                  Organizar em coleções
                </Button>
              </div>
            </div>

            <h2 className="mt-6 mb-1 text-base font-semibold">Informações</h2>
            <dl>
              {comic.series && (
                <Row label="Série" source={src.series}>
                  {comic.series}
                </Row>
              )}
              {(comic.issueNumber || comic.volume) && (
                <Row label="Número / volume" source={src.issueNumber ?? src.volume}>
                  {[comic.issueNumber && `#${comic.issueNumber}`, comic.volume && `vol. ${comic.volume}`].filter(Boolean).join(" · ")}
                </Row>
              )}
              {comic.writer && (
                <Row label="Autor" source={src.writer}>
                  {comic.writer}
                </Row>
              )}
              {comic.artist && (
                <Row label="Artista" source={src.artist}>
                  {comic.artist}
                </Row>
              )}
              {comic.publisher && (
                <Row label="Editora" source={src.publisher}>
                  {comic.publisher}
                </Row>
              )}
              {comic.year && (
                <Row label="Ano" source={src.year}>
                  {comic.year}
                </Row>
              )}
              {comic.tags.length > 0 && (
                <Row label="Tags" source={src.tags}>
                  <span className="flex flex-wrap gap-1.5">
                    {comic.tags.map((t) => (
                      <span key={t} className="rounded-full border border-border px-2 text-sm">
                        {t}
                      </span>
                    ))}
                  </span>
                </Row>
              )}
              {comic.collections.length > 0 && (
                <Row label="Coleções">
                  <span className="flex flex-wrap gap-1.5">
                    {comic.collections.map((c) => (
                      <button
                        key={c.id}
                        type="button"
                        onClick={() => nav.navigate({ name: "collection", id: c.id })}
                        className="rounded-full border border-border px-2 text-sm text-accent-fg hover:bg-hover"
                      >
                        {c.name}
                      </button>
                    ))}
                  </span>
                </Row>
              )}
              {comic.summaryText && (
                <Row label="Sinopse" source={src.summary}>
                  <span className="whitespace-pre-line">{comic.summaryText}</span>
                </Row>
              )}
            </dl>

            <h2 className="mt-6 mb-1 text-base font-semibold">Arquivo</h2>
            <dl>
              <Row label="Nome do arquivo">{comic.fileName}</Row>
              <Row label="Formato">{FORMAT_LABELS[comic.format]}</Row>
              <Row label="Páginas">{comic.pageCount ?? "Será conhecido ao abrir"}</Row>
              <Row label="Tamanho">{formatBytes(comic.fileSize)}</Row>
              <Row label="Localização">
                <span className="font-mono text-sm break-all select-text">{comic.path}</span>
                {!comic.missing && (
                  <button
                    type="button"
                    className="ml-2 text-sm font-medium text-accent-fg hover:underline"
                    onClick={() => void api.revealComic(comic.id).catch((e) => toast.error("Não foi possível abrir a pasta", errorMessage(e)))}
                  >
                    Mostrar na pasta
                  </button>
                )}
              </Row>
              <Row label="Adicionado em">{formatDate(comic.addedAt)}</Row>
              <Row label="Último acesso">{comic.lastOpenedAt ? formatDate(comic.lastOpenedAt) : "Nunca aberto"}</Row>
            </dl>

            <div className="mt-8 border-t border-border pt-4">
              <Button variant="danger" size="sm" icon={Trash2} onClick={() => actions.remove(comic)}>
                Remover da biblioteca…
              </Button>
            </div>
          </div>
        </div>
      </div>
      <ConfirmDialog
        open={confirmReset}
        title="Reiniciar o progresso?"
        confirmLabel="Reiniciar progresso"
        onCancel={() => setConfirmReset(false)}
        onConfirm={() => {
          setConfirmReset(false);
          void api
            .resetProgress(comic.id)
            .then(() => toast.success("Progresso reiniciado", "A leitura recomeçará da primeira página."))
            .catch((e) => toast.error("Não foi possível reiniciar", errorMessage(e)));
        }}
      >
        A posição salva será substituída pela primeira página.
      </ConfirmDialog>
    </div>
  );
}
