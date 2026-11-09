import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Bell, Compass, RefreshCw, Search, X } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { Segmented, Select } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { TOOLTIP_DELAY_MS } from "../../components/Tooltip";
import { useLibraryData } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import { KIND_LABELS, groupByTitle, languageName, nextHistory, runLimited, type BrowsePage, type SeriesCard, type SourceSummary, type TitleGroup } from "../../domain/online";
import { ApiError, api, errorMessage } from "../../services/api";
import { GridSkeleton, OnlineCover, SeriesGrid, SourceTrouble, siteAnswered } from "./parts";

type Mode = "popular" | "latest" | "following";

interface Section {
  source: SourceSummary;
  status: "loading" | "ready" | "failed";
  results: SeriesCard[];
  page: number;
  hasMore: boolean;
  unverified: number;
  loadingMore: boolean;
  error: string | null;
  /** O site pediu uma verificação anti-robô. */
  protected: boolean;
}

const HISTORY_KEY = "online.searchHistory";
const CONCURRENT_SOURCES = 3;

const keyOf = (source: SourceSummary) => `${source.extensionId}/${source.sourceId}`;

// Volta da página de uma obra para a mesma pesquisa.
const memory = { mode: "popular" as Mode, query: "", source: "", language: "", kind: "" };

/** Quando uma fonte não abre a obra: Explorar já chega com a pesquisa feita em todas. */
export function searchAcrossSources(title: string) {
  Object.assign(memory, { mode: "popular", query: title, source: "" });
}

let requestCounter = 0;

function fetchPage(requestId: string, source: SourceSummary, mode: Mode, query: string, page: number, kind: string): Promise<BrowsePage> {
  const chosenKind = kind || null;
  if (query) return api.sourceSearch(requestId, source, query, page, chosenKind);
  return mode === "latest" ? api.sourceLatest(requestId, source, page, chosenKind) : api.sourcePopular(requestId, source, page, chosenKind);
}

function isCancelled(error: unknown): boolean {
  return error instanceof ApiError && error.code === "extension_cancelled";
}

export function ExploreScreen() {
  const nav = useNav();
  const { data: sources, error: sourcesError } = useLibraryData(() => api.sources(), []);
  const [mode, setMode] = useState<Mode>(memory.mode);
  const [draft, setDraft] = useState(memory.query);
  const [query, setQuery] = useState(memory.query);
  const [sourceFilter, setSourceFilter] = useState(memory.source);
  const [language, setLanguage] = useState(memory.language);
  const [kind, setKind] = useState(memory.kind);
  const [sections, setSections] = useState<Section[]>([]);
  const [following, setFollowing] = useState<SeriesCard[] | null>(null);
  const [followingError, setFollowingError] = useState<string | null>(null);
  const [checkingNew, setCheckingNew] = useState(false);
  const checkRequest = useRef<string | null>(null);
  // Só as obras acompanhadas são consultadas. Cada uma é aberta na fonte, uma de cada vez, e os contadores voltam com a lista.
  const checkNew = async () => {
    if (checkRequest.current) return;
    const requestId = `following-${Date.now()}`;
    checkRequest.current = requestId;
    setCheckingNew(true);
    try {
      setFollowing(await api.seriesRefreshFollowing(requestId));
    } catch (e) {
      if (!(e instanceof ApiError && e.code === "extension_cancelled")) setFollowingError(errorMessage(e));
    } finally {
      checkRequest.current = null;
      setCheckingNew(false);
    }
  };
  useEffect(
    () => () => {
      if (checkRequest.current) void api.sourceCancel(checkRequest.current).catch(() => undefined);
    },
    [],
  );
  const [history, setHistory] = useState<string[]>([]);
  const [focused, setFocused] = useState(false);
  const inFlight = useRef(new Set<string>());
  const run = useRef<object | null>(null);
  const searchBox = useRef<HTMLInputElement>(null);

  Object.assign(memory, { mode, query, source: sourceFilter, language, kind });

  useEffect(() => {
    let alive = true;
    api
      .getSettings()
      .then((settings) => {
        const saved = settings[HISTORY_KEY];
        if (alive && Array.isArray(saved)) setHistory(saved.filter((item): item is string => typeof item === "string"));
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  const saveHistory = (next: string[]) => {
    setHistory(next);
    void api.setSetting(HISTORY_KEY, next).catch(() => undefined);
  };

  const filtered = useMemo(() => {
    return (sources ?? []).filter((source) => (!sourceFilter || keyOf(source) === sourceFilter) && (!language || source.language === language) && (!kind || source.kinds.includes(kind)));
  }, [sources, sourceFilter, language, kind]);
  const selected = useMemo(() => filtered.filter((source) => (query ? source.supportsSearch : mode !== "latest" || source.supportsLatest)), [filtered, query, mode]);

  const nextRequest = useCallback(() => {
    const id = `explore-${++requestCounter}`;
    inFlight.current.add(id);
    return id;
  }, []);
  const cancelAll = useCallback(() => {
    for (const id of inFlight.current) void api.sourceCancel(id).catch(() => undefined);
    inFlight.current.clear();
  }, []);

  const showingFollowing = mode === "following" && !query;

  const loadFirstPage = useCallback(
    async (source: SourceSummary, thisRun: object) => {
      if (run.current !== thisRun) return;
      const patch = (change: Partial<Section>) => {
        if (run.current === thisRun) setSections((all) => all.map((section) => (keyOf(section.source) === keyOf(source) ? { ...section, ...change } : section)));
      };
      const id = nextRequest();
      try {
        const page = await fetchPage(id, source, mode, query, 1, kind);
        siteAnswered(source.sourceId);
        patch({ status: "ready", results: page.results, page: 1, hasMore: page.hasMore, unverified: page.unverified, error: null, protected: false });
      } catch (e) {
        if (!isCancelled(e)) patch({ status: "failed", error: errorMessage(e), protected: e instanceof ApiError && e.code === "source_protected" });
      } finally {
        inFlight.current.delete(id);
      }
    },
    [mode, query, kind, nextRequest],
  );

  // Cada fonte responde no seu tempo: uma lenta ou com erro não segura as outras.
  useEffect(() => {
    if (showingFollowing) return;
    const thisRun = {};
    run.current = thisRun;
    setSections(selected.map((source) => ({ source, status: "loading", results: [], page: 1, hasMore: false, unverified: 0, loadingMore: false, error: null, protected: false })));
    void runLimited(
      selected.map((source) => () => loadFirstPage(source, thisRun)),
      CONCURRENT_SOURCES,
    );
    return () => {
      run.current = null;
      cancelAll();
    };
  }, [selected, loadFirstPage, showingFollowing, cancelAll]);

  // Só a fonte que falhou: as outras já responderam, e o que elas trouxeram continua na tela.
  const retrySource = (source: SourceSummary) => {
    if (!run.current) return;
    setSections((all) => all.map((section) => (keyOf(section.source) === keyOf(source) ? { ...section, status: "loading", error: null, protected: false } : section)));
    void loadFirstPage(source, run.current);
  };

  const groups = useMemo(() => groupByTitle(sections.flatMap(({ source, results }) => results.map((card) => ({ ...card, sourceName: source.name, language: card.language ?? source.language }))), query), [sections, query]);

  useEffect(() => {
    if (!showingFollowing) return;
    let alive = true;
    setFollowingError(null);
    api
      .seriesFollowing()
      .then((items) => alive && setFollowing(items))
      .catch((e: unknown) => alive && setFollowingError(errorMessage(e)));
    return () => {
      alive = false;
    };
  }, [showingFollowing]);

  const loadMore = async (section: Section) => {
    const { source } = section;
    const matches = (other: Section) => keyOf(other.source) === keyOf(source);
    setSections((all) => all.map((other) => (matches(other) ? { ...other, loadingMore: true } : other)));
    const id = nextRequest();
    try {
      const page = await fetchPage(id, source, mode, query, section.page + 1, kind);
      setSections((all) =>
        all.map((other) => {
          if (!matches(other)) return other;
          const known = new Set(other.results.map((item) => item.id));
          return { ...other, results: [...other.results, ...page.results.filter((item) => !known.has(item.id))], page: section.page + 1, hasMore: page.hasMore, unverified: other.unverified + page.unverified, loadingMore: false };
        }),
      );
    } catch (e) {
      setSections((all) => all.map((other) => (matches(other) ? { ...other, loadingMore: false, error: isCancelled(e) ? other.error : errorMessage(e) } : other)));
    } finally {
      inFlight.current.delete(id);
    }
  };

  const submit = (text: string) => {
    const clean = text.trim();
    setDraft(clean);
    setQuery(clean);
    if (clean) {
      if (mode === "following") setMode("popular");
      saveHistory(nextHistory(history, clean));
    }
  };

  const loadMoreEverywhere = () => {
    void runLimited(
      sections.filter((section) => section.hasMore && !section.loadingMore).map((section) => () => loadMore(section)),
      CONCURRENT_SOURCES,
    );
  };

  const openSeries = (series: SeriesCard) => nav.navigate({ name: "series", id: series.id });

  // Sem filtro de fonte, uma pesquisa já mostra todas. Sem pesquisa, falta o nome da obra.
  const searchElsewhere = () => {
    setSourceFilter("");
    if (!query) searchBox.current?.focus();
  };

  if (sourcesError && !sources) {
    return (
      <div className="h-full overflow-y-auto">
        <PageHeader title="Explorar" />
        <div className="p-6">
          <ErrorState title="Não foi possível abrir as fontes" message={sourcesError} />
        </div>
      </div>
    );
  }
  if (!sources) {
    return (
      <div className="p-6">
        <Spinner label="Carregando…" />
      </div>
    );
  }

  const languages = [...new Set(sources.map((source) => source.language))];
  const kinds = [...new Set(sources.flatMap((source) => source.kinds))];
  const searching = sections.filter((section) => section.status === "loading").length;
  const failed = sections.filter((section) => section.status === "failed");
  const byTitle = Boolean(query) && selected.length > 1;
  const withoutSearch = query ? filtered.filter((source) => !source.supportsSearch) : [];
  const uncheckedIn = sections.filter((section) => section.status === "ready" && section.unverified > 0);
  const finished = byTitle ? (failed.length > 0 ? `${sections.length - failed.length} de ${sections.length} fontes responderam` : `${sections.length} fontes consultadas`) : undefined;

  return (
    <div className="h-full overflow-y-auto">
      <PageHeader
        title="Explorar"
        subtitle={searching > 0 ? `Consultando ${searching} de ${sections.length} ${sections.length === 1 ? "fonte" : "fontes"}…` : finished}
        actions={
          sources.length > 0 && !query ? (
            <Segmented
              label="Mostrar"
              hideLabel
              value={mode}
              onChange={setMode}
              options={[
                { value: "popular", label: "Populares" },
                { value: "latest", label: "Recentes" },
                { value: "following", label: "Acompanhando" },
              ]}
            />
          ) : undefined
        }
      >
        {sources.length > 0 && (
          <div className="flex flex-wrap items-center gap-2">
            <form
              role="search"
              className="relative min-w-52 flex-1"
              onSubmit={(e) => {
                e.preventDefault();
                submit(draft);
              }}
            >
              <Search size={14} className="pointer-events-none absolute top-2 left-2.5 text-muted" aria-hidden />
              <input
                ref={searchBox}
                type="search"
                aria-label="Pesquisar nas fontes"
                placeholder="Pesquisar nas fontes"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onFocus={() => setFocused(true)}
                onBlur={() => setFocused(false)}
                className="h-8 w-full rounded-md border border-border-control bg-bg pr-8 pl-8 text-base placeholder:text-muted focus-visible:border-accent focus-visible:outline-2 focus-visible:outline-offset-0 focus-visible:outline-accent [&::-webkit-search-cancel-button]:hidden"
              />
              {draft && <IconButton icon={X} label="Limpar pesquisa" variant="ghost" size="sm" className="absolute top-0.5 right-0.5" onClick={() => submit("")} />}
            </form>
            {sources.length > 1 && (
              <Select label="Fonte" hideLabel value={sourceFilter} onChange={(e) => setSourceFilter(e.target.value)} className="w-44">
                <option value="">Todas as fontes</option>
                {sources.map((source) => (
                  <option key={keyOf(source)} value={keyOf(source)}>
                    {languages.length > 1 ? `${source.name} · ${languageName(source.language)}` : source.name}
                  </option>
                ))}
              </Select>
            )}
            {languages.length > 1 && (
              <Select label="Idioma" hideLabel value={language} onChange={(e) => setLanguage(e.target.value)} className="w-44">
                <option value="">Todos os idiomas</option>
                {languages.map((code) => (
                  <option key={code} value={code}>
                    {languageName(code)}
                  </option>
                ))}
              </Select>
            )}
            {kinds.length > 1 && (
              <Select label="Tipo" hideLabel value={kind} onChange={(e) => setKind(e.target.value)} className="w-44">
                <option value="">Todos os tipos</option>
                {kinds.map((name) => (
                  <option key={name} value={name}>
                    {KIND_LABELS[name] ?? name}
                  </option>
                ))}
              </Select>
            )}
          </div>
        )}
        {focused && !draft && history.length > 0 && (
          // O clique não pode tirar o foco antes de chegar ao botão.
          <div className="mt-2 flex flex-wrap items-center gap-1.5" onMouseDown={(e) => e.preventDefault()}>
            {history.map((item) => (
              <button key={item} type="button" onClick={() => submit(item)} className="rounded-full border border-border px-2 text-sm hover:bg-hover">
                {item}
              </button>
            ))}
            <Button variant="ghost" size="sm" onClick={() => saveHistory([])}>
              Limpar
            </Button>
          </div>
        )}
      </PageHeader>
      <div className="px-6 py-4">
        {sources.length === 0 && (
          <EmptyState icon={Compass} title="Nenhuma fonte instalada" actions={<Button onClick={() => nav.navigate({ name: "extensions" })}>Abrir extensões</Button>}>
            Fontes vêm de extensões, que você instala a partir de um repositório.
          </EmptyState>
        )}
        {showingFollowing && sources.length > 0 && (
          <>
            {followingError && <ErrorState title="Não foi possível abrir a lista" message={followingError} />}
            {following?.length === 0 && (
              <EmptyState icon={Bell} title="Nenhuma obra acompanhada">
                Abra uma obra e escolha “Acompanhar” para encontrá-la aqui, mesmo sem baixar nada.
              </EmptyState>
            )}
            {following && following.length > 0 && (
              <div className="mb-3 flex items-center justify-end gap-2">
                {checkingNew && <Spinner size={14} label="Procurando capítulos novos…" />}
                <IconButton icon={RefreshCw} label="Procurar capítulos novos" variant="secondary" size="sm" tooltipDelay={TOOLTIP_DELAY_MS} disabled={checkingNew} onClick={() => void checkNew()} />
              </div>
            )}
            {following && <SeriesGrid items={following} onOpen={openSeries} showSource />}
          </>
        )}
        {!showingFollowing && sources.length > 0 && selected.length === 0 && <EmptyState icon={Search} title="Nenhuma fonte com esses filtros" />}
        {!showingFollowing && byTitle && (
          <>
            {failed.length > 0 && <UnansweredSources failed={failed} onRetry={retrySource} />}
            {groups.length === 0 && searching > 0 && <TitlesSkeleton />}
            {searching > 0 && <p className="p-enter-late mt-3 text-sm text-muted">{CHECKING_NOTE}</p>}
            {groups.length === 0 && searching === 0 && uncheckedIn.length === 0 && (
              <EmptyState icon={Search} title="Nada encontrado">
                {failed.length > 0 ? "Nada com esse nome nas fontes que responderam. As outras podem ter." : "Nenhuma fonte encontrou esse nome."} A mesma obra pode ter outro título em cada fonte: vale tentar outro.
              </EmptyState>
            )}
            {groups.length > 0 && <TitleResults groups={groups} showLanguage={languages.length > 1} onOpen={openSeries} />}
            {sections.some((section) => section.hasMore) && (
              <div className="mt-3">
                <Button onClick={loadMoreEverywhere} loading={sections.some((section) => section.loadingMore)}>
                  Carregar mais
                </Button>
              </div>
            )}
            {groups.length > 0 && searching === 0 && <p className="mt-4 text-sm text-muted">A mesma obra pode ter outro título em cada fonte. Se não achar, vale tentar outro nome.</p>}
            {withoutSearch.length > 0 && <p className="mt-1 text-sm text-muted">Sem pesquisa, ficaram de fora: {LIST_FORMAT.format(withoutSearch.map((source) => source.name))}.</p>}
            {uncheckedIn.length > 0 && (
              <p className="mt-1 flex flex-wrap items-center gap-2 text-sm text-muted">
                Não deu para conferir os detalhes de obras de: {LIST_FORMAT.format(uncheckedIn.map((section) => section.source.name))}.
                <Button size="sm" onClick={() => uncheckedIn.forEach((section) => retrySource(section.source))}>
                  Tentar de novo
                </Button>
              </p>
            )}
          </>
        )}
        {!showingFollowing &&
          !byTitle &&
          sections.map((section) => (
            <section key={keyOf(section.source)} aria-label={section.source.name} className="mb-8 last:mb-0">
              <h2 className="mb-2 flex flex-wrap items-center gap-2 text-base font-semibold">
                {section.source.name}
                {sources.length > 1 && <span className="text-sm font-normal text-muted">{languageName(section.source.language)}</span>}
                {section.status === "loading" && <Spinner size={14} />}
                {section.status === "loading" && <span className="p-enter-late text-sm font-normal text-muted">{CHECKING_NOTE}</span>}
              </h2>
              {section.status === "loading" && <GridSkeleton />}
              {section.status === "failed" && (
                <div role="alert">
                  <SourceTrouble message={section.error ?? ""} site={section.protected ? { sourceId: section.source.sourceId } : undefined} sourceId={section.source.sourceId} onRetry={() => retrySource(section.source)} elsewhere={sources.length > 1 ? searchElsewhere : undefined} size="sm">
                    {troubleLayout}
                  </SourceTrouble>
                </div>
              )}
              {section.status === "ready" && section.results.length === 0 && !(section.unverified > 0) && <p className="text-sm text-muted">Nada encontrado.</p>}
              {section.results.length > 0 && <SeriesGrid items={section.results} onOpen={openSeries} />}
              {section.status === "ready" && section.unverified > 0 && (
                <p className="mt-3 flex flex-wrap items-center gap-2 text-sm text-muted">
                  Não deu para conferir os detalhes de {section.unverified === 1 ? "1 obra" : `${section.unverified} obras`} agora.
                  <Button size="sm" onClick={() => retrySource(section.source)}>
                    Tentar de novo
                  </Button>
                </p>
              )}
              {section.hasMore && (
                <div className="mt-3">
                  <Button onClick={() => void loadMore(section)} loading={section.loadingMore}>
                    Carregar mais
                  </Button>
                </div>
              )}
            </section>
          ))}
      </div>
    </div>
  );
}

const LIST_FORMAT = new Intl.ListFormat("pt-BR", { type: "conjunction" });
const CHECKING_NOTE = "Conferindo os detalhes das obras. Na primeira visita, pode passar de um minuto.";
const NAMES_SHOWN = 3;

const troubleLayout = (text: string, actions: ReactNode) => (
  <>
    <p className="mb-2 text-sm text-muted">{text}</p>
    {actions}
  </>
);

/** Quem não respondeu não disse que a obra não está lá. A explicação e as saídas abrem quando a pessoa quiser. */
function UnansweredSources({ failed, onRetry }: { failed: Section[]; onRetry: (source: SourceSummary) => void }) {
  const names = failed.map((section) => section.source.name);
  const rest = names.length - NAMES_SHOWN;
  const listed = LIST_FORMAT.format(rest > 0 ? [...names.slice(0, NAMES_SHOWN), `mais ${rest}`] : names);
  return (
    <details className="mb-4 rounded-md border border-border bg-surface px-4 py-2.5">
      <summary className="cursor-pointer text-sm">
        <span className="font-medium">
          {listed} {failed.length === 1 ? "não respondeu" : "não responderam"}.
        </span>{" "}
        A obra pode estar {failed.length === 1 ? "nela" : "nelas"}.
      </summary>
      <ul className="mt-2 divide-y divide-border">
        {failed.map((section) => (
          <li key={keyOf(section.source)} className="py-3">
            <p className="text-sm font-medium">{section.source.name}</p>
            <SourceTrouble message={section.error ?? ""} site={section.protected ? { sourceId: section.source.sourceId } : undefined} sourceId={section.source.sourceId} onRetry={() => onRetry(section.source)} size="sm">
              {troubleLayout}
            </SourceTrouble>
          </li>
        ))}
      </ul>
    </details>
  );
}

function TitleResults({ groups, showLanguage, onOpen }: { groups: TitleGroup[]; showLanguage: boolean; onOpen: (series: SeriesCard) => void }) {
  return (
    <ul className="divide-y divide-border">
      {groups.map((group) => {
        const sourceCount = new Set(group.entries.map((series) => `${series.extensionId}/${series.sourceId}`)).size;
        return (
          <li key={group.key} className="py-4 first:pt-0">
            <h2 className="text-base font-semibold break-words">{group.title}</h2>
            {sourceCount > 1 && <p className="text-sm text-muted">Mesmo título em {sourceCount} fontes. Confira as capas.</p>}
            <ul aria-label={`Onde ler ${group.title}`} className="mt-2 grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-2">
              {group.entries.map((series) => (
                <li key={series.id}>
                  <button type="button" aria-label={`Abrir ${series.title} em ${series.sourceName}`} onClick={() => onOpen(series)} className="flex w-full items-center gap-3 rounded-md border border-border p-2 text-left hover:bg-hover focus-visible:outline-2 focus-visible:outline-accent">
                    <span className="block w-12 shrink-0">
                      <OnlineCover series={series} />
                    </span>
                    <span className="min-w-0">
                      <span className="block truncate text-sm font-medium">{series.sourceName}</span>
                      {showLanguage && series.language && <span className="block truncate text-xs text-muted">{languageName(series.language)}</span>}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </li>
        );
      })}
    </ul>
  );
}

function TitlesSkeleton() {
  return (
    <div className="flex flex-col gap-3" aria-hidden>
      {Array.from({ length: 3 }, (_, i) => (
        <div key={i} className="p-skeleton h-24 rounded-md bg-elevated" />
      ))}
    </div>
  );
}
