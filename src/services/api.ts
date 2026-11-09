// Única porta de entrada para os comandos nativos. Erros chegam como { code, message } em português.

import { invoke, type Channel } from "@tauri-apps/api/core";
import type {
  AppErrorInfo,
  AppInfo,
  CollectionInfo,
  ComicDetails,
  ComicSummary,
  HomeData,
  LibraryQuery,
  MetadataPatch,
  ReadStatus,
  ReaderSession,
  StorageInfo,
} from "../domain/types";
import type { GuidedPage, GuidedRegion } from "../domain/guided";
import type {
  BrowsePage,
  ChapterSession,
  CatalogItem,
  CatalogQuery,
  ComicOrigin,
  DownloadView,
  InstalledExtension,
  RepositoryInfo,
  RepositoryPreview,
  LibrarySeries,
  RecommendedList,
  SeriesCard,
  ChapterReadInfo,
  SeriesView,
  SourceSummary,
} from "../domain/online";
import type { RatingDetails, RatingState } from "../domain/rating";
import type { ReaderMode } from "../domain/settings";
import type { UpdateProgress, UpdatesHistory, UpdatesSnapshot } from "../domain/updates";
import type { NotificationAction, NotificationCategory, NotificationPage, NotificationQuery, NotificationTarget } from "../domain/notifications";

export class ApiError extends Error {
  code: string;
  constructor(info: AppErrorInfo) {
    super(info.message);
    this.code = info.code;
  }
}

function normalizeError(e: unknown): ApiError {
  if (e instanceof ApiError) return e;
  if (e && typeof e === "object" && "message" in e) {
    const o = e as { code?: unknown; message: unknown };
    return new ApiError({ code: typeof o.code === "string" ? o.code : "unknown", message: String(o.message) });
  }
  return new ApiError({ code: "unknown", message: typeof e === "string" ? e : "Ocorreu um erro inesperado." });
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw normalizeError(e);
  }
}

export function errorMessage(e: unknown): string {
  return normalizeError(e).message;
}

/** O que a pessoa estava tentando abrir, para a verificação começar pela mesma página. */
export type VerifiableSite = { chapterId: number } | { seriesId: number } | { sourceId: string };

export const api = {
  appInfo: () => call<AppInfo>("get_app_info"),
  getSettings: () => call<Record<string, unknown>>("get_settings"),
  setSetting: (key: string, value: unknown) => call<void>("set_setting", { key, value }),

  listComics: (query: LibraryQuery) => call<ComicSummary[]>("list_comics", { query }),
  home: () => call<HomeData>("get_home"),
  comic: (id: number) => call<ComicDetails>("get_comic", { id }),
  tags: () => call<string[]>("list_tags"),
  updateMetadata: (id: number, patch: MetadataPatch) => call<ComicDetails>("update_metadata", { id, patch }),
  setFavorite: (id: number, favorite: boolean) => call<void>("set_favorite", { id, favorite }),
  setReadStatus: (id: number, status: Exclude<ReadStatus, "reading">) => call<void>("set_read_status", { id, status }),
  resetProgress: (id: number) => call<void>("reset_progress", { id }),
  removeComic: (id: number, deleteOriginal: boolean) =>
    call<{ trashed: boolean }>("remove_comic", { id, deleteOriginal }),
  relocateComic: (id: number, force: boolean, path?: string) =>
    call<{ picked: boolean; matched: boolean; updated: boolean; path: string | null }>("relocate_comic", {
      id,
      force,
      path: path ?? null,
    }),
  revealComic: (id: number) => call<void>("reveal_comic", { id }),
  openDataDir: () => call<void>("open_data_dir"),

  pickImportPaths: (mode: "files" | "folder") => call<string[] | null>("pick_import_paths", { mode }),
  startImport: (paths: string[]) => call<number>("start_import", { paths }),
  cancelImport: (jobId: number) => call<boolean>("cancel_import", { jobId }),

  openComic: (id: number) => call<ReaderSession>("open_comic", { id }),
  saveProgress: (id: number, pageIndex: number, pageCount: number, scrollRatio: number) =>
    call<ReadStatus>("save_progress", { id, pageIndex, pageCount, scrollRatio }),
  setPdfPageCount: (id: number, pageCount: number) => call<void>("set_pdf_page_count", { id, pageCount }),
  pendingPdfCovers: () => call<number[]>("pending_pdf_covers"),
  savePdfCover: (id: number, image: Uint8Array, pageCount: number | null) =>
    call<void>("save_pdf_cover", { id, image: Array.from(image), pageCount }),
  reportPdfCoverFailure: (id: number, message: string) => call<void>("report_pdf_cover_failure", { id, message }),

  guidedPage: (comicId: number, pageIndex: number, rtl: boolean) => call<GuidedPage | null>("guided_page", { comicId, pageIndex, rtl }),
  guidedAnalyzeImage: (comicId: number, pageIndex: number, rtl: boolean, image: Uint8Array) =>
    call<GuidedPage>("guided_analyze_image", { comicId, pageIndex, rtl, image: Array.from(image) }),
  guidedSaveCorrections: (comicId: number, pageIndex: number, regions: GuidedRegion[]) =>
    call<void>("guided_save_corrections", { comicId, pageIndex, regions }),
  guidedClearCorrections: (comicId: number, pageIndex: number) => call<void>("guided_clear_corrections", { comicId, pageIndex }),

  collections: () => call<CollectionInfo[]>("list_collections"),
  createCollection: (name: string) => call<number>("create_collection", { name }),
  renameCollection: (id: number, name: string) => call<void>("rename_collection", { id, name }),
  deleteCollection: (id: number) => call<void>("delete_collection", { id }),
  setComicCollections: (comicId: number, collectionIds: number[]) =>
    call<void>("set_comic_collections", { comicId, collectionIds }),
  addToCollection: (collectionId: number, comicIds: number[]) =>
    call<void>("add_to_collection", { collectionId, comicIds }),
  removeFromCollection: (collectionId: number, comicId: number) =>
    call<void>("remove_from_collection", { collectionId, comicId }),

  storageInfo: () => call<StorageInfo>("storage_info"),
  clearCoverCache: () => call<number>("clear_cover_cache"),
  clearPageCache: () => call<number>("clear_page_cache"),
  regenerateCovers: () => call<void>("regenerate_covers"),
  verifyLibrary: () => call<{ missing: number; restored: number }>("verify_library"),

  updatesSnapshot: () => call<UpdatesSnapshot>("updates_snapshot"),
  updatesCheck: () => call<UpdatesSnapshot>("updates_check"),
  updatesDecline: (version: string) => call<void>("updates_decline", { version }),
  updatesNotesShown: (version: string) => call<void>("updates_notes_shown", { version }),
  updatesInstall: (onProgress: Channel<UpdateProgress>) => call<void>("updates_install", { onProgress }),
  updatesCancel: () => call<void>("updates_cancel"),
  updatesHistory: (force: boolean) => call<UpdatesHistory>("updates_history", { force }),
  updatesOpenRelease: (version: string) => call<void>("updates_open_release", { version }),
  /** Abre o formulário do GitHub já preenchido; quem envia é a pessoa, lá. */
  openImprovementForm: (title: string, text: string) => call<void>("open_improvement_form", { title, text }),

  repositories: () => call<RepositoryInfo[]>("repositories_list"),
  repositoriesRecommended: (refresh: boolean) => call<RecommendedList>("repositories_recommended", { refresh }),
  repositoryPreview: (address: string) => call<RepositoryPreview>("repository_preview", { address }),
  repositoryAdd: (address: string, fingerprint: string) => call<RepositoryInfo>("repository_add", { address, fingerprint }),
  repositoryAcceptKey: (id: string, fingerprint: string) => call<RepositoryInfo>("repository_accept_key", { id, fingerprint }),
  repositoryRemove: (id: string) => call<void>("repository_remove", { id }),
  repositorySetEnabled: (id: string, enabled: boolean) => call<RepositoryInfo>("repository_set_enabled", { id, enabled }),
  repositorySync: (id: string) => call<RepositoryInfo>("repository_sync", { id }),
  repositoriesSyncAll: () => call<RepositoryInfo[]>("repositories_sync_all"),

  extensionCatalog: (query: CatalogQuery) => call<CatalogItem[]>("extension_catalog", { query }),
  extensionCatalogLanguages: () => call<string[]>("extension_catalog_languages"),
  extensions: () => call<InstalledExtension[]>("extensions_list"),
  extensionInstall: (repositoryId: string, extensionId: string) => call<InstalledExtension>("extension_install", { repositoryId, extensionId }),
  extensionUpdate: (id: string) => call<InstalledExtension>("extension_update", { id }),
  extensionRemove: (id: string) => call<void>("extension_remove", { id }),
  extensionSetEnabled: (id: string, enabled: boolean) => call<InstalledExtension>("extension_set_enabled", { id, enabled }),

  sources: () => call<SourceSummary[]>("sources_list"),
  sourcePopular: (requestId: string, source: SourceSummary, page: number, kind: string | null) =>
    call<BrowsePage>("source_popular", { requestId, extensionId: source.extensionId, sourceId: source.sourceId, page, kind }),
  sourceLatest: (requestId: string, source: SourceSummary, page: number, kind: string | null) =>
    call<BrowsePage>("source_latest", { requestId, extensionId: source.extensionId, sourceId: source.sourceId, page, kind }),
  sourceSearch: (requestId: string, source: SourceSummary, query: string, page: number, kind: string | null) =>
    call<BrowsePage>("source_search", { requestId, extensionId: source.extensionId, sourceId: source.sourceId, query, page, kind }),
  sourceCancel: (requestId: string) => call<void>("source_cancel", { requestId }),

  seriesStored: (id: number) => call<SeriesView>("series_stored", { id }),
  seriesOpen: (requestId: string, id: number) => call<SeriesView>("series_open", { requestId, id }),
  seriesSetFollowing: (id: number, following: boolean) => call<SeriesCard>("series_set_following", { id, following }),
  seriesSetChapterSort: (id: number, sort: "asc" | "desc") => call<SeriesCard>("series_set_chapter_sort", { id, sort }),
  seriesSetReaderMode: (id: number, mode: ReaderMode) => call<void>("series_set_reader_mode", { id, mode }),
  chapterOpen: (requestId: string, id: number) => call<ChapterSession>("chapter_open", { requestId, id }),
  chapterSaveProgress: (id: number, pageIndex: number, pageCount: number, scrollRatio: number) => call<void>("chapter_save_progress", { id, pageIndex, pageCount, scrollRatio }),
  chapterResetProgress: (id: number) => call<void>("chapter_reset_progress", { id }),
  chapterAcknowledge: (id: number) => call<ChapterReadInfo>("chapter_acknowledge", { id }),
  chapterReadingStart: (id: number) => call<ChapterReadInfo>("chapter_reading_start", { id }),
  chapterReadingTouch: (id: number) => call<boolean>("chapter_reading_touch", { id }),
  chapterReadingEnd: (id: number) => call<ChapterReadInfo>("chapter_reading_end", { id }),
  chapterReadingComplete: (id: number) => call<ChapterReadInfo>("chapter_reading_complete", { id }),
  chaptersSetRead: (ids: number[], read: boolean) => call<ChapterReadInfo[]>("chapters_set_read", { ids, read }),
  chaptersMarkPreviousRead: (id: number) => call<ChapterReadInfo[]>("chapters_mark_previous_read", { id }),
  seriesRefreshFollowing: (requestId: string) => call<SeriesCard[]>("series_refresh_following", { requestId }),
  onlineStop: () => call<void>("online_stop"),
  seriesFollowing: () => call<SeriesCard[]>("series_following"),
  seriesLibrary: () => call<LibrarySeries[]>("series_library"),
  seriesRating: (id: number) => call<RatingDetails>("series_rating", { id }),
  /** `null` volta à classificação automática. */
  seriesSetRating: (id: number, rating: Extract<RatingState, "confirmed" | "none"> | null, reason: string | null) => call<RatingDetails>("series_set_rating", { id, rating, reason }),
  seriesSetFavorite: (id: number, favorite: boolean) => call<void>("series_set_favorite", { id, favorite }),
  seriesSetCollections: (id: number, collectionIds: number[]) => call<void>("series_set_collections", { id, collectionIds }),
  addSeriesToCollection: (collectionId: number, ids: number[]) => call<void>("series_add_to_collection", { collectionId, ids }),
  removeSeriesFromCollection: (collectionId: number, id: number) => call<void>("series_remove_from_collection", { collectionId, id }),
  seriesSetRead: (id: number, read: boolean) => call<void>("series_set_read", { id, read }),
  seriesForget: (id: number) => call<void>("series_forget", { id }),
  /** Termina quando a janela do site fecha, e diz se o site liberou. */
  sourceVerify: (site: VerifiableSite) => call<boolean>("source_verify", { ...site }),
  comicOrigin: (id: number) => call<ComicOrigin | null>("comic_origin", { id }),

  downloads: (limit: number) => call<DownloadView[]>("downloads_list", { limit }),
  downloadsEnqueue: (chapterIds: number[]) => call<number[]>("downloads_enqueue", { chapterIds }),
  downloadPause: (id: number) => call<void>("download_pause", { id }),
  downloadResume: (id: number) => call<void>("download_resume", { id }),
  downloadCancel: (id: number) => call<void>("download_cancel", { id }),
  downloadRetry: (id: number) => call<void>("download_retry", { id }),
  downloadsClearFinished: () => call<number>("downloads_clear_finished"),

  notificationsList: (query: NotificationQuery) => call<NotificationPage>("notifications_list", { query }),
  notificationsUnread: () => call<number>("notifications_unread"),
  notificationsSetRead: (ids: number[], read: boolean) => call<number>("notifications_set_read", { ids, read }),
  notificationsMarkAllRead: (category?: NotificationCategory) => call<number>("notifications_mark_all_read", { category: category ?? null }),
  notificationsRemove: (ids: number[]) => call<number>("notifications_remove", { ids }),
  notificationsClearRead: () => call<number>("notifications_clear_read"),
  notificationTarget: (id: number, action?: NotificationAction) => call<NotificationTarget>("notification_target", { id, action: action ?? null }),
  notificationsNativeStatus: () => call<{ permission: "granted" | "denied" | "prompt" }>("notifications_native_status"),
  notificationsNativeTest: () => call<{ permission: "granted" | "denied" | "prompt" }>("notifications_native_test"),
};
