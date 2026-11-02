// Testes E2E no executável real (backend Rust + WebView2). Apenas a escolha de caminho do
// diálogo nativo é substituída (PRANCHA_E2E_PICK_FILE, só em builds de depuração); a importação,
// o banco, as capas e a leitura são reais.
import { existsSync, readFileSync, renameSync, rmSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { FIXTURES, ROOT, copyDir, freshDataDir, launchApp, shot, type AppSession } from "./harness";

function copyFixtures(dataDir: string): { lib: string; errors: string } {
  const dest = join(dataDir, "..", `${dataDir.split(/[\\/]/).pop()}-arquivos`);
  rmSync(dest, { recursive: true, force: true });
  copyDir(FIXTURES, dest);
  return { lib: join(dest, "biblioteca"), errors: join(dest, "erros") };
}

async function waitPageLoaded(page: Page, index = 1) {
  const img = page.locator(`img[alt="Página ${index}"]`).first();
  const canvas = page.locator(`canvas[aria-label="Página ${index}"]`).first();
  await expect(img.or(canvas)).toBeVisible();
  if (await img.count()) {
    await expect.poll(() => img.evaluate((el: HTMLImageElement) => el.complete && el.naturalWidth > 0)).toBe(true);
  } else {
    await expect(page.getByText(`Desenhando página ${index}…`)).toHaveCount(0);
  }
}

async function indicator(page: Page) {
  return page.locator("header p[aria-live='polite']").first();
}

async function goLibrary(page: Page) {
  await page.getByRole("navigation", { name: "Navegação principal" }).getByRole("button", { name: "Minha biblioteca" }).click();
  await expect(page.getByRole("heading", { name: "Minha biblioteca", level: 1 })).toBeVisible();
}

async function openReader(page: Page, title: string) {
  await goLibrary(page);
  await page.getByRole("button", { name: `Ver detalhes de ${title}` }).click();
  await page.getByRole("button", { name: /^(Ler|Continuar leitura|Ler novamente)$/ }).click();
  await expect(page.getByRole("button", { name: "Biblioteca" })).toBeVisible();
}

test.describe.serial("primeira execução e tutorial", () => {
  let app: AppSession;
  let files: { lib: string; errors: string };
  const dir = freshDataDir("tutorial");

  test.beforeAll(async () => {
    files = copyFixtures(dir);
    app = await launchApp(dir);
  });
  test.afterAll(async () => app?.close());

  test("biblioteca vazia e etapa 1", async () => {
    const page = app.page;
    await expect(page.getByRole("dialog", { name: "Bem-vindo ao Prancha" })).toBeVisible();
    await shot(page, "01-primeira-abertura-tutorial");
    await page.getByRole("button", { name: "Começar" }).click();
    await expect(page.getByRole("dialog", { name: "Vamos adicionar sua primeira HQ" })).toBeVisible();
    await shot(page, "02-tutorial-adicionar-hq");
  });

  test("cancelamento e arquivo inválido mantêm o tutorial compreensível", async () => {
    const page = app.page;
    const dialog = page.getByRole("dialog", { name: "Vamos adicionar sua primeira HQ" });
    app.nextPick(null);
    await dialog.getByRole("button", { name: "Selecionar arquivo" }).click();
    await expect(dialog.getByText(/Nenhum arquivo foi selecionado/)).toBeVisible();
    app.nextPick([join(files.errors, "arquivo-corrompido.cbz")]);
    await dialog.getByRole("button", { name: "Selecionar arquivo" }).click();
    await expect(dialog.getByRole("alert")).toContainText("Não foi possível adicionar");
    await shot(page, "03-tutorial-arquivo-invalido");
  });

  test("importação real avança para a etapa 3", async () => {
    const page = app.page;
    const dialog = page.getByRole("dialog", { name: "Vamos adicionar sua primeira HQ" });
    app.nextPick([join(files.lib, "O Farol de Papel 01.cbz")]);
    await dialog.getByRole("button", { name: "Selecionar arquivo" }).click();
    const step3 = page.getByRole("dialog", { name: "Sua HQ está na biblioteca" });
    await expect(step3).toBeVisible();
    await expect(step3.getByText("O Farol de Papel", { exact: false }).first()).toBeVisible();
    // O cartão de exemplo é decorativo (aria-hidden); confere a imagem real da capa.
    const cover = step3.locator('img[alt="Capa de O Farol de Papel"]');
    await expect(cover).toBeVisible();
    await expect.poll(() => cover.evaluate((el: HTMLImageElement) => el.naturalWidth)).toBeGreaterThan(0);
    await shot(page, "04-tutorial-hq-na-biblioteca");
  });

  test("etapa 4 no leitor e conclusão", async () => {
    const page = app.page;
    await page.getByRole("button", { name: "Abrir HQ" }).click();
    const tip = page.getByRole("dialog", { name: "Fazendo a primeira leitura" });
    await expect(tip).toBeVisible();
    await waitPageLoaded(page);
    await shot(page, "05-tutorial-primeira-leitura");
    await tip.getByRole("button", { name: "Entendi" }).click();
    await expect(tip).toHaveCount(0);
    await page.locator("[data-reader-viewport]").click({ position: { x: 5, y: 5 } });
    for (let i = 0; i < 3; i++) await page.keyboard.press("ArrowRight");
    await expect(await indicator(page)).toHaveText("Página 4 de 14");
  });

  test("progresso e conclusão do tutorial sobrevivem ao reinício", async () => {
    await app.close(true);
    app = await launchApp(dir);
    const page = app.page;
    await expect(page.getByRole("heading", { name: "Início", level: 1 })).toBeVisible();
    await expect(page.getByRole("dialog", { name: /Bem-vindo/ })).toHaveCount(0);
    await expect(page.getByText("Parou na página 4 de 14").first()).toBeVisible();
    await page.getByRole("button", { name: "Continuar leitura" }).first().click();
    await expect(await indicator(page)).toHaveText("Página 4 de 14");
    await waitPageLoaded(page, 4);
  });

  test("fechamento inesperado não corrompe o banco", async () => {
    const page = app.page;
    await page.locator("[data-reader-viewport]").click({ position: { x: 5, y: 5 } });
    await page.keyboard.press("ArrowRight");
    await expect(await indicator(page)).toHaveText("Página 5 de 14");
    await page.waitForTimeout(1500); // gravação com debounce
    await app.close(false); // encerra o processo à força
    app = await launchApp(dir);
    await expect(app.page.getByText("Parou na página 5 de 14").first()).toBeVisible();
  });
});

test.describe.serial("biblioteca, formatos e leitor", () => {
  let app: AppSession;
  let files: { lib: string; errors: string };
  const dir = freshDataDir("biblioteca");

  test.beforeAll(async () => {
    files = copyFixtures(dir);
    app = await launchApp(dir);
    await app.page.getByRole("button", { name: "Pular tutorial" }).click();
  });
  test.afterAll(async () => app?.close());

  test("importa pastas com todos os formatos e relata falhas", async () => {
    const page = app.page;
    app.nextPick([files.lib, files.errors]);
    await page.getByRole("button", { name: "Selecionar uma pasta" }).click();
    const panel = page.getByRole("region", { name: "Importação de quadrinhos" });
    await expect(panel.getByText("Importação concluída")).toBeVisible({ timeout: 60_000 });
    await expect(panel).toContainText("7 adicionados");
    await expect(panel).toContainText("3 com falhas");
    await panel.getByRole("button", { name: "Ver detalhes" }).click();
    const dialog = page.getByRole("dialog", { name: "Resultado da importação" });
    await expect(dialog).toContainText("arquivo-corrompido.cbz");
    await expect(dialog).toContainText("Formato não suportado");
    await expect(dialog).toContainText("o arquivo está vazio");
    await expect(dialog).toContainText("livro-digital.epub");
    await expect(dialog).toContainText("Adicionado com capa substituta");
    await shot(page, "08-importacao-resultado-com-erros");
    await dialog.getByRole("button", { name: "Fechar", exact: true }).first().click();
    await panel.getByRole("button", { name: "Fechar resumo da importação" }).click();
  });

  test("toda HQ tem capa real ou substituta identificada", async () => {
    const page = app.page;
    await goLibrary(page);
    // A capa do PDF é renderizada pelo PDF.js em segundo plano.
    await expect(page.getByText("Preparando capa…")).toHaveCount(0, { timeout: 30_000 });
    for (const t of ["O Farol de Papel", "Rio Vermelho 02", "Cidade Nublada", "Tiras de Domingo", "Caderno de Esboços", "Manual Ilustrado"]) {
      const img = page.getByRole("img", { name: `Capa de ${t}` });
      await expect(img).toBeVisible();
      await expect.poll(() => img.evaluate((el: HTMLImageElement) => el.naturalWidth)).toBeGreaterThan(0);
    }
    await expect(page.getByRole("img", { name: /Capa substituta de Sem Capa Legível/ })).toBeVisible();
  });

  test("abre e navega em cada formato", async () => {
    const page = app.page;
    const cases: Array<[string, number]> = [
      ["Rio Vermelho 02", 10],
      ["Cidade Nublada", 8],
      ["Tiras de Domingo", 6],
      ["Caderno de Esboços", 10],
      ["Manual Ilustrado", 5],
    ];
    for (const [title, count] of cases) {
      await openReader(page, title);
      await expect(await indicator(page)).toHaveText(`Página 1 de ${count}`);
      await waitPageLoaded(page, 1);
      await page.getByRole("button", { name: "Próxima página" }).last().click();
      await expect(await indicator(page)).toHaveText(`Página 2 de ${count}`);
      await waitPageLoaded(page, 2);
      if (title === "Manual Ilustrado") await shot(page, "09c-leitor-pdf");
      await page.getByRole("button", { name: "Página anterior" }).last().click();
      await expect(await indicator(page)).toHaveText(`Página 1 de ${count}`);
      await page.getByRole("button", { name: "Biblioteca" }).click();
    }
  });

  test("modos, zoom, ajuste, direção e progresso", async () => {
    const page = app.page;
    await openReader(page, "O Farol de Papel");
    await waitPageLoaded(page);
    await shot(page, "09-leitor-pagina-unica");
    await page.getByRole("button", { name: "Aumentar zoom" }).click();
    await expect(page.getByRole("button", { name: /Zoom atual 110%/ })).toBeVisible();
    await page.keyboard.press("0");
    await expect(page.getByRole("button", { name: /Zoom atual 100%/ })).toBeVisible();
    await page.getByRole("button", { name: "Ajustar à largura" }).click();
    await expect(page.getByRole("button", { name: "Ajustar à largura" })).toHaveAttribute("aria-pressed", "true");
    await page.getByRole("button", { name: "Ajustar à página" }).click();
    // Página dupla (capa sozinha): 1 | 2–3 | 4–5 ...
    await page.keyboard.press("m");
    await page.keyboard.press("ArrowRight");
    await expect(await indicator(page)).toHaveText("Páginas 2–3 de 14");
    await waitPageLoaded(page, 2);
    await shot(page, "09a-leitor-pagina-dupla");
    // Direita para a esquerda: a seta para a esquerda avança.
    await page.keyboard.press("d");
    await page.keyboard.press("ArrowLeft");
    await expect(await indicator(page)).toHaveText("Páginas 4–5 de 14");
    await page.keyboard.press("ArrowRight");
    await expect(await indicator(page)).toHaveText("Páginas 2–3 de 14");
    await page.keyboard.press("d");
    // Rolagem vertical
    await page.keyboard.press("m");
    await expect(page.getByRole("button", { name: /atual: Rolagem vertical/ })).toBeVisible();
    await waitPageLoaded(page, 2);
    await shot(page, "09b-leitor-rolagem-vertical");
    await page.keyboard.press("m"); // volta para página única
    await page.keyboard.press("End");
    await expect(await indicator(page)).toHaveText("Página 14 de 14");
    await page.keyboard.press("ArrowRight");
    await expect(page.getByText(/Você chegou ao fim/)).toBeVisible();
    // Atalhos documentados no próprio leitor
    await page.keyboard.press("?");
    await expect(page.getByRole("dialog", { name: "Atalhos do leitor" })).toBeVisible();
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Biblioteca" }).first().click();
    // Mais uma HQ em andamento para a captura da biblioteca.
    await openReader(page, "Rio Vermelho 02");
    for (let i = 0; i < 3; i++) await page.keyboard.press("ArrowRight");
    await page.getByRole("button", { name: "Biblioteca" }).click();
  });

  test("índice de páginas respeita a rolagem do usuário e o painel usa menus", async () => {
    const page = app.page;
    await openReader(page, "O Farol de Papel");
    await page.getByRole("button", { name: "Configurações de leitura" }).click();
    const panel = page.getByRole("complementary", { name: "Configurações de leitura" });
    await panel.getByLabel("Modo de página").selectOption("vertical");
    await expect(panel.getByLabel("Direção de leitura")).toHaveValue("ltr");
    await shot(page, "09d-leitor-configuracoes");
    await panel.getByRole("button", { name: "Fechar configurações de leitura" }).click();
    await page.keyboard.press("i");
    const index = page.getByRole("complementary", { name: "Índice de páginas" });
    const scroller = index.locator("div.overflow-y-auto");
    await scroller.evaluate((el) => (el.scrollTop = el.scrollHeight));
    const before = await scroller.evaluate((el) => el.scrollTop);
    expect(before).toBeGreaterThan(0);
    // Mover o mouse redesenha o leitor (controles); antes isso puxava a rolagem de volta.
    for (let i = 0; i < 8; i++) await page.mouse.move(300 + i * 20, 300 + i * 10);
    await page.waitForTimeout(600);
    expect(await scroller.evaluate((el) => el.scrollTop)).toBe(before);
    await index.getByRole("button", { name: "Fechar índice de páginas" }).click();
    await page.getByRole("button", { name: "Configurações de leitura" }).click();
    await panel.getByLabel("Modo de página").selectOption("single");
    await panel.getByRole("button", { name: "Fechar configurações de leitura" }).click();
    await page.getByRole("button", { name: "Biblioteca" }).click();
  });

  test("biblioteca com progresso, lista, busca e filtros", async () => {
    const page = app.page;
    await goLibrary(page);
    await expect(page.getByText("Leitura concluída").or(page.getByText("Concluído")).first()).toBeVisible();
    await expect(page.getByText("Parou na página 4 de 10")).toBeVisible();
    await expect(page.getByText("Não iniciado").first()).toBeVisible();
    await shot(page, "06-biblioteca-capas-e-progresso");
    await page.getByRole("radio", { name: /Lista/ }).click();
    await shot(page, "06b-biblioteca-lista");
    await page.getByRole("radio", { name: /Grade/ }).click();
    await page.getByLabel(/Pesquisar por título/).fill("farol");
    await expect(page.getByText("1 encontrado")).toBeVisible();
    await page.getByRole("button", { name: "Limpar pesquisa" }).click();
    // Abrir cada formato no teste anterior deixou 5 HQs em andamento; a Farol foi concluída.
    await page.getByRole("button", { name: "Filtrar" }).click();
    await page.getByRole("button", { name: "Em andamento", exact: true }).click();
    await expect(page.getByRole("button", { name: /^Ver detalhes de/ })).toHaveCount(5);
    await page.getByRole("button", { name: "Em andamento", exact: true }).click();
    await page.getByRole("button", { name: "Concluído", exact: true }).click();
    await expect(page.getByRole("button", { name: /^Ver detalhes de/ })).toHaveCount(1);
    await page.getByRole("button", { name: "Concluído", exact: true }).click();
    await page.getByLabel("Tipo de arquivo").selectOption("pdf");
    await expect(page.getByRole("button", { name: /^Ver detalhes de/ })).toHaveCount(1);
    await expect(page.getByRole("button", { name: "Remover filtro PDF" })).toBeVisible();
    await page.getByLabel("Tipo de arquivo").selectOption("");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog", { name: "Filtros" })).toHaveCount(0);
  });

  test("detalhes, edição, favoritos e coleções", async () => {
    const page = app.page;
    await page.getByRole("button", { name: "Ver detalhes de O Farol de Papel" }).click();
    await expect(page.getByRole("heading", { name: "O Farol de Papel", level: 1 })).toBeVisible();
    // Capas reais não recebem legenda; só a substituta é explicada.
    await expect(page.getByText("Capa substituta: a imagem original")).toHaveCount(0);
    await shot(page, "07-detalhes");
    await page.getByRole("button", { name: "Editar informações" }).click();
    const edit = page.getByRole("dialog", { name: "Editar informações" });
    await edit.getByLabel("Título").fill("O Farol de Papel (edição revisada)");
    await edit.getByLabel("Editora").fill("Editora Teste");
    await edit.getByRole("button", { name: "Salvar alterações" }).click();
    await expect(page.getByRole("heading", { name: "O Farol de Papel (edição revisada)", level: 1 })).toBeVisible();
    await expect(page.getByText("editado").first()).toBeVisible();
    await page.getByRole("button", { name: "Favoritar" }).click();
    await expect(page.getByRole("button", { name: "Remover dos favoritos" })).toBeVisible();
    await page.getByRole("button", { name: "Organizar em coleções" }).click();
    const col = page.getByRole("dialog", { name: "Coleções" });
    await col.getByLabel("Nova coleção").fill("Clássicos de teste");
    await col.getByRole("button", { name: "Criar coleção" }).click();
    await expect(col.getByRole("checkbox", { name: /Clássicos de teste/ })).toBeChecked();
    await col.getByRole("button", { name: "Salvar coleções" }).click();
    await expect(page.getByRole("button", { name: "Clássicos de teste" })).toBeVisible();
    await page.getByRole("navigation", { name: "Navegação principal" }).getByRole("button", { name: "Favoritos" }).click();
    await expect(page.getByRole("button", { name: "Ver detalhes de O Farol de Papel (edição revisada)" })).toBeVisible();
    await page.getByRole("navigation", { name: "Navegação principal" }).getByRole("button", { name: "Coleções" }).click();
    await expect(page.getByText("Clássicos de teste")).toBeVisible();
  });

  test("configurações nos temas escuro e claro", async () => {
    const page = app.page;
    await page.getByRole("navigation", { name: "Navegação principal" }).getByRole("button", { name: "Configurações" }).click();
    await expect(page.getByRole("heading", { name: "Configurações", level: 1 })).toBeVisible();
    await shot(page, "10-configuracoes-tema-escuro");
    await page.getByRole("radiogroup", { name: "Tema" }).getByRole("radio", { name: /Claro/ }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await shot(page, "11-configuracoes-tema-claro");
    await goLibrary(page);
    await shot(page, "11b-biblioteca-tema-claro");
    await page.getByRole("button", { name: "Ver detalhes de Rio Vermelho 02" }).click();
    await page.getByRole("button", { name: "Continuar leitura" }).click();
    await waitPageLoaded(page, 4);
    await shot(page, "11c-leitor-tema-claro");
    await page.getByRole("button", { name: "Biblioteca" }).click();
  });

  test("janela pequena mantém os controles essenciais", async () => {
    const page = app.page;
    await page.setViewportSize({ width: 760, height: 560 });
    await goLibrary(page);
    await expect(page.getByRole("button", { name: "Adicionar quadrinhos" }).first()).toBeVisible();
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    expect(overflow).toBe(false);
    await shot(page, "12-biblioteca-janela-pequena");
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.getByRole("navigation", { name: "Navegação principal" }).getByRole("button", { name: "Configurações" }).click();
    await page.getByRole("radiogroup", { name: "Tema" }).getByRole("radio", { name: /Escuro/ }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  });

  test("o menu das configurações acompanha a rolagem", async () => {
    const page = app.page;
    const nav = page.getByRole("navigation", { name: "Seções das configurações" });
    const expectCurrent = (name: string) => expect(nav.getByRole("link", { name })).toHaveAttribute("aria-current", "true");
    const scrollTo = (id: string, block: ScrollLogicalPosition = "start") => page.locator(`#settings-${id}`).evaluate((el, b) => el.scrollIntoView({ block: b }), block);

    await scrollTo("biblioteca");
    await expectCurrent("Biblioteca");
    await scrollTo("geral", "end");
    await expectCurrent("Geral");
    await scrollTo("aparencia");
    await expectCurrent("Aparência");

    // O clique vale até o fim da rolagem, mesmo que a seção não chegue ao topo.
    await nav.getByRole("link", { name: "Atualizações" }).click();
    await page.waitForTimeout(1500);
    await expectCurrent("Atualizações");
    await scrollTo("aparencia");
    await expectCurrent("Aparência");
  });

  test("limpar miniaturas não apaga o progresso", async () => {
    const page = app.page;
    await page.getByRole("button", { name: "Limpar miniaturas" }).click();
    await page.getByRole("dialog", { name: "Limpar as miniaturas?" }).getByRole("button", { name: "Limpar miniaturas" }).click();
    await goLibrary(page);
    await expect(page.getByText("Preparando capa…")).toHaveCount(0, { timeout: 30_000 });
    await expect(page.getByText("Parou na página 4 de 10")).toBeVisible();
    await expect(page.getByRole("img", { name: "Capa de Cidade Nublada" })).toBeVisible();
  });

  test("remover da biblioteca preserva o arquivo original", async () => {
    const page = app.page;
    await page.getByRole("button", { name: "Mais ações para Sem Capa Legível" }).click();
    await page.getByRole("menuitem", { name: /Remover da biblioteca/ }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Remover da biblioteca" }).click();
    await expect(page.getByRole("button", { name: "Ver detalhes de Sem Capa Legível" })).toHaveCount(0);
    expect(existsSync(join(files.lib, "Sem Capa Legível.cbz"))).toBe(true);
  });

  test("arquivo movido é indicado e pode ser localizado de novo", async () => {
    const page = app.page;
    const original = join(files.lib, "Tiras de Domingo.cbt");
    const moved = join(files.lib, "Tiras (movido).cbt");
    renameSync(original, moved);
    await page.getByRole("button", { name: "Ver detalhes de Tiras de Domingo" }).click();
    await expect(page.getByText("Arquivo original não encontrado")).toBeVisible();
    await shot(page, "13-detalhes-arquivo-ausente");
    app.nextPick([moved]);
    await page.getByRole("button", { name: "Localizar arquivo…" }).click();
    await expect(page.getByText("Arquivo original não encontrado")).toHaveCount(0);
    await expect(page.getByText("Tiras (movido).cbt").first()).toBeVisible();
  });
});

// Roda só com páginas reais: os quadrinhos e o gabarito ficam em tests/real, fora do git.
test.describe.serial("Leitura Guiada em páginas reais", () => {
  const manifest = JSON.parse(readFileSync(join(ROOT, "tests", "real", "manifest.json"), "utf8")) as {
    comics: Array<{ title: string; path: string; pageCount: number }>;
    pdf: { title: string; path: string; pageCount: number };
    guided: { comic: number; dense: number; denseCount: number; nextCount: number; empty: number; afterEmptyCount: number; last: number; doubtful: number; doubtfulCount: number };
  };
  const plan = manifest.guided;
  const comic = manifest.comics[plan.comic]!;
  const of = (n: number) => `Página ${n + 1} de ${comic.pageCount}`;
  let app: AppSession;
  const dir = freshDataDir("guiada");

  const stage = (page: Page) => page.getByTestId("guided-reader");
  const progress = (page: Page) => page.getByTestId("guided-progress");
  const pill = (page: Page) => page.getByRole("toolbar", { name: "Leitura Guiada" });
  const balloonX = async (page: Page) => {
    await page.waitForTimeout(420);
    const box = (await page.getByTestId("guided-spot").boundingBox())!;
    return box.x + box.width / 2;
  };
  const shotOutsideGit = async (page: Page, name: string) => {
    await page.waitForTimeout(420);
    await page.screenshot({ path: join(ROOT, "tests", "real", "screenshots", `${name}.png`) });
  };
  const goTo = async (page: Page, index: number) => {
    await page.mouse.move(400, 300);
    const slider = page.locator("#reader-progress");
    await slider.fill(String(index + 1));
    await waitPageLoaded(page, index + 1);
    // Com o foco no controle deslizante, os atalhos do leitor ficariam desligados.
    await slider.blur();
  };
  const enter = async (page: Page) => {
    await page.mouse.move(400, 300);
    await page.getByRole("button", { name: "Leitura Guiada" }).first().click();
    await expect(stage(page)).toBeVisible();
  };
  const leave = async (page: Page) => {
    await page.keyboard.press("Escape");
    await expect(stage(page)).toHaveCount(0);
    // Os controles do leitor somem sozinhos: sem mover o mouse, "Biblioteca" fica fora da tela e o clique espera para sempre.
    await page.mouse.move(410, 310);
  };
  const editor = (page: Page) => page.getByRole("button", { name: /^Corrigir regiões/ }).click();
  // A barra de progresso continua na tela por baixo do editor: só o editor fechado prova que a gravação terminou.
  const editorClosed = (page: Page) => expect(page.getByTestId("guided-editor-page")).toHaveCount(0);

  test.beforeAll(async () => {
    app = await launchApp(dir);
    await app.page.getByRole("button", { name: "Pular tutorial" }).click();
    // A importação só lê os originais; banco e cache ficam numa pasta de dados descartável.
    app.nextPick([comic.path, join(ROOT, manifest.pdf.path)]);
    await app.page.getByRole("button", { name: "Selecionar uma pasta" }).click();
    const panel = app.page.getByRole("region", { name: "Importação de quadrinhos" });
    await expect(panel.getByText("Importação concluída")).toBeVisible({ timeout: 120_000 });
    await panel.getByRole("button", { name: "Fechar resumo da importação" }).click();
  });
  test.afterAll(async () => app?.close());

  test("destaca cada balão sem mexer na página, navega e respeita os limites", async () => {
    const page = app.page;
    await openReader(page, comic.title);
    await goTo(page, plan.dense);
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`, { timeout: 60_000 });
    await expect(pill(page).getByRole("button", { name: "Anterior" })).toBeDisabled();
    await page.waitForTimeout(420);
    // O balão ativo fica inteiro na tela e a página inteira continua à vista, acima dos controles.
    const spot = (await page.getByTestId("guided-spot").boundingBox())!;
    const view = await page.evaluate(() => ({ width: window.innerWidth, height: window.innerHeight }));
    const sheet = (await page.getByTestId("guided-page").boundingBox())!;
    expect(spot.x).toBeGreaterThanOrEqual(0);
    expect(spot.y).toBeGreaterThanOrEqual(0);
    expect(spot.x + spot.width).toBeLessThanOrEqual(view.width);
    expect(spot.y + spot.height).toBeLessThanOrEqual(view.height - 60);
    expect(sheet.y).toBeGreaterThanOrEqual(0);
    expect(sheet.y + sheet.height).toBeLessThanOrEqual((await pill(page).boundingBox())!.y);
    expect(sheet.height / view.height).toBeGreaterThan(0.6);

    await page.keyboard.press("ArrowRight");
    await expect(progress(page)).toHaveText(`2 de ${plan.denseCount}`);
    await page.waitForTimeout(420);
    // A página não mudou de lugar nem de tamanho: quem se moveu foi o balão.
    expect(await page.getByTestId("guided-page").boundingBox()).toEqual(sheet);
    expect(await page.getByTestId("guided-spot").boundingBox()).not.toEqual(spot);
    await page.keyboard.press("Space");
    await expect(progress(page)).toHaveText(`3 de ${plan.denseCount}`);
    await page.keyboard.press("Enter");
    await expect(progress(page)).toHaveText(`4 de ${plan.denseCount}`);
    await page.keyboard.press("ArrowLeft");
    await expect(progress(page)).toHaveText(`3 de ${plan.denseCount}`);
    await pill(page).getByRole("button", { name: "Próximo" }).click();
    await expect(progress(page)).toHaveText(`4 de ${plan.denseCount}`);
    // Toques em sequência, sem esperar a tela: nenhum se perde.
    for (let i = 4; i <= plan.denseCount; i++) await page.keyboard.press("ArrowRight");
    await expect(stage(page).getByText("Página concluída")).toBeVisible();
    await stage(page).getByRole("button", { name: "Rever último balão" }).click();
    await expect(progress(page)).toHaveText(`${plan.denseCount} de ${plan.denseCount}`);
    await page.keyboard.press("ArrowRight");
    await stage(page).getByRole("button", { name: "Próxima página" }).click();
    await expect(progress(page)).toHaveText(`1 de ${plan.nextCount}`, { timeout: 60_000 });

    // Ao sair, o leitor comum está na mesma página e responde como antes.
    await leave(page);
    await expect(await indicator(page)).toHaveText(of(plan.dense + 1));
    await page.keyboard.press("ArrowLeft");
    await expect(await indicator(page)).toHaveText(of(plan.dense));
    await page.keyboard.press("ArrowRight");
    await expect(await indicator(page)).toHaveText(of(plan.dense + 1));
  });

  test("página real sem balões não inventa regiões", async () => {
    const page = app.page;
    await goTo(page, plan.empty);
    await enter(page);
    await expect(stage(page).getByText("Nenhum balão identificado nesta página")).toBeVisible({ timeout: 60_000 });
    await shotOutsideGit(page, "sem-baloes");
    await page.keyboard.press("ArrowRight");
    await expect(progress(page)).toHaveText(`1 de ${plan.afterEmptyCount}`, { timeout: 60_000 });
    await leave(page);
    // Última página da HQ: não há próxima para oferecer.
    await goTo(page, plan.last);
    await enter(page);
    await expect(stage(page).getByText("Nenhum balão identificado nesta página")).toBeVisible({ timeout: 60_000 });
    await expect(stage(page).getByRole("button", { name: "Próxima página" })).toHaveCount(0);
    await leave(page);
  });

  test("OLÁ e RAHHH! aparecem, e o balão duvidoso fica marcado", async () => {
    const page = app.page;
    await goTo(page, plan.doubtful);
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.doubtfulCount}`, { timeout: 60_000 });
    await expect(page.getByTestId("guided-spot-doubtful")).toBeVisible();
    await expect(page.getByRole("button", { name: "Corrigir regiões (há regiões duvidosas)" })).toBeVisible();
    await shotOutsideGit(page, "ola-duvidoso");
    for (let i = 1; i < plan.doubtfulCount; i++) await page.keyboard.press("ArrowRight");
    await expect(progress(page)).toHaveText(`${plan.doubtfulCount} de ${plan.doubtfulCount}`);
    await shotOutsideGit(page, "rahhh");
    await page.keyboard.press("ArrowRight");
    await expect(stage(page).getByText("Página concluída")).toBeVisible();
    // O cartão de fim fica abaixo da página, como os controles: a arte continua inteira.
    const card = (await page.getByRole("status").filter({ hasText: "Página concluída" }).boundingBox())!;
    const sheet = (await page.getByTestId("guided-page").boundingBox())!;
    expect(card.y).toBeGreaterThanOrEqual(sheet.y + sheet.height - 1);
    await shotOutsideGit(page, "pagina-concluida");
    await leave(page);
  });

  test("os controles acompanham o tema claro", async () => {
    const page = app.page;
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.doubtfulCount}`);
    await shotOutsideGit(page, "tema-claro");
    await leave(page);
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
  });

  test("a transição mantém os quadros da animação", async () => {
    const page = app.page;
    await goTo(page, plan.dense);
    await page.keyboard.press("g");
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    await page.waitForTimeout(420);
    const sample = page.evaluate(
      () =>
        new Promise<number[]>((resolve) => {
          const deltas: number[] = [];
          let last = performance.now();
          const start = last;
          const tick = (now: number) => {
            deltas.push(now - last);
            last = now;
            if (now - start < 450) requestAnimationFrame(tick);
            else resolve(deltas);
          };
          requestAnimationFrame(tick);
        }),
    );
    await page.keyboard.press("ArrowRight");
    const deltas = (await sample).sort((a, b) => a - b);
    const p95 = deltas[Math.floor(deltas.length * 0.95)]!;
    console.log(`Leitura Guiada: ${deltas.length} quadros, p95 ${p95.toFixed(1)} ms`);
    expect(p95).toBeLessThan(50);
    await shotOutsideGit(page, "balao-destacado");
    await leave(page);
  });

  test("a ordem segue a direção de leitura", async () => {
    const page = app.page;
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    const first = await balloonX(page);
    await page.keyboard.press("ArrowRight");
    await expect(progress(page)).toHaveText(`2 de ${plan.denseCount}`);
    // Esquerda para a direita: o segundo balão está à direita do primeiro.
    expect(await balloonX(page)).toBeGreaterThan(first);
    await leave(page);

    await page.keyboard.press("d");
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    const firstRtl = await balloonX(page);
    await page.keyboard.press("ArrowLeft");
    await expect(progress(page)).toHaveText(`2 de ${plan.denseCount}`);
    expect(await balloonX(page)).toBeLessThan(firstRtl);
    await page.keyboard.press("ArrowRight");
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    await leave(page);
    await page.keyboard.press("d");
  });

  test("funciona nos modos duplo e vertical, sem mudar o modo", async () => {
    const page = app.page;
    await page.keyboard.press("m");
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    await leave(page);
    await expect(page.getByRole("button", { name: /atual: Página dupla/ })).toBeVisible();
    await page.keyboard.press("m");
    await expect(page.getByRole("button", { name: /atual: Rolagem vertical/ })).toBeVisible();
    await waitPageLoaded(page, plan.dense + 1);
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    await leave(page);
    await expect(page.getByRole("button", { name: /atual: Rolagem vertical/ })).toBeVisible();
    await expect(await indicator(page)).toHaveText(of(plan.dense));
    await page.keyboard.press("m");
    await expect(await indicator(page)).toHaveText(of(plan.dense));
  });

  test("acompanha a janela redimensionada", async () => {
    const page = app.page;
    await enter(page);
    await page.waitForTimeout(420);
    await page.setViewportSize({ width: 760, height: 560 });
    await page.waitForTimeout(150);
    const spot = (await page.getByTestId("guided-spot").boundingBox())!;
    expect(spot.x).toBeGreaterThanOrEqual(0);
    expect(spot.x + spot.width).toBeLessThanOrEqual(760);
    expect(spot.y + spot.height).toBeLessThanOrEqual(560 - 60);
    await expect(pill(page)).toBeInViewport({ ratio: 1 });
    await shotOutsideGit(page, "janela-pequena");
    await page.setViewportSize({ width: 1280, height: 800 });
    await leave(page);
  });

  test("correções sobrevivem ao reinício e podem ser desfeitas", async () => {
    let page = app.page;
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    await editor(page);
    const rows = page.getByTestId("guided-editor-row");
    await expect(rows).toHaveCount(plan.denseCount);
    await shotOutsideGit(page, "editor");
    const originalKind = await page.getByLabel("Tipo da região 2", { exact: true }).inputValue();

    await page.getByRole("button", { name: `Excluir a região ${plan.denseCount}` }).click();
    await expect(rows).toHaveCount(plan.denseCount - 1);
    // Desenha uma região nova arrastando numa parte da página sem regiões.
    const sheet = (await page.getByTestId("guided-editor-page").boundingBox())!;
    await page.mouse.move(sheet.x + sheet.width * 0.15, sheet.y + sheet.height * 0.55);
    await page.mouse.down();
    await page.mouse.move(sheet.x + sheet.width * 0.3, sheet.y + sheet.height * 0.65, { steps: 4 });
    await page.mouse.up();
    await expect(rows).toHaveCount(plan.denseCount);
    // Arrasta a região 1 e redimensiona pela alça.
    const region = page.getByRole("button", { name: /^Região 1:/ });
    const before = (await region.boundingBox())!;
    await page.mouse.move(before.x + before.width / 2, before.y + before.height / 2);
    await page.mouse.down();
    await page.mouse.move(before.x + before.width / 2 + 20, before.y + before.height / 2 + 30, { steps: 3 });
    await page.mouse.up();
    const moved = (await region.boundingBox())!;
    expect(moved.x).toBeGreaterThan(before.x + 10);
    await page.mouse.move(moved.x + moved.width, moved.y + moved.height);
    await page.mouse.down();
    await page.mouse.move(moved.x + moved.width + 15, moved.y + moved.height + 15, { steps: 3 });
    await page.mouse.up();
    expect((await region.boundingBox())!.width).toBeGreaterThan(moved.width + 5);
    // Muda o tipo e a ordem: a região 1 vira "Pensamento" e passa a ser a segunda.
    await page.getByLabel("Tipo da região 1", { exact: true }).selectOption({ label: "Pensamento" });
    await page.getByRole("button", { name: "Ler a região 1 depois" }).click();
    await expect(page.getByLabel("Tipo da região 2", { exact: true })).toHaveValue("thought");
    await page.getByRole("button", { name: "Confirmar sequência" }).click();
    await editorClosed(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    await leave(page);
    await page.getByRole("button", { name: "Biblioteca" }).click();

    // Reinício real do aplicativo com os mesmos dados.
    await app.close();
    app = await launchApp(dir);
    page = app.page;
    await openReader(page, comic.title);
    await goTo(page, plan.dense);
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    await editor(page);
    await expect(page.getByLabel("Tipo da região 2", { exact: true })).toHaveValue("thought");

    // Cancelar com mudanças pendentes pergunta antes de descartar.
    await page.getByRole("button", { name: "Excluir a região 3" }).click();
    await page.getByRole("button", { name: "Fechar corrigir regiões" }).click();
    const confirm = page.getByRole("dialog", { name: "Descartar as correções?" });
    await confirm.getByRole("button", { name: "Descartar" }).click();
    await editorClosed(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);

    await editor(page);
    await page.getByRole("button", { name: "Restaurar detecção automática" }).click();
    await editorClosed(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`);
    await editor(page);
    await expect(page.getByLabel("Tipo da região 2", { exact: true })).toHaveValue(originalKind);
    await expect(page.getByRole("button", { name: "Restaurar detecção automática" })).toHaveCount(0);
    await page.getByRole("button", { name: "Fechar corrigir regiões" }).click();
    await editorClosed(page);
    await leave(page);
  });

  test("página real sem balões pode receber regiões à mão", async () => {
    const page = app.page;
    await goTo(page, plan.empty);
    await enter(page);
    await stage(page).getByRole("button", { name: "Marcar balões" }).click();
    await page.getByRole("button", { name: "Adicionar região" }).click();
    await page.getByRole("button", { name: "Confirmar sequência" }).click();
    await editorClosed(page);
    await expect(progress(page)).toHaveText("1 de 1");
    await leave(page);
    await page.getByRole("button", { name: "Biblioteca" }).click();
  });

  test("páginas reais em PDF são analisadas a partir do PDF.js", async () => {
    const page = app.page;

    await openReader(page, manifest.pdf.title);
    await waitPageLoaded(page);
    await enter(page);
    await expect(progress(page)).toHaveText(`1 de ${plan.denseCount}`, { timeout: 60_000 });
    await expect(page.getByTestId("guided-spot").locator("img")).toHaveJSProperty("complete", true);
    await shotOutsideGit(page, "pdf");
    await leave(page);
    await expect(await indicator(page)).toHaveText(`Página 1 de ${manifest.pdf.pageCount}`);
    await page.getByRole("button", { name: "Biblioteca" }).click();
  });
});
