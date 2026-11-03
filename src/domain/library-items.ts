// Quadrinhos da biblioteca e obras das fontes, juntos nas mesmas telas.

import type { LibrarySeries } from "./online";
import { progressRatio } from "./progress";
import type { ComicSummary, HomeData, LibraryQuery, SortKey } from "./types";

export type LibraryItem = ComicSummary | LibrarySeries;

export const isSeries = (item: LibraryItem): item is LibrarySeries => "chapterCount" in item;
export const itemKey = (item: LibraryItem) => `${isSeries(item) ? "series" : "comic"}-${item.id}`;
export const foldText = (text: string) => text.toLocaleLowerCase("pt-BR").normalize("NFD").replace(/\p{M}/gu, "");

/** Os mesmos filtros das HQs. Uma obra não é um arquivo: filtrar por tipo de arquivo deixa só os quadrinhos. */
export function seriesMatches(series: LibrarySeries, query: LibraryQuery): boolean {
  if (query.formats?.length) return false;
  if (query.favoritesOnly && !series.favorite) return false;
  if (query.openedOnly && series.lastOpenedAt === null) return false;
  if (query.statuses?.length && !query.statuses.includes(series.readStatus)) return false;
  if (query.collectionId !== undefined && !series.collections.includes(query.collectionId)) return false;
  const haystack = foldText(`${series.title} ${series.sourceName ?? ""}`);
  return foldText(query.text ?? "")
    .split(/\s+/)
    .filter(Boolean)
    .every((word) => haystack.includes(word));
}

/** Fração lida, contada em capítulos. */
export function seriesRatio(series: LibrarySeries): number | null {
  if (series.readStatus === "completed") return 1;
  if (series.readStatus === "unread" || series.chapterCount === 0) return null;
  return series.chaptersRead / series.chapterCount;
}

export function seriesProgressLabel(series: Pick<LibrarySeries, "readStatus" | "resume">): string {
  if (series.readStatus === "completed") return "Leitura concluída";
  if (series.readStatus === "unread" || !series.resume) return "Não iniciado";
  const { title, progress } = series.resume;
  return progress ? `${title} · página ${progress.pageIndex + 1} de ${progress.pageCount}` : title;
}

const titles = new Intl.Collator("pt-BR", { numeric: true, sensitivity: "base" });
const ratioOf = (item: LibraryItem) => (isSeries(item) ? seriesRatio(item) : progressRatio(item)) ?? 0;
const recencyOf = (item: LibraryItem) => (isSeries(item) ? item.lastOpenedAt : (item.progress?.updatedAt ?? item.lastOpenedAt)) ?? 0;

function compare(a: LibraryItem, b: LibraryItem, sort: SortKey): number {
  switch (sort) {
    case "added":
      return a.addedAt - b.addedAt;
    case "opened":
      return (a.lastOpenedAt ?? 0) - (b.lastOpenedAt ?? 0);
    case "progress":
      return ratioOf(a) - ratioOf(b);
    case "recentProgress":
      return recencyOf(a) - recencyOf(b);
    case "title":
      return titles.compare(a.title, b.title);
  }
}

/** Sem obras, a ordem que o backend deu fica como está. */
export function mergeItems(comics: ComicSummary[], series: LibrarySeries[], sort: SortKey = "title", descending = false): LibraryItem[] {
  if (series.length === 0) return comics;
  const direction = descending ? -1 : 1;
  return [...comics, ...series].sort((a, b) => direction * compare(a, b, sort) || titles.compare(a.title, b.title));
}

const SHELF = 12;

/** As prateleiras da tela inicial com as obras guardadas junto. A primeira de "continuar" vira o destaque. */
export function homeItems(home: HomeData, series: LibrarySeries[]) {
  return {
    total: home.total + series.length,
    continueReading: mergeItems(home.continueReading, series.filter((item) => item.readStatus === "reading"), "recentProgress", true).slice(0, SHELF + 1),
    recentlyAdded: mergeItems(home.recentlyAdded, series, "added", true).slice(0, SHELF),
    favorites: mergeItems(home.favorites, series.filter((item) => item.favorite), "title").slice(0, SHELF),
    recentlyOpened: mergeItems(home.recentlyOpened, series.filter((item) => item.lastOpenedAt !== null), "opened", true).slice(0, SHELF),
  };
}
