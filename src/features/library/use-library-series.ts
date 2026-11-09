import type { LibrarySeries } from "../../domain/online";
import { api } from "../../services/api";
import { useLibraryData } from "../../app/library-events";
import { useSettings } from "../../app/settings-context";

const NONE: LibrarySeries[] = [];

/** As obras guardadas. Sem o plugin, nenhuma: a biblioteca volta a ser só de HQs. */
export function useLibrarySeries(): LibrarySeries[] {
  const { settings } = useSettings();
  const online = settings["plugins.online"];
  const { data } = useLibraryData(() => (online ? api.seriesLibrary() : Promise.resolve(NONE)), [online], "series-library");
  return online ? (data ?? NONE) : NONE;
}
