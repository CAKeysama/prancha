import { useMemo, type ReactNode } from "react";
import { BookOpen, ChevronRight, FolderPlus, Library, Plus } from "lucide-react";
import { Button } from "../../components/Button";
import { ErrorState, ProgressBar, Spinner } from "../../components/Feedback";
import { PageHeader } from "../../components/PageHeader";
import { Cover } from "../../components/Cover";
import { homeItems, isSeries, seriesProgressLabel, seriesRatio, type LibraryItem } from "../../domain/library-items";
import { progressLabel, progressRatio, subtitle } from "../../domain/progress";
import { api } from "../../services/api";
import { useLibraryData } from "../../app/library-events";
import { useNav, type Route } from "../../app/navigation";
import { SUPPORTED_FORMATS_TEXT } from "../../app/config";
import { useScrollMemory } from "../../app/scroll-memory";
import { AddComicsButtons } from "./AddComicsButtons";
import { ComicShelf } from "./ComicCollectionView";
import { useImport } from "./import-context";
import { useComicActions } from "./comic-actions";
import { useLibrarySeries } from "./use-library-series";
import { OnlineCover } from "../online/parts";

function Section({ title, to, children }: { title: string; to?: Route; children: ReactNode }) {
  const nav = useNav();
  return (
    <section aria-label={title} className="mb-8">
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-lg font-semibold">{title}</h2>
        {to && (
          <Button variant="ghost" size="sm" trailingIcon={ChevronRight} onClick={() => nav.navigate(to)}>
            Ver tudo
          </Button>
        )}
      </div>
      {children}
    </section>
  );
}

/** Destaque para retomar a leitura mais recente com um clique. */
function ResumeHero({ item }: { item: LibraryItem }) {
  const actions = useComicActions();
  const online = isSeries(item);
  const ratio = online ? seriesRatio(item) : progressRatio(item);
  const sub = online ? item.sourceName : subtitle(item);
  return (
    <div className="mb-8 flex items-center gap-5 rounded-lg border border-border bg-surface p-4">
      <div className="w-20 shrink-0">
        {online ? <OnlineCover series={item} eager /> : <Cover comicId={item.id} title={item.title} kind={item.coverKind} rev={item.coverRev} error={item.coverError} eager />}
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-xs font-medium tracking-wide text-muted uppercase">Continue de onde parou</p>
        <p className="mt-0.5 truncate text-xl font-semibold">{item.title}</p>
        {sub && <p className="truncate text-sm text-muted">{sub}</p>}
        <p className="mt-2 text-sm">{online ? seriesProgressLabel(item) : progressLabel(item)}</p>
        {ratio !== null && <ProgressBar value={ratio} label={`Progresso de ${item.title}`} className="mt-1.5 max-w-sm" />}
      </div>
      <div className="flex shrink-0 flex-col gap-2 sm:flex-row">
        <Button variant="ghost" onClick={() => (online ? actions.openSeries(item) : actions.openDetails(item))}>
          Detalhes
        </Button>
        <Button variant="primary" icon={BookOpen} onClick={() => (online ? actions.readSeries(item) : actions.read(item))} disabled={!online && item.missing}>
          Continuar leitura
        </Button>
      </div>
    </div>
  );
}

function EmptyHome() {
  const { pickAndImport } = useImport();
  return (
    <div className="mx-auto flex max-w-xl flex-col items-center px-6 py-14 text-center">
      <Library size={32} className="mb-4 text-accent" aria-hidden />
      <h2 className="text-2xl font-semibold">Sua biblioteca está vazia</h2>
      <p className="mt-2 text-base text-muted">{SUPPORTED_FORMATS_TEXT}, ou arraste para esta janela.</p>
      <div className="mt-6 flex flex-wrap justify-center gap-2">
        <Button variant="primary" icon={Plus} onClick={() => void pickAndImport("files")}>
          Adicionar quadrinhos
        </Button>
        <Button icon={FolderPlus} onClick={() => void pickAndImport("folder")}>
          Adicionar pasta
        </Button>
      </div>
    </div>
  );
}

export function HomeScreen() {
  const { data: home, error, reload } = useLibraryData(() => api.home(), [], "home");
  const series = useLibrarySeries();
  const data = useMemo(() => home && homeItems(home, series), [home, series]);
  const scrollRef = useScrollMemory<HTMLDivElement>("home");
  return (
    <div ref={scrollRef} className="h-full overflow-y-auto">
      <PageHeader title="Início" actions={data && data.total > 0 ? <AddComicsButtons /> : undefined} />
      <div className="px-6 py-6">
        {error && (
          <ErrorState
            title="Não foi possível carregar a tela inicial"
            message={error}
            action={
              <Button size="sm" onClick={reload}>
                Tentar novamente
              </Button>
            }
          />
        )}
        {!data && !error && <Spinner label="Carregando…" />}
        {data && data.total === 0 && <EmptyHome />}
        {data && data.total > 0 && (
          <>
            {data.continueReading[0] && <ResumeHero item={data.continueReading[0]} />}
            {data.continueReading.length > 1 && (
              <Section title="Continuar lendo" to={{ name: "continue" }}>
                <ComicShelf items={data.continueReading.slice(1)} />
              </Section>
            )}
            {data.recentlyAdded.length > 0 && (
              <Section title="Adicionados recentemente" to={{ name: "library" }}>
                <ComicShelf items={data.recentlyAdded} />
              </Section>
            )}
            {data.favorites.length > 0 && (
              <Section title="Favoritos" to={{ name: "favorites" }}>
                <ComicShelf items={data.favorites} />
              </Section>
            )}
            {data.recentlyOpened.length > 0 && (
              <Section title="Abertos recentemente" to={{ name: "recent" }}>
                <ComicShelf items={data.recentlyOpened} />
              </Section>
            )}
          </>
        )}
      </div>
    </div>
  );
}
