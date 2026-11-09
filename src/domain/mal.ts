// Integração com o MyAnimeList: tipos que vêm de src-tauri/src/mal/mod.rs e os textos usados nas telas.

export type MalAccountState = "disconnected" | "connected" | "expired";

export interface MalFailure {
  malId: number;
  title: string | null;
  chapters: number;
  error: string | null;
}

export interface MalStatus {
  /** Esta versão do Prancha tem a integração configurada. */
  available: boolean;
  state: MalAccountState;
  userName: string | null;
  connectedAt: number | null;
  lastSyncAt: number | null;
  pending: number;
  pendingError: string | null;
  failures: MalFailure[];
}

export interface MalLink {
  malId: number | null;
  title: string | null;
  mediaType: string | null;
  numChapters: number | null;
  origin: "auto" | "manual" | "none";
  confidence: number;
  syncedChapters: number | null;
  linkedAt: number;
}

export interface MalCandidate {
  malId: number;
  title: string;
  alternative: string[];
  mediaType: string | null;
  numChapters: number | null;
  status: string | null;
  year: string | null;
  authors: string[];
  score: number;
}

export interface MalSyncState {
  status: "pending" | "failed";
  chapters: number;
  attempts: number;
  error: string | null;
}

export type MalSeriesState = "linked" | "none" | "ambiguous" | "not_found" | "unchecked" | "error";

export interface MalSeriesStatus {
  available: boolean;
  connected: boolean;
  state: MalSeriesState;
  link: MalLink | null;
  candidates: MalCandidate[];
  sync: MalSyncState | null;
  error: string | null;
}

export const MAL_MEDIA_TYPES: Record<string, string> = {
  manga: "Mangá",
  manhwa: "Manhwa",
  manhua: "Manhua",
  one_shot: "One-shot",
  doujinshi: "Doujinshi",
  oel: "OEL",
  novel: "Romance",
  light_novel: "Light novel",
};

export const MAL_PUBLISHING: Record<string, string> = {
  currently_publishing: "Em publicação",
  finished: "Finalizada",
  not_yet_published: "Ainda não publicada",
  on_hiatus: "Em hiato",
  discontinued: "Cancelada",
};

/** Uma linha curta sobre a candidata: tipo, ano, capítulos e autoria. */
export function candidateDetails(c: MalCandidate): string {
  const parts = [c.mediaType ? (MAL_MEDIA_TYPES[c.mediaType] ?? c.mediaType) : null, c.year, c.numChapters ? `${c.numChapters} capítulos` : null, c.status ? (MAL_PUBLISHING[c.status] ?? null) : null, c.authors.length ? c.authors.join(", ") : null];
  return parts.filter(Boolean).join(" · ");
}

/** O estado da sincronização da obra, em poucas palavras. */
export function syncLabel(s: MalSeriesStatus): string | null {
  if (s.state !== "linked") return null;
  if (s.sync?.status === "failed") return `Não sincronizou o capítulo ${s.sync.chapters}`;
  if (s.sync?.status === "pending") return s.sync.attempts > 0 ? `Capítulo ${s.sync.chapters} aguardando nova tentativa` : `Sincronizando capítulo ${s.sync.chapters}`;
  if (s.link?.syncedChapters) return `${s.link.syncedChapters} capítulos no MAL`;
  return null;
}
