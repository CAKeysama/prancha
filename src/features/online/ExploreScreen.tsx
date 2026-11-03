import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Bell, Compass, Search, X } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { Segmented, Select } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { useLibraryData } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import { KIND_LABELS, languageName, nextHistory, runLimited, type BrowsePage, type SeriesCard, type SourceSummary } from "../../domain/online";
import { ApiError, api, errorMessage } from "../../services/api";
import { GridSkeleton, SeriesGrid, VerifySiteButton } from "./parts";

type Mode = "popular" | "latest" | "following";

interface Section {
  source: SourceSummary;
  status: "loading" | "ready" | "failed";
  results: SeriesCard[];
  page: number;
  hasMore: boolean;
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
  const [history, setHistory] = useState<string[]>([]);
  const [focused, setFocused] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const inFlight = useRef(new Set<string>());

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

  const selected = useMemo(() => {
    return (sources ?? []).filter(
      (source) =>
        (!sourceFilter || keyOf(source) === sourceFilter) &&
        (!language || source.language === language) &&
        (!kind || source.kinds.includes(kind)) &&
        (query ? source.supportsSearch : mode !== "latest" || source.supportsLatest),
    );
  }, [sources, sourceFilter, language, kind, query, mode]);

  const nextRequest = () => {
    const id = `explore-${++requestCounter}`;
    inFlight.current.add(id);
    return id;
  };
  const cancelAll = useCallback(() => {
    for (const id of inFlight.current) void api.sourceCancel(id).catch(() => undefined);
    inFlight.current.clear();
  }, []);

  const showingFollowing = mode === "following" && !query;

  // Cada fonte responde no seu tempo: uma lenta ou com erro não segura as outras.
  useEffect(() => {
    if (showingFollowing) return;
    let alive = true;
    setSections(selected.map((source) => ({ source, status: "loading", results: [], page: 1, hasMore: false, loadingMore: false, error: null, protected: false })));
    const patch = (source: SourceSummary, change: Partial<Section>) => {
      if (alive) setSections((all) => all.map((section) => (keyOf(section.source) === keyOf(source) ? { ...section, ...change } : section)));
    };
    void runLimited(
      selected.map((source) => async () => {
        if (!alive) return;
        const id = nextRequest();
        try {
          const page = await fetchPage(id, source, mode, query, 1, kind);
          patch(source, { status: "ready", results: page.results, hasMore: page.hasMore });
        } catch (e) {
          if (!isCancelled(e)) patch(source, { status: "failed", error: errorMessage(e), protected: e instanceof ApiError && e.code === "source_protected" });
        } finally {
          inFlight.current.delete(id);
        }
      }),
      CONCURRENT_SOURCES,
    );
    return () => {
      alive = false;
      cancelAll();
    };
  }, [selected, mode, query, kind, attempt, showingFollowing, cancelAll]);

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
          return { ...other, results: [...other.results, ...page.results.filter((item) => !known.has(item.id))], page: section.page + 1, hasMore: page.hasMore, loadingMore: false };
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

  const openSeries = (series: SeriesCard) => nav.navigate({ name: "series", id: series.id });

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

  return (
    <div className="h-full overflow-y-auto">
      <PageHeader
        title="Explorar"
        subtitle={searching > 0 ? `Consultando ${searching} de ${sections.length} ${sections.length === 1 ? "fonte" : "fontes"}…` : undefined}
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
            {following && <SeriesGrid items={following} onOpen={openSeries} showSource />}
          </>
        )}
        {!showingFollowing && sources.length > 0 && selected.length === 0 && <EmptyState icon={Search} title="Nenhuma fonte com esses filtros" />}
        {!showingFollowing &&
          sections.map((section) => (
            <section key={keyOf(section.source)} aria-label={section.source.name} className="mb-8 last:mb-0">
              <h2 className="mb-2 flex flex-wrap items-center gap-2 text-base font-semibold">
                {section.source.name}
                {sources.length > 1 && <span className="text-sm font-normal text-muted">{languageName(section.source.language)}</span>}
                {section.status === "loading" && <Spinner size={14} />}
              </h2>
              {section.status === "loading" && <GridSkeleton />}
              {section.status === "failed" && (
                <p role="alert" className="flex flex-wrap items-center gap-2 text-sm text-danger">
                  {section.error}
                  {section.protected && <VerifySiteButton size="sm" site={{ sourceId: section.source.sourceId }} onDone={() => setAttempt((n) => n + 1)} />}
                  <Button size="sm" onClick={() => setAttempt((n) => n + 1)}>
                    Tentar de novo
                  </Button>
                </p>
              )}
              {section.status === "ready" && section.results.length === 0 && <p className="text-sm text-muted">Nada encontrado.</p>}
              {section.results.length > 0 && <SeriesGrid items={section.results} onOpen={openSeries} />}
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
