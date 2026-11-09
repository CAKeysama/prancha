// Interface da integração com o MyAnimeList, com o backend simulado. A lógica (OAuth, tokens, associação, fila)
// é testada no Rust, em src-tauri/src/mal/tests.rs.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../../src/app/App";
import { ToastProvider } from "../../src/components/Toast";
import { MalSeriesFact } from "../../src/features/mal/MalSeries";
import { candidateDetails, syncLabel, type MalCandidate, type MalSeriesStatus, type MalStatus } from "../../src/domain/mal";

type Handler = (args: Record<string, unknown>) => unknown;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;
let handlers: Record<string, Handler>;

const disconnected: MalStatus = { available: true, state: "disconnected", userName: null, connectedAt: null, lastSyncAt: null, pending: 0, pendingError: null, failures: [] };
const connected: MalStatus = { ...disconnected, state: "connected", userName: "leitora", connectedAt: 1_700_000_000_000 };

const berserk: MalCandidate = { malId: 2, title: "Berserk", alternative: ["ベルセルク"], mediaType: "manga", numChapters: 380, status: "currently_publishing", year: "1989", authors: ["Kentaro Miura"], score: 90 };
const prototype: MalCandidate = { malId: 99, title: "Berserk: The Prototype", alternative: [], mediaType: "one_shot", numChapters: 1, status: "finished", year: "1988", authors: [], score: 60 };

function series(partial: Partial<MalSeriesStatus>): MalSeriesStatus {
  return { available: true, connected: true, state: "unchecked", link: null, candidates: [], sync: null, error: null, ...partial };
}

beforeEach(() => {
  calls = [];
  handlers = {};
  mockWindows("main");
  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      if (handlers[cmd]) return handlers[cmd]!(args);
      switch (cmd) {
        case "get_settings":
          return { onboarding: { status: "completed", step: 4, comicId: 1 } };
        case "get_home":
          return { total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] };
        case "pending_pdf_covers":
        case "list_collections":
        case "list_comics":
          return [];
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  cleanup();
  clearMocks();
});

async function openIntegrations() {
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: "Configurações" }));
  await user.click(await screen.findByRole("button", { name: "Integrações" }));
  return user;
}

describe("configurações", () => {
  it("conecta pelo navegador e mostra a conta", async () => {
    let status = disconnected;
    handlers.mal_status = () => status;
    handlers.mal_connect = () => {
      status = connected;
      return connected;
    };
    const user = await openIntegrations();
    expect(await screen.findByTestId("mal-state")).toHaveTextContent("Desconectada");
    await user.click(screen.getByRole("button", { name: "Conectar conta" }));
    await waitFor(() => expect(screen.getByTestId("mal-state")).toHaveTextContent("Conectada"));
    expect(screen.getByText(/Conectada como leitora/)).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === "mal_connect")).toBe(true);
  });

  it("desconecta só depois de confirmar e avisa o que será descartado", async () => {
    let status: MalStatus = { ...connected, pending: 2 };
    handlers.mal_status = () => status;
    handlers.mal_disconnect = () => {
      status = disconnected;
      return 2;
    };
    const user = await openIntegrations();
    await user.click(await screen.findByRole("button", { name: "Desconectar" }));
    const dialog = await screen.findByRole("dialog", { name: "Desconectar o MyAnimeList?" });
    expect(dialog).toHaveTextContent("2 atualização(ões)");
    expect(calls.some((c) => c.cmd === "mal_disconnect")).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: "Desconectar" }));
    await waitFor(() => expect(screen.getByTestId("mal-state")).toHaveTextContent("Desconectada"));
  });

  it("sessão expirada pede para conectar de novo sem perder a fila", async () => {
    handlers.mal_status = () => ({ ...connected, state: "expired", pending: 1 });
    await openIntegrations();
    expect(await screen.findByTestId("mal-state")).toHaveTextContent("Sessão expirada");
    expect(screen.getByRole("button", { name: "Conectar de novo" })).toBeEnabled();
    expect(screen.getByText(/nada pendente foi perdido/)).toBeInTheDocument();
  });

  it("mostra as falhas e tenta de novo", async () => {
    handlers.mal_status = () => ({ ...connected, pending: 1, pendingError: "Sem conexão com o MyAnimeList.", failures: [{ malId: 2, title: "Berserk", chapters: 12, error: "O MyAnimeList recusou a atualização (erro 400)." }] });
    handlers.mal_retry = () => 2;
    const user = await openIntegrations();
    expect(await screen.findByText("Berserk")).toBeInTheDocument();
    expect(screen.getByText(/erro 400/)).toBeInTheDocument();
    expect(screen.getByText(/1 atualização esperando a vez\. Última tentativa: Sem conexão/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Tentar agora" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "mal_retry")).toBe(true));
  });

  it("sem a integração configurada, não dá para conectar", async () => {
    handlers.mal_status = () => ({ ...disconnected, available: false });
    await openIntegrations();
    expect(await screen.findByText(/não tem a integração com o MyAnimeList configurada/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Conectar conta" })).toBeDisabled();
  });

  it("a pesquisa das configurações encontra o MyAnimeList", async () => {
    handlers.mal_status = () => disconnected;
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    await user.type(screen.getByRole("searchbox", { name: /Pesquisar/ }), "mal");
    expect(await screen.findByText("MyAnimeList")).toBeInTheDocument();
  });
});

function renderFact(id = 1) {
  return render(
    <ToastProvider>
      <dl>
        <MalSeriesFact seriesId={id} title="Berserk" />
      </dl>
    </ToastProvider>,
  );
}

describe("página da obra", () => {
  it("sem conta conectada, nada aparece e o MAL não é consultado além do estado", async () => {
    handlers.mal_series = () => series({ connected: false });
    const { container } = renderFact();
    await waitFor(() => expect(calls.some((c) => c.cmd === "mal_series")).toBe(true));
    expect(container).not.toHaveTextContent("MyAnimeList");
    expect(calls.filter((c) => c.cmd.startsWith("mal_")).map((c) => c.cmd)).toEqual(["mal_series"]);
  });

  it("procura ao abrir a obra e mostra a associação e a sincronização", async () => {
    handlers.mal_series = () =>
      series({ state: "linked", link: { malId: 2, title: "Berserk", mediaType: "manga", numChapters: 380, origin: "auto", confidence: 100, syncedChapters: 12, linkedAt: 1 } });
    renderFact();
    expect(await screen.findByRole("button", { name: /Berserk/ })).toBeInTheDocument();
    expect(screen.getByTestId("mal-sync")).toHaveTextContent("12 capítulos no MAL");
    expect(calls.find((c) => c.cmd === "mal_series")!.args).toEqual({ seriesId: 1, lookup: true });
  });

  it("mostra o erro de uma atualização que não foi enviada", async () => {
    handlers.mal_series = () =>
      series({
        state: "linked",
        link: { malId: 2, title: "Berserk", mediaType: "manga", numChapters: 380, origin: "manual", confidence: 100, syncedChapters: null, linkedAt: 1 },
        sync: { status: "failed", chapters: 13, attempts: 10, error: "O MyAnimeList está com problemas (erro 503)." },
      });
    renderFact();
    expect(await screen.findByTestId("mal-sync")).toHaveTextContent("Não sincronizou o capítulo 13");
    expect(screen.getByText(/erro 503/)).toBeInTheDocument();
  });

  it("com várias possíveis, a pessoa escolhe a certa", async () => {
    handlers.mal_series = () => series({ state: "ambiguous", candidates: [berserk, prototype] });
    handlers.mal_link = (args) => series({ state: "linked", link: { malId: args.malId as number, title: "Berserk", mediaType: "manga", numChapters: 380, origin: "manual", confidence: 100, syncedChapters: null, linkedAt: 2 } });
    const user = userEvent.setup();
    renderFact();
    await user.click(await screen.findByRole("button", { name: "Escolher" }));
    const dialog = await screen.findByRole("dialog", { name: "Associar ao MyAnimeList" });
    const list = within(dialog).getByRole("list", { name: "Obras encontradas no MyAnimeList" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
    expect(within(list).getByText(/One-shot · 1988 · 1 capítulos/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Associar a Berserk" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "mal_link")?.args).toEqual({ seriesId: 1, malId: 2 }));
    expect(await screen.findByRole("button", { name: /Berserk/ })).toBeInTheDocument();
  });

  it("corrige pesquisando de novo ou marca que não está no MAL", async () => {
    handlers.mal_series = () =>
      series({ state: "linked", link: { malId: 99, title: "Berserk: The Prototype", mediaType: "one_shot", numChapters: 1, origin: "auto", confidence: 90, syncedChapters: null, linkedAt: 1 } });
    handlers.mal_search = () => [berserk];
    handlers.mal_unlink = () => series({ state: "none", link: { malId: null, title: null, mediaType: null, numChapters: null, origin: "none", confidence: 0, syncedChapters: null, linkedAt: 3 } });
    const user = userEvent.setup();
    renderFact();
    await user.click(await screen.findByRole("button", { name: "Corrigir" }));
    const dialog = await screen.findByRole("dialog", { name: "Associar ao MyAnimeList" });
    await waitFor(() => expect(calls.find((c) => c.cmd === "mal_search")?.args).toEqual({ seriesId: 1, query: "Berserk" }));
    expect(await within(dialog).findByRole("button", { name: "Associar a Berserk" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Não está no MyAnimeList" }));
    expect(await screen.findByText("Sem associação")).toBeInTheDocument();
  });

  it("falha de rede na procura não quebra a página e permite tentar de novo", async () => {
    let attempt = 0;
    handlers.mal_series = () => (++attempt === 1 ? series({ state: "error", error: "Sem conexão com o MyAnimeList." }) : series({ state: "not_found" }));
    const user = userEvent.setup();
    renderFact();
    expect(await screen.findByText(/Sem conexão com o MyAnimeList/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Tentar de novo" }));
    expect(await screen.findByText("Não encontrada no MAL")).toBeInTheDocument();
  });
});

describe("textos", () => {
  it("resume a candidata e o estado da sincronização", () => {
    expect(candidateDetails(berserk)).toBe("Mangá · 1989 · 380 capítulos · Em publicação · Kentaro Miura");
    expect(syncLabel(series({ state: "linked", sync: { status: "pending", chapters: 5, attempts: 0, error: null } }))).toBe("Sincronizando capítulo 5");
    expect(syncLabel(series({ state: "linked", sync: { status: "pending", chapters: 5, attempts: 2, error: "x" } }))).toBe("Capítulo 5 aguardando nova tentativa");
    expect(syncLabel(series({ state: "ambiguous" }))).toBeNull();
  });
});
