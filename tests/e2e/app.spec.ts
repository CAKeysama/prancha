// Testes E2E no executável real (backend Rust + WebView2). Apenas a escolha de caminho do
// diálogo nativo é substituída (PRANCHA_E2E_PICK_FILE, só em builds de depuração); a importação,
// o banco, as capas e a leitura são reais.
import { cpSync, existsSync, renameSync, rmSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { FIXTURES, freshDataDir, launchApp, shot, type AppSession } from "./harness";

function copyFixtures(dataDir: string): { lib: string; errors: string } {
  const dest = join(dataDir, "..", `${dataDir.split(/[\\/]/).pop()}-arquivos`);
  rmSync(dest, { recursive: true, force: true });
  cpSync(FIXTURES, dest, { recursive: true });
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
