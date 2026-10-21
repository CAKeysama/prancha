import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowDownWideNarrow, ArrowUpNarrowWide, LayoutGrid, List, Search, SearchX, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { IconButton } from "../../components/Button";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { Segmented, Select } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { cn } from "../../components/cn";
import { STATUS_LABELS } from "../../domain/progress";
import { FORMAT_LABELS, type ComicFormat, type LibraryQuery, type ReadStatus, type SortKey } from "../../domain/types";
import { api } from "../../services/api";
import { useLibraryData } from "../../app/library-events";
import { useSettings } from "../../app/settings-context";
import { AddComicsButtons } from "./AddComicsButtons";
import { ComicCollectionView } from "./ComicCollectionView";

export interface LibraryPreset {
  title: string;
  subtitle?: ReactNode;
  base: LibraryQuery;
  /** A biblioteca principal persiste ordenação e permite todos os filtros. */
  persistSort?: boolean;
  defaultSort?: SortKey;
  defaultDesc?: boolean;
  allowStatusFilter?: boolean;
  emptyIcon: LucideIcon;
  emptyTitle: string;
  emptyText: ReactNode;
  emptyActions?: ReactNode;
  headerActions?: ReactNode;
  onBack?: () => void;
  backLabel?: string;
  collectionId?: number;
}

const SORT_LABELS: Record<Exclude<SortKey, "recentProgress">, string> = {
  title: "Título",
  added: "Data de adição",
  opened: "Último acesso",
  progress: "Progresso",
};

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = window.setTimeout(() => setV(value), ms);
    return () => window.clearTimeout(t);
  }, [value, ms]);
  return v;
}

function Chip({ pressed, onClick, children }: { pressed: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      onClick={onClick}
      className={cn(
        "inline-flex h-7 items-center gap-1 rounded-full border px-3 text-sm font-medium transition-colors",
        pressed ? "border-accent bg-accent-subtle text-accent-fg" : "border-border text-muted hover:bg-hover hover:text-fg",
      )}
    >
      {children}
    </button>
  );
}

export function LibraryScreen({ preset }: { preset: LibraryPreset }) {
  const { settings, update } = useSettings();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [text, setText] = useState("");
  const [statuses, setStatuses] = useState<ReadStatus[]>([]);
  const [format, setFormat] = useState<ComicFormat | "">("");
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [localSort, setLocalSort] = useState<SortKey>(preset.defaultSort ?? "title");
  const [localDesc, setLocalDesc] = useState(preset.defaultDesc ?? false);
  const sort = preset.persistSort ? settings["library.sort"] : localSort;
  const desc = preset.persistSort ? settings["library.sortDesc"] : localDesc;
  const view = settings["library.view"];
  const search = useDebounced(text, 180);

  const query = useMemo<LibraryQuery>(
    () => ({
      ...preset.base,
      text: search.trim() || undefined,
      statuses: statuses.length ? statuses : preset.base.statuses,
      formats: format ? [format] : undefined,
      favoritesOnly: favoritesOnly || preset.base.favoritesOnly,
      sort,
      descending: desc,
    }),
    [preset.base, search, statuses, format, favoritesOnly, sort, desc],
  );
  const { data, error, loading, reload } = useLibraryData(() => api.listComics(query), [JSON.stringify(query)]);
  const filtering = Boolean(search.trim() || statuses.length || format || favoritesOnly);

  const setSort = (s: SortKey) => (preset.persistSort ? void update("library.sort", s) : setLocalSort(s));
  const toggleDesc = () => (preset.persistSort ? void update("library.sortDesc", !desc) : setLocalDesc(!desc));
  const toggleStatus = (s: ReadStatus) => setStatuses((v) => (v.includes(s) ? v.filter((x) => x !== s) : [...v, s]));
  const clearFilters = () => {
    setText("");
    setStatuses([]);
    setFormat("");
    setFavoritesOnly(false);
  };

  const countText = data ? `${data.length} ${data.length === 1 ? "quadrinho" : "quadrinhos"}${filtering ? " encontrados" : ""}` : undefined;

  return (
    <div ref={scrollRef} className="h-full overflow-y-auto">
      <PageHeader
        title={preset.title}
        subtitle={preset.subtitle ?? countText}
        onBack={preset.onBack}
        backLabel={preset.backLabel}
        actions={preset.headerActions ?? <AddComicsButtons />}
      >
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <div className="relative w-full max-w-xs min-w-48 flex-1">
            <Search size={15} aria-hidden className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted" />
            <label htmlFor="library-search" className="sr-only">
              Pesquisar por título, arquivo, série, autor ou tag
            </label>
            <input
              id="library-search"
              type="search"
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape" && text) {
                  e.stopPropagation();
                  setText("");
                }
              }}
              placeholder="Pesquisar título, arquivo, série, autor, tag…"
              className="h-8 w-full rounded-md border border-border-control bg-bg pr-8 pl-8 text-base placeholder:text-muted focus-visible:border-accent focus-visible:outline-2 focus-visible:outline-offset-0 focus-visible:outline-accent"
            />
            {text && (
              <button
                type="button"
                aria-label="Limpar pesquisa"
                onClick={() => setText("")}
                className="absolute top-1/2 right-1.5 inline-flex size-6 -translate-y-1/2 items-center justify-center rounded text-muted hover:bg-hover hover:text-fg"
              >
                <X size={14} aria-hidden />
              </button>
            )}
          </div>
          {preset.allowStatusFilter !== false && (
            <div role="group" aria-label="Filtrar por estado de leitura" className="flex flex-wrap gap-1.5">
              {(["unread", "reading", "completed"] as ReadStatus[]).map((s) => (
                <Chip key={s} pressed={statuses.includes(s)} onClick={() => toggleStatus(s)}>
                  {STATUS_LABELS[s]}
                </Chip>
              ))}
              {!preset.base.favoritesOnly && (
                <Chip pressed={favoritesOnly} onClick={() => setFavoritesOnly((v) => !v)}>
                  Favoritos
                </Chip>
              )}
            </div>
          )}
          <Select label="Tipo de arquivo" hideLabel value={format} onChange={(e) => setFormat(e.target.value as ComicFormat | "")} className="w-40">
            <option value="">Todos os tipos</option>
            {(Object.keys(FORMAT_LABELS) as ComicFormat[]).map((f) => (
              <option key={f} value={f}>
                {FORMAT_LABELS[f]}
              </option>
            ))}
          </Select>
          <div className="ml-auto flex items-center gap-1.5">
            <Select label="Ordenar por" hideLabel value={sort === "recentProgress" ? "opened" : sort} onChange={(e) => setSort(e.target.value as SortKey)} className="w-40">
              {(Object.keys(SORT_LABELS) as Array<keyof typeof SORT_LABELS>).map((k) => (
                <option key={k} value={k}>
                  Ordenar: {SORT_LABELS[k]}
                </option>
              ))}
            </Select>
            <IconButton
              variant="secondary"
              icon={desc ? ArrowDownWideNarrow : ArrowUpNarrowWide}
              label={desc ? "Ordem decrescente (clique para inverter)" : "Ordem crescente (clique para inverter)"}
              onClick={toggleDesc}
            />
            <Segmented
              label="Modo de visualização"
              hideLabel
              value={view}
              onChange={(v) => void update("library.view", v)}
              options={[
                { value: "grid", label: "Grade", icon: LayoutGrid },
                { value: "list", label: "Lista", icon: List },
              ]}
            />
          </div>
        </div>
      </PageHeader>
      <div className="px-6 py-5">
        {error && <ErrorState title="Não foi possível carregar a biblioteca" message={error} action={<button className="text-sm font-medium text-accent-fg hover:underline" onClick={reload}>Tentar novamente</button>} />}
        {!data && loading && <Spinner label="Carregando quadrinhos…" />}
        {data && data.length === 0 && !filtering && (
          <EmptyState icon={preset.emptyIcon} title={preset.emptyTitle} actions={preset.emptyActions}>
            {preset.emptyText}
          </EmptyState>
        )}
        {data && data.length === 0 && filtering && (
          <EmptyState
            icon={SearchX}
            title="Nenhum quadrinho corresponde aos filtros"
            actions={
              <button type="button" onClick={clearFilters} className="text-sm font-medium text-accent-fg hover:underline">
                Limpar pesquisa e filtros
              </button>
            }
          >
            Tente outro termo ou remova algum filtro.
          </EmptyState>
        )}
        {data && data.length > 0 && <ComicCollectionView comics={data} view={view} scrollRef={scrollRef} collectionId={preset.collectionId} />}
      </div>
    </div>
  );
}
