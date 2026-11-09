// Estado da guia Explorar, fora dos componentes: o que as fontes responderam continua aqui quando a pessoa
// vai para outra tela, e a volta mostra tudo na hora. Cada fonte tem a sua entrada por pesquisa, carrega no
// seu tempo e só é consultada de novo quando a resposta passou do prazo, quando falhou ou quando a pessoa pede.
// As obras em si ficam no SQLite (o backend grava cada página): aqui fica só a lista que cada fonte mandou.
import type { BrowsePage, SeriesCard, SourceSummary } from "../../domain/online";
import { ApiError, api, errorMessage } from "../../services/api";
import { siteAnswered } from "./parts";

export type Mode = "popular" | "latest" | "following";

export interface Search {
  mode: Mode;
  query: string;
  kind: string;
}

export interface Section {
  source: SourceSummary;
  status: "loading" | "ready" | "failed";
  results: SeriesCard[];
  page: number;
  hasMore: boolean;
  unverified: number;
  loadingMore: boolean;
  /** Quantas obras da fonte estão na tela. Cresce de STEP em STEP. */
  shown: number;
  error: string | null;
  /** O site pediu uma verificação anti-robô. */
  protected: boolean;
  /** Atualizando em segundo plano, com a lista anterior na tela. */
  refreshing: boolean;
  /** A última atualização falhou: a lista na tela é a anterior. */
  refreshError: string | null;
  /** Quando a lista na tela chegou da fonte, ou 0. */
  updatedAt: number;
}

interface Entry extends Section {
  /** Só a operação mais nova grava: a resposta de uma consulta antiga não passa por cima de uma nova. */
  token: number;
  /** Consultas desta entrada que ainda estão no backend, para cancelar. */
  requests: Set<string>;
  /** Quantas obras a entrada precisa ter. Um clique durante uma busca aumenta a meta da busca que já está andando. */
  goal: number;
  /** Quando a fonte foi consultada pela última vez, com ou sem sucesso. */
  checkedAt: number;
}

/** Por quanto tempo a lista de uma fonte vale sem consultar de novo. */
export const EXPLORE_TTL_MS = 15 * 60 * 1000;
/** Uma fonte que falhou só é consultada sozinha de novo depois disto. Antes, só pelo botão. */
export const RETRY_AFTER_MS = 60 * 1000;
/** Quantas consultas às fontes ficam abertas ao mesmo tempo, somando todas as pesquisas. */
export const CONCURRENT_REQUESTS = 3;
/** "Mostrar mais" põe na tela mais esta quantidade de obras por clique. */
export const STEP = 10;
/** Teto de páginas pedidas à fonte num clique, para uma fonte que só repete obras não ficar pedindo para sempre. */
const MAX_PAGES_PER_STEP = 5;
/** Pesquisas guardadas. A mais antiga sai primeiro. */
const MAX_SEARCHES = 20;

export const keyOf = (source: SourceSummary) => `${source.extensionId}/${source.sourceId}`;

/** Com texto, a pesquisa vale para qualquer modo. */
export function searchKey({ mode, query, kind }: Search): string {
  return query ? `search\n${query}\n${kind}` : `${mode}\n\n${kind}`;
}

const entries = new Map<string, Entry>();
const searchesUsed = new Map<string, number>();
const titlesShown = new Map<string, number>();
const listeners = new Set<() => void>();
let version = 0;
let requestCounter = 0;
/** Quantas consultas foram evitadas porque a lista guardada ainda valia. */
let reused = 0;

function changed() {
  version++;
  for (const listener of listeners) listener();
}

export function subscribeExplore(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const exploreVersion = () => version;

export function exploreStats() {
  return { reused, entries: entries.size };
}

const entryKey = (search: string, source: SourceSummary) => `${search}#${keyOf(source)}`;

export function sectionOf(search: Search, source: SourceSummary): Section | undefined {
  return entries.get(entryKey(searchKey(search), source));
}

export function titlesShownFor(search: Search): number {
  return titlesShown.get(searchKey(search)) ?? STEP;
}

export function setTitlesShown(search: Search, count: number) {
  titlesShown.set(searchKey(search), count);
  changed();
}

// --- Fila das consultas: no máximo CONCURRENT_REQUESTS abertas, na ordem em que foram pedidas. ---

let active = 0;
const waiting: Array<() => void> = [];

async function limited<T>(task: () => Promise<T>): Promise<T> {
  if (active >= CONCURRENT_REQUESTS) await new Promise<void>((resolve) => waiting.push(resolve));
  active++;
  try {
    return await task();
  } finally {
    active = Math.max(0, active - 1);
    waiting.shift()?.();
  }
}

function fetchPage(requestId: string, source: SourceSummary, search: Search, page: number): Promise<BrowsePage> {
  const kind = search.kind || null;
  if (search.query) return api.sourceSearch(requestId, source, search.query, page, kind);
  return search.mode === "latest" ? api.sourceLatest(requestId, source, page, kind) : api.sourcePopular(requestId, source, page, kind);
}

function isCancelled(error: unknown): boolean {
  return error instanceof ApiError && error.code === "extension_cancelled";
}

/** Altera a entrada só se a operação `token` ainda for a mais nova. */
function patch(key: string, token: number, change: Partial<Entry>): boolean {
  const entry = entries.get(key);
  if (!entry || entry.token !== token) return false;
  entries.set(key, { ...entry, ...change });
  changed();
  return true;
}

class Stale extends Error {}

/** Uma página, pela fila. Uma consulta que ficou velha enquanto esperava a vez nem chega à fonte. */
async function request(key: string, token: number, search: Search, page: number): Promise<BrowsePage> {
  return limited(async () => {
    const entry = entries.get(key);
    if (!entry || entry.token !== token) throw new Stale();
    const id = `explore-${++requestCounter}`;
    entry.requests.add(id);
    try {
      return await fetchPage(id, entry.source, search, page);
    } finally {
      entry.requests.delete(id);
    }
  });
}

function cancelRequests(entry: Entry) {
  for (const id of entry.requests) void api.sourceCancel(id).catch(() => undefined);
  entry.requests.clear();
}

function merge(results: SeriesCard[], more: SeriesCard[]): SeriesCard[] {
  const known = new Set(results.map((item) => item.id));
  return [...results, ...more.filter((item) => !known.has(item.id))];
}

type Loaded = Pick<Section, "results" | "page" | "hasMore" | "unverified">;

/** Pede as próximas páginas até ter `goal` obras, sem repetir as que já vieram. Muitas fontes trazem menos de STEP por página. */
async function fill(key: string, search: Search) {
  const start = entries.get(key);
  if (!start || start.loadingMore || start.status !== "ready" || start.results.length >= start.goal || !start.hasMore) return;
  const token = start.token;
  patch(key, token, { loadingMore: true });
  let pages = 0;
  try {
    for (;;) {
      const entry = entries.get(key);
      if (!entry || entry.token !== token || entry.results.length >= entry.goal || !entry.hasMore || pages >= MAX_PAGES_PER_STEP) break;
      const page = await request(key, token, search, entry.page + 1);
      pages++;
      const now = entries.get(key)!;
      patch(key, token, { results: merge(now.results, page.results), page: now.page + 1, hasMore: page.hasMore, unverified: now.unverified + page.unverified });
    }
  } catch (e) {
    if (!(e instanceof Stale) && !isCancelled(e)) patch(key, token, { error: errorMessage(e) });
  } finally {
    patch(key, token, { loadingMore: false });
  }
}

function blank(source: SourceSummary): Entry {
  return { source, status: "loading", results: [], page: 1, hasMore: false, unverified: 0, loadingMore: false, shown: STEP, error: null, protected: false, refreshing: false, refreshError: null, updatedAt: 0, token: 0, requests: new Set(), goal: STEP, checkedAt: 0 };
}

/** A primeira leva da fonte. O que a fonte já tinha mandado fica na tela até a resposta chegar. */
async function load(key: string, search: Search) {
  const before = entries.get(key)!;
  cancelRequests(before);
  const token = before.token + 1;
  entries.set(key, { ...before, token, status: "loading", error: null, protected: false, loadingMore: false, refreshing: false, refreshError: null, goal: STEP, shown: STEP });
  changed();
  try {
    const page = await request(key, token, search, 1);
    siteAnswered(before.source.sourceId);
    if (!patch(key, token, { status: "ready", results: page.results, page: 1, hasMore: page.hasMore, unverified: page.unverified, updatedAt: Date.now(), checkedAt: Date.now() })) return;
  } catch (e) {
    if (e instanceof Stale) return;
    if (isCancelled(e)) {
      // Abandonada: na próxima visita, começa de novo.
      if (entries.get(key)?.token === token) entries.delete(key);
      changed();
      return;
    }
    patch(key, token, { status: "failed", error: errorMessage(e), protected: e instanceof ApiError && e.code === "source_protected", checkedAt: Date.now() });
    return;
  }
  // A primeira leva já chega com STEP obras, mesmo quando a fonte manda menos por página.
  await fill(key, search);
}

/**
 * Consulta a fonte de novo com a lista anterior na tela, e troca tudo de uma vez quando a resposta chega,
 * com as obras que estavam à mostra. Se falhar, a lista anterior fica e o problema aparece discreto.
 */
async function refresh(key: string, search: Search) {
  const before = entries.get(key)!;
  if (before.refreshing) return;
  cancelRequests(before);
  const token = before.token + 1;
  entries.set(key, { ...before, token, refreshing: true, loadingMore: false, refreshError: null });
  changed();
  try {
    const first = await request(key, token, search, 1);
    siteAnswered(before.source.sourceId);
    let state: Loaded = { results: first.results, page: 1, hasMore: first.hasMore, unverified: first.unverified };
    const want = Math.max(before.shown, STEP);
    const maxPages = Math.max(before.page, MAX_PAGES_PER_STEP);
    while (state.results.length < want && state.hasMore && state.page < maxPages) {
      const page = await request(key, token, search, state.page + 1);
      state = { results: merge(state.results, page.results), page: state.page + 1, hasMore: page.hasMore, unverified: state.unverified + page.unverified };
    }
    patch(key, token, { ...state, status: "ready", error: null, protected: false, refreshing: false, goal: Math.max(want, STEP), updatedAt: Date.now(), checkedAt: Date.now() });
  } catch (e) {
    if (e instanceof Stale) return;
    if (isCancelled(e)) {
      patch(key, token, { refreshing: false });
      return;
    }
    patch(key, token, { refreshing: false, refreshError: errorMessage(e), protected: e instanceof ApiError && e.code === "source_protected", checkedAt: Date.now() });
  }
}

function stale(entry: Entry, now: number): boolean {
  if (entry.refreshing || entry.loadingMore || entry.status === "loading") return false;
  if (now - entry.checkedAt < RETRY_AFTER_MS) return false;
  return entry.status === "failed" || entry.refreshError !== null || now - entry.updatedAt > EXPLORE_TTL_MS;
}

/** Deixa de lado o que outras pesquisas ainda carregavam: a pessoa mudou de ideia. As listas prontas continuam guardadas. */
function abandonOthers(current: string) {
  let touched = false;
  for (const [key, entry] of entries) {
    if (key.startsWith(`${current}#`) || (entry.status !== "loading" && !entry.refreshing && !entry.loadingMore)) continue;
    cancelRequests(entry);
    touched = true;
    if (entry.status === "loading") entries.delete(key);
    else entries.set(key, { ...entry, token: entry.token + 1, refreshing: false, loadingMore: false });
  }
  if (touched) changed();
}

function evict(current: string) {
  searchesUsed.set(current, Date.now());
  if (searchesUsed.size <= MAX_SEARCHES) return;
  const oldest = [...searchesUsed].filter(([key]) => key !== current).sort((a, b) => a[1] - b[1]);
  for (const [key] of oldest.slice(0, searchesUsed.size - MAX_SEARCHES)) {
    searchesUsed.delete(key);
    titlesShown.delete(key);
    for (const [entry, value] of entries) {
      if (entry.startsWith(`${key}#`)) {
        cancelRequests(value);
        entries.delete(entry);
      }
    }
  }
}

/**
 * A tela abriu, ou a pesquisa mudou: consulta só as fontes sem lista, com lista vencida ou que falharam há algum tempo.
 * Uma lista dentro do prazo aparece como está, sem consulta.
 */
export function ensure(search: Search, sources: SourceSummary[]) {
  const current = searchKey(search);
  abandonOthers(current);
  evict(current);
  const now = Date.now();
  let added = false;
  const toLoad: string[] = [];
  const toRefresh: string[] = [];
  for (const source of sources) {
    const key = entryKey(current, source);
    const entry = entries.get(key);
    if (!entry) {
      entries.set(key, blank(source));
      added = true;
      toLoad.push(key);
    } else if (stale(entry, now)) {
      (entry.status === "failed" ? toLoad : toRefresh).push(key);
    } else if (entry.status === "ready") {
      reused++;
    }
  }
  if (added) changed();
  for (const key of toLoad) void load(key, search);
  for (const key of toRefresh) void refresh(key, search);
}

/** Só a fonte pedida: as outras já responderam, e o que elas trouxeram continua na tela. */
export function retry(search: Search, source: SourceSummary) {
  const key = entryKey(searchKey(search), source);
  const entry = entries.get(key);
  if (!entry) return ensure(search, [source]);
  if (entry.status === "ready") void refresh(key, search);
  else void load(key, search);
}

/** Atualização pedida pela pessoa: todas as fontes da pesquisa, menos as que já estão carregando. */
export function refreshAll(search: Search, sources: SourceSummary[]) {
  for (const source of sources) {
    const entry = entries.get(entryKey(searchKey(search), source));
    if (!entry) ensure(search, [source]);
    else if (entry.status !== "loading" && !entry.refreshing) retry(search, source);
  }
}

/** Primeiro mostra o que já veio da fonte. Só pede outra página quando falta obra para completar a leva. */
export function showMore(search: Search, source: SourceSummary) {
  const key = entryKey(searchKey(search), source);
  const entry = entries.get(key);
  if (!entry || entry.status !== "ready") return;
  const shown = entry.shown + STEP;
  patch(key, entry.token, { shown, goal: Math.max(entry.goal, shown) });
  void fill(key, search);
}

/** Na pesquisa em várias fontes, a leva é de títulos. Faltando título, cada fonte que tem mais manda a próxima página. */
export function moreTitles(search: Search, sources: SourceSummary[]) {
  const current = searchKey(search);
  for (const source of sources) {
    const key = entryKey(current, source);
    const entry = entries.get(key);
    if (!entry || entry.status !== "ready" || !entry.hasMore || entry.loadingMore || entry.refreshing) continue;
    patch(key, entry.token, { goal: entry.results.length + 1 });
    void fill(key, search);
  }
}

/** Esquece tudo e cancela o que estava andando. Para os testes, que começam cada caso do zero. */
export function resetExplore() {
  for (const entry of entries.values()) cancelRequests(entry);
  entries.clear();
  searchesUsed.clear();
  titlesShown.clear();
  waiting.splice(0).forEach((resolve) => resolve());
  active = 0;
  reused = 0;
  changed();
}
