import type { ReactNode } from "react";
import { BookOpen, ChevronRight, FolderPlus, GraduationCap, Library, Plus } from "lucide-react";
import { Button } from "../../components/Button";
import { ErrorState, Spinner } from "../../components/Feedback";
import { PageHeader } from "../../components/PageHeader";
import { Cover } from "../../components/Cover";
import { ProgressBar } from "../../components/Feedback";
import { progressLabel, progressRatio, subtitle } from "../../domain/progress";
import type { ComicSummary } from "../../domain/types";
import { api } from "../../services/api";
import { useLibraryData } from "../../app/library-events";
import { useNav, type Route } from "../../app/navigation";
import { APP_NAME, SUPPORTED_FORMATS_TEXT } from "../../app/config";
import { AddComicsButtons } from "./AddComicsButtons";
import { ComicShelf } from "./ComicCollectionView";
import { useImport } from "./import-context";
import { useComicActions } from "./comic-actions";
import { useOnboarding } from "../onboarding/onboarding-context";

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
function ResumeHero({ comic }: { comic: ComicSummary }) {
  const actions = useComicActions();
  const ratio = progressRatio(comic);
  return (
    <div className="mb-8 flex items-center gap-5 rounded-lg border border-border bg-surface p-4">
      <div className="w-20 shrink-0">
        <Cover comicId={comic.id} title={comic.title} kind={comic.coverKind} rev={comic.coverRev} error={comic.coverError} eager />
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-xs font-medium tracking-wide text-muted uppercase">Continue de onde parou</p>
        <p className="mt-0.5 truncate text-xl font-semibold">{comic.title}</p>
        {subtitle(comic) && <p className="truncate text-sm text-muted">{subtitle(comic)}</p>}
        <p className="mt-2 text-sm">{progressLabel(comic)}</p>
        {ratio !== null && <ProgressBar value={ratio} label={`Progresso de ${comic.title}`} className="mt-1.5 max-w-sm" />}
      </div>
      <div className="flex shrink-0 flex-col gap-2 sm:flex-row">
        <Button variant="ghost" onClick={() => actions.openDetails(comic)}>
          Detalhes
        </Button>
        <Button variant="primary" icon={BookOpen} onClick={() => actions.read(comic)} disabled={comic.missing}>
          Continuar leitura
        </Button>
      </div>
    </div>
  );
}

function EmptyHome() {
  const { pickAndImport } = useImport();
  const onboarding = useOnboarding();
  return (
    <div className="mx-auto flex max-w-xl flex-col items-center px-6 py-14 text-center">
      <div className="mb-5 flex size-14 items-center justify-center rounded-xl border border-border bg-surface text-accent">
        <Library size={26} aria-hidden />
      </div>
      <h2 className="text-2xl font-semibold">Sua biblioteca está vazia</h2>
      <p className="mt-2 text-base text-muted">
        O {APP_NAME} organiza seus quadrinhos em um só lugar, gera as capas e guarda onde você parou em cada leitura.
        Os arquivos continuam onde estão no seu computador.
      </p>
      <div className="mt-6 flex flex-wrap justify-center gap-2">
        <Button variant="primary" icon={Plus} onClick={() => void pickAndImport("files")} data-tour="add-comics">
          Adicionar quadrinhos
        </Button>
        <Button icon={FolderPlus} onClick={() => void pickAndImport("folder")}>
          Selecionar uma pasta
        </Button>
      </div>
      <p className="mt-4 text-sm text-muted">Formatos aceitos: {SUPPORTED_FORMATS_TEXT}. Você também pode arrastar arquivos para esta janela.</p>
      <Button variant="ghost" size="sm" icon={GraduationCap} className="mt-6" onClick={onboarding.restart}>
        Ver o tutorial novamente
      </Button>
    </div>
  );
}

export function HomeScreen() {
  const { data, error, reload } = useLibraryData(() => api.home(), []);
  return (
    <div className="h-full overflow-y-auto">
      <PageHeader title="Início" subtitle={data && data.total > 0 ? `${data.total} ${data.total === 1 ? "quadrinho" : "quadrinhos"} na biblioteca` : undefined} actions={data && data.total > 0 ? <AddComicsButtons /> : undefined} />
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
            {data.continueReading[0] && <ResumeHero comic={data.continueReading[0]} />}
            {data.continueReading.length > 1 && (
              <Section title="Continuar lendo" to={{ name: "continue" }}>
                <ComicShelf comics={data.continueReading.slice(1)} />
              </Section>
            )}
            {data.recentlyAdded.length > 0 && (
              <Section title="Adicionados recentemente" to={{ name: "library" }}>
                <ComicShelf comics={data.recentlyAdded} />
              </Section>
            )}
            {data.favorites.length > 0 && (
              <Section title="Favoritos" to={{ name: "favorites" }}>
                <ComicShelf comics={data.favorites} />
              </Section>
            )}
            {data.recentlyOpened.length > 0 && (
              <Section title="Acessados recentemente" to={{ name: "recent" }}>
                <ComicShelf comics={data.recentlyOpened} />
              </Section>
            )}
          </>
        )}
      </div>
    </div>
  );
}
