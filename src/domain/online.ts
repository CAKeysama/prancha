// Repositórios, extensões, fontes e downloads, como chegam do backend.

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
}

export interface BrowsePage {
  results: SeriesCard[];
  hasMore: boolean;
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
}

export interface SeriesView extends SeriesCard {
  chapters: ChapterView[];
  sourceError: string | null;
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

const HISTORY_LIMIT = 8;

/** Pesquisas recentes: a mais nova primeiro, sem repetir, ignorando maiúsculas. */
export function nextHistory(history: string[], query: string): string[] {
  const text = query.trim();
  if (!text) return history;
  return [text, ...history.filter((item) => item.toLowerCase() !== text.toLowerCase())].slice(0, HISTORY_LIMIT);
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
