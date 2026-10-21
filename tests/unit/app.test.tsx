// Testes de interface com o backend simulado por `mockIPC` (API oficial de testes do Tauri).
// O backend real é coberto pelos testes Rust e pelos testes E2E no aplicativo compilado.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../../src/app/App";
import { Cover } from "../../src/components/Cover";
import type { ComicSummary } from "../../src/domain/types";

type Handler = (args: Record<string, unknown>) => unknown;

const comic: ComicSummary = {
  id: 7,
  title: "Teste em Quadrinhos",
  fileName: "teste.cbz",
  format: "cbz",
  series: "Série",
  issueNumber: "1",
  volume: null,
  writer: null,
  pageCount: 12,
  coverKind: "placeholder",
  coverRev: 1,
  coverError: "nenhuma página decodificou",
  favorite: false,
  status: "reading",
  missing: false,
  addedAt: 1,
  lastOpenedAt: 2,
  progress: { pageIndex: 3, pageCount: 12, scrollRatio: 0, updatedAt: 3 },
};

let settings: Record<string, unknown>;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;
let overrides: Record<string, Handler>;
let comics: ComicSummary[];

function setup() {
  mockWindows("main");
  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      if (overrides[cmd]) return overrides[cmd]!(args);
      switch (cmd) {
        case "get_settings":
          return settings;
        case "set_setting":
          settings[args.key as string] = args.value;
          return null;
        case "get_home":
          return {
            total: comics.length,
            continueReading: comics.filter((c) => c.status === "reading"),
            recentlyAdded: comics,
            favorites: [],
            recentlyOpened: comics,
          };
        case "list_comics":
          return comics;
        case "pending_pdf_covers":
          return [];
        case "list_collections":
          return [];
        case "get_app_info":
          return { name: "Prancha", version: "0.1.0", tauriVersion: "2", dataDir: "C:/dados", schemaVersion: 1, supportedExtensions: [] };
        case "storage_info":
          return { dataDir: "C:/dados", databaseBytes: 1024, coversBytes: 2048, pagesBytes: 0, comicCount: comics.length, missingCount: 0 };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}

beforeEach(() => {
  settings = {};
  calls = [];
  overrides = {};
  comics = [];
  localStorage.clear();
  setup();
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("primeira execução", () => {
  it("mostra o tutorial e permite pular", async () => {
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByRole("dialog", { name: "Bem-vindo ao Prancha" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Pular tutorial" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Bem-vindo ao Prancha" })).not.toBeInTheDocument());
    expect((settings.onboarding as { status: string }).status).toBe("skipped");
    // Depois de pular, a tela inicial continua ensinando a adicionar quadrinhos.
    expect(await screen.findByText("Sua biblioteca está vazia")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Adicionar quadrinhos" }).length).toBeGreaterThan(0);
    expect(screen.getByText(/Formatos aceitos/)).toBeInTheDocument();
  });

  it("não reaparece quando já foi concluído", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: 1 };
    render(<App />);
    expect(await screen.findByText("Sua biblioteca está vazia")).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: /Bem-vindo/ })).not.toBeInTheDocument();
  });

  it("retoma na etapa salva e trata o cancelamento do seletor", async () => {
    settings.onboarding = { status: "active", step: 2, comicId: null };
    overrides.pick_import_paths = () => null;
    const user = userEvent.setup();
    render(<App />);
    const dialog = await screen.findByRole("dialog", { name: "Vamos adicionar sua primeira HQ" });
    await user.click(within(dialog).getByRole("button", { name: "Selecionar arquivo" }));
    expect(await within(dialog).findByText(/Nenhum arquivo foi selecionado/)).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === "start_import")).toBe(false);
    // Continua possível sair do tutorial.
    await user.click(within(dialog).getByRole("button", { name: "Fazer depois" }));
    await waitFor(() => expect((settings.onboarding as { status: string }).status).toBe("skipped"));
  });

  it("reabre o tutorial pelas configurações", async () => {
    settings.onboarding = { status: "skipped", step: 1, comicId: null };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    await user.click(await screen.findByRole("button", { name: "Abrir tutorial" }));
    expect(await screen.findByRole("dialog", { name: "Bem-vindo ao Prancha" })).toBeInTheDocument();
    expect(settings.onboarding).toEqual({ status: "active", step: 1, comicId: null });
  });
});

describe("configurações", () => {
  it("troca o tema na hora e persiste", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    const group = await screen.findByRole("radiogroup", { name: "Tema" });
    await user.click(within(group).getByRole("radio", { name: /Claro/ }));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
    expect(settings["appearance.theme"]).toBe("light");
    expect(localStorage.getItem("prancha.theme")).toBe("light");
    await user.click(within(group).getByRole("radio", { name: /Escuro/ }));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
  });

  it("restaura a preferência salva ao iniciar", async () => {
    settings = { "appearance.theme": "light", onboarding: { status: "completed", step: 4, comicId: null } };
    render(<App />);
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
  });
});

describe("biblioteca", () => {
  it("mostra progresso real e ações separadas", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    comics = [comic];
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Minha biblioteca" }));
    expect(await screen.findAllByText("Parou na página 4 de 12")).not.toHaveLength(0);
    expect(screen.getByRole("button", { name: "Ver detalhes de Teste em Quadrinhos" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continuar leitura: Teste em Quadrinhos" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Mais ações para Teste em Quadrinhos" }));
    const menu = await screen.findByRole("menu");
    expect(within(menu).getByRole("menuitem", { name: /Remover da biblioteca/ })).toBeInTheDocument();
  });

  it("remover pede confirmação e não apaga o original por padrão", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    comics = [comic];
    overrides.remove_comic = () => ({ removed: true, trashed: false });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Minha biblioteca" }));
    await user.click(await screen.findByRole("button", { name: "Mais ações para Teste em Quadrinhos" }));
    await user.click(await screen.findByRole("menuitem", { name: /Remover da biblioteca/ }));
    const dialog = await screen.findByRole("dialog", { name: /Remover “Teste em Quadrinhos”/ });
    expect(within(dialog).getByText(/não será apagado/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("checkbox"));
    expect(within(dialog).getByRole("button", { name: "Remover e mover para a Lixeira" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("checkbox"));
    await user.click(within(dialog).getByRole("button", { name: "Remover da biblioteca" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "remove_comic")?.args).toEqual({ id: 7, deleteOriginal: false }));
  });

  it("pesquisa envia o texto ao banco", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    comics = [comic];
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Minha biblioteca" }));
    await user.type(await screen.findByLabelText(/Pesquisar por título/), "saga");
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === "list_comics" && (c.args.query as { text?: string }).text === "saga")).toBe(true),
    );
  });
});

describe("capas", () => {
  it("a capa substituta é identificada como tal", () => {
    render(<Cover comicId={1} title="Sem Capa" kind="placeholder" rev={1} error="arquivo danificado" />);
    expect(screen.getByRole("img", { name: /Capa substituta de Sem Capa/ })).toBeInTheDocument();
    expect(screen.getByText("Capa indisponível")).toBeInTheDocument();
  });
  it("capa pendente não exibe imagem inventada", () => {
    render(<Cover comicId={1} title="Nova" kind="pending" rev={0} />);
    expect(screen.getByRole("img", { name: /sendo preparada/ })).toBeInTheDocument();
    expect(document.querySelector("img")).toBeNull();
  });
});
