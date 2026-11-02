import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { LayoutGrid, List, Search, SearchX, SlidersHorizontal, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Button } from "../../components/Button";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { Checkbox, Segmented, Select } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { cn } from "../../components/cn";
import { STATUS_LABELS } from "../../domain/progress";
import { FORMAT_LABELS, type ComicFormat, type LibraryQuery, type ReadStatus, type SortKey } from "../../domain/types";
import { api } from "../../services/api";
import { useLibraryData } from "../../app/library-events";
import { useSettings } from "../../app/settings-context";
import { AddComicsButtons } from "./AddComicsButtons";
import { ComicCollectionView } from "./ComicCollectionView";

interface LibraryPreset {
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

/** Ordenação em um único seletor (campo e direção juntos, com nomes que dizem o resultado). */
const SORT_OPTIONS: Array<{ value: string; label: string }> = [
  { value: "title:asc", label: "Título (A–Z)" },
  { value: "title:desc", label: "Título (Z–A)" },
  { value: "added:desc", label: "Adicionados recentemente" },
  { value: "added:asc", label: "Adicionados há mais tempo" },
  { value: "opened:desc", label: "Abertos recentemente" },
  { value: "progress:desc", label: "Mais lidos" },
  { value: "progress:asc", label: "Menos lidos" },
];

function ActiveChip({ label, onRemove }: { label: string; onRemove: () => void }) {
  return (
    <span className="inline-flex h-6 items-center gap-1 rounded-full border border-accent/50 bg-accent-subtle pr-1 pl-2.5 text-sm text-accent-fg">
      {label}
      <button type="button" onClick={onRemove} aria-label={`Remover filtro ${label}`} className="inline-flex size-5 items-center justify-center rounded-full hover:bg-hover">
        <X size={12} aria-hidden />
      </button>
    </span>
  );
}

/** Filtros ficam um nível abaixo (revelação progressiva); o botão mostra quantos estão ativos. */
function FilterButton(props: {
  count: number;
  allowStatus: boolean;
  allowFavorites: boolean;
  statuses: ReadStatus[];
  onToggleStatus: (s: ReadStatus) => void;
  favoritesOnly: boolean;
  onFavorites: (v: boolean) => void;
  format: ComicFormat | "";
  onFormat: (f: ComicFormat | "") => void;
  onClear: () => void;
}) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);
  return (
    <div
      ref={wrap}
      className="relative"
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.stopPropagation();
          setOpen(false);
        }
      }}
    >
      <Button icon={SlidersHorizontal} aria-expanded={open} aria-haspopup="dialog" onClick={() => setOpen((o) => !o)}>
        Filtrar
        {props.count > 0 && (
          <span className="ml-0.5 inline-flex min-w-5 items-center justify-center rounded-full bg-accent-emphasis px-1.5 text-xs text-on-emphasis">
            {props.count}
            <span className="sr-only"> ativos</span>
          </span>
        )}
      </Button>
      {open && (
        <div role="dialog" aria-label="Filtros" className="p-enter absolute top-full left-0 z-30 mt-1 w-72 space-y-4 rounded-lg border border-border bg-elevated p-4 shadow-overlay">
          {props.allowStatus && (
            <div role="group" aria-label="Estado de leitura">
              <p className="mb-1.5 text-sm font-medium">Estado de leitura</p>
              <div className="flex flex-wrap gap-1.5">
                {(["unread", "reading", "completed"] as ReadStatus[]).map((s) => (
                  <Chip key={s} pressed={props.statuses.includes(s)} onClick={() => props.onToggleStatus(s)}>
                    {STATUS_LABELS[s]}
                  </Chip>
                ))}
              </div>
            </div>
          )}
          {props.allowFavorites && (
            <Checkbox label="Somente favoritos" checked={props.favoritesOnly} onChange={props.onFavorites} />
          )}
          <Select label="Tipo de arquivo" value={props.format} onChange={(e) => props.onFormat(e.target.value as ComicFormat | "")}>
            <option value="">Todos os tipos</option>
            {(Object.keys(FORMAT_LABELS) as ComicFormat[]).map((f) => (
              <option key={f} value={f}>
                {FORMAT_LABELS[f]}
              </option>
            ))}
          </Select>
          {props.count > 0 && (
            <button type="button" onClick={props.onClear} className="text-sm font-medium text-accent-fg hover:underline">
              Limpar filtros
            </button>
          )}
        </div>
      )}
    </div>
  );
}

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

  const setSortValue = (value: string) => {
    const [key, dir] = value.split(":") as [SortKey, string];
    if (preset.persistSort) {
      void update("library.sort", key);
      void update("library.sortDesc", dir === "desc");
    } else {
      setLocalSort(key);
      setLocalDesc(dir === "desc");
    }
  };
  const toggleStatus = (s: ReadStatus) => setStatuses((v) => (v.includes(s) ? v.filter((x) => x !== s) : [...v, s]));
  const clearChips = () => {
    setStatuses([]);
    setFormat("");
    setFavoritesOnly(false);
  };
  const clearFilters = () => {
    setText("");
    clearChips();
  };

  const countText = data
    ? filtering
      ? `${data.length} ${data.length === 1 ? "encontrado" : "encontrados"}`
      : `${data.length} ${data.length === 1 ? "quadrinho" : "quadrinhos"}`
    : undefined;

  return (
    <div ref={scrollRef} className="h-full overflow-y-auto">
      <PageHeader
        title={preset.title}
        subtitle={preset.subtitle ?? countText}
        onBack={preset.onBack}
        backLabel={preset.backLabel}
        actions={preset.headerActions ?? <AddComicsButtons />}
      >
        <div className="flex items-center gap-2">
          <div className="relative min-w-40 flex-1 sm:max-w-sm">
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
              placeholder="Pesquisar"
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
          <FilterButton
            count={statuses.length + (format ? 1 : 0) + (favoritesOnly ? 1 : 0)}
            allowStatus={preset.allowStatusFilter !== false}
            allowFavorites={!preset.base.favoritesOnly}
            statuses={statuses}
            onToggleStatus={toggleStatus}
            favoritesOnly={favoritesOnly}
            onFavorites={setFavoritesOnly}
            format={format}
            onFormat={setFormat}
            onClear={clearChips}
          />
          <div className="ml-auto flex items-center gap-2">
            <Select label="Ordenar" hideLabel value={`${sort === "recentProgress" ? "opened" : sort}:${desc ? "desc" : "asc"}`} onChange={(e) => setSortValue(e.target.value)} className="w-52 max-sm:w-40">
              {SORT_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </Select>
            <Segmented
              label="Modo de visualização"
              hideLabel
              compactLabels
              value={view}
              onChange={(v) => void update("library.view", v)}
              options={[
                { value: "grid", label: "Grade", icon: LayoutGrid },
                { value: "list", label: "Lista", icon: List },
              ]}
            />
          </div>
        </div>
        {(statuses.length > 0 || format || favoritesOnly) && (
          <div className="mt-2 flex flex-wrap items-center gap-1.5" aria-label="Filtros ativos">
            {statuses.map((s) => (
              <ActiveChip key={s} label={STATUS_LABELS[s]} onRemove={() => toggleStatus(s)} />
            ))}
            {favoritesOnly && <ActiveChip label="Favoritos" onRemove={() => setFavoritesOnly(false)} />}
            {format && <ActiveChip label={FORMAT_LABELS[format]} onRemove={() => setFormat("")} />}
          </div>
        )}
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
