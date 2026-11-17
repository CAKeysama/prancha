import { defineConfig } from "@playwright/test";

// Os testes E2E abrem o executável real do Prancha (WebView2) e se conectam via CDP.
export default defineConfig({
  testDir: "tests/e2e",
  testMatch: /.*\.spec\.ts/,
  timeout: 120_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
});
