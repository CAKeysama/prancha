// Testes de interface com o backend simulado por `mockIPC` (API oficial de testes do Tauri).
// O backend real é coberto pelos testes Rust e pelos testes E2E no aplicativo compilado.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../../src/app/App";
import { Cover } from "../../src/components/Cover";
import { SETTINGS_CATEGORIES, SETTINGS_INDEX } from "../../src/domain/settings-search";
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
          return { version: "0.1.0", tauriVersion: "2", schemaVersion: 1 };
        case "storage_info":
          return { dataDir: "C:/dados", databaseBytes: 1024, coversBytes: 2048, pagesBytes: 0 };
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
  const wizard = () => screen.getByRole("dialog", { name: /Boas-vindas|Aparência|Fonte da interface|Sons|Leitura|Tudo pronto/ });

  it("mostra a configuração inicial e permite pular", async () => {
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByRole("dialog", { name: "Boas-vindas ao Prancha" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Pular configuração" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Boas-vindas ao Prancha" })).not.toBeInTheDocument());
    expect((settings.onboarding as { status: string }).status).toBe("skipped");
    // Depois de pular, a tela inicial continua ensinando a adicionar quadrinhos.
    expect(await screen.findByText("Sua biblioteca está vazia")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Adicionar quadrinhos" }).length).toBeGreaterThan(0);
    expect(screen.getByText(/CBZ, CBR, CB7, CBT, PDF/)).toBeInTheDocument();
  });

  it("não reaparece para quem concluiu o tutorial antigo", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: 1 };
    render(<App />);
    expect(await screen.findByText("Sua biblioteca está vazia")).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: /Boas-vindas/ })).not.toBeInTheDocument();
  });

  it("percorre as etapas aplicando cada escolha na hora e salvando o progresso", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Começar" }));

    expect(wizard()).toHaveAccessibleName("Aparência");
    await user.click(within(screen.getByRole("radiogroup", { name: "Tema" })).getByRole("radio", { name: /Claro/ }));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
    expect(settings.onboarding).toMatchObject({ status: "active", step: 1 });
    await user.click(screen.getByRole("button", { name: "Continuar" }));

    expect(wizard()).toHaveAccessibleName("Fonte da interface");
    await user.click(screen.getByRole("radio", { name: /Atkinson Hyperlegible/ }));
    await waitFor(() => expect(document.documentElement.dataset.font).toBe("atkinson"));
    expect(settings["appearance.font"]).toBe("atkinson");
    expect(localStorage.getItem("prancha.font")).toBe("atkinson");
    await user.click(screen.getByRole("button", { name: "Continuar" }));

    // Quem nunca escolheu encontra os sons ligados, com o controle à vista.
    expect(wizard()).toHaveAccessibleName("Sons");
    await waitFor(() => expect(screen.getByRole("switch", { name: "Sons da interface" })).toHaveAttribute("aria-checked", "true"));
    expect(settings["sound.enabled"]).toBe(true);
    await user.click(screen.getByRole("button", { name: "Continuar" }));

    expect(wizard()).toHaveAccessibleName("Leitura");
    expect(screen.getByRole("radiogroup", { name: "Direção de leitura" })).toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: /Webtoon/ }));
    await waitFor(() => expect(settings["reader.mode"]).toBe("webtoon"));
    // Na rolagem contínua, a direção não muda nada e sai da tela.
    expect(screen.queryByRole("radiogroup", { name: "Direção de leitura" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continuar" }));

    expect(wizard()).toHaveAccessibleName("Tudo pronto");
    expect(settings.onboarding).toMatchObject({ status: "active", step: 5 });
    await user.click(screen.getByRole("button", { name: "Entrar no Prancha" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Tudo pronto" })).not.toBeInTheDocument());
    expect(settings.onboarding).toEqual({ status: "completed", step: 5, readerTip: true, flow: 2 });
  });

  it("retoma na etapa salva, volta e não muda os sons de quem já escolheu", async () => {
    settings.onboarding = { status: "active", step: 4, readerTip: false, flow: 2 };
    settings["sound.enabled"] = false;
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByRole("dialog", { name: "Leitura" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Voltar" }));
    expect(await screen.findByRole("dialog", { name: "Sons" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Sons da interface" })).toHaveAttribute("aria-checked", "false");
    expect(settings["sound.enabled"]).toBe(false);
    expect(settings.onboarding).toMatchObject({ step: 3 });
  });

  it("adiciona HQs pela última etapa e mostra o resultado", async () => {
    settings.onboarding = { status: "active", step: 5, readerTip: false, flow: 2 };
    overrides.pick_import_paths = () => ["C:/HQs/um.cbz", "C:/HQs/dois.cbz"];
    overrides.start_import = () => 41;
    const user = userEvent.setup();
    render(<App />);
    const dialog = await screen.findByRole("dialog", { name: "Tudo pronto" });
    await user.click(within(dialog).getByRole("button", { name: "Escolher arquivos" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "start_import")).toBe(true));
    const counts = { total: 2, processed: 2, added: 1, skipped: 0, failed: 1 };
    await act(() => emit("import:progress", { jobId: 41, phase: "processing", counts, current: null, item: { path: "C:/HQs/um.cbz", name: "um.cbz", status: "added", message: null, comicId: 3 } }));
    await act(() => emit("import:progress", { jobId: 41, phase: "processing", counts, current: null, item: { path: "C:/HQs/dois.cbz", name: "dois.cbz", status: "failed", message: "arquivo corrompido", comicId: null } }));
    await act(() => emit("import:progress", { jobId: 41, phase: "done", counts, current: null, item: null }));
    expect(await within(dialog).findByText("1 HQ adicionada à biblioteca. 1 não pôde ser adicionado.")).toBeInTheDocument();
  });

  it("reabre a configuração inicial pelas configurações sem desfazer as escolhas", async () => {
    settings.onboarding = { status: "skipped", step: 0, readerTip: false, flow: 2 };
    settings["appearance.theme"] = "light";
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    await user.click(await screen.findByRole("button", { name: "Geral" }));
    await user.click(await screen.findByRole("button", { name: "Refazer" }));
    expect(await screen.findByRole("dialog", { name: "Boas-vindas ao Prancha" })).toBeInTheDocument();
    expect(settings.onboarding).toEqual({ status: "active", step: 0, readerTip: false, flow: 2 });
    expect(settings["appearance.theme"]).toBe("light");
  });

  it("escreve a sugestão no app, valida e só avisa sucesso quando o navegador abre", async () => {
    let fail = true;
    overrides.open_improvement_form = () => {
      if (fail) throw { code: "opener", message: "sem navegador" };
      return null;
    };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    await user.click(await screen.findByRole("button", { name: "Geral" }));
    await user.click(await screen.findByRole("button", { name: "Escrever sugestão" }));
    let dialog = await screen.findByRole("dialog", { name: "Sugerir uma melhoria" });

    // Vazio não sai do aplicativo.
    await user.click(within(dialog).getByRole("button", { name: "Abrir no GitHub" }));
    expect(within(dialog).getByText("Escreva um título.")).toBeInTheDocument();
    expect(within(dialog).getByText("Descreva a sua ideia.")).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Título")).toHaveFocus();
    expect(calls.some((c) => c.cmd === "open_improvement_form")).toBe(false);

    await user.type(within(dialog).getByLabelText("Título"), "Marcar páginas");
    await user.type(within(dialog).getByLabelText("Descrição"), "Guardar a página favorita & voltar a ela.");
    await user.click(within(dialog).getByRole("button", { name: "Abrir no GitHub" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("sem navegador");
    expect(screen.queryByText("Formulário aberto no navegador")).not.toBeInTheDocument();

    // O texto continua depois do erro e depois de fechar e abrir de novo.
    await user.click(within(dialog).getByRole("button", { name: "Cancelar" }));
    await user.click(screen.getByRole("button", { name: "Escrever sugestão" }));
    dialog = await screen.findByRole("dialog", { name: "Sugerir uma melhoria" });
    expect(within(dialog).getByLabelText("Título")).toHaveValue("Marcar páginas");
    expect(within(dialog).getByLabelText("Descrição")).toHaveValue("Guardar a página favorita & voltar a ela.");

    fail = false;
    await user.click(within(dialog).getByRole("button", { name: "Abrir no GitHub" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Sugerir uma melhoria" })).not.toBeInTheDocument());
    expect(calls.filter((c) => c.cmd === "open_improvement_form").at(-1)?.args).toEqual({ title: "Marcar páginas", text: "Guardar a página favorita & voltar a ela." });
    expect(await screen.findByText("Formulário aberto no navegador")).toBeInTheDocument();
  });

  it("abre a visão geral do projeto no navegador, sem consultar o GitHub pelo app", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    await user.click(await screen.findByRole("button", { name: "Geral" }));
    await user.click(await screen.findByRole("button", { name: "Ver visão geral" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "open_project_overview")).toBe(true));
    expect(calls.filter((c) => c.cmd === "open_project_overview").at(-1)?.args ?? {}).toEqual({});
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

  it("troca a fonte da interface na hora, sem mexer nas páginas", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    const group = await screen.findByRole("radiogroup", { name: "Fonte da interface" });
    expect(within(group).getByRole("radio", { name: /Hanken Grotesk/ })).toHaveAttribute("aria-checked", "true");
    await user.click(within(group).getByRole("radio", { name: /IBM Plex Sans/ }));
    await waitFor(() => expect(document.documentElement.dataset.font).toBe("plex"));
    expect(settings["appearance.font"]).toBe("plex");
    expect(localStorage.getItem("prancha.font")).toBe("plex");
  });

  it("sons: começam desligados para quem já usava o app e voltam ao padrão", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    await user.click(within(await screen.findByRole("navigation", { name: "Categorias das configurações" })).getByRole("button", { name: "Sons" }));
    const toggle = screen.getByRole("switch", { name: "Sons da interface" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("slider", { name: "Volume" })).toBeDisabled();
    // Ouvir um exemplo funciona mesmo desligado e não muda a preferência.
    await user.click(screen.getByRole("button", { name: "Notificação" }));
    expect(settings["sound.enabled"]).toBeUndefined();
    await user.click(screen.getByRole("button", { name: "Restaurar padrão" }));
    await waitFor(() => expect(settings["sound.enabled"]).toBe(true));
    expect(settings["sound.volume"]).toBe(60);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("button", { name: "Restaurar padrão" })).not.toBeInTheDocument();
  });

  it("mostra só as opções da categoria escolhida", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    const nav = await screen.findByRole("navigation", { name: "Categorias das configurações" });
    expect(await screen.findByRole("radiogroup", { name: "Tema" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refazer" })).not.toBeInTheDocument();
    expect(within(nav).getByRole("button", { name: "Aparência" })).toHaveAttribute("aria-current", "true");

    await user.click(within(nav).getByRole("button", { name: "Geral" }));
    expect(screen.getByRole("button", { name: "Refazer" })).toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "Tema" })).not.toBeInTheDocument();
    expect(within(nav).getByRole("button", { name: "Geral" })).toHaveAttribute("aria-current", "true");
    expect(within(nav).getByRole("button", { name: "Aparência" })).not.toHaveAttribute("aria-current");
    // Uma seção por vez: nenhum título de categoria repetido.
    expect(screen.getAllByRole("heading", { level: 2 })).toHaveLength(1);
  });

  it("toda opção do índice da pesquisa existe na sua categoria", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    const nav = await screen.findByRole("navigation", { name: "Categorias das configurações" });
    for (const category of SETTINGS_CATEGORIES) {
      await user.click(within(nav).getByRole("button", { name: category.label }));
      for (const entry of SETTINGS_INDEX.filter((e) => e.place === category.id)) {
        expect(document.getElementById(`setting-${entry.id}`), `${category.id}/${entry.id}`).not.toBeNull();
      }
    }
  });

  it("pesquisa pela finalidade, mostra a categoria e leva direto à opção", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    const search = await screen.findByRole("searchbox", { name: "Pesquisar nas configurações" });
    const result = (name: RegExp) => within(screen.getByRole("region", { name: "Resultados da pesquisa" })).getByRole("button", { name });

    for (const [query, title, place] of [
      ["Tema", /^Tema/, "Aparência"],
      ["idioma", /^Idioma/, "Geral"],
      ["Atualização", /^Procurar nova versão/, "Atualizações"],
      ["LEITURA", /^Direção de leitura/, "Leitura"],
    ] as const) {
      await user.clear(search);
      await user.type(search, query);
      expect(result(title)).toHaveTextContent(place);
    }
    expect(screen.queryByRole("radiogroup", { name: "Tema" })).not.toBeInTheDocument();

    await user.click(result(/^Direção de leitura/));
    expect(search).toHaveValue("");
    expect(screen.getByRole("radiogroup", { name: "Direção de leitura" })).toBeInTheDocument();
    expect(document.getElementById("setting-direcao")).toHaveClass("bg-accent-subtle");
    expect(within(screen.getByRole("navigation", { name: "Categorias das configurações" })).getByRole("button", { name: "Leitura" })).toHaveAttribute("aria-current", "true");
  });

  it("pesquisa sem resultado explica e oferece limpar", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    const search = await screen.findByRole("searchbox", { name: "Pesquisar nas configurações" });
    await user.type(search, "xyzzy");
    expect(screen.getByText("Nenhum resultado para “xyzzy”")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Nenhuma opção encontrada");
    await user.click(screen.getByRole("button", { name: "Limpar pesquisa" }));
    expect(search).toHaveValue("");
    expect(screen.getByRole("radiogroup", { name: "Tema" })).toBeInTheDocument();
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
    overrides.remove_comic = () => ({ trashed: false });
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

  it("pesquisa sem resultado diz isso e oferece limpar", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    comics = [comic];
    overrides.list_comics = (args) => ((args.query as { text?: string }).text ? [] : comics);
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Minha biblioteca" }));
    expect(await screen.findByText("1 HQ")).toBeInTheDocument();
    await user.type(screen.getByLabelText(/Pesquisar por título/), "zzz");
    const empty = (await screen.findByRole("heading", { name: "Nenhum resultado encontrado" })).parentElement!;
    expect(screen.getByText("0 resultados")).toBeInTheDocument();
    await user.click(within(empty).getByRole("button", { name: "Limpar pesquisa" }));
    expect(screen.getByLabelText(/Pesquisar por título/)).toHaveValue("");
    expect(await screen.findByRole("button", { name: "Ver detalhes de Teste em Quadrinhos" })).toBeInTheDocument();
  });

  it("biblioteca vazia mostra as ações de adicionar uma vez só", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Minha biblioteca" }));
    await screen.findByRole("heading", { name: "Minha biblioteca", level: 1 });
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Adicionar pasta" })).toHaveLength(1));
    expect(screen.getAllByRole("button", { name: "Adicionar quadrinhos" })).toHaveLength(1);
  });

  it("detalhes: favorito com botão de estado, menu sem ação repetida e reinício com foco em Cancelar", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    comics = [comic];
    overrides.get_comic = () => ({ ...comic, path: "C:/HQs/teste.cbz", fileSize: 2048, year: null, artist: null, publisher: null, summaryText: null, coverPage: null, tags: [], collections: [], fieldSources: {} });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Minha biblioteca" }));
    await user.click(await screen.findByRole("button", { name: "Ver detalhes de Teste em Quadrinhos" }));
    expect(await screen.findByRole("heading", { name: "Teste em Quadrinhos", level: 1 })).toBeInTheDocument();
    const favorite = screen.getByRole("button", { name: "Favoritar" });
    expect(favorite).toHaveAttribute("aria-pressed", "false");
    await user.click(favorite);
    await waitFor(() => expect(calls.find((c) => c.cmd === "set_favorite")?.args).toEqual({ id: 7, favorite: true }));
    // "Mostrar na pasta" já está ao lado da localização: não se repete no menu.
    expect(screen.getByRole("button", { name: "Mostrar na pasta" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Mais ações" }));
    const menu = await screen.findByRole("menu");
    expect(within(menu).queryByRole("menuitem", { name: "Mostrar na pasta" })).not.toBeInTheDocument();
    await user.click(within(menu).getByRole("menuitem", { name: "Reiniciar progresso" }));
    const dialog = await screen.findByRole("dialog", { name: "Reiniciar o progresso?" });
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "Cancelar" })).toHaveFocus());
  });

  it("coleção vazia: uma ação de adicionar, e renomear e excluir no menu", async () => {
    settings.onboarding = { status: "completed", step: 4, comicId: null };
    overrides.list_collections = () => [{ id: 3, name: "Lendo agora", comicCount: 0, preview: [], createdAt: 1, updatedAt: 1 }];
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Coleções" }));
    await user.click(await screen.findByRole("button", { name: "Abrir coleção Lendo agora" }));
    expect(await screen.findByRole("heading", { name: "Esta coleção está vazia" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Adicionar à coleção" })).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "Mais ações" }));
    const menu = await screen.findByRole("menu");
    expect(within(menu).getByRole("menuitem", { name: "Renomear…" })).toBeInTheDocument();
    await user.click(within(menu).getByRole("menuitem", { name: "Excluir coleção…" }));
    const dialog = await screen.findByRole("dialog", { name: "Excluir a coleção “Lendo agora”?" });
    expect(within(dialog).getByText(/Nada sai da biblioteca/)).toBeInTheDocument();
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "Cancelar" })).toHaveFocus());
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
