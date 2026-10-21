// Modelos compartilhados com o backend (espelham as structs serializadas em Rust).

export type ComicFormat = "cbz" | "cbr" | "cb7" | "cbt" | "pdf" | "folder";
export type ReadStatus = "unread" | "reading" | "completed";
export type CoverKind = "pending" | "metadata" | "format" | "first_page" | "placeholder";
export type FieldSource = "file" | "filename" | "user";

export interface Progress {
  pageIndex: number;
  pageCount: number;
  scrollRatio: number;
  updatedAt: number;
}

export interface ComicSummary {
  id: number;
  title: string;
  fileName: string;
  format: ComicFormat;
  series: string | null;
  issueNumber: string | null;
  volume: string | null;
  writer: string | null;
  pageCount: number | null;
  coverKind: CoverKind;
  coverRev: number;
  coverError: string | null;
  favorite: boolean;
  status: ReadStatus;
  missing: boolean;
  addedAt: number;
  lastOpenedAt: number | null;
  progress: Progress | null;
}

export interface CollectionRef {
  id: number;
  name: string;
}

export interface ComicDetails extends ComicSummary {
  path: string;
  fileSize: number;
  year: number | null;
  artist: string | null;
  publisher: string | null;
  summaryText: string | null;
  coverPage: number | null;
  tags: string[];
  collections: CollectionRef[];
  fieldSources: Partial<Record<string, FieldSource>>;
}

export interface CoverRef {
  id: number;
  title: string;
  coverKind: CoverKind;
  coverRev: number;
}

export interface CollectionInfo {
  id: number;
  name: string;
  comicCount: number;
  createdAt: number;
  updatedAt: number;
  preview: CoverRef[];
}

export type SortKey = "title" | "added" | "opened" | "progress" | "recentProgress";

export interface LibraryQuery {
  text?: string;
  statuses?: ReadStatus[];
  favoritesOnly?: boolean;
  formats?: ComicFormat[];
  collectionId?: number;
  openedOnly?: boolean;
  sort?: SortKey;
  descending?: boolean;
  limit?: number;
}

export interface HomeData {
  total: number;
  continueReading: ComicSummary[];
  recentlyAdded: ComicSummary[];
  favorites: ComicSummary[];
  recentlyOpened: ComicSummary[];
}

export interface MetadataPatch {
  title: string;
  series: string | null;
  issueNumber: string | null;
  volume: string | null;
  year: number | null;
  writer: string | null;
  artist: string | null;
  publisher: string | null;
  summary: string | null;
  tags: string[];
}

export interface ReaderSession {
  comic: ComicSummary;
  path: string;
  pageCount: number;
  start: Progress;
}

export type ImportItemStatus = "added" | "relinked" | "duplicate" | "skipped" | "failed";

export interface ImportItem {
  path: string;
  name: string;
  status: ImportItemStatus;
  message: string | null;
  comicId: number | null;
}

export interface ImportCounts {
  total: number;
  processed: number;
  added: number;
  skipped: number;
  failed: number;
}

export interface ImportEvent {
  jobId: number;
  phase: "scanning" | "processing" | "done" | "cancelled";
  counts: ImportCounts;
  current: string | null;
  item: ImportItem | null;
}

export interface AppInfo {
  name: string;
  version: string;
  tauriVersion: string;
  dataDir: string;
  schemaVersion: number;
  supportedExtensions: string[];
}

export interface StorageInfo {
  dataDir: string;
  databaseBytes: number;
  coversBytes: number;
  pagesBytes: number;
  comicCount: number;
  missingCount: number;
}

export interface AppErrorInfo {
  code: string;
  message: string;
}

export const FORMAT_LABELS: Record<ComicFormat, string> = {
  cbz: "CBZ",
  cbr: "CBR",
  cb7: "CB7",
  cbt: "CBT",
  pdf: "PDF",
  folder: "Pasta de imagens",
};
