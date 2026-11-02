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
import type { UpdateProgress, UpdatesHistory, UpdatesSnapshot } from "../domain/updates";

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
    call<{ removed: boolean; trashed: boolean }>("remove_comic", { id, deleteOriginal }),
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
};
