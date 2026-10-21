// Monta a landing page em site/: copia instaladores, fontes, ícone e captura,
// e atualiza no HTML o tamanho e os SHA-256 dos instaladores a partir dos arquivos reais.
// Uso: npm run site:build  (depois de `npm run tauri build`)
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const site = join(root, "site");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const version = pkg.version;
const exe = `Prancha_${version}_x64-setup.exe`;
const msi = `Prancha_${version}_x64_pt-BR.msi`;

const target = process.env.CARGO_TARGET_DIR ?? join(root, "src-tauri", "target");
const sources = [join(target, "release", "bundle"), join(root, "artifacts", "installer")];
function findInstaller(name, sub) {
  for (const base of sources) {
    for (const p of [join(base, sub, name), join(base, name)]) if (existsSync(p)) return p;
  }
  throw new Error(`Instalador ${name} não encontrado. Rode "npm run tauri build".`);
}

for (const dir of ["downloads", "fonts", "assets"]) mkdirSync(join(site, dir), { recursive: true });
const copy = (from, to) => copyFileSync(join(root, from), join(site, to));
copyFileSync(findInstaller(exe, "nsis"), join(site, "downloads", exe));
copyFileSync(findInstaller(msi, "msi"), join(site, "downloads", msi));
for (const w of ["400", "600"]) {
  copy(`node_modules/@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-${w}-normal.woff2`, `fonts/ibm-plex-sans-${w}.woff2`);
}
copy("assets/app-icon.svg", "assets/favicon.svg");
// As imagens de vitrine podem ser trocadas à mão: só usa as capturas dos testes se ainda não existirem.
const copyIfMissing = (from, to) => existsSync(join(site, to)) || copy(from, to);
copyIfMissing("artifacts/screenshots/06-biblioteca-capas-e-progresso.png", "assets/biblioteca.png");
copyIfMissing("artifacts/screenshots/11b-biblioteca-tema-claro.png", "assets/biblioteca-claro.png");

const sha = (f) => createHash("sha256").update(readFileSync(f)).digest("hex");
const exeFile = join(site, "downloads", exe);
const msiFile = join(site, "downloads", msi);
const mb = (statSync(exeFile).size / 1024 / 1024).toLocaleString("pt-BR", { maximumFractionDigits: 1 });

let html = readFileSync(join(site, "index.html"), "utf8");
html = html
  .replace(/Prancha_[\d.]+_x64-setup\.exe/g, exe)
  .replace(/Prancha_[\d.]+_x64_pt-BR\.msi/g, msi)
  .replace(/Prancha [\d.]+ · Código aberto/, `Prancha ${version} · Código aberto`)
  .replace(/· [\d,]+ MB ·/, `· ${mb} MB ·`)
  .replace(/(EXE <code>)[0-9a-f]{64}/, `$1${sha(exeFile)}`)
  .replace(/(MSI <code>)[0-9a-f]{64}/, `$1${sha(msiFile)}`);
writeFileSync(join(site, "index.html"), html);
console.log(`site/ pronto: Prancha ${version}, instalador ${mb} MB`);
