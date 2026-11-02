// O CHANGELOG.md é a fonte única das notas: daqui saem as releases do GitHub e a página de downloads.
// Uso: node scripts/changelog.mjs <versão> [SHA256SUMS.txt]  (escreve as notas da release na saída padrão)
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const MONTHS = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];

export const ROOT = join(import.meta.dirname, "..");

/** `2026-11-01` vira `1 de novembro de 2026`. */
export function longDate(iso) {
  const [year, month, day] = iso.split("-").map(Number);
  return `${day} de ${MONTHS[month - 1]} de ${year}`;
}

/** Cada item ocupa uma linha só; um sumário é o texto entre o título da versão e a primeira seção. */
export function parseChangelog(text) {
  const versions = [];
  let version;
  let section;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    const head = /^## \[(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)\] - (\d{4}-\d{2}-\d{2})$/.exec(line);
    if (head) {
      version = { version: head[1], date: head[2], summary: "", sections: [] };
      section = undefined;
      versions.push(version);
    } else if (version && line.startsWith("### ")) {
      section = { title: line.slice(4), items: [] };
      version.sections.push(section);
    } else if (version && section && line.startsWith("- ")) {
      section.items.push(line.slice(2));
    } else if (version && !section && line) {
      version.summary = `${version.summary} ${line}`.trim();
    }
  }
  return versions;
}

export function readChangelog() {
  return parseChangelog(readFileSync(join(ROOT, "CHANGELOG.md"), "utf8"));
}

/** Lê um SHA256SUMS.txt (`hash  nome` ou `hash *nome`) como uma lista de arquivos. */
export function parseSums(text) {
  return text
    .split(/\r?\n/)
    .map((line) => /^([0-9a-f]{64})\s+\*?(.+)$/.exec(line.trim()))
    .filter(Boolean)
    .map(([, sha256, name]) => ({ name, sha256 }));
}

export function assetLabel(name) {
  if (name.endsWith("-setup.exe")) return "Instalador para Windows (recomendado)";
  if (name.endsWith(".msi")) return "Pacote MSI para Windows";
  return name;
}

/** Notas em Markdown para a release: as seções do changelog e, quando há, os arquivos com seus SHA-256. */
export function releaseNotes(entry, files = []) {
  const parts = [`Lançada em ${longDate(entry.date)}.`, entry.summary];
  for (const { title, items } of entry.sections) {
    parts.push(`## ${title}`, items.map((item) => `- ${item}`).join("\n"));
  }
  if (files.length) {
    const rows = files.map(({ name, sha256 }) => `| ${assetLabel(name)} | \`${name}\` | \`${sha256}\` |`);
    parts.push("## Arquivos", ["| Tipo | Arquivo | SHA-256 |", "|---|---|---|", ...rows].join("\n"));
  }
  return `${parts.join("\n\n")}\n`;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [wanted, sumsFile] = process.argv.slice(2);
  const entry = readChangelog().find((e) => e.version === wanted);
  if (!entry) throw new Error(`A versão ${wanted} não está no CHANGELOG.md.`);
  process.stdout.write(releaseNotes(entry, sumsFile ? parseSums(readFileSync(sumsFile, "utf8")) : []));
}
