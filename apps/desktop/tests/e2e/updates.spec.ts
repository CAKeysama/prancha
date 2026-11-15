// As janelas e a seção de atualizações no executável real, nos dois temas e na menor janela.
// Builds de depuração não acessam a rede: o estado é gravado direto, como o backend grava depois de consultar.
// O download e a instalação de verdade estão em tests/update/run.mjs.
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { ROOT, freshDataDir, launchApp, type AppSession } from "./harness";

const SHOTS = join(ROOT, "tests", ".tmp", "update-shots");
const NEXT = "99.0.0";
const NOTES = `Lançada em 1 de novembro de 2026.

Resumo da versão, curto o bastante para caber na janela.

## Novidades

- **Algo novo** para testar uma linha um pouco mais longa que as outras e ver como ela quebra dentro da janela

## Melhorias

- Algo melhor

## Correções

- Algo corrigido
`;

const release = (version: string) => ({ version, name: `Prancha v${version}`, date: "2026-11-01T12:00:00Z", notes: NOTES, url: "" });
const invoke = <T>(page: Page, cmd: string, args: Record<string, unknown> = {}) =>
  page.evaluate(([c, a]) => (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<T> } }).__TAURI_INTERNALS__.invoke(c as string, a), [cmd, args] as const);

async function seed(page: Page, theme: string, state: Record<string, unknown>, releases: unknown[]) {
  await invoke(page, "set_setting", { key: "onboarding", value: { status: "skipped", step: 1, comicId: null } });
  await invoke(page, "set_setting", { key: "appearance.theme", value: theme });
  await invoke(page, "set_setting", { key: "updates.state", value: state });
  await invoke(page, "set_setting", { key: "updates.releases", value: releases });
}

async function fitsTheWindow(page: Page) {
  const box = await page.getByRole("dialog").boundingBox();
  const viewport = page.viewportSize() ?? (await page.evaluate(() => ({ width: innerWidth, height: innerHeight })));
  expect(box).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
}

for (const theme of ["dark", "light"]) {
  test.describe.serial(`atualizações no tema ${theme}`, () => {
    let app: AppSession;
    let current = "";
    const dir = freshDataDir(`updates-${theme}`);
    const shot = (name: string) => app.page.screenshot({ path: join(SHOTS, `${theme}-${name}.png`) });

    test.beforeAll(async () => {
      app = await launchApp(dir);
      current = (await invoke<{ version: string }>(app.page, "get_app_info")).version;
      await seed(app.page, theme, { installedVersion: current, available: release(NEXT) }, []);
      await app.close();
      app = await launchApp(dir);
    });
    test.afterAll(async () => {
      await app.close();
    });

    test("nova versão, indicador e reabertura", async () => {
      const page = app.page;
      const offer = page.getByRole("dialog", { name: "Uma nova versão do Prancha está disponível!" });
      await expect(offer).toBeVisible();
      await expect(offer.getByText(`v${current}`)).toBeVisible();
      await expect(offer.getByText(`v${NEXT}`)).toBeVisible();
      await fitsTheWindow(page);
      await shot("oferta");

      await page.getByRole("button", { name: "Depois", exact: true }).click();
      await expect(offer).toHaveCount(0);
      const indicator = page.getByRole("button", { name: `v${NEXT} disponível` });
      await expect(indicator).toBeVisible();
      await shot("indicador");

      await indicator.click();
      await expect(offer).toBeVisible();
      await page.getByRole("button", { name: "Depois", exact: true }).click();
    });

    test("a versão recusada fica gravada e a janela não volta sozinha", async () => {
      await app.close();
      app = await launchApp(dir);
      const page = app.page;
      await expect(page.getByRole("button", { name: `v${NEXT} disponível` })).toBeVisible();
      await page.waitForTimeout(800);
      await expect(page.getByRole("dialog")).toHaveCount(0);
    });

    test("seção Atualizações nas configurações", async () => {
      const page = app.page;
      await page.getByRole("button", { name: "Configurações" }).click();
      await page.getByRole("navigation", { name: "Categorias das configurações" }).getByRole("button", { name: "Atualizações" }).click();
      const section = page.getByRole("region", { name: "Atualizações" });
      await expect(section.getByText(`v${current}`, { exact: true })).toBeVisible();
      await expect(section.getByText(`Nova versão: v${NEXT}`)).toBeVisible();
      await section.scrollIntoViewIfNeeded();
      await shot("configuracoes");
      await section.getByRole("button", { name: "Atualizar agora" }).click();
      await expect(page.getByRole("dialog", { name: "Uma nova versão do Prancha está disponível!" })).toBeVisible();
      await page.getByRole("button", { name: "Depois", exact: true }).click();
    });

    test("novidades depois de atualizar, sem repetir", async () => {
      await seed(app.page, theme, { installedVersion: current, welcomeVersion: current }, [release(current), release("0.0.1")]);
      await app.close();
      app = await launchApp(dir);
      const page = app.page;
      const welcome = page.getByRole("dialog", { name: "O Prancha está atualizado!" });
      await expect(welcome).toBeVisible();
      await expect(welcome.getByText(`Você está utilizando a versão v${current}.`)).toBeVisible();
      await expect(welcome.getByText("Obrigado por manter o Prancha atualizado e fazer parte da nossa evolução!")).toBeVisible();
      await expect(welcome.getByText("Algo corrigido")).toBeVisible();
      await fitsTheWindow(page);
      await shot("novidades");
      await page.getByRole("button", { name: "Começar a ler" }).click();
      await expect(welcome).toHaveCount(0);

      await app.close();
      app = await launchApp(dir);
      await app.page.waitForTimeout(800);
      await expect(app.page.getByRole("dialog")).toHaveCount(0);
    });

    test("histórico e menor janela", async () => {
      const page = app.page;
      await seed(page, theme, { installedVersion: current, available: release(NEXT), declinedVersion: NEXT }, [release(current), release("0.0.1")]);
      await app.close();
      app = await launchApp(dir);
      await app.page.setViewportSize({ width: 720, height: 520 });
      const narrow = app.page;
      const indicator = narrow.getByRole("button", { name: `v${NEXT} disponível` });
      await expect(indicator).toBeVisible();
      await expect(indicator.locator("span")).toHaveCount(0);
      await shot("indicador-estreito");

      await narrow.getByRole("button", { name: "Configurações" }).click();
      await narrow.getByRole("navigation", { name: "Categorias das configurações" }).getByRole("button", { name: "Atualizações" }).click();
      const section = narrow.getByRole("region", { name: "Atualizações" });
      const row = section.getByRole("list", { name: "Versões publicadas" }).getByText("v0.0.1");
      await row.scrollIntoViewIfNeeded();
      await row.click();
      await expect(section.getByText("Algo corrigido").last()).toBeVisible();
      await expect(section.getByRole("button", { name: "Ver no GitHub" }).last()).toBeVisible();
      await shot("historico-estreito");

      await indicator.click();
      await expect(narrow.getByRole("dialog", { name: "Uma nova versão do Prancha está disponível!" })).toBeVisible();
      await fitsTheWindow(narrow);
      await shot("oferta-estreita");
    });
  });
}
