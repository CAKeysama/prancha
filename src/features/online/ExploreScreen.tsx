import { useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { Bell, Compass, RefreshCw, Search, X } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { EmptyState, ErrorState, Spinner } from "../../components/Feedback";
import { Segmented, Select } from "../../components/Form";
import { PageHeader } from "../../components/PageHeader";
import { TOOLTIP_DELAY_MS } from "../../components/Tooltip";
import { useLibraryData } from "../../app/library-events";
import { useNav } from "../../app/navigation";
import { useScrollMemory } from "../../app/scroll-memory";
import { KIND_LABELS, groupByTitle, languageName, nextHistory, type SeriesCard, type SourceSummary, type TitleGroup } from "../../domain/online";
import { ApiError, api, errorMessage } from "../../services/api";
import { setData } from "../../services/query-cache";
import { STEP, ensure, exploreVersion, keyOf, moreTitles, refreshAll, retry, sectionOf, setTitlesShown, showMore, subscribeExplore, titlesShownFor, type Mode, type Search as ExploreSearch, type Section } from "./explore-store";
import { GridSkeleton, OnlineCover, SeriesGrid, SourceTrouble } from "./parts";

const HISTORY_KEY = "online.searchHistory";

// Volta da página de uma obra, ou de outra guia, para a mesma pesquisa e os mesmos filtros.
const memory = { mode: "popular" as Mode, query: "", source: "", language: "", kind: "" };

/** Quando uma fonte não abre a obra: Explorar já chega com a pesquisa feita em todas. */
export function searchAcrossSources(title: string) {
  Object.assign(memory, { mode: "popular", query: title, source: "" });
}

const placeholder = (source: SourceSummary): Section => ({ source, status: "loading", results: [], page: 1, hasMore: false, unverified: 0, loadingMore: false, shown: STEP, error: null, protected: false, refreshing: false, refreshError: null, updatedAt: 0 });

export function ExploreScreen() {
  const nav = useNav();
  const { data: sources, error: sourcesError, reload: reloadSources } = useLibraryData(() => api.sources(), [], "sources");
  const [mode, setMode] = useState<Mode>(memory.mode);
  const [draft, setDraft] = useState(memory.query);
  const [query, setQuery] = useState(memory.query);
  const [sourceFilter, setSourceFilter] = useState(memory.source);
  const [language, setLanguage] = useState(memory.language);
  const [kind, setKind] = useState(memory.kind);
  const showingFollowing = mode === "following" && !query;
  // Só com a lista na tela: as outras visitas da guia não precisam dela.
  const { data: following, error: followingLoadError } = useLibraryData(() => (showingFollowing ? api.seriesFollowing() : Promise.resolve(null)), [showingFollowing], "following");
  const [checkError, setCheckError] = useState<string | null>(null);
  const [checkingNew, setCheckingNew] = useState(false);
  const checkRequest = useRef<string | null>(null);
  // Só as obras acompanhadas são consultadas. Cada uma é aberta na fonte, uma de cada vez, e os contadores voltam com a lista.
  const checkNew = async () => {
    if (checkRequest.current) return;
    const requestId = `following-${Date.now()}`;
    checkRequest.current = requestId;
    setCheckingNew(true);
    try {
      setData("following:[true]", await api.seriesRefreshFollowing(requestId));
    } catch (e) {
      if (!(e instanceof ApiError && e.code === "extension_cancelled")) setCheckError(errorMessage(e));
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
  const searchBox = useRef<HTMLInputElement>(null);

  useEffect(() => {
    Object.assign(memory, { mode, query, source: sourceFilter, language, kind });
  }, [mode, query, sourceFilter, language, kind]);

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
  const followingError = checkError ?? followingLoadError;
  const search = useMemo<ExploreSearch>(() => ({ mode, query, kind }), [mode, query, kind]);

  // O que as fontes responderam vive fora da tela: sair e voltar não perde nada nem consulta de novo.
  const storeVersion = useSyncExternalStore(subscribeExplore, exploreVersion);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- a versão do estado da guia é o que muda as seções
  const sections = useMemo(() => selected.map((source) => sectionOf(search, source) ?? placeholder(source)), [selected, search, storeVersion]);

  // Cada fonte responde no seu tempo: uma lenta ou com erro não segura as outras. Só as fontes sem lista válida são consultadas.
  // A chave é texto para que a mesma lista de fontes, lida de novo, não recomece nada.
  const selectedKey = selected.map(keyOf).join(",");
  const selectedRef = useRef(selected);
  useEffect(() => {
    selectedRef.current = selected;
  });
  useEffect(() => {
    if (showingFollowing) return;
    ensure(search, selectedRef.current);
  }, [search, selectedKey, showingFollowing]);

  const retrySource = (source: SourceSummary) => retry(search, source);

  // Uma pesquisa nova, ou outras fontes, recomeça da primeira leva de títulos.
  const shownGroups = titlesShownFor(search);
  const groups = useMemo(() => groupByTitle(sections.flatMap(({ source, results }) => results.map((card) => ({ ...card, sourceName: source.name, language: card.language ?? source.language }))), query), [sections, query]);

  const submit = (text: string) => {
    const clean = text.trim();
    setDraft(clean);
    setQuery(clean);
    if (clean) {
      if (mode === "following") setMode("popular");
      saveHistory(nextHistory(history, clean));
    }
  };

  const showMoreTitles = () => {
    const target = shownGroups + STEP;
    setTitlesShown(search, target);
    if (groups.length >= target) return;
    moreTitles(search, selected);
  };

  const scrollRef = useScrollMemory<HTMLDivElement>(showingFollowing ? "explore:following" : `explore:${sourceFilter}\n${language}\n${mode}\n${query}\n${kind}`);

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
          <ErrorState title="Não foi possível abrir as fontes" message={sourcesError} action={<Button onClick={reloadSources}>Tentar de novo</Button>} />
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
  const refreshing = sections.filter((section) => section.refreshing).length;
  const staleIn = sections.filter((section) => section.status === "ready" && section.refreshError);
  const finished = byTitle ? (failed.length > 0 ? `${sections.length - failed.length} de ${sections.length} fontes responderam` : `${sections.length} fontes consultadas`) : undefined;

  return (
    <div ref={scrollRef} className="h-full overflow-y-auto">
      <PageHeader
        title="Explorar"
        subtitle={searching > 0 ? `Consultando ${searching} de ${sections.length} ${sections.length === 1 ? "fonte" : "fontes"}…` : refreshing > 0 ? "Atualizando…" : finished}
        actions={
          sources.length > 0 ? (
            <>
              {!showingFollowing && selected.length > 0 && (
                <IconButton icon={RefreshCw} label="Atualizar" variant="secondary" size="sm" tooltipDelay={TOOLTIP_DELAY_MS} disabled={searching + refreshing > 0} onClick={() => refreshAll(search, selected)} />
              )}
              {!query && (
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
              )}
            </>
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
                Use “Acompanhar” na página de uma obra para vê-la aqui, mesmo sem baixar nada.
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
            {staleIn.length > 0 && <p className="mb-3 text-sm text-muted">Não deu para atualizar {LIST_FORMAT.format(staleIn.map((section) => section.source.name))} agora. Os resultados são os da consulta anterior.</p>}
            {groups.length === 0 && searching > 0 && <TitlesSkeleton />}
            {searching > 0 && <p className="p-enter-late mt-3 text-sm text-muted">{CHECKING_NOTE}</p>}
            {groups.length === 0 && searching === 0 && uncheckedIn.length === 0 && (
              <EmptyState icon={Search} title="Nenhum resultado encontrado">
                {failed.length > 0 ? "As fontes que não responderam podem ter. " : ""}A mesma obra pode ter outro título em cada fonte.
              </EmptyState>
            )}
            {groups.length > 0 && <TitleResults groups={groups.slice(0, shownGroups)} showLanguage={languages.length > 1} onOpen={openSeries} />}
            {(groups.length > shownGroups || sections.some((section) => section.hasMore)) && (
              <div className="mt-3">
                <Button onClick={showMoreTitles} loading={sections.some((section) => section.loadingMore)}>
                  Mostrar mais
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
              {section.status === "ready" && section.refreshError && <p className="mb-2 text-sm text-muted">Não deu para atualizar agora. A lista é a da consulta anterior.</p>}
              {section.status === "loading" && section.results.length === 0 && <GridSkeleton />}
              {section.status === "failed" && (
                <div role="alert">
                  <SourceTrouble message={section.error ?? ""} site={section.protected ? { sourceId: section.source.sourceId } : undefined} sourceId={section.source.sourceId} onRetry={() => retrySource(section.source)} elsewhere={sources.length > 1 ? searchElsewhere : undefined} size="sm">
                    {troubleLayout}
                  </SourceTrouble>
                </div>
              )}
              {section.status === "ready" && section.results.length === 0 && !(section.unverified > 0) && <p className="text-sm text-muted">Nada encontrado.</p>}
              {section.results.length > 0 && <SeriesGrid items={section.results.slice(0, section.shown)} onOpen={openSeries} />}
              {section.status === "ready" && section.unverified > 0 && (
                <p className="mt-3 flex flex-wrap items-center gap-2 text-sm text-muted">
                  Não deu para conferir os detalhes de {section.unverified === 1 ? "1 obra" : `${section.unverified} obras`} agora.
                  <Button size="sm" onClick={() => retrySource(section.source)}>
                    Tentar de novo
                  </Button>
                </p>
              )}
              {section.status === "ready" && (section.results.length > section.shown || section.hasMore) && (
                <div className="mt-3">
                  <Button onClick={() => showMore(search, section.source)} loading={section.loadingMore}>
                    Mostrar mais
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
