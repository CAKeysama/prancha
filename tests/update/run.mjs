// Atualização de verdade, entre duas versões instaladas, conduzida pela interface. Só roda localmente.
// Uso: npm run test:update            (reaproveita os instaladores já gerados)
//      npm run test:update -- --build (compila de novo as duas versões; use depois de mudar o código)
//
// Os instaladores são reais e assinados, mas com uma chave descartável e um servidor local no lugar do GitHub.
// Nome, identificador e executável são outros: o Prancha instalado e os dados dele não são tocados.
import { execSync, spawn, spawnSync } from "node:child_process";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { homedir } from "node:os";
import { join } from "node:path";
import { chromium } from "@playwright/test";
import { buildManifest, folderReader, packageBaseUrl } from "../../scripts/updater-manifest.mjs";

const ROOT = join(import.meta.dirname, "..", "..");
const WORK = join(homedir(), ".cache", "prancha-update-test");
const INSTALLERS = join(WORK, "installers");
const INSTALL = join(WORK, "install");
const SHOTS = join(ROOT, "tests", ".tmp", "update-shots");
const COMIC = join(ROOT, "tests", "real", "Paginas Reais.pdf");

const PORT = 18765;
const PRODUCT = "PranchaTesteUpdate";
const IDENTIFIER = "app.prancha.updatetest";
const BINARY = "prancha-teste-update";
const EXE = join(INSTALL, `${BINARY}.exe`);
const OLD = "9.8.0";
const NEW = "9.8.1";
const installerName = (version) => `${PRODUCT}_${version}_x64-setup.exe`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function log(message) {
  console.log(`  ${message}`);
}

// ---------------------------------------------------------------------------
// Instaladores
// ---------------------------------------------------------------------------

function buildInstallers() {
  rmSync(INSTALLERS, { recursive: true, force: true });
  mkdirSync(INSTALLERS, { recursive: true });
  const key = join(WORK, "descartavel.key");
  const password = randomBytes(16).toString("hex");
  rmSync(key, { force: true });
  rmSync(`${key}.pub`, { force: true });
  execSync(`npx tauri signer generate --ci -w "${key}" -p ${password}`, { cwd: ROOT, stdio: "ignore" });
  const target = join(WORK, "target");
  for (const version of [OLD, NEW]) {
    const overlay = join(WORK, `config-${version}.json`);
    writeFileSync(
      overlay,
      JSON.stringify({
        version,
        productName: PRODUCT,
        identifier: IDENTIFIER,
        // O instalador encerra todo processo com o nome do executável: precisa ser outro nome.
        mainBinaryName: BINARY,
        bundle: { targets: ["nsis"], createUpdaterArtifacts: true },
        plugins: {
          updater: {
            pubkey: readFileSync(`${key}.pub`, "utf8").trim(),
            endpoints: [`http://127.0.0.1:${PORT}/releases/latest/download/latest.json`],
            dangerousInsecureTransportProtocol: true,
            requireSignedVersion: true,
            windows: { installMode: "passive" },
          },
        },
      }),
    );
    log(`compilando ${version}…`);
    execSync(`npx tauri build --config "${overlay}"`, {
      cwd: ROOT,
      stdio: ["ignore", "ignore", "inherit"],
      env: {
        ...process.env,
        CARGO_TARGET_DIR: target,
        CARGO_PROFILE_RELEASE_LTO: "off",
        CARGO_PROFILE_RELEASE_OPT_LEVEL: "1",
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS: "16",
        TAURI_SIGNING_PRIVATE_KEY: key,
        TAURI_SIGNING_PRIVATE_KEY_PASSWORD: password,
      },
    });
    const built = join(target, "release", "bundle", "nsis", installerName(version));
    copyFileSync(built, join(INSTALLERS, installerName(version)));
    copyFileSync(`${built}.sig`, join(INSTALLERS, `${installerName(version)}.sig`));
  }
}

// ---------------------------------------------------------------------------
// Servidor no lugar do GitHub
// ---------------------------------------------------------------------------

const NOTES = `Lançada em 1 de novembro de 2026.

Resumo da versão de teste do atualizador.

## Novidades

- Novidade da versão de teste

## Melhorias

- Melhoria da versão de teste

## Correções

- Correção da versão de teste
`;

let mode = "none";

function manifestFor(version) {
  const manifest = buildManifest({
    version,
    notes: NOTES,
    date: "2026-11-01",
    read: folderReader(INSTALLERS),
    baseUrl: packageBaseUrl(`http://127.0.0.1:${PORT}/releases/latest/download/latest.json`, version),
    nsis: installerName(version),
  });
  if (mode === "wrong-sha") {
    for (const platform of Object.values(manifest.platforms)) platform.sha256 = "0".repeat(64);
  }
  if (mode === "foreign") {
    for (const platform of Object.values(manifest.platforms)) platform.url = `http://127.0.0.1:${PORT}/outro/${installerName(version)}`;
  }
  if (mode === "no-package") manifest.platforms = { "linux-x86_64": manifest.platforms["windows-x86_64"] };
  return manifest;
}

function servePackage(res, file) {
  const bytes = readFileSync(file);
  const half = bytes.subarray(0, Math.floor(bytes.length / 2));
  res.setHeader("Content-Type", "application/octet-stream");
  if (mode === "tampered") {
    const copy = Buffer.from(bytes);
    copy[Math.floor(copy.length / 2)] ^= 0xff;
    res.setHeader("Content-Length", copy.length);
    res.end(copy);
  } else if (mode === "short") {
    res.setHeader("Content-Length", half.length);
    res.end(half);
  } else if (mode === "cut") {
    res.setHeader("Content-Length", bytes.length);
    res.write(half, () => setTimeout(() => res.destroy(), 100));
  } else if (mode === "slow") {
    res.setHeader("Content-Length", bytes.length);
    let sent = 0;
    const timer = setInterval(() => {
      if (res.destroyed || sent >= bytes.length) return clearInterval(timer);
      res.write(bytes.subarray(sent, sent + 256 * 1024));
      sent += 256 * 1024;
    }, 250);
    res.once("close", () => clearInterval(timer));
  } else {
    res.setHeader("Content-Length", bytes.length);
    res.end(bytes);
  }
}

const server = createServer((req, res) => {
  const url = req.url ?? "";
  if (mode === "down") return void req.socket.destroy();
  if (mode === "hang") return;
  if (mode === "error") {
    res.statusCode = 500;
    return void res.end("erro");
  }
  if (url === "/releases/latest/download/latest.json") {
    res.setHeader("Content-Type", "application/json");
    return void res.end(JSON.stringify(manifestFor(mode === "none" ? OLD : NEW)));
  }
  if (url === "/releases/api") {
    res.setHeader("Content-Type", "application/json");
    const release = (version) => ({ tag_name: `v${version}`, name: `Prancha v${version}`, body: NOTES, published_at: "2026-11-01T12:00:00Z", html_url: `http://127.0.0.1:${PORT}/tag/v${version}`, draft: false, prerelease: false });
    return void res.end(JSON.stringify([release(NEW), release(OLD)]));
  }
  const pkg = /^\/releases\/download\/v[\d.]+\/([^/]+)$/.exec(url);
  if (pkg && existsSync(join(INSTALLERS, pkg[1]))) return servePackage(res, join(INSTALLERS, pkg[1]));
  res.statusCode = 404;
  res.end();
});

// ---------------------------------------------------------------------------
// Aplicativo instalado
// ---------------------------------------------------------------------------

function installedVersion() {
  const script = `(Get-Item '${EXE}').VersionInfo.ProductVersion`;
  return spawnSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).stdout.trim();
}

function running() {
  const out = spawnSync("tasklist", ["/FI", `IMAGENAME eq ${BINARY}.exe`, "/FO", "CSV", "/NH"], { encoding: "utf8" }).stdout;
  return out.includes(`${BINARY}.exe`);
}

async function stopAll() {
  spawnSync("taskkill", ["/IM", `${BINARY}.exe`], { stdio: "ignore" });
  for (let i = 0; i < 40 && running(); i++) await sleep(250);
  if (running()) spawnSync("taskkill", ["/F", "/IM", `${BINARY}.exe`], { stdio: "ignore" });
  for (let i = 0; i < 40 && running(); i++) await sleep(250);
}

async function uninstall() {
  await stopAll();
  const uninstaller = join(INSTALL, "uninstall.exe");
  if (existsSync(uninstaller)) {
    spawnSync(uninstaller, ["/S"], { stdio: "ignore" });
    for (let i = 0; i < 80 && existsSync(EXE); i++) await sleep(250);
  }
  rmSync(INSTALL, { recursive: true, force: true });
  // O desinstalador deixa esta chave, que guarda a pasta de instalação.
  spawnSync("reg", ["delete", `HKCU\\Software\\Prancha\\${PRODUCT}`, "/f"], { stdio: "ignore" });
  rmSync(join(process.env.APPDATA, IDENTIFIER), { recursive: true, force: true });
  rmSync(join(process.env.LOCALAPPDATA, IDENTIFIER), { recursive: true, force: true });
}

let port = 9850;

async function launch() {
  const debugPort = port++;
  const proc = spawn(EXE, [], { env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort}` }, stdio: "ignore" });
  let browser;
  for (let i = 0; i < 120 && !browser; i++) {
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${debugPort}`).catch(() => undefined);
    if (!browser) await sleep(250);
  }
  assert.ok(browser, "o WebView2 não abriu a porta de depuração");
  let page;
  for (let i = 0; i < 80 && !page; i++) {
    page = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().startsWith("devtools"));
    if (!page) await sleep(250);
  }
  assert.ok(page, "página do aplicativo não encontrada");
  await page.getByRole("navigation", { name: "Navegação principal" }).or(page.getByRole("dialog")).first().waitFor({ timeout: 30_000 });
  const close = async () => {
    await browser.close().catch(() => undefined);
    await stopAll();
  };
  return { proc, browser, page, close };
}

const invoke = (page, cmd, args = {}) => page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

async function shot(page, name) {
  mkdirSync(SHOTS, { recursive: true });
  await sleep(250);
  await page.screenshot({ path: join(SHOTS, `${name}.png`) });
}

const offerOf = (page) => page.getByRole("dialog", { name: "Uma nova versão do Prancha está disponível!" });
const failedOf = (page) => page.getByRole("dialog", { name: "Não foi possível atualizar" });
const welcomeOf = (page) => page.getByRole("dialog", { name: "O Prancha está atualizado!" });
const indicatorOf = (page, version = NEW) => page.getByRole("button", { name: `v${version} disponível` });
const button = (page, name) => page.getByRole("button", { name, exact: true });

/** Dá tempo para a verificação em segundo plano responder. */
const settle = () => sleep(2500);

async function expectFailure(page, text) {
  await indicatorOf(page).click();
  await button(page, "Atualizar agora").click();
  await failedOf(page).getByText(text).waitFor({ timeout: 90_000 });
  assert.equal(installedVersion(), OLD);
  await button(page, "Depois").click();
  await failedOf(page).waitFor({ state: "detached" });
  assert.ok(await indicatorOf(page).isVisible(), "o indicador precisa continuar depois da falha");
}

// ---------------------------------------------------------------------------
// Roteiro
// ---------------------------------------------------------------------------

async function scenario() {
  // Sem atualização: nada aparece.
  mode = "none";
  let app = await launch();
  await invoke(app.page, "set_setting", { key: "onboarding", value: { status: "skipped", step: 1, comicId: null } });
  // O convite da telemetria abre 4 s depois da abertura nos builds de distribuição: recusado aqui, só os diálogos da atualização aparecem.
  await invoke(app.page, "telemetry_set_consent", { granted: false });
  await invoke(app.page, "set_setting", { key: "teste.preferencia", value: "preservada" });
  await invoke(app.page, "start_import", { paths: [COMIC] });
  let comic;
  for (let i = 0; i < 120 && !comic; i++) {
    comic = (await invoke(app.page, "get_home")).recentlyAdded[0];
    if (!comic) await sleep(250);
  }
  assert.ok(comic, "a importação não terminou");
  await invoke(app.page, "save_progress", { id: comic.id, pageIndex: 1, pageCount: 2, scrollRatio: 0.5 });
  await invoke(app.page, "set_favorite", { id: comic.id, favorite: true });
  await app.close();

  app = await launch();
  await settle();
  assert.equal(await offerOf(app.page).count(), 0);
  assert.equal(await app.page.getByText("disponível").count(), 0);
  log("ok  sem versão nova, nada aparece");
  await app.close();

  // Servidor pendurado: a tela abre antes do prazo da consulta, e nada aparece depois dele.
  mode = "hang";
  const hangStart = Date.now();
  app = await launch();
  assert.ok(Date.now() - hangStart < 8000, "a abertura não pode esperar a consulta");
  await sleep(11_000);
  assert.equal(await app.page.getByRole("dialog").count(), 0);
  log("ok  consulta sem resposta não segura a abertura e não mostra erro");
  await app.close();

  // Sem conexão e erro da API: silêncio.
  for (const failure of ["down", "error"]) {
    mode = failure;
    app = await launch();
    await settle();
    assert.equal(await app.page.getByRole("dialog").count(), 0);
    log(`ok  ${failure === "down" ? "sem conexão" : "erro do servidor"}: abre normal e sem alerta`);
    await app.close();
  }

  // Release sem pacote para o sistema, ou com pacote fora das releases oficiais: não é oferecida.
  for (const unusable of ["no-package", "foreign"]) {
    mode = unusable;
    app = await launch();
    await settle();
    assert.equal(await offerOf(app.page).count(), 0);
    assert.equal(await indicatorOf(app.page).count(), 0);
    log(`ok  ${unusable === "foreign" ? "pacote fora das releases oficiais" : "release sem pacote para o sistema"}: nada é oferecido`);
    await app.close();
  }

  // Versão nova: janela, Depois, indicador e reabertura.
  mode = "normal";
  app = await launch();
  await offerOf(app.page).waitFor({ timeout: 15_000 });
  const text = await offerOf(app.page).innerText();
  for (const expected of [`v${OLD}`, `v${NEW}`, "Preparamos melhorias e novidades para sua experiência de leitura.", "Resumo da versão de teste do atualizador."]) {
    assert.ok(text.includes(expected), `falta na janela: ${expected}`);
  }
  await shot(app.page, "1-nova-versao");
  await button(app.page, "Depois").click();
  await offerOf(app.page).waitFor({ state: "detached" });
  await indicatorOf(app.page).waitFor();
  await shot(app.page, "2-indicador");
  assert.equal(installedVersion(), OLD);
  log("ok  janela mostra as versões e as notas; Depois fecha e deixa o indicador");

  await app.page.getByRole("button", { name: "Minha biblioteca" }).click();
  await settle();
  assert.equal(await offerOf(app.page).count(), 0);
  await indicatorOf(app.page).click();
  await offerOf(app.page).waitFor();
  await button(app.page, "Depois").click();
  log("ok  navegar não reabre; o indicador reabre");
  await app.close();

  app = await launch();
  await indicatorOf(app.page).waitFor({ timeout: 15_000 });
  await settle();
  assert.equal(await offerOf(app.page).count(), 0);
  log("ok  ao abrir de novo, a versão recusada fica só no indicador");

  // Falhas de download e de conferência: nada é instalado, e o aplicativo segue funcionando.
  mode = "tampered";
  await expectFailure(app.page, "verificação de segurança");
  log("ok  pacote adulterado é recusado pela assinatura");
  mode = "short";
  await expectFailure(app.page, "verificação de segurança");
  log("ok  pacote incompleto é recusado");
  mode = "cut";
  await expectFailure(app.page, "Não foi possível falar com o GitHub");
  log("ok  download interrompido vira mensagem clara");
  mode = "wrong-sha";
  await invoke(app.page, "updates_check");
  await expectFailure(app.page, "não confere com o publicado");
  log("ok  SHA-256 diferente do anunciado é recusado");

  mode = "slow";
  await invoke(app.page, "updates_check");
  await indicatorOf(app.page).click();
  await button(app.page, "Atualizar agora").click();
  await app.page.getByText(/^[1-9]\d*%$/).waitFor({ timeout: 30_000 });
  await shot(app.page, "3-progresso");
  await button(app.page, "Cancelar").click();
  await app.page.getByRole("dialog").waitFor({ state: "detached" });
  assert.equal(installedVersion(), OLD);
  await indicatorOf(app.page).waitFor();
  log("ok  progresso real com percentual; cancelar não instala nada");

  // Sem conexão na hora de atualizar, depois de reabrir: falha clara e nova tentativa.
  mode = "slow";
  await invoke(app.page, "updates_check");
  await indicatorOf(app.page).click();
  await button(app.page, "Atualizar agora").click();
  await app.page.getByText(/^[1-9]\d*%$/).waitFor({ timeout: 30_000 });
  await app.close();
  assert.equal(installedVersion(), OLD);
  log("ok  fechar durante o download não instala nada");

  mode = "down";
  app = await launch();
  await indicatorOf(app.page).waitFor({ timeout: 15_000 });
  await indicatorOf(app.page).click();
  await button(app.page, "Atualizar agora").click();
  await failedOf(app.page).waitFor({ timeout: 30_000 });
  await shot(app.page, "4-falha");

  // Atualização real, pelo botão de tentar de novo.
  mode = "normal";
  const exited = new Promise((resolve) => app.proc.once("exit", resolve));
  await button(app.page, "Tentar novamente").click();
  await exited;
  await app.browser.close().catch(() => undefined);
  log("ok  o aplicativo fecha para o instalador rodar");
  let relaunched = false;
  for (let i = 0; i < 240 && !(relaunched = running() && installedVersion() === NEW); i++) await sleep(500);
  assert.ok(relaunched, "o instalador não abriu a nova versão");
  log(`ok  o instalador trocou o executável por ${installedVersion()} e abriu a nova versão sozinho`);
  await sleep(4000);
  await stopAll();

  // Novidades: mesmo sem internet, com as notas guardadas antes do download.
  mode = "down";
  app = await launch();
  await welcomeOf(app.page).waitFor({ timeout: 15_000 });
  const welcome = await welcomeOf(app.page).innerText();
  for (const expected of [
    `Você está utilizando a versão v${NEW}.`,
    "Obrigado por manter o Prancha atualizado e fazer parte da nossa evolução!",
    "Resumo da versão de teste do atualizador.",
    "Novidade da versão de teste",
    "Melhoria da versão de teste",
    "Correção da versão de teste",
  ]) {
    assert.ok(welcome.includes(expected), `falta nas novidades: ${expected}`);
  }
  await shot(app.page, "5-novidades");
  log("ok  janela de novidades com agradecimento e notas guardadas, sem internet");
  await app.close();

  app = await launch();
  await welcomeOf(app.page).waitFor({ timeout: 15_000 });
  log("ok  fechou sem ver: as novidades aparecem na próxima abertura");
  await button(app.page, "Começar a ler").click();
  await welcomeOf(app.page).waitFor({ state: "detached" });
  await app.close();

  mode = "none";
  app = await launch();
  await settle();
  assert.equal(await app.page.getByRole("dialog").count(), 0);
  assert.equal(await app.page.getByText("disponível").count(), 0);
  log("ok  as novidades não se repetem e o indicador sumiu");

  const home = await invoke(app.page, "get_home");
  const kept = home.recentlyAdded[0];
  assert.equal(home.total, 1);
  assert.equal(kept.progress.pageIndex, 1);
  assert.equal(kept.progress.scrollRatio, 0.5);
  assert.equal(kept.favorite, true);
  const settings = await invoke(app.page, "get_settings");
  assert.equal(settings["teste.preferencia"], "preservada");
  assert.equal(settings.onboarding.status, "skipped");
  assert.equal(settings["updates.state"].installedVersion, NEW);
  assert.equal(settings["updates.state"].notesShownVersion, NEW);
  log("ok  biblioteca, progresso, favorito e preferências preservados");

  // Instalador que não chegou ao fim: a próxima abertura avisa e não diz que atualizou.
  await invoke(app.page, "set_setting", { key: "updates.state", value: { ...settings["updates.state"], pendingVersion: "9.8.2" } });
  await app.close();
  app = await launch();
  await failedOf(app.page).getByText("A instalação da versão v9.8.2 não foi concluída.").waitFor({ timeout: 15_000 });
  assert.equal(await welcomeOf(app.page).count(), 0);
  await button(app.page, "Depois").click();
  await app.close();
  log("ok  instalação que não terminou é informada na abertura seguinte");
}

// ---------------------------------------------------------------------------

async function main() {
  assert.ok(existsSync(COMIC), `Quadrinho de teste não encontrado: ${COMIC}`);
  mkdirSync(WORK, { recursive: true });
  if (process.argv.includes("--build") || !existsSync(join(INSTALLERS, installerName(NEW)))) buildInstallers();

  const realInstall = join(process.env.LOCALAPPDATA, "Prancha", "prancha.exe");
  const realBefore = existsSync(realInstall) ? statSync(realInstall).mtimeMs : null;

  await new Promise((resolve) => server.listen(PORT, "127.0.0.1", resolve));
  await uninstall();
  try {
    log(`instalando ${OLD}…`);
    spawnSync(join(INSTALLERS, installerName(OLD)), ["/S", `/D=${INSTALL}`], { windowsVerbatimArguments: true, stdio: "ignore" });
    assert.equal(installedVersion(), OLD);
    await scenario();
  } finally {
    await uninstall();
    server.closeAllConnections();
    server.close();
  }
  assert.ok(!existsSync(EXE), "a desinstalação do teste falhou");
  if (realBefore !== null) assert.equal(statSync(realInstall).mtimeMs, realBefore, "o Prancha real foi alterado");
  log("ok  instalação de teste removida; o Prancha real não foi tocado");
  console.log("\nAtualização real validada.");
}

main().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
