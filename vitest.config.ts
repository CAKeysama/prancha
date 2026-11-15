import { defineConfig } from "vitest/config";

// Um `npm test` na raiz roda os testes de todos os pacotes e os do repositório (versão, changelog, site).
export default defineConfig({
  test: {
    projects: [
      "packages/shared",
      "apps/desktop",
      {
        test: {
          name: "repositório",
          environment: "jsdom",
          include: ["tests/*.test.ts"],
        },
      },
    ],
  },
});
