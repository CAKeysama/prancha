// Os 16 passos da definição de pronto no executável real, com a extensão de demonstração de verdade:
// repositório assinado por uma chave descartável, servido por um servidor local que o próprio teste controla.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { extname, join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { ROOT, freshDataDir, launchApp, type AppSession } from "./harness";

const REPO_NAME = "Demonstração do Prancha";
const EXTENSION = "Acervo de demonstração";
const SHOTS = join(ROOT, "tests", ".tmp", "extension-shots");
const OUT = join(ROOT, "tests", ".tmp", "demo-repo");
const KEY = join(ROOT, "tests", ".tmp", "demo-repo.key");

const CONTENT_TYPES: Record<string, string> = { ".json": "application/json", ".png": "image/png", ".wasm": "application/wasm", ".sig": "text/plain" };

const nav = (page: Page, name: string) => page.getByRole("navigation", { name: "Navegação principal" }).getByRole("button", { name, exact: true }).click();
const chapterRow = (page: Page, title: string) => page.getByTestId("chapter").filter({ hasText: title });
const downloadRow = (page: Page, title: string) => page.getByTestId("download").filter({ hasText: title });
const indicator = (page: Page) => page.locator("header p[aria-live='polite']").first();

async function imageLoaded(page: Page, selector: string) {
  await expect.poll(() => page.locator(selector).first().evaluate((img: HTMLImageElement) => img.naturalWidth).catch(() => 0)).toBeGreaterThan(0);
}

async function fitsTheWindow(page: Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
}

/** Abre uma obra a partir de Explorar, pesquisando pelo título. */
async function openSeries(page: Page, query: string, title: string) {
  await nav(page, "Explorar");
  const search = page.getByRole("searchbox", { name: "Pesquisar nas fontes" });
  await search.fill(query);
  await search.press("Enter");
  await page.getByRole("region", { name: EXTENSION }).getByRole("button", { name: new RegExp(title) }).click();
  await expect(page.getByRole("heading", { name: title, level: 1 })).toBeVisible();
}

test.describe.serial("extensões: da adição do repositório à leitura offline", () => {
  let app: AppSession;
  let server: Server;
  let base = "";
  let dir = "";
  let delay = 0;
  let failing: string | null = null;
  const seen: string[] = [];

  const stopServer = () =>
    new Promise<void>((resolve) => {
      server.close(() => resolve());
      server.closeAllConnections();
    });

  test.beforeAll(async () => {
    test.setTimeout(600_000);
    server = createServer((req, res) => {
      const path = decodeURIComponent(new URL(req.url ?? "/", "http://local").pathname);
      seen.push(path);
      if (path === "/mihon/repo.json") {
        // O formato real publicado pela Keiyoushi, sem nenhum dado dela.
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ index_v2: `${base}/mihon/index.pb`, meta: { name: "Keiyoushi", website: "https://example.test", signingKeyFingerprint: "abc" } }));
        return;
      }
      if (failing && path.includes(failing)) {
        res.writeHead(500);
        res.end();
        return;
      }
      const file = join(OUT, path);
      if (!file.startsWith(OUT) || !existsSync(file) || !statSync(file).isFile()) {
        res.writeHead(404);
        res.end();
        return;
      }
      setTimeout(
        () => {
          res.writeHead(200, { "content-type": CONTENT_TYPES[extname(file)] ?? "application/octet-stream" });
          res.end(readFileSync(file));
        },
        extname(file) === ".png" ? delay : 0,
      );
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address();
    if (!address || typeof address === "string") throw new Error("Servidor local sem porta");
    base = `http://127.0.0.1:${address.port}`;

    rmSync(OUT, { recursive: true, force: true });
    rmSync(KEY, { force: true });
    execFileSync(process.execPath, ["scripts/build-demo-repo.mjs", "--out", OUT, "--base-url", base, "--key", KEY], { cwd: ROOT, stdio: "inherit" });

    // Só em build de depuração: o endereço local continua bloqueado no app publicado.
    process.env.PRANCHA_E2E_ALLOW_LOOPBACK = "1";
    dir = freshDataDir("extensions");
    app = await launchApp(dir);
    await app.page.getByRole("button", { name: "Pular tutorial" }).click();
  });

  test.afterAll(async () => {
    delete process.env.PRANCHA_E2E_ALLOW_LOOPBACK;
    await app?.close();
    if (server?.listening) await stopServer();
  });

  test("1 a 5: abre, recusa Mihon, revisa a chave, confia e sincroniza", async () => {
    const page = app.page;
    await nav(page, "Repositórios");
    await expect(page.getByText("Nenhum repositório")).toBeVisible();
    await page.getByRole("button", { name: "Adicionar", exact: true }).first().click();

    const adding = page.getByRole("dialog", { name: "Adicionar repositório" });
    const field = adding.getByLabel("Endereço");
    await field.fill(`${base}/mihon/repo.json`);
    await adding.getByRole("button", { name: "Continuar" }).click();
    await expect(adding.getByText(/extensões para Mihon/)).toBeVisible();
    await expect(field).toHaveValue(`${base}/mihon/repo.json`);

    await field.fill(`${base}/prancha-repo.json`);
    await adding.getByRole("button", { name: "Continuar" }).click();
    const trust = page.getByRole("dialog", { name: "Confiar neste repositório?" });
    await expect(trust.getByText(REPO_NAME)).toBeVisible();
    await expect(trust.getByText(/código de terceiros/)).toBeVisible();
    await page.screenshot({ path: join(SHOTS, "repositorio-confianca.png") });

    // A impressão digital na tela é a da chave que o servidor publicou, recalculada aqui.
    const published = JSON.parse(readFileSync(join(OUT, "prancha-repo.json"), "utf8")) as { publicKey: string };
    const expected = createHash("sha256").update(Buffer.from(published.publicKey, "base64")).digest("hex");
    expect((await trust.getByTestId("fingerprint").innerText()).replace(/\s/g, "")).toBe(expected);
    // A revisão lê o índice para contar as extensões, mas nada fica gravado antes da confirmação.
    const saved = () => page.evaluate(() => (window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string) => Promise<unknown[]> } }).__TAURI_INTERNALS__.invoke("repositories_list"));
    expect(await saved()).toEqual([]);

    await trust.getByRole("button", { name: "Confiar e adicionar" }).click();
    await expect(page.getByText("1 extensão", { exact: false })).toBeVisible();
    expect(await saved()).toHaveLength(1);
    expect(seen).toContain("/prancha-index.json");
    expect(seen).toContain("/prancha-index.json.sig");

    const before = seen.filter((path) => path === "/prancha-index.json.sig").length;
    await page.getByRole("button", { name: `Ações de ${REPO_NAME}` }).click();
    await page.getByRole("menuitem", { name: "Atualizar" }).click();
    await expect.poll(() => seen.filter((path) => path === "/prancha-index.json.sig").length).toBeGreaterThan(before);
  });

  test("6 a 8: consulta o catálogo, vê as permissões, instala e vê as fontes", async () => {
    const page = app.page;
    await nav(page, "Extensões");
    await expect(page.getByText("Nenhuma extensão instalada")).toBeVisible();
    await page.getByRole("radio", { name: "Disponíveis" }).click();

    await page.getByRole("button", { name: new RegExp(EXTENSION) }).click();
    const details = page.getByRole("dialog", { name: EXTENSION });
    await expect(details.getByText("Acessar: 127.0.0.1")).toBeVisible();
    await expect(details.getByText(/código de terceiros/)).toBeVisible();
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(SHOTS, "extensao-detalhes.png") });
    await details.getByRole("button", { name: "Instalar" }).click();
    await expect(details).toBeHidden();
    expect(seen.some((path) => path.endsWith(".wasm"))).toBe(true);
    expect(seen.some((path) => path.endsWith(".wasm.sig"))).toBe(true);

    await page.getByRole("radio", { name: "Instaladas" }).click();
    await expect(page.getByText(EXTENSION).first()).toBeVisible();
    await page.getByRole("button", { name: `Ações de ${EXTENSION}` }).click();
    await page.getByRole("menuitem", { name: "Detalhes e permissões" }).click();
    const installed = page.getByRole("dialog", { name: EXTENSION });
    await expect(installed.getByText(`${EXTENSION} · Português (Brasil)`)).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(installed).toBeHidden();
  });

  test("9 a 12: navega, pagina, pesquisa e abre os detalhes e os capítulos", async () => {
    const page = app.page;
    await nav(page, "Explorar");
    const section = page.getByRole("region", { name: EXTENSION });
    await expect(section.getByRole("button", { name: /Farol de Papel/ })).toBeVisible();
    await expect(section.getByRole("listitem")).toHaveCount(4);
    await imageLoaded(page, 'img[alt="Capa de Farol de Papel"]');
    await section.getByRole("button", { name: "Carregar mais" }).click();
    await expect(section.getByRole("listitem")).toHaveCount(6);
    await expect(section.getByRole("button", { name: "Carregar mais" })).toBeHidden();

    await page.getByRole("radio", { name: "Recentes" }).click();
    await expect(section.getByRole("listitem").first()).toContainText("Jardim de Engrenagens");

    const search = page.getByRole("searchbox", { name: "Pesquisar nas fontes" });
    await search.fill("nada parecido");
    await search.press("Enter");
    await expect(section.getByText("Nada encontrado.")).toBeVisible();

    // Sem acento na pesquisa e com acento no título.
    await search.fill("mares");
    await search.press("Enter");
    await expect(section.getByRole("button", { name: /O Mapa das Marés/ })).toBeVisible();
    await page.screenshot({ path: join(SHOTS, "explorar-pesquisa.png") });

    await openSeries(page, "farol", "Farol de Papel");
    await expect(page.getByText(/farol feito de cartas/)).toBeVisible();
    await expect(page.getByText("Projeto Prancha")).toBeVisible();
    await expect(page.getByText("Aventura, Fantasia")).toBeVisible();
    await expect(page.getByTestId("chapter")).toHaveCount(3);
    await expect(chapterRow(page, "A primeira carta")).toContainText("Disponível online");
    await expect(page.getByTestId("chapter").first()).toContainText("O remetente");
    await page.getByRole("button", { name: "Do último ao primeiro" }).click();
    await expect(page.getByTestId("chapter").first()).toContainText("A primeira carta");
    await page.getByRole("button", { name: "Acompanhar", exact: true }).click();
    await expect(page.getByRole("button", { name: "Acompanhando", exact: true })).toBeVisible();
    await page.screenshot({ path: join(SHOTS, "obra.png") });
  });

  test("13 a 15: baixa um capítulo, encontra na biblioteca e lê no leitor de sempre", async () => {
    const page = app.page;
    await chapterRow(page, "A primeira carta").getByRole("button", { name: "Baixar", exact: true }).click();
    const read = chapterRow(page, "A primeira carta").getByRole("button", { name: "Ler" });
    await expect(read).toBeVisible();
    await expect(chapterRow(page, "A primeira carta")).toContainText("Na biblioteca");

    await nav(page, "Minha biblioteca");
    await page.getByRole("button", { name: "Ver detalhes de A primeira carta" }).click();
    await expect(page.getByText("Farol de Papel").first()).toBeVisible();
    await expect(page.getByText(`· ${EXTENSION}`)).toBeVisible();
    await page.getByRole("button", { name: /^(Ler|Continuar leitura|Ler novamente)$/ }).click();

    await expect(indicator(page)).toHaveText("Página 1 de 6");
    await imageLoaded(page, 'img[alt="Página 1"]');
    await page.keyboard.press("ArrowRight");
    await expect(indicator(page)).toHaveText("Página 2 de 6");
    await page.keyboard.press("ArrowRight");
    await expect(indicator(page)).toHaveText("Página 3 de 6");
    await page.screenshot({ path: join(SHOTS, "leitor.png") });
    await page.getByRole("button", { name: "Biblioteca" }).click();

    const files = readdirSync(join(dir, "downloads"), { recursive: true }).map(String);
    expect(files.filter((name) => name.endsWith(".cbz"))).toHaveLength(1);
    expect(files.filter((name) => name.endsWith(".cbz"))[0]).toContain("Farol de Papel - A primeira carta");
  });

  test("downloads: progresso, pausa, retomada, falha e leitura local durante o download", async () => {
    const page = app.page;
    await openSeries(page, "farol", "Farol de Papel");
    delay = 500;
    await chapterRow(page, "Maré de envelopes").getByRole("button", { name: "Baixar", exact: true }).click();
    await nav(page, "Downloads");
    const row = downloadRow(page, "Maré de envelopes");
    await expect(row).toContainText("Farol de Papel");
    await expect(row).toContainText(EXTENSION);
    await expect(row).toContainText(/de 5 páginas/);
    await expect(row.getByRole("progressbar")).toBeVisible();
    await page.screenshot({ path: join(SHOTS, "downloads.png") });

    await row.getByRole("button", { name: "Pausar Maré de envelopes" }).click();
    await expect(row).toContainText("Pausado");

    // Fonte lenta e download em andamento não atrapalham a leitura do que é local.
    await row.getByRole("button", { name: "Retomar Maré de envelopes" }).click();
    await nav(page, "Minha biblioteca");
    const started = Date.now();
    await page.getByRole("button", { name: "Ver detalhes de A primeira carta" }).click();
    await page.getByRole("button", { name: /^(Ler|Continuar leitura|Ler novamente)$/ }).click();
    await expect(indicator(page)).toHaveText("Página 3 de 6");
    await page.keyboard.press("ArrowRight");
    await expect(indicator(page)).toHaveText("Página 4 de 6");
    await page.keyboard.press("ArrowLeft");
    await expect(indicator(page)).toHaveText("Página 3 de 6");
    expect(Date.now() - started).toBeLessThan(10_000);
    await page.getByRole("button", { name: "Biblioteca" }).click();

    delay = 0;
    await nav(page, "Downloads");
    await expect(row).toContainText("Concluído");
    await expect(row.getByRole("button", { name: "Ler" })).toBeVisible();

    // Uma página com erro falha o capítulo, com o motivo; depois de corrigida, repetir conclui.
    await openSeries(page, "farol", "Farol de Papel");
    failing = "/cap-03/03.png";
    await chapterRow(page, "O remetente").getByRole("button", { name: "Baixar", exact: true }).click();
    await nav(page, "Downloads");
    const failed = downloadRow(page, "O remetente");
    await expect(failed).toContainText("Falhou");
    await expect(failed).toContainText("500");
    failing = null;
    await failed.getByRole("button", { name: "Tentar de novo" }).click();
    await expect(failed).toContainText("Concluído");
  });

  test("telas nos dois temas e na janela pequena", async () => {
    const page = app.page;
    const screens: Array<[string, () => Promise<void>]> = [
      ["repositorios", () => nav(page, "Repositórios")],
      ["extensoes", () => nav(page, "Extensões")],
      ["explorar", () => nav(page, "Explorar")],
      ["obra", () => openSeries(page, "farol", "Farol de Papel")],
      ["downloads", () => nav(page, "Downloads")],
    ];
    for (const [width, height] of [[1280, 800], [760, 560]] as const) {
      await page.setViewportSize({ width, height });
      for (const [name, open] of screens) {
        await open();
        await fitsTheWindow(page);
        for (const theme of ["light", "dark"]) {
          await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
          // As cores têm transição: antes de ela terminar, a captura mostra os dois temas misturados.
          await page.waitForTimeout(500);
          await page.screenshot({ path: join(SHOTS, `${name}-${theme}-${width}.png`) });
        }
      }
    }
    await page.setViewportSize({ width: 1280, height: 800 });
  });

  test("16: sem servidor e depois de reabrir, a leitura continua de onde parou", async () => {
    await stopServer();
    await app.close();
    app = await launchApp(dir);
    const page = app.page;

    await nav(page, "Minha biblioteca");
    await page.getByRole("button", { name: "Ver detalhes de A primeira carta" }).click();
    await page.getByRole("button", { name: /^(Ler|Continuar leitura|Ler novamente)$/ }).click();
    await expect(indicator(page)).toHaveText("Página 3 de 6");
    await imageLoaded(page, 'img[alt="Página 3"]');
    await page.keyboard.press("ArrowRight");
    await expect(indicator(page)).toHaveText("Página 4 de 6");
    await page.getByRole("button", { name: "Biblioteca" }).click();

    // A fonte fora do ar aparece como erro dela, e a obra continua aberta com o último conhecido.
    await nav(page, "Explorar");
    const section = page.getByRole("region", { name: EXTENSION });
    await expect(section.getByRole("alert")).toBeVisible();
    await page.getByRole("radio", { name: "Acompanhando" }).click();
    await page.getByRole("button", { name: /Farol de Papel/ }).click();
    await expect(page.getByText("A fonte não respondeu")).toBeVisible();
    await expect(page.getByTestId("chapter")).toHaveCount(3);
    await chapterRow(page, "A primeira carta").getByRole("button", { name: "Ler" }).click();
    await expect(indicator(page)).toHaveText("Página 4 de 6");
    await page.getByRole("button", { name: "Biblioteca" }).click();

    // O catálogo já baixado continua na tela e o erro fica no repositório.
    await nav(page, "Repositórios");
    await page.getByRole("button", { name: `Ações de ${REPO_NAME}` }).click();
    await page.getByRole("menuitem", { name: "Atualizar" }).click();
    await expect(page.getByText(/1 extensão/)).toBeVisible();
  });

  test("remover a extensão e o repositório não apaga nada da biblioteca", async () => {
    const page = app.page;
    await nav(page, "Extensões");
    await page.getByRole("button", { name: `Ações de ${EXTENSION}` }).click();
    await page.getByRole("menuitem", { name: "Remover" }).click();
    await page.getByRole("dialog", { name: `Remover ${EXTENSION}?` }).getByRole("button", { name: "Remover" }).click();
    await expect(page.getByText("Nenhuma extensão instalada")).toBeVisible();

    await nav(page, "Repositórios");
    await page.getByRole("button", { name: `Ações de ${REPO_NAME}` }).click();
    await page.getByRole("menuitem", { name: "Remover" }).click();
    await page.getByRole("dialog", { name: `Remover ${REPO_NAME}?` }).getByRole("button", { name: "Remover" }).click();
    await expect(page.getByText("Nenhum repositório")).toBeVisible();

    await nav(page, "Explorar");
    await expect(page.getByText("Nenhuma fonte instalada")).toBeVisible();

    await nav(page, "Minha biblioteca");
    await page.getByRole("button", { name: "Ver detalhes de A primeira carta" }).click();
    await expect(page.getByText("· Fonte não instalada")).toBeVisible();
    await page.getByRole("button", { name: /^(Ler|Continuar leitura|Ler novamente)$/ }).click();
    await expect(indicator(page)).toHaveText("Página 4 de 6");
    await imageLoaded(page, 'img[alt="Página 4"]');

    const files = readdirSync(join(dir, "downloads"), { recursive: true }).map(String);
    expect(files.filter((name) => name.endsWith(".cbz"))).toHaveLength(3);
  });
});
