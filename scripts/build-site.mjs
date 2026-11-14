// Monta o site em site/: ícone, capturas e a página de downloads,
// e atualiza no HTML a versão, os links, o tamanho e os SHA-256 a partir da release mais recente publicada no GitHub.
// Uso: npm run site:build  (o pages.yml roda sozinho depois de cada versão e de cada mudança no site)
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { buildDownloads, megabytes } from "./build-downloads.mjs";
import { ROOT } from "./changelog.mjs";

const site = join(ROOT, "site");

// A versão vem da release, não do package.json: entre o merge de uma versão nova e a release dela, o site segue na anterior.
const manifest = await buildDownloads();
const release = manifest.releases[0];
if (!release) throw new Error("Nenhuma versão publicada ainda.");
const { version } = release;
const exe = release.assets.find((a) => a.name.endsWith("-setup.exe"));
const msi = release.assets.find((a) => a.name.endsWith(".msi"));
if (!exe?.sha256 || !msi?.sha256) throw new Error(`A release ${release.tag} está sem instaladores ou sem SHA256SUMS.txt.`);

// As fontes do site (IBM Plex Sans 400 e 600) ficam no git, em site/fonts: o app usa fontes variáveis e não traz mais o pacote estático.
mkdirSync(join(site, "assets"), { recursive: true });
copyFileSync(join(ROOT, "assets", "app-icon.svg"), join(site, "assets", "favicon.svg"));
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
