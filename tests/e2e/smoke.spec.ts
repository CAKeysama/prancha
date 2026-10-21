import { expect, test } from "@playwright/test";
import { freshDataDir, launchApp, shot } from "./harness";

test("o aplicativo inicia e mostra o tutorial", async () => {
  const app = await launchApp(freshDataDir("smoke"));
  try {
    await expect(app.page.getByRole("dialog", { name: "Bem-vindo ao Prancha" })).toBeVisible();
    await shot(app.page, "00-smoke");
  } finally {
    await app.close();
  }
});
