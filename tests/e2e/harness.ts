// Inicia o executável real do Prancha e conecta o Playwright ao WebView2 via CDP.
// Cada sessão usa uma pasta de dados isolada; nada toca a biblioteca do usuário.
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { chromium, type Browser, type Page } from "@playwright/test";

export const ROOT = resolve(fileURLToPath(new URL(".", import.meta.url)), "..", "..");
export const FIXTURES = join(ROOT, "tests", ".tmp", "fixtures");
export const SHOTS = join(ROOT, "artifacts", "screenshots");

export function appExe(): string {
  const exe = process.env.PRANCHA_EXE ?? join(ROOT, "tests", ".tmp", "bin", "prancha.exe");
  if (!existsSync(exe)) throw new Error(`Executável não encontrado: ${exe}. Rode "npm run e2e:build".`);
  return exe;
}

export interface AppSession {
  proc: ChildProcess;
  browser: Browser;
  page: Page;
  dataDir: string;
  pickFile: string;
  /** Define a resposta do próximo seletor de arquivos (substitui o diálogo nativo). */
  nextPick: (paths: string[] | null) => void;
  close: (graceful?: boolean) => Promise<void>;
}

let portSeq = 9400 + Math.floor(Math.random() * 400);

/** Cópia recursiva sem `fs.cpSync` (que falha no Node 22 em caminhos com "Á", ver scripts/copy-dir.mjs). */
export function copyDir(from: string, to: string): void {
  mkdirSync(to, { recursive: true });
  for (const entry of readdirSync(from, { withFileTypes: true })) {
    const src = join(from, entry.name);
    const dest = join(to, entry.name);
    if (entry.isDirectory()) copyDir(src, dest);
    else if (entry.isFile()) copyFileSync(src, dest);
  }
}

export function freshDataDir(name: string): string {
  const dir = join(ROOT, "tests", ".tmp", "data", name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  return dir;
}

async function waitForCdp(port: number, timeoutMs = 30_000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (res.ok) return;
    } catch {
      /* ainda iniciando */
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`WebView2 não abriu a porta de depuração ${port}`);
}

export async function launchApp(dataDir: string): Promise<AppSession> {
  const port = portSeq++;
  const pickFile = join(dataDir, "e2e-pick.json");
  const proc = spawn(appExe(), [], {
    env: {
      ...process.env,
      PRANCHA_DATA_DIR: dataDir,
      PRANCHA_E2E_PICK_FILE: pickFile,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
      WEBVIEW2_USER_DATA_FOLDER: join(dataDir, "webview"),
    },
    stdio: "ignore",
  });
  await waitForCdp(port);
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  let page: Page | undefined;
  for (let i = 0; i < 80 && !page; i++) {
    page = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().startsWith("devtools"));
    if (!page) await new Promise((r) => setTimeout(r, 250));
  }
  if (!page) throw new Error("Página do aplicativo não encontrada");
  await page.waitForLoadState("domcontentloaded");
  const session: AppSession = {
    proc,
    browser,
    page,
    dataDir,
    pickFile,
    nextPick: (paths) => writeFileSync(pickFile, JSON.stringify(paths ? { paths } : { cancel: true })),
    close: async (graceful = true) => {
      if (graceful) {
        // Igual ao botão fechar da janela: o Windows envia WM_CLOSE (dispara o salvamento no fechamento).
        spawnSync("taskkill", ["/PID", String(proc.pid)]);
        await new Promise<void>((r) => {
          if (proc.exitCode !== null) return r();
          const t = setTimeout(() => {
            proc.kill();
            r();
          }, 8000);
          proc.once("exit", () => {
            clearTimeout(t);
            r();
          });
        });
      } else {
        proc.kill();
      }
      await browser.close().catch(() => undefined);
    },
  };
  return session;
}

export async function shot(page: Page, name: string): Promise<void> {
  mkdirSync(SHOTS, { recursive: true });
  await page.waitForTimeout(350);
  await page.screenshot({ path: join(SHOTS, `${name}.png`) });
}
