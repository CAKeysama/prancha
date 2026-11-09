// Repositórios, extensões, fontes e downloads, como chegam do backend.

import type { RatingOrigin, RatingState } from "./rating";
import type { ReaderChapter } from "./reader";
import type { ReaderMode } from "./settings";
import type { ReadStatus } from "./types";

/** Extensões do Mihon rodam no Suwayomi, fora do isolamento do Prancha. */
export const EXTERNAL_RUNTIME = "suwayomi";

export interface ComponentProgress {
  stage: "download" | "unpack" | "start";
  percent: number;
}

export interface RepositoryPreview {
  url: string;
  id: string;
  name: string;
  description: string | null;
  website: string | null;
  publicKey: string;
  fingerprint: string;
  extensionCount: number;
  alreadyAdded: boolean;
  external: boolean;
}

/** Só entra na lista o que o Prancha adiciona e usa hoje. */
export interface RecommendedRepository {
  id: string;
  name: string;
  description: string;
  url: string;
  note: string | null;
  added: boolean;
}

export interface RecommendedList {
  entries: RecommendedRepository[];
  origin: "remote" | "cache" | "bundled";
  fetchedAt: number | null;
  /** A cópia passou de um dia, ou é a embutida: vale pedir a publicada. */
  stale: boolean;
  error: string | null;
}

export type RepositoryStatus = "pending" | "ok" | "error" | "key_changed";

export interface RepositoryInfo {
  id: string;
  url: string;
  name: string;
  description: string | null;
  website: string | null;
  publicKey: string;
  fingerprint: string;
  pendingPublicKey: string | null;
  pendingFingerprint: string | null;
  enabled: boolean;
  status: RepositoryStatus;
  lastError: string | null;
  lastSyncAt: number | null;
  lastSuccessAt: number | null;
  extensionCount: number;
  addedAt: number;
}

export interface Capabilities {
  network: boolean;
  storage: boolean;
  authentication: boolean;
  allowedHosts: string[];
}

export interface CatalogItem {
  id: string;
  name: string;
  version: string;
  description: string | null;
  authors: string[];
  languages: string[];
  icon: string | null;
  supportedKinds: string[];
  contentRating: string;
  runtime: string;
  size: number;
  sources: Array<{ id: string; name: string; language: string; contentRating: string }>;
  capabilities: Capabilities;
  repositoryId: string;
  repositoryName: string;
  installedVersion: string | null;
  installedHere: boolean;
  updateAvailable: boolean;
  incompatible: string | null;
}

export interface CatalogQuery {
  text?: string;
  language?: string;
  sort?: "name" | "repository";
  limit?: number;
}

export interface InstalledSource {
  id: string;
  name: string;
  language: string;
  description: string | null;
  kinds: string[];
  rating: string;
  direction: string;
  supportsLatest: boolean;
  supportsSearch: boolean;
  available: boolean;
}

export interface InstalledExtension {
  id: string;
  name: string;
  version: string;
  description: string | null;
  authors: string[];
  icon: string | null;
  repositoryId: string;
  repositoryName: string;
  fingerprint: string;
  runtime: string;
  capabilities: Capabilities;
  enabled: boolean;
  loadError: string | null;
  installedAt: number;
  updatedAt: number;
  sources: InstalledSource[];
  availableVersion: string | null;
}

export interface SourceSummary {
  extensionId: string;
  extensionName: string;
  repositoryName: string;
  sourceId: string;
  name: string;
  language: string;
  description: string | null;
  kinds: string[];
  rating: string;
  direction: string;
  supportsLatest: boolean;
  supportsSearch: boolean;
}

export interface SeriesCard {
  id: number;
  extensionId: string;
  sourceId: string;
  sourceName: string | null;
  remoteId: string;
  title: string;
  hasCover: boolean;
  summary: string | null;
  author: string | null;
  genres: string[];
  status: string | null;
  language: string | null;
  following: boolean;
  chapterSort: "asc" | "desc";
  lastCheckedAt: number | null;
  lastUpdateAt: number | null;
  favorite: boolean;
  /** Capítulos novos: chegaram depois da primeira sincronização e ainda não foram abertos nem marcados como lidos. */
  newCount: number;
  ratingState: RatingState;
  ratingOrigin: RatingOrigin;
}

export interface BrowsePage {
  results: SeriesCard[];
  hasMore: boolean;
  /** Obras que o filtro +18 escondeu. Com ele desligado, aparecem. */
  hidden: number;
  /** Obras que não puderam ser conferidas agora: ficam de fora com o filtro ligado ou desligado. */
  unverified: number;
}

export type DownloadStatus = "queued" | "running" | "paused" | "completed" | "failed" | "cancelled";

export interface ChapterView {
  id: number;
  remoteId: string;
  title: string;
  number: string | null;
  publishedAt: number | null;
  order: number;
  state: "available" | "downloaded" | "offline";
  comicId: number | null;
  progress: { pageIndex: number; pageCount: number } | null;
  download: { id: number; status: DownloadStatus; pagesDone: number; pagesTotal: number; error: string | null } | null;
  readState: ChapterReadState;
  /** Independe do estado de leitura: um capítulo novo pode estar em andamento, e um antigo, não lido. */
  isNew: boolean;
  firstSeenAt: number | null;
  completedAt: number | null;
}

/**
 * Os quatro estados de um capítulo (ver docs/adr/0003-progresso-de-leitura.md).
 * `reading` só existe enquanto o capítulo está aberto no leitor com leitura efetiva; ao sair, vira `in_progress`.
 */
export type ChapterReadState = "unread" | "reading" | "in_progress" | "read";

export const READ_STATE_LABELS: Record<ChapterReadState, string> = {
  unread: "Não lido",
  reading: "Lendo",
  in_progress: "Em andamento",
  read: "Lido",
};

/** O que o backend devolve depois de uma mudança de estado. */
export interface ChapterReadInfo {
  id: number;
  seriesId: number;
  readState: ChapterReadState;
  isNew: boolean;
  completedAt: number | null;
}

/** Aplica as mudanças à lista na hora, sem esperar a recarga. */
export function applyReadInfo(chapters: ChapterView[], infos: ChapterReadInfo[]): ChapterView[] {
  if (infos.length === 0) return chapters;
  const byId = new Map(infos.map((info) => [info.id, info]));
  return chapters.map((chapter) => {
    const info = byId.get(chapter.id);
    return info ? { ...chapter, readState: info.readState, isNew: info.isNew, completedAt: info.completedAt } : chapter;
  });
}

/** Os anteriores na ordem de leitura informada pela fonte. Posições iguais à do escolhido não contam: sem como saber, nada é marcado. */
export function previousChapters(chapters: ChapterView[], id: number): ChapterView[] {
  const chosen = chapters.find((chapter) => chapter.id === id);
  if (!chosen) return [];
  return chapters.filter((chapter) => chapter.order < chosen.order);
}

export interface ChapterSession {
  id: number;
  seriesId: number;
  seriesTitle: string;
  title: string;
  pageCount: number;
  start: { pageIndex: number; pageCount: number; scrollRatio: number; updatedAt: number };
  previousId: number | null;
  nextId: number | null;
  comicId: number | null;
  genres: string[];
  readerMode: ReaderMode | null;
}

export interface SeriesResume {
  chapterId: number;
  title: string;
  progress: { pageIndex: number; pageCount: number } | null;
  state: ChapterReadState;
}

/** Até onde a pessoa leu e onde a obra está guardada. Quem decide onde a leitura continua é o backend. */
export interface SeriesReading {
  readStatus: ReadStatus;
  chapterCount: number;
  chaptersRead: number;
  resume: SeriesResume | null;
  collections: number[];
}

export interface SeriesView extends SeriesCard, SeriesReading {
  chapters: ChapterView[];
  sourceError: string | null;
  /** A fonte pediu uma verificação anti-robô, que a pessoa pode passar no site. */
  sourceProtected: boolean;
}

export interface LibrarySeries extends SeriesReading {
  id: number;
  title: string;
  hasCover: boolean;
  sourceName: string | null;
  favorite: boolean;
  addedAt: number;
  lastOpenedAt: number | null;
  following: boolean;
  newCount: number;
  ratingState: RatingState;
}

export interface ComicOrigin {
  seriesId: number;
  seriesTitle: string;
  chapterTitle: string;
  sourceName: string | null;
}

export interface DownloadView {
  id: number;
  chapterId: number;
  seriesId: number;
  seriesTitle: string;
  chapterTitle: string;
  sourceName: string | null;
  status: DownloadStatus;
  pagesDone: number;
  pagesTotal: number;
  bytes: number;
  error: string | null;
  comicId: number | null;
  createdAt: number;
  finishedAt: number | null;
}

export const KIND_LABELS: Record<string, string> = {
  manga: "Mangá",
  manhwa: "Manhwa",
  manhua: "Manhua",
  comic: "Quadrinhos",
  other: "Outros",
};

export const SERIES_STATUS_LABELS: Record<string, string> = {
  ongoing: "Em publicação",
  completed: "Concluída",
  hiatus: "Em pausa",
  cancelled: "Cancelada",
};

export const CHAPTER_STATE_LABELS: Record<ChapterView["state"], string> = {
  available: "Disponível online",
  downloaded: "Baixado",
  offline: "Na biblioteca",
};

export const DOWNLOAD_STATUS_LABELS: Record<DownloadStatus, string> = {
  queued: "Na fila",
  running: "Baixando",
  paused: "Pausado",
  completed: "Concluído",
  failed: "Falhou",
  cancelled: "Cancelado",
};

export function languageName(code: string): string {
  try {
    return new Intl.DisplayNames(["pt-BR"], { type: "language" }).of(code) ?? code;
  } catch {
    return code;
  }
}

/** Impressão digital em blocos de 4, mais fácil de comparar a olho. */
export function groupedFingerprint(fingerprint: string): string {
  return fingerprint.match(/.{1,4}/g)?.join(" ") ?? fingerprint;
}

/** Capítulos na ordem escolhida. `order` é a posição informada pela fonte, do primeiro ao último. */
export function sortedChapters(chapters: ChapterView[], sort: "asc" | "desc"): ChapterView[] {
  const ordered = [...chapters].sort((a, b) => a.order - b.order || a.id - b.id);
  return sort === "asc" ? ordered : ordered.reverse();
}

/** Capítulos abertos em sequência, cada um começando onde o anterior termina. */
export function chapterStream(sessions: ChapterSession[]): Array<ReaderChapter & { id: number }> {
  let first = 0;
  return sessions.map((session) => {
    const chapter = { id: session.id, title: session.title, first, count: session.pageCount, guidedComicId: session.comicId };
    first += session.pageCount;
    return chapter;
  });
}

export function chapterFinished(chapter: ChapterView): boolean {
  return chapter.readState === "read";
}

const HISTORY_LIMIT = 8;

/** Pesquisas recentes: a mais nova primeiro, sem repetir, ignorando maiúsculas. */
export function nextHistory(history: string[], query: string): string[] {
  const text = query.trim();
  if (!text) return history;
  return [text, ...history.filter((item) => item.toLowerCase() !== text.toLowerCase())].slice(0, HISTORY_LIMIT);
}

/** Compara títulos sem caixa, acento e pontuação. Os sinais de outras escritas mudam a palavra e ficam. */
export function titleKey(title: string): string {
  const key = title
    .normalize("NFD")
    .replace(/(\p{Script=Latin})\p{M}+/gu, "$1")
    .normalize("NFC")
    .toLowerCase()
    .replace(/[^\p{L}\p{M}\p{N}]+/gu, " ")
    .trim();
  return key || title.trim().toLowerCase();
}

export interface TitleGroup {
  key: string;
  title: string;
  entries: SeriesCard[];
}

/**
 * Junta só títulos iguais: parecido não prova que é a mesma obra, e igual também não, por isso a tela mostra as capas.
 * O título pesquisado vem primeiro. A ordem não depende de quantas fontes têm a obra, para a lista não pular enquanto elas respondem.
 */
export function groupByTitle(cards: SeriesCard[], query: string): TitleGroup[] {
  const groups = new Map<string, TitleGroup>();
  for (const card of cards) {
    const key = titleKey(card.title);
    const group = groups.get(key);
    if (group) group.entries.push(card);
    else groups.set(key, { key, title: card.title, entries: [card] });
  }
  const wanted = titleKey(query);
  const rank = ({ key }: TitleGroup) => (key === wanted ? 0 : key.startsWith(wanted) ? 1 : 2);
  return [...groups.values()].sort((a, b) => rank(a) - rank(b));
}

/** Roda as tarefas com um limite de simultâneas. Uma falha não impede as outras. */
export async function runLimited(tasks: Array<() => Promise<void>>, limit: number): Promise<void> {
  let next = 0;
  const worker = async () => {
    while (next < tasks.length) {
      const task = tasks[next++]!;
      await task().catch(() => undefined);
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, tasks.length) }, worker));
}
