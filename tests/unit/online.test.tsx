// Telas de repositórios, extensões, Explorar, obra e downloads, com o backend simulado.
// O caminho de verdade, com extensão real e servidor local, é coberto por tests/e2e/extensions.spec.ts.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../../src/app/App";
import { chapterFinished, chapterStream, groupByTitle, nextHistory, runLimited, sortedChapters, titleKey, type CatalogItem, type ChapterView, type DownloadView, type RecommendedList, type RepositoryInfo, type SeriesCard, type SourceSummary } from "../../src/domain/online";

const MIHON_MESSAGE = "Este repositório distribui extensões para Mihon e não possui pacotes compatíveis com o Prancha.";
const FINGERPRINT = "6660b9881c97baa88baa6554bd13968143242814febe85825bc8a69b736ff054";

function repository(overrides: Partial<RepositoryInfo> = {}): RepositoryInfo {
  return {
    id: "repo",
    url: "https://exemplo.test/prancha-repo.json",
    name: "Repositório de teste",
    description: null,
    website: null,
    publicKey: "chave",
    fingerprint: FINGERPRINT,
    pendingPublicKey: null,
    pendingFingerprint: null,
    enabled: true,
    status: "ok",
    lastError: null,
    lastSyncAt: 1,
    lastSuccessAt: 1,
    extensionCount: 1,
    addedAt: 1,
    ...overrides,
  };
}

function catalogItem(overrides: Partial<CatalogItem> = {}): CatalogItem {
  return {
    id: "ext",
    name: "Extensão de teste",
    version: "1.0.0",
    description: null,
    authors: ["Alguém"],
    languages: ["pt-BR"],
    icon: null,
    supportedKinds: ["comic"],
    contentRating: "general",
    runtime: "wasm-component",
    size: 1024,
    sources: [{ id: "a", name: "Fonte A", language: "pt-BR", contentRating: "general" }],
    capabilities: { network: true, storage: false, authentication: false, allowedHosts: ["exemplo.test"] },
    repositoryId: "repo",
    repositoryName: "Repositório de teste",
    installedVersion: null,
    installedHere: false,
    updateAvailable: false,
    incompatible: null,
    ...overrides,
  };
}

function source(id: string, name: string): SourceSummary {
  return {
    extensionId: "ext",
    extensionName: "Extensão",
    repositoryName: "Repositório",
    sourceId: id,
    name,
    language: "pt-BR",
    description: null,
    kinds: ["comic"],
    rating: "general",
    direction: "left-to-right",
    supportsLatest: true,
    supportsSearch: true,
  };
}

function card(id: number, title: string, overrides: Partial<SeriesCard> = {}): SeriesCard {
  return {
    id,
    extensionId: "ext",
    sourceId: "a",
    sourceName: "Fonte A",
    remoteId: String(id),
    title,
    hasCover: false,
    summary: null,
    author: null,
    genres: [],
    status: null,
    language: "pt-BR",
    following: false,
    chapterSort: "desc",
    lastCheckedAt: null,
    lastUpdateAt: null,
    favorite: false,
    ...overrides,
  };
}

function chapter(id: number, title: string, overrides: Partial<ChapterView> = {}): ChapterView {
  return { id, remoteId: String(id), title, number: String(id), publishedAt: null, order: id, state: "available", comicId: null, progress: null, download: null, ...overrides };
}

let handlers: Record<string, (args: Record<string, unknown>) => unknown>;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;

function reject(code: string, message: string): never {
  throw { code, message };
}

beforeEach(() => {
  calls = [];
  handlers = {
    get_settings: () => ({ "plugins.online": true, onboarding: { status: "completed", step: 4, comicId: null } }),
    set_setting: () => null,
    get_home: () => ({ total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] }),
    list_comics: () => [],
    list_collections: () => [],
    updates_snapshot: () => ({ currentVersion: "0.2.0", available: null, declinedVersion: null, failedVersion: null, welcome: null, updatedAt: null, lastCheckAt: null, needsAdmin: false }),
    repositories_list: () => [],
    repositories_recommended: () => ({ entries: [], origin: "bundled", fetchedAt: null, stale: false, error: null }),
    extension_catalog: () => [],
    extension_catalog_languages: () => [],
    extensions_list: () => [],
    sources_list: () => [],
    series_following: () => [],
    downloads_list: () => [],
  };
  mockWindows("main");
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    const handler = handlers[cmd];
    return handler ? handler((args ?? {}) as Record<string, unknown>) : null;
  });
});

afterEach(() => {
  cleanup();
  clearMocks();
});

async function open(section: string) {
  render(<App />);
  await userEvent.click(await screen.findByRole("button", { name: section }));
}

describe("Plugins", () => {
  const ONLINE = ["Explorar", "Downloads", "Extensões", "Repositórios"];

  it("sem o plugin o menu não tem nada das fontes, e instalar traz tudo de volta", async () => {
    handlers.get_settings = () => ({ onboarding: { status: "completed", step: 4, comicId: null } });
    await open("Plugins");
    for (const name of ONLINE) expect(screen.queryByRole("button", { name })).toBeNull();
    await userEvent.click(await screen.findByRole("button", { name: "Instalar" }));
    expect(await screen.findByRole("button", { name: "Explorar" })).toBeTruthy();
    expect(calls.find((c) => c.cmd === "set_setting" && c.args.key === "plugins.online")?.args.value).toBe(true);
  });

  it("remover pergunta antes, tira do menu e para o que estava rodando", async () => {
    handlers.online_stop = () => null;
    await open("Plugins");
    await userEvent.click(await screen.findByRole("button", { name: "Remover" }));
    expect(calls.some((c) => c.cmd === "online_stop")).toBe(false);
    await userEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Remover" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "online_stop")).toBe(true));
    for (const name of ONLINE) expect(screen.queryByRole("button", { name })).toBeNull();
    expect(calls.find((c) => c.cmd === "set_setting" && c.args.key === "plugins.online")?.args.value).toBe(false);
  });
});

const RECOMMENDED: RecommendedList = {
  entries: [
    { id: "keiyoushi", name: "Keiyoushi", description: "Extensões para o Mihon.", url: "https://github.com/keiyoushi/extensions/raw/repo/index.pb", note: "Roda pelo Suwayomi.", added: false },
    { id: "demo", name: "Demonstração do Prancha", description: "Obras de teste.", url: "https://exemplo.test/prancha-repo.json", note: null, added: true },
  ],
  origin: "cache",
  fetchedAt: 1,
  stale: false,
  error: null,
};

describe("Repositórios recomendados", () => {
  it("sem repositórios, mostra um só botão de adicionar", async () => {
    await open("Repositórios");
    expect(await screen.findByText("Nenhum repositório")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Adicionar" })).toHaveLength(1);
  });

  it("oferece adicionar só o que ainda não está na lista", async () => {
    handlers.repositories_recommended = () => RECOMMENDED;
    await open("Repositórios");
    const section = await screen.findByRole("region", { name: "Recomendados" });
    const [keiyoushi, demo] = within(section).getAllByRole("listitem");
    expect(within(keiyoushi!).getByText("Roda pelo Suwayomi.")).toBeTruthy();
    expect(within(keiyoushi!).getByRole("button", { name: "Adicionar Keiyoushi" })).toBeTruthy();
    expect(within(demo!).getByText("Adicionado")).toBeTruthy();
    expect(within(demo!).queryByRole("button")).toBeNull();
  });

  it("adicionar um recomendado passa pela revisão da chave", async () => {
    handlers.repositories_recommended = () => RECOMMENDED;
    handlers.repository_preview = () => ({
      url: "https://github.com/keiyoushi/extensions/raw/repo/repo.json",
      id: "mihon-0123456789abcdef",
      name: "Keiyoushi",
      description: null,
      website: null,
      publicKey: "",
      fingerprint: FINGERPRINT,
      extensionCount: 10,
      alreadyAdded: false,
      external: true,
    });
    await open("Repositórios");
    await userEvent.click(await screen.findByRole("button", { name: "Adicionar Keiyoushi" }));
    const dialog = await screen.findByRole("dialog", { name: "Confiar neste repositório?" });
    expect(calls.find((c) => c.cmd === "repository_preview")?.args).toEqual({ address: RECOMMENDED.entries[0]!.url });
    expect(within(dialog).getByText(/sem o isolamento do Prancha/)).toBeTruthy();
    expect(calls.some((c) => c.cmd === "repository_add")).toBe(false);
  });

  it("um recomendado que falha na consulta deixa o endereço para corrigir", async () => {
    handlers.repositories_recommended = () => RECOMMENDED;
    handlers.repository_preview = () => reject("repo_network", "Não foi possível conectar ao repositório.");
    await open("Repositórios");
    await userEvent.click(await screen.findByRole("button", { name: "Adicionar Keiyoushi" }));
    expect(await screen.findByText("Não foi possível conectar ao repositório.")).toBeTruthy();
    expect((screen.getByLabelText("Endereço") as HTMLInputElement).value).toBe(RECOMMENDED.entries[0]!.url);
  });

  it("uma cópia antiga aparece na hora e é trocada pela publicada", async () => {
    handlers.repositories_recommended = ({ refresh }) => (refresh ? { ...RECOMMENDED, origin: "remote" } : { ...RECOMMENDED, entries: [RECOMMENDED.entries[1]!], stale: true });
    await open("Repositórios");
    expect(await screen.findByText("Keiyoushi")).toBeTruthy();
    expect(calls.filter((c) => c.cmd === "repositories_recommended").map((c) => c.args.refresh)).toEqual([false, true]);
  });

  it("sem rede, avisa e mostra a lista salva", async () => {
    handlers.repositories_recommended = () => ({ ...RECOMMENDED, error: "Não foi possível consultar a lista mais recente agora." });
    await open("Repositórios");
    expect(await screen.findByText(/Não foi possível consultar a lista mais recente agora\. Mostrando a lista salva em/)).toBeTruthy();
    expect(screen.getByText("Keiyoushi")).toBeTruthy();
  });
});

describe("Barra lateral", () => {
  it("deixa Downloads com os utilitários, depois das telas de leitura online", async () => {
    render(<App />);
    const nav = await screen.findByRole("navigation", { name: "Navegação principal" });
    const downloads = await within(nav).findByRole("button", { name: "Downloads" });
    const repositories = within(nav).getByRole("button", { name: "Repositórios" });
    const plugins = within(nav).getByRole("button", { name: "Plugins" });
    expect(repositories.compareDocumentPosition(downloads) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(downloads.compareDocumentPosition(plugins) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(downloads.closest("ul")).toBeNull();
  });
});

describe("Repositórios", () => {
  it("recusa um endereço Mihon, com o motivo, sem apagar o que foi digitado", async () => {
    handlers.repository_preview = () => reject("repo_mihon", MIHON_MESSAGE);
    await open("Repositórios");
    await userEvent.click((await screen.findAllByRole("button", { name: "Adicionar" }))[0]!);
    const field = await screen.findByLabelText("Endereço");
    await userEvent.type(field, "https://raw.githubusercontent.com/keiyoushi/extensions/repo/repo.json");
    await userEvent.click(screen.getByRole("button", { name: "Continuar" }));
    expect(await screen.findByText(MIHON_MESSAGE)).toBeTruthy();
    expect((field as HTMLInputElement).value).toContain("keiyoushi");
    expect(calls.some((c) => c.cmd === "repository_add")).toBe(false);
  });

  it("só adiciona depois de mostrar a chave, e confirma a impressão digital revisada", async () => {
    handlers.repository_preview = () => ({
      url: "https://exemplo.test/prancha-repo.json",
      id: "repo",
      name: "Repositório de teste",
      description: null,
      website: "https://exemplo.test",
      publicKey: "CHAVEPUBLICA",
      fingerprint: FINGERPRINT,
      extensionCount: 3,
      alreadyAdded: false,
      external: false,
    });
    handlers.repository_add = () => repository();
    await open("Repositórios");
    await userEvent.click((await screen.findAllByRole("button", { name: "Adicionar" }))[0]!);
    await userEvent.type(await screen.findByLabelText("Endereço"), "https://exemplo.test/prancha-repo.json");
    await userEvent.click(screen.getByRole("button", { name: "Continuar" }));

    const dialog = await screen.findByRole("dialog", { name: "Confiar neste repositório?" });
    expect(within(dialog).getByText("CHAVEPUBLICA")).toBeTruthy();
    expect(within(dialog).getByTestId("fingerprint").textContent).toContain("6660 b988");
    expect(within(dialog).getByText(/código de terceiros/)).toBeTruthy();
    expect(calls.some((c) => c.cmd === "repository_add")).toBe(false);

    await userEvent.click(within(dialog).getByRole("button", { name: "Confiar e adicionar" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "repository_add")?.args.fingerprint).toBe(FINGERPRINT));
  });

  it("avisa que um repositório do Mihon roda sem isolamento antes de confiar", async () => {
    handlers.repository_preview = () => ({
      url: "https://exemplo.test/repo.json",
      id: "mihon-abc",
      name: "Keiyoushi",
      description: null,
      website: null,
      publicKey: FINGERPRINT,
      fingerprint: FINGERPRINT,
      extensionCount: 564,
      alreadyAdded: false,
      external: true,
    });
    await open("Repositórios");
    await userEvent.click((await screen.findAllByRole("button", { name: "Adicionar" }))[0]!);
    await userEvent.type(await screen.findByLabelText("Endereço"), "https://exemplo.test");
    await userEvent.click(screen.getByRole("button", { name: "Continuar" }));

    const dialog = await screen.findByRole("dialog", { name: "Confiar neste repositório?" });
    expect(within(dialog).getByText(/sem o isolamento do Prancha/)).toBeTruthy();
    expect(within(dialog).getByText(/340 MB/)).toBeTruthy();
    expect(within(dialog).queryByText("Chave pública")).toBeNull();
    expect(within(dialog).getByTestId("fingerprint").textContent).toContain("6660 b988");
  });

  it("avisa sobre a troca de chave e não aceita sozinho", async () => {
    handlers.repositories_list = () => [repository({ status: "key_changed", pendingFingerprint: "abcd".repeat(16), pendingPublicKey: "nova" })];
    await open("Repositórios");
    expect(await screen.findByRole("alert")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Revisar" }));
    const dialog = await screen.findByRole("dialog", { name: "A chave deste repositório mudou" });
    expect(within(dialog).getByText(/abcd abcd/)).toBeTruthy();
    await userEvent.click(within(dialog).getByRole("button", { name: "Manter a chave atual" }));
    expect(calls.some((c) => c.cmd === "repository_accept_key")).toBe(false);
  });

  it("explica o efeito antes de remover", async () => {
    handlers.repositories_list = () => [repository()];
    handlers.repository_remove = () => null;
    await open("Repositórios");
    await userEvent.click(await screen.findByRole("button", { name: "Ações de Repositório de teste" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Remover" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/extensões já instaladas continuam funcionando/)).toBeTruthy();
    await userEvent.click(within(dialog).getByRole("button", { name: "Remover" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "repository_remove")?.args.id).toBe("repo"));
  });
});

describe("Extensões", () => {
  it("mostra as permissões antes de instalar", async () => {
    handlers.repositories_list = () => [repository()];
    handlers.extension_catalog = () => [catalogItem()];
    handlers.extension_install = () => null;
    await open("Extensões");
    await userEvent.click(await screen.findByRole("radio", { name: "Disponíveis" }));
    await userEvent.click(await screen.findByRole("button", { name: /Extensão de teste/ }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Acessar: exemplo.test")).toBeTruthy();
    expect(within(dialog).getByText(/código de terceiros/)).toBeTruthy();
    await userEvent.click(within(dialog).getByRole("button", { name: "Instalar" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "extension_install")?.args).toEqual({ repositoryId: "repo", extensionId: "ext" }));
  });

  it("uma extensão do Mihon diz que roda sem isolamento, e não promete o contrário", async () => {
    handlers.repositories_list = () => [repository()];
    handlers.extension_catalog = () => [catalogItem({ runtime: "suwayomi", size: 0, capabilities: { network: true, storage: false, authentication: false, allowedHosts: [] } })];
    await open("Extensões");
    await userEvent.click(await screen.findByRole("radio", { name: "Disponíveis" }));
    await userEvent.click(await screen.findByRole("button", { name: /Extensão de teste/ }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Sem isolamento: acesso aos seus arquivos e à internet")).toBeTruthy();
    expect(within(dialog).getByText(/Esta roda sem isolamento/)).toBeTruthy();
    expect(within(dialog).queryByText(/roda isolada/)).toBeNull();
    expect(within(dialog).queryByText("Tamanho")).toBeNull();
  });

  it("atualizar mostra as duas versões e o acesso novo antes de aplicar", async () => {
    const hosts = { network: true, storage: false, authentication: false };
    handlers.extensions_list = () => [
      {
        id: "ext",
        name: "Extensão de teste",
        version: "1.0.0",
        description: null,
        authors: [],
        icon: null,
        enabled: true,
        repositoryId: "repo",
        repositoryName: "Repositório de teste",
        fingerprint: FINGERPRINT,
        capabilities: { ...hosts, allowedHosts: ["exemplo.test"] },
        sources: [],
        loadError: null,
        availableVersion: "1.1.0",
        installedAt: 1,
        updatedAt: 1,
      },
    ];
    handlers.extension_catalog = () => [
      catalogItem({ version: "1.1.0", installedVersion: "1.0.0", installedHere: true, updateAvailable: true, capabilities: { ...hosts, allowedHosts: ["exemplo.test", "novo.test"] } }),
    ];
    handlers.extension_update = () => null;
    await open("Extensões");
    await userEvent.click(await screen.findByRole("button", { name: "Atualizar para 1.1.0" }));

    const dialog = await screen.findByRole("dialog", { name: "Extensão de teste" });
    expect(within(dialog).getByText("1.0.0 → 1.1.0")).toBeTruthy();
    expect(await within(dialog).findByText(/pede acesso novo: novo\.test\./)).toBeTruthy();
    expect(calls.some((c) => c.cmd === "extension_update")).toBe(false);

    await userEvent.click(within(dialog).getByRole("button", { name: "Atualizar" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "extension_update")?.args).toEqual({ id: "ext" }));
  });

  it("o catálogo carrega aos poucos", async () => {
    handlers.repositories_list = () => [repository()];
    handlers.extension_catalog = (args) => {
      const { limit } = args.query as { limit: number };
      return Array.from({ length: Math.min(limit, 150) }, (_, i) => catalogItem({ id: `ext-${i}`, name: `Extensão ${i}` }));
    };
    await open("Extensões");
    await userEvent.click(await screen.findByRole("radio", { name: "Disponíveis" }));
    await screen.findByRole("button", { name: /Extensão 99/ });
    expect(screen.queryByRole("button", { name: /Extensão 100/ })).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "Carregar mais" }));
    await screen.findByRole("button", { name: /Extensão 149/ });
    expect(screen.queryByRole("button", { name: "Carregar mais" })).toBeNull();
  });

  it("não deixa instalar uma extensão incompatível e diz o motivo", async () => {
    handlers.repositories_list = () => [repository()];
    handlers.extension_catalog = () => [catalogItem({ incompatible: "Exige o Prancha 9.0.0 ou mais novo." })];
    await open("Extensões");
    await userEvent.click(await screen.findByRole("radio", { name: "Disponíveis" }));
    await userEvent.click(await screen.findByRole("button", { name: /Extensão de teste/ }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Exige o Prancha 9.0.0 ou mais novo.")).toBeTruthy();
    expect((within(dialog).getByRole("button", { name: "Instalar" }) as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("Explorar", () => {
  it("sem fontes, aponta para as extensões", async () => {
    await open("Explorar");
    expect(await screen.findByText("Nenhuma fonte instalada")).toBeTruthy();
  });

  async function searchFor(text: string) {
    const box = await screen.findByRole("searchbox", { name: "Pesquisar nas fontes" });
    await userEvent.clear(box);
    await userEvent.type(box, `${text}{Enter}`);
  }

  it("a pesquisa mostra cada obra com as fontes onde está, sem juntar título só parecido", async () => {
    handlers.sources_list = () => [source("a", "Fonte A"), source("b", "Fonte B"), source("c", "Fonte C")];
    handlers.source_popular = () => ({ results: [], hasMore: false });
    handlers.source_search = (args) => {
      if (args.sourceId === "c") reject("source_failed", "A fonte está em manutenção.");
      return { results: args.sourceId === "a" ? [card(1, "Aurora!"), card(2, "Aurora Boreal")] : [card(3, "aurora", { sourceId: "b", sourceName: null })], hasMore: false };
    };
    await open("Explorar");
    await searchFor("aurora");

    const whereToRead = await screen.findByRole("list", { name: "Onde ler Aurora!" });
    expect(within(whereToRead).getByRole("button", { name: "Abrir Aurora! em Fonte A" })).toBeTruthy();
    expect(await within(whereToRead).findByRole("button", { name: "Abrir aurora em Fonte B" })).toBeTruthy();
    expect(screen.getByText(/Mesmo título em 2 fontes/)).toBeTruthy();
    expect(within(screen.getByRole("list", { name: "Onde ler Aurora Boreal" })).getAllByRole("listitem")).toHaveLength(1);

    // Quem falhou não disse que a obra não está lá.
    expect(await screen.findByText("Fonte C não respondeu.")).toBeTruthy();
    expect(screen.getByText(/A obra pode estar nela/)).toBeTruthy();
    expect(await screen.findByText("2 de 3 fontes responderam")).toBeTruthy();
    expect(calls.filter((c) => c.cmd === "source_search").map((c) => c.args.query)).toEqual(["aurora", "aurora", "aurora"]);
    await waitFor(() => expect(calls.find((c) => c.cmd === "set_setting" && c.args.key === "online.searchHistory")?.args.value).toEqual(["aurora"]));
  });

  it("tentar de novo consulta só a fonte que falhou e mantém o resto na tela", async () => {
    let down = true;
    handlers.sources_list = () => [source("a", "Fonte A"), source("b", "Fonte B")];
    handlers.source_popular = () => ({ results: [], hasMore: false });
    handlers.source_search = (args) => {
      if (args.sourceId === "b" && down) reject("source_failed", "A fonte está em manutenção.");
      return { results: [card(args.sourceId === "a" ? 1 : 2, "Lume", { sourceId: String(args.sourceId) })], hasMore: false };
    };
    await open("Explorar");
    await searchFor("lume");
    await userEvent.click(await screen.findByText("Fonte B não respondeu."));
    expect(screen.getByRole("button", { name: "Abrir Lume em Fonte A" })).toBeTruthy();

    const before = calls.filter((c) => c.cmd === "source_search" && c.args.sourceId === "a").length;
    down = false;
    await userEvent.click(screen.getByRole("button", { name: "Tentar de novo" }));
    expect(await screen.findByRole("button", { name: "Abrir Lume em Fonte B" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Abrir Lume em Fonte A" })).toBeTruthy();
    expect(calls.filter((c) => c.cmd === "source_search" && c.args.sourceId === "a")).toHaveLength(before);
    expect(screen.queryByText(/não respondeu/)).toBeNull();
    expect(await screen.findByText("2 fontes consultadas")).toBeTruthy();
  });

  it("sem resultado, não afirma que a obra não existe enquanto faltar fonte", async () => {
    handlers.sources_list = () => [source("a", "Fonte A"), source("b", "Fonte B")];
    handlers.source_popular = () => ({ results: [], hasMore: false });
    handlers.source_search = (args) => (args.sourceId === "a" ? reject("source_failed", "Fora do ar.") : { results: [], hasMore: false });
    await open("Explorar");
    await searchFor("vazio");
    expect(await screen.findByText(/As outras podem ter/)).toBeTruthy();
  });

  it("diz quando quem escondeu as obras foi o filtro +18, em vez de dizer que não há nada", async () => {
    handlers.sources_list = () => [source("a", "Fonte A"), source("b", "Fonte B")];
    handlers.source_popular = (args) => ({ results: [], hasMore: false, hidden: args.sourceId === "a" ? 23 : 0, unverified: 0 });
    handlers.source_search = (args) => ({ results: [], hasMore: false, hidden: args.sourceId === "a" ? 2 : 0, unverified: 0 });
    await open("Explorar");
    // A tela guarda a pesquisa do teste anterior.
    await searchFor("");
    const hiding = await screen.findByRole("region", { name: "Fonte A" });
    expect(await within(hiding).findByText(/O filtro \+18 escondeu 23 obras, adultas ou sem classificação\. Para ver, desligue o filtro em Plugins\./)).toBeTruthy();
    expect(within(hiding).queryByText("Nada encontrado.")).toBeNull();
    expect(await within(screen.getByRole("region", { name: "Fonte B" })).findByText("Nada encontrado.")).toBeTruthy();

    await searchFor("escondida");
    expect(await screen.findByText(/O filtro \+18 escondeu obras adultas ou sem classificação de: Fonte A\./)).toBeTruthy();
    expect(screen.queryByText(/Nenhuma fonte encontrou esse nome/)).toBeNull();
  });

  it("mostra o que passou e oferece conferir de novo só a fonte que ficou sem resposta", async () => {
    handlers.sources_list = () => [source("a", "Fonte A"), source("b", "Fonte B")];
    const asked: string[] = [];
    let answered = false;
    handlers.source_popular = (args) => {
      asked.push(String(args.sourceId));
      if (args.sourceId === "b") return { results: [card(9, "Da outra")], hasMore: false, hidden: 0, unverified: 0 };
      return answered ? { results: [card(1, "Conferida"), card(2, "Depois")], hasMore: false, hidden: 0, unverified: 0 } : { results: [card(1, "Conferida")], hasMore: false, hidden: 0, unverified: 1 };
    };
    handlers.source_search = handlers.source_popular;
    await open("Explorar");
    // A tela guarda a pesquisa do teste anterior.
    await searchFor("");
    const section = await screen.findByRole("region", { name: "Fonte A" });
    expect(await within(section).findByRole("button", { name: /Conferida/ })).toBeTruthy();
    expect(within(section).getByText(/Não deu para conferir a classificação de 1 obra agora\./)).toBeTruthy();
    await screen.findByRole("button", { name: /Da outra/ });
    expect(screen.getAllByRole("button", { name: "Tentar de novo" })).toHaveLength(1);

    answered = true;
    asked.length = 0;
    await userEvent.click(within(section).getByRole("button", { name: "Tentar de novo" }));
    expect(await within(section).findByRole("button", { name: /Depois/ })).toBeTruthy();
    expect(within(section).queryByText(/Não deu para conferir/)).toBeNull();
    expect(asked).toEqual(["a"]);
  });

  it("com uma fonte só, a pesquisa continua na grade daquela fonte", async () => {
    handlers.sources_list = () => [source("a", "Fonte A")];
    handlers.source_popular = () => ({ results: [], hasMore: false });
    handlers.source_search = () => ({ results: [card(1, "Sozinha")], hasMore: false });
    await open("Explorar");
    await searchFor("sozinha");
    expect(await within(await screen.findByRole("region", { name: "Fonte A" })).findByRole("button", { name: /Sozinha/ })).toBeTruthy();
  });

  it("depois de o site liberar e recusar, o aviso é um só e dá o caminho", async () => {
    const protectedError = "Este site pede uma verificação anti-robô, que só você pode passar.";
    handlers.sources_list = () => [source("a", "Fonte A"), source("b", "Fonte B")];
    handlers.source_popular = (args) => (args.sourceId === "a" ? reject("source_protected", protectedError) : { results: [card(1, "Lista da B", { sourceId: "b" })], hasMore: false });
    handlers.source_search = () => ({ results: [], hasMore: false });
    handlers.source_verify = () => true;
    await open("Explorar");
    await userEvent.click(await screen.findByRole("button", { name: "Limpar pesquisa" }).catch(() => screen.findByRole("radio", { name: /Populares/ })));

    const blocked = await screen.findByRole("region", { name: "Fonte A" });
    expect(await within(blocked).findByText(protectedError)).toBeTruthy();
    await userEvent.click(within(blocked).getByRole("button", { name: "Verificar no site" }));

    expect(await within(blocked).findByText(/mesmo depois da verificação/)).toBeTruthy();
    await waitFor(() => expect(calls.filter((c) => c.cmd === "source_popular" && c.args.sourceId === "a").length).toBeGreaterThan(1));
    expect(await within(blocked).findByText(/mesmo depois da verificação/)).toBeTruthy();
    expect(within(blocked).queryByText(protectedError)).toBeNull();
    expect(within(blocked).queryByRole("button", { name: "Verificar no site" })).toBeNull();

    await userEvent.click(within(blocked).getByRole("button", { name: "Procurar em outras fontes" }));
    expect(document.activeElement).toBe(screen.getByRole("searchbox", { name: "Pesquisar nas fontes" }));
  });

  it("carrega mais resultados sem repetir", async () => {
    handlers.sources_list = () => [source("a", "Fonte A")];
    handlers.source_popular = (args) => (args.page === 1 ? { results: [card(1, "Um"), card(2, "Dois")], hasMore: true } : { results: [card(2, "Dois"), card(3, "Três")], hasMore: false });
    handlers.source_search = () => ({ results: [], hasMore: false });
    await open("Explorar");
    await userEvent.click(await screen.findByRole("button", { name: "Limpar pesquisa" }).catch(() => screen.findByRole("radio", { name: /Populares/ })));
    await userEvent.click(await screen.findByRole("button", { name: "Carregar mais" }));
    expect(await screen.findByRole("button", { name: /Três/ })).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /Dois/ })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Carregar mais" })).toBeNull();
  });
});

describe("Obra e capítulos", () => {
  const view = (chapters: ChapterView[], overrides = {}) => ({
    ...card(1, "Obra"),
    chapters,
    readStatus: "unread",
    chapterCount: chapters.length,
    chaptersRead: 0,
    resume: chapters[0] ? { chapterId: chapters[0].id, title: chapters[0].title, progress: null } : null,
    collections: [],
    sourceError: null,
    sourceProtected: false,
    ...overrides,
  });

  async function openSeries() {
    handlers.sources_list = () => [source("a", "Fonte A")];
    handlers.source_popular = () => ({ results: [card(1, "Obra")], hasMore: false });
    handlers.source_search = () => ({ results: [card(1, "Obra")], hasMore: false });
    await open("Explorar");
    await userEvent.click(await screen.findByRole("button", { name: /Obra/ }));
  }

  it("distingue os estados do capítulo e omite o que a fonte não informou", async () => {
    const chapters = [chapter(1, "Disponível"), chapter(2, "Baixado", { state: "downloaded" }), chapter(3, "Offline", { state: "offline", comicId: 9, progress: { pageIndex: 2, pageCount: 5 } })];
    handlers.series_stored = () => view(chapters);
    handlers.series_open = () => view(chapters);
    await openSeries();
    const rows = await screen.findAllByTestId("chapter");
    expect(rows).toHaveLength(3);
    const text = rows.map((row) => row.textContent ?? "").join("|");
    expect(text).toContain("Disponível online");
    expect(text).toContain("Baixado");
    expect(text).toContain("Na biblioteca");
    expect(text).toContain("Página 3 de 5");
    expect(screen.queryByText("Autoria")).toBeNull();
    expect(screen.queryByText("Gêneros")).toBeNull();
  });

  it("com a fonte fora do ar, mostra o último conhecido e deixa ler o que é local", async () => {
    const chapters = [chapter(1, "Local", { state: "offline", comicId: 9 })];
    handlers.series_stored = () => view(chapters);
    handlers.series_open = () => view(chapters, { sourceError: "Não foi possível falar com a fonte." });
    await openSeries();
    expect(await screen.findByText("A fonte não respondeu")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Ler" })).toBeTruthy();
  });

  it("todo capítulo tem Ler, sem precisar baixar, e a obra continua de onde parou", async () => {
    const chapters = [chapter(1, "Um", { order: 0, progress: { pageIndex: 4, pageCount: 5 } }), chapter(2, "Dois", { order: 1 }), chapter(3, "Três", { order: 2 })];
    const reading = { readStatus: "reading", chaptersRead: 1, resume: { chapterId: 2, title: "Dois", progress: null } };
    handlers.series_stored = () => view(chapters, reading);
    handlers.series_open = () => view(chapters, reading);
    handlers.chapter_open = () => reject("source_error", "Fora do ar.");
    await openSeries();
    const rows = await screen.findAllByTestId("chapter");
    for (const row of rows) expect(within(row).getByRole("button", { name: "Ler" })).toBeTruthy();
    expect(rows.map((row) => row.textContent).join("|")).toContain("Lido");
    await userEvent.click(screen.getByRole("button", { name: "Continuar" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "chapter_open")?.args.id).toBe(2));
    expect(calls.some((c) => c.cmd === "downloads_enqueue")).toBe(false);
    expect(await screen.findByText("Fora do ar.")).toBeTruthy();
  });

  it("baixa um capítulo, só os selecionados ou todos", async () => {
    const chapters = [chapter(1, "Um"), chapter(2, "Dois"), chapter(3, "Três")];
    handlers.series_stored = () => view(chapters);
    handlers.series_open = () => view(chapters);
    handlers.downloads_enqueue = () => [];
    await openSeries();
    await screen.findAllByTestId("chapter");
    await userEvent.click(screen.getByRole("button", { name: "Baixar todos" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "downloads_enqueue").at(-1)?.args.chapterIds).toEqual([3, 2, 1]));
    await userEvent.click(screen.getByLabelText("Selecionar Dois"));
    await userEvent.click(screen.getByRole("button", { name: "Baixar 1" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "downloads_enqueue").at(-1)?.args.chapterIds).toEqual([2]));
  });

  it("uma obra +18 fica de fora enquanto o filtro está ligado", async () => {
    const refused = "Esta obra é +18. Para ver, desligue o filtro +18 em Plugins.";
    handlers.series_stored = () => view([]);
    handlers.series_open = () => reject("source_rating", refused);
    await openSeries();
    expect(await screen.findByText(refused)).toBeTruthy();
    expect(screen.queryByTestId("chapter")).toBeNull();
  });

  it("favoritar e guardar em pasta valem para a obra, como para uma HQ", async () => {
    const chapters = [chapter(1, "Um")];
    handlers.series_stored = () => view(chapters);
    handlers.series_open = () => view(chapters);
    handlers.series_set_favorite = () => null;
    handlers.list_collections = () => [{ id: 7, name: "Lendo agora", comicCount: 0, preview: [], createdAt: 1, updatedAt: 1 }];
    handlers.series_set_collections = () => null;
    await openSeries();
    await userEvent.click(await screen.findByRole("button", { name: "Favoritar" }));
    expect(calls.find((c) => c.cmd === "series_set_favorite")?.args).toEqual({ id: 1, favorite: true });
    expect(await screen.findByRole("button", { name: "Favorito" })).toBeTruthy();

    await userEvent.click(within(screen.getByRole("main")).getByRole("button", { name: "Coleções" }));
    await userEvent.click(await screen.findByRole("checkbox", { name: /Lendo agora/ }));
    await userEvent.click(screen.getByRole("button", { name: "Salvar coleções" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "series_set_collections")?.args).toEqual({ id: 1, collectionIds: [7] }));
  });

  it("se o site liberar e recusar mesmo assim, para de oferecer a verificação e leva às outras fontes", async () => {
    const chapters = [chapter(1, "Um")];
    handlers.series_stored = () => view(chapters);
    handlers.series_open = () => view(chapters, { sourceError: "Este site pede uma verificação anti-robô, que só você pode passar.", sourceProtected: true });
    handlers.source_verify = () => true;
    await openSeries();
    await userEvent.click(await screen.findByRole("button", { name: "Verificar no site" }));
    expect(await screen.findByText(/mesmo depois da verificação/)).toBeTruthy();
    expect(screen.queryByText(/que só você pode passar/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Verificar no site" })).toBeNull();

    const before = calls.filter((c) => c.cmd === "source_search").length;
    await userEvent.click(within(screen.getByRole("alert")).getByRole("button", { name: "Procurar em outras fontes" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "source_search").length).toBeGreaterThan(before));
    expect(calls.filter((c) => c.cmd === "source_search").at(-1)?.args.query).toBe("Obra");
  });

  it("um site com verificação oferece abrir o site, e tenta de novo quando a janela fecha", async () => {
    const chapters = [chapter(1, "Um")];
    let protectedNow = true;
    handlers.series_stored = () => view(chapters);
    handlers.series_open = () => (protectedNow ? view(chapters, { sourceError: "Este site pede uma verificação anti-robô, que só você pode passar.", sourceProtected: true }) : view(chapters));
    handlers.source_verify = () => {
      protectedNow = false;
      return null;
    };
    await openSeries();
    await userEvent.click(await screen.findByRole("button", { name: "Verificar no site" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "source_verify")?.args).toEqual({ seriesId: 1 }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Verificar no site" })).toBeNull());
  });
});

describe("Downloads", () => {
  function download(overrides: Partial<DownloadView> = {}): DownloadView {
    return { id: 1, chapterId: 1, seriesId: 1, seriesTitle: "Obra", chapterTitle: "Capítulo 1", sourceName: "Fonte A", status: "running", pagesDone: 2, pagesTotal: 5, bytes: 2048, error: null, comicId: null, createdAt: 1, finishedAt: null, ...overrides };
  }

  it("mostra obra, capítulo, fonte, progresso e estado, com as ações de cada estado", async () => {
    handlers.downloads_list = () => [download(), download({ id: 2, chapterTitle: "Capítulo 2", status: "paused" }), download({ id: 3, chapterTitle: "Capítulo 3", status: "failed", error: "Página 2: o servidor respondeu 500." }), download({ id: 4, chapterTitle: "Capítulo 4", status: "completed", comicId: 7 })];
    handlers.download_pause = () => null;
    handlers.download_resume = () => null;
    handlers.download_retry = () => null;
    await open("Downloads");
    const rows = await screen.findAllByTestId("download");
    expect(rows[0]!.textContent).toContain("Obra");
    expect(rows[0]!.textContent).toContain("Fonte A");
    expect(rows[0]!.textContent).toContain("2 de 5 páginas");
    expect(within(rows[0]!).getByRole("progressbar").getAttribute("aria-valuenow")).toBe("40");
    expect(rows[2]!.textContent).toContain("o servidor respondeu 500");
    expect(within(rows[3]!).getByRole("button", { name: "Ler" })).toBeTruthy();

    await userEvent.click(screen.getByRole("button", { name: "Pausar Capítulo 1" }));
    await userEvent.click(screen.getByRole("button", { name: "Retomar Capítulo 2" }));
    await userEvent.click(screen.getByRole("button", { name: "Tentar de novo" }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === "download_pause")?.args.id).toBe(1);
      expect(calls.find((c) => c.cmd === "download_resume")?.args.id).toBe(2);
      expect(calls.find((c) => c.cmd === "download_retry")?.args.id).toBe(3);
    });
  });
});

describe("regras puras", () => {
  it("histórico: mais nova primeiro, sem repetir e com limite", () => {
    expect(nextHistory(["Aurora"], "  aurora ")).toEqual(["aurora"]);
    expect(nextHistory(["a"], "   ")).toEqual(["a"]);
    expect(nextHistory(Array.from({ length: 8 }, (_, i) => `q${i}`), "nova")).toHaveLength(8);
  });

  it("títulos: junta só os iguais, sem contar caixa, acento e pontuação", () => {
    expect(titleKey("Ação: Total!")).toBe(titleKey("acao  total"));
    expect(titleKey("Aurora")).not.toBe(titleKey("Aurora Boreal"));
    expect(titleKey("Aurora (Novel)")).not.toBe(titleKey("Aurora"));
    expect(titleKey("がく")).not.toBe(titleKey("かく"));
    expect(titleKey("!!!")).toBe("!!!");
  });

  it("títulos: o pesquisado vem primeiro, e a ordem não muda com mais uma fonte", () => {
    const ids = (cards: SeriesCard[], query: string) => groupByTitle(cards, query).map((group) => group.entries.map((entry) => entry.id));
    const some = [card(1, "A Volta de Lume"), card(2, "Lume Dourado"), card(3, "Lume")];
    expect(ids(some, "lume")).toEqual([[3], [2], [1]]);
    expect(ids([...some, card(4, "LUME!"), card(5, "A volta de lume")], "lume")).toEqual([[3, 4], [2], [1, 5]]);
    expect(groupByTitle(some, "lume")[0]?.title).toBe("Lume");
  });

  it("ordena os capítulos pela posição informada pela fonte", () => {
    const chapters = [chapter(2, "b", { order: 1 }), chapter(1, "a", { order: 0 }), chapter(3, "c", { order: 2 })];
    expect(sortedChapters(chapters, "asc").map((c) => c.title)).toEqual(["a", "b", "c"]);
    expect(sortedChapters(chapters, "desc").map((c) => c.title)).toEqual(["c", "b", "a"]);
  });

  it("capítulos em sequência começam onde o anterior termina", () => {
    const session = (id: number, pageCount: number, comicId: number | null = null) => ({ id, seriesId: 1, seriesTitle: "Obra", title: `Cap ${id}`, pageCount, start: { pageIndex: 0, pageCount, scrollRatio: 0, updatedAt: 0 }, previousId: null, nextId: null, comicId, genres: [], readerMode: null });
    expect(chapterStream([session(7, 6), session(8, 5, 3), session(9, 2)])).toEqual([
      { id: 7, title: "Cap 7", first: 0, count: 6, guidedComicId: null },
      { id: 8, title: "Cap 8", first: 6, count: 5, guidedComicId: 3 },
      { id: 9, title: "Cap 9", first: 11, count: 2, guidedComicId: null },
    ]);
  });

  it("um capítulo só termina na última página", () => {
    const read = (pageIndex: number) => ({ progress: { pageIndex, pageCount: 5 } });
    expect(chapterFinished(chapter(1, "a", read(4)))).toBe(true);
    expect(chapterFinished(chapter(1, "a", read(3)))).toBe(false);
    expect(chapterFinished(chapter(1, "a"))).toBe(false);
  });

  it("limita as tarefas simultâneas e continua depois de uma falha", async () => {
    let running = 0;
    let peak = 0;
    let finished = 0;
    const task = (fail: boolean) => async () => {
      running++;
      peak = Math.max(peak, running);
      await new Promise((resolve) => setTimeout(resolve, 1));
      running--;
      if (fail) throw new Error("falha");
      finished++;
    };
    await runLimited([task(true), task(false), task(false), task(false), task(false)], 2);
    expect(peak).toBe(2);
    expect(finished).toBe(4);
  });
});
