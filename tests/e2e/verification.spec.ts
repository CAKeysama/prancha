// A janela de verificação, com um site local no lugar do site da fonte. Nenhum site de verdade aqui.
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { freshDataDir, launchApp } from "./harness";

test("a janela do site não alcança o app, e o que o site liberou chega ao servidor das fontes", async () => {
  const seen = { userAgent: "", library: "" };
  const site = createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://site");
    if (url.pathname === "/") {
      seen.userAgent = request.headers["user-agent"] ?? "";
      response.setHeader("Content-Type", "text/html");
      response.end(`<script>
        fetch("http://prancha.localhost/online-cover/1")
          .then((reply) => reply.status, () => "bloqueado")
          .then((status) => fetch("/relatorio?biblioteca=" + status))
          .then(() => { location.href = "/liberar"; });
      </script>`);
    } else if (url.pathname === "/relatorio") {
      seen.library = url.searchParams.get("biblioteca") ?? "";
      response.end();
    } else if (url.pathname === "/liberar") {
      response.setHeader("Set-Cookie", ["sessao=1; Path=/", "cf_clearance=prova; Path=/; Max-Age=3600"]);
      response.end("liberado");
    } else {
      response.statusCode = 404;
      response.end();
    }
  });
  await new Promise<void>((resolve) => site.listen(0, "127.0.0.1", resolve));
  const { port } = site.address() as AddressInfo;

  process.env.PRANCHA_E2E_VERIFY_URL = `http://127.0.0.1:${port}/`;
  const dir = freshDataDir("verificacao");
  const app = await launchApp(dir);
  try {
    const released = await app.page.evaluate(() => (window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args: unknown) => Promise<boolean> } }).__TAURI_INTERNALS__.invoke("source_verify", { sourceId: "x" }));
    expect(released).toBe(true);

    // A liberação vale para o navegador que passou por ela, e o servidor declara este.
    expect(seen.userAgent).toContain("Chrome/120.0.0.0");
    // O site pede a biblioteca, e o protocolo recusa: só a janela principal pode ler.
    expect(seen.library).toBe("403");

    const store = readFileSync(join(dir, "suwayomi", "dados", "settings", "cookie_store.xml"), "utf8");
    expect(store).toContain('<entry key="127.0.0.1.size">2</entry>');
    expect(store).toMatch(/<entry key="127\.0\.0\.1\.\d">cf_clearance=prova;/);
    expect(store).toMatch(/^<\?xml/);
  } finally {
    delete process.env.PRANCHA_E2E_VERIFY_URL;
    await app.close();
    site.close();
  }
});
