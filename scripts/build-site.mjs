// Monta o site em site/: fontes, ícone, capturas e a página de downloads,
// e atualiza no HTML os links, o tamanho e os SHA-256 a partir da release publicada no GitHub.
// Uso: npm run site:build  (depois de publicar a release da versão atual)
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { buildDownloads, megabytes } from "./build-downloads.mjs";
import { ROOT } from "./changelog.mjs";

const site = join(ROOT, "site");
const { version } = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));

const manifest = await buildDownloads();
const release = manifest.releases[0];
if (release?.version !== version) {
  throw new Error(`A versão ${version} ainda não foi publicada: a release mais recente é ${release?.version ?? "nenhuma"}.`);
}
const exe = release.assets.find((a) => a.name.endsWith("-setup.exe"));
const msi = release.assets.find((a) => a.name.endsWith(".msi"));
if (!exe?.sha256 || !msi?.sha256) throw new Error(`A release ${release.tag} está sem instaladores ou sem SHA256SUMS.txt.`);

for (const dir of ["fonts", "assets"]) mkdirSync(join(site, dir), { recursive: true });
const copy = (from, to) => copyFileSync(join(ROOT, from), join(site, to));
for (const w of ["400", "600"]) {
  copy(`node_modules/@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-${w}-normal.woff2`, `fonts/ibm-plex-sans-${w}.woff2`);
}
copy("assets/app-icon.svg", "assets/favicon.svg");
// As imagens de vitrine mostram obras de terceiros, então ficam fora do git, em site/assets/prints.
if (!existsSync(join(site, "assets", "prints", "inicio-dark-2000.webp"))) console.warn("Sem as imagens de vitrine em site/assets/prints: a página sai sem elas.");

let html = readFileSync(join(site, "index.html"), "utf8");
html = html
  .replace(/href="[^"]*Prancha_[\d.]+_x64-setup\.exe"/g, `href="${exe.url}"`)
  .replace(/href="[^"]*Prancha_[\d.]+_x64_pt-BR\.msi"/g, `href="${msi.url}"`)
  .replace(/Prancha [\d.]+ · Código aberto/, `Prancha ${version} · Código aberto`)
  .replace(/· [\d,]+ MB ·/, `· ${megabytes(exe.size)} ·`)
  .replace(/(EXE <code>)[0-9a-f]{64}/, `$1${exe.sha256}`)
  .replace(/(MSI <code>)[0-9a-f]{64}/, `$1${msi.sha256}`);
writeFileSync(join(site, "index.html"), html);
console.log(`site/ pronto: Prancha ${version}, instalador ${megabytes(exe.size)}`);
