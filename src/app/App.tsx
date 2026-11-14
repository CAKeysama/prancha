import { useEffect } from "react";
import { BookOpen, History, Library, Star } from "lucide-react";
import { ToastProvider } from "../components/Toast";
import { ErrorState, Spinner } from "../components/Feedback";
import { CollectionDetailScreen, CollectionsScreen } from "../features/library/CollectionsScreen";
import { DetailsScreen } from "../features/library/DetailsScreen";
import { HomeScreen } from "../features/library/HomeScreen";
import { ImportPanel } from "../features/library/ImportPanel";
import { ImportProvider } from "../features/library/import-context";
import { LibraryScreen } from "../features/library/LibraryScreen";
import { PdfCoverWorker } from "../features/library/PdfCoverWorker";
import { AddComicsButtons } from "../features/library/AddComicsButtons";
import { ComicActionsProvider } from "../features/library/comic-actions";
import { OnboardingProvider } from "../features/onboarding/onboarding-context";
import { OnboardingWizard } from "../features/onboarding/OnboardingWizard";
import { DownloadsScreen } from "../features/online/DownloadsScreen";
import { ExploreScreen } from "../features/online/ExploreScreen";
import { ExtensionsScreen } from "../features/online/ExtensionsScreen";
import { PluginsScreen } from "../features/online/PluginsScreen";
import { RepositoriesScreen } from "../features/online/RepositoriesScreen";
import { SeriesScreen } from "../features/online/SeriesScreen";
import { ChapterReaderScreen } from "../features/reader/ChapterReaderScreen";
import { ReaderScreen } from "../features/reader/ReaderScreen";
import { SettingsScreen } from "../features/settings/SettingsScreen";
import { NotificationsProvider } from "../features/notifications/notifications-context";
import { NotificationsScreen } from "../features/notifications/NotificationsScreen";
import { UpdateDialogs } from "../features/updates/UpdateDialogs";
import { UpdatesProvider } from "../features/updates/updates-context";
import { TelemetryProvider } from "../features/telemetry/telemetry-context";
import { TelemetryInvite } from "../features/telemetry/TelemetryInvite";
import { LibraryEventsProvider } from "./library-events";
import { NavigationProvider, sectionOf, useNav, type Route } from "./navigation";
import { SettingsProvider, useSettings } from "./settings-context";
import { Sidebar } from "./Sidebar";
import { SUPPORTED_FORMATS_TEXT } from "./config";
import { disableContextMenu } from "./context-menu";
import { sound } from "../services/sound";

function Screen({ route }: { route: Route }) {
  switch (route.name) {
    case "home":
      return <HomeScreen />;
    case "library":
      return (
        <LibraryScreen
          key="library"
          preset={{
            title: "Minha biblioteca",
            base: {},
            persistSort: true,
            emptyIcon: Library,
            emptyTitle: "Sua biblioteca está vazia",
            emptyText: `${SUPPORTED_FORMATS_TEXT}, ou arraste para esta janela.`,
            emptyActions: <AddComicsButtons />,
          }}
        />
      );
    case "continue":
      return (
        <LibraryScreen
          key="continue"
          preset={{
            title: "Continuar lendo",
            base: { statuses: ["reading"] },
            defaultSort: "recentProgress",
            defaultDesc: true,
            allowStatusFilter: false,
            emptyIcon: BookOpen,
            emptyTitle: "Nenhuma leitura em andamento",
            emptyText: "O que você começar a ler aparece aqui.",
          }}
        />
      );
    case "recent":
      return (
        <LibraryScreen
          key="recent"
          preset={{
            title: "Recentes",
            base: { openedOnly: true },
            defaultSort: "opened",
            defaultDesc: true,
            emptyIcon: History,
            emptyTitle: "Nada aberto ainda",
            emptyText: "O que você abrir aparece aqui.",
          }}
        />
      );
    case "favorites":
      return (
        <LibraryScreen
          key="favorites"
          preset={{
            title: "Favoritos",
            base: { favoritesOnly: true },
            emptyIcon: Star,
            emptyTitle: "Nenhum favorito",
            emptyText: "Use “Favoritar” no menu de um item ou nos detalhes.",
          }}
        />
      );
    case "collections":
      return <CollectionsScreen />;
    case "collection":
      return <CollectionDetailScreen id={route.id} />;
    case "comic":
      return <DetailsScreen key={route.id} id={route.id} />;
    case "explore":
      return <ExploreScreen />;
    case "series":
      return <SeriesScreen key={route.id} id={route.id} />;
    case "downloads":
      return <DownloadsScreen />;
    case "extensions":
      return <ExtensionsScreen />;
    case "repositories":
      return <RepositoriesScreen />;
    case "plugins":
      return <PluginsScreen />;
    case "notifications":
      return <NotificationsScreen />;
    case "settings":
      return <SettingsScreen key={route.section ?? ""} section={route.section} />;
    case "reader":
      return <ReaderScreen key={route.id} id={route.id} />;
    case "chapter":
      return <ChapterReaderScreen key={route.id} id={route.id} />;
  }
}

function Shell() {
  const { route, stack } = useNav();
  const { loaded, loadError } = useSettings();
  // Durante a leitura, só o leitor faz som.
  const reading = route.name === "reader" || route.name === "chapter";
  useEffect(() => sound.setReading(reading), [reading]);
  if (!loaded) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label="Abrindo a biblioteca…" />
      </div>
    );
  }
  if (loadError) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <ErrorState title="Não foi possível carregar os dados do aplicativo" message={loadError} />
      </div>
    );
  }
  if (reading) {
    return (
      <main className="h-full">
        <Screen route={route} />
      </main>
    );
  }
  return (
    <div className="flex h-full">
      <Sidebar current={sectionOf(stack)} />
      <main className="h-full min-w-0 flex-1">
        <Screen route={route} />
      </main>
    </div>
  );
}

export function App() {
  // O menu do botão direito do navegador sai do app inteiro. Ver context-menu.ts.
  useEffect(() => disableContextMenu(), []);
  return (
    <ToastProvider>
      <SettingsProvider>
        <NavigationProvider>
          <LibraryEventsProvider>
            <ImportProvider>
              <OnboardingProvider>
                <ComicActionsProvider>
                  <UpdatesProvider>
                    <NotificationsProvider>
                      <TelemetryProvider>
                        <Shell />
                        <ImportPanel />
                        <OnboardingWizard />
                        <PdfCoverWorker />
                        <UpdateDialogs />
                        <TelemetryInvite />
                      </TelemetryProvider>
                    </NotificationsProvider>
                  </UpdatesProvider>
                </ComicActionsProvider>
              </OnboardingProvider>
            </ImportProvider>
          </LibraryEventsProvider>
        </NavigationProvider>
      </SettingsProvider>
    </ToastProvider>
  );
}
