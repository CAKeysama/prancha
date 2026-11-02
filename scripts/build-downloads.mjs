// Gera a página de downloads (site/downloads.html) e o manifesto (site/releases.json) como arquivos estáticos:
// a página não depende de nenhuma chamada no navegador, então continua útil mesmo com o GitHub fora do ar.
// As versões e as notas vêm do CHANGELOG.md; os arquivos, só das releases realmente publicadas.
// Uso: node scripts/build-downloads.mjs  (GITHUB_TOKEN é opcional: só evita o limite da API pública)
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { ROOT, assetLabel, longDate, parseSums, readChangelog } from "./changelog.mjs";

export const REPO = "CAKeysama/prancha";
const REPO_URL = `https://github.com/${REPO}`;
const SITE_URL = "https://prancha-beta.vercel.app/";
const NOTE_SECTIONS = ["Novidades", "Melhorias", "Correções"];

async function fetchReleases() {
  const headers = { Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" };
  if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  const res = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=100`, { headers });
  if (!res.ok) throw new Error(`A API do GitHub respondeu ${res.status} ao listar as releases.`);
  return (await res.json()).filter((r) => !r.draft && !r.prerelease);
}

async function fetchSums(release) {
  const asset = release.assets.find((a) => a.name === "SHA256SUMS.txt");
  if (!asset) return new Map();
  const res = await fetch(asset.browser_download_url);
  if (!res.ok) throw new Error(`Não foi possível ler ${asset.name} de ${release.tag_name} (${res.status}).`);
  return new Map(parseSums(await res.text()).map((f) => [f.name, f.sha256]));
}

/** Junta cada release publicada à sua entrada do changelog, da mais nova para a mais antiga. */
export async function buildManifest(changelog = readChangelog()) {
  const published = await fetchReleases();
  const releases = [];
  for (const entry of changelog) {
    const release = published.find((r) => r.tag_name === `v${entry.version}`);
    if (!release) continue;
    const sums = await fetchSums(release);
    const assets = release.assets
      .filter((a) => a.name.endsWith(".exe") || a.name.endsWith(".msi"))
      .sort((a, b) => Number(b.name.endsWith(".exe")) - Number(a.name.endsWith(".exe")))
      .map((a) => ({ name: a.name, label: assetLabel(a.name), size: a.size, sha256: sums.get(a.name) ?? null, url: a.browser_download_url }));
    releases.push({ ...entry, tag: release.tag_name, url: release.html_url, assets });
  }
  const orphan = published.find((r) => !releases.some((e) => e.tag === r.tag_name));
  if (orphan) throw new Error(`A release ${orphan.tag_name} não tem entrada no CHANGELOG.md.`);
  return { repository: REPO_URL, latest: releases[0]?.version ?? null, releases };
}

const escapeHtml = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** O changelog só usa negrito e código em linha: é tudo o que precisa virar HTML. */
const inline = (s) =>
  escapeHtml(s)
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/`(.+?)`/g, "<code>$1</code>");

export const megabytes = (bytes) => `${(bytes / 1024 / 1024).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} MB`;

const DOWNLOAD_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12" /><path d="m7 10 5 5 5-5" /><path d="M5 21h14" /></svg>';

function renderNotes(release) {
  return release.sections
    .filter((s) => NOTE_SECTIONS.includes(s.title))
    .map((s) => `<h3>${inline(s.title)}</h3>\n<ul>${s.items.map((item) => `<li>${inline(item)}</li>`).join("")}</ul>`)
    .join("\n");
}

function renderHashes(release) {
  const rows = release.assets.filter((a) => a.sha256).map((a) => `<p>${escapeHtml(a.name)}<br /><code>${a.sha256}</code></p>`);
  return rows.length ? `<details class="hashes"><summary>Verificar o download (SHA-256)</summary>${rows.join("")}</details>` : "";
}

function renderFiles(release) {
  if (!release.assets.length) return '<p class="muted">Esta versão não tem instaladores publicados.</p>';
  const links = release.assets.map((a) => `<li><a href="${escapeHtml(a.url)}">${escapeHtml(a.name)}</a> <span class="muted">${megabytes(a.size)}</span></li>`);
  return `<ul class="files">${links.join("")}</ul>`;
}

function renderLatest(release) {
  if (!release) {
    return `<section class="latest" aria-labelledby="versao-atual"><h2 id="versao-atual">Ainda não há versão publicada</h2><p class="muted">Quando a primeira versão sair, os instaladores aparecem aqui e em <a href="${REPO_URL}/releases">Releases</a>.</p></section>`;
  }
  const [primary, ...others] = release.assets;
  const buttons = [];
  if (primary) buttons.push(`<a class="button" href="${escapeHtml(primary.url)}">${DOWNLOAD_ICON} Baixar para Windows</a>`);
  for (const a of others) buttons.push(`<a class="button button-secondary" href="${escapeHtml(a.url)}">Baixar versão MSI</a>`);
  const size = primary ? ` · ${megabytes(primary.size)}` : "";
  return `<section class="latest" aria-labelledby="versao-atual">
          <p class="eyebrow">Versão estável mais recente</p>
          <h2 id="versao-atual">Prancha ${release.version}</h2>
          <p class="meta">${longDate(release.date)} · Windows 10 e 11 (64 bits)${size}</p>
          <div class="cta-actions">${buttons.join("")}</div>
          ${renderHashes(release)}
        </section>`;
}

function renderEntry(release, index) {
  const current = index === 0;
  return `<details class="entry${current ? " entry-current" : ""}"${current ? " open" : ""}>
          <summary><span class="entry-version">${release.version}${current ? ' <span class="tag">Atual</span>' : ""}</span><time datetime="${release.date}">${longDate(release.date)}</time></summary>
          <div class="entry-body">
            <p>${inline(release.summary)}</p>
            ${renderNotes(release)}
            <h3>Arquivos</h3>
            ${renderFiles(release)}
            ${current ? "" : renderHashes(release)}
            <p class="entry-link"><a href="${escapeHtml(release.url)}">Notas completas no GitHub</a></p>
          </div>
        </details>`;
}

export function renderPage(manifest) {
  const template = readFileSync(join(ROOT, "scripts", "downloads.template.html"), "utf8");
  const history = manifest.releases.map(renderEntry).join("\n        ") || '<p class="muted">Nenhuma versão publicada ainda.</p>';
  return template
    .replaceAll("{{SITE_URL}}", SITE_URL)
    .replaceAll("{{REPO_URL}}", REPO_URL)
    .replace("{{LATEST}}", () => renderLatest(manifest.releases[0]))
    .replace("{{HISTORY}}", () => history);
}

export async function buildDownloads() {
  const manifest = await buildManifest();
  writeFileSync(join(ROOT, "site", "releases.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync(join(ROOT, "site", "downloads.html"), renderPage(manifest));
  return manifest;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const manifest = await buildDownloads();
  console.log(`Página de downloads pronta: ${manifest.releases.length} versões, mais recente ${manifest.latest ?? "nenhuma"}.`);
}
