// Toda mudança na main sai como versão nova, e os commits dizem qual: `feat` sobe a minor, `!` ou `BREAKING CHANGE:` sobe a major, o resto sobe a patch.
// Uso:
//   node scripts/version.mjs check [base]   confere os commits desde a base (padrão: origin/main) e se a versão do branch é a esperada
//   node scripts/version.mjs bump [base]    sobe a versão nos cinco arquivos e abre a entrada no CHANGELOG.md
//   node scripts/version.mjs title "<título>"  confere um título de PR
//   node scripts/version.mjs released <ref> confere se a ref (um merge na main) trouxe versão nova em relação ao primeiro pai
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { ROOT, parseChangelog } from "./changelog.mjs";

export const TYPES = ["feat", "fix", "perf", "refactor", "style", "test", "docs", "build", "ci", "chore", "revert", "site"];
const LEVELS = ["patch", "minor", "major"];
const HEADER = new RegExp(`^(${TYPES.join("|")})(\\([^()\\s][^()]*\\))?(!)?: \\S`);

/** Lê a primeira linha e o corpo de um commit. Devolve `null` quando a primeira linha não segue o formato. */
export function parseCommit(message) {
  const [subject = "", ...rest] = message.replace(/\r\n/g, "\n").split("\n");
  const match = HEADER.exec(subject.trim());
  if (!match) return null;
  const body = rest.join("\n");
  const breaking = Boolean(match[3]) || /^BREAKING[ -]CHANGE: /m.test(body);
  return { type: match[1], breaking, level: breaking ? "major" : match[1] === "feat" ? "minor" : "patch" };
}

/** O maior nível entre os commits. */
export function bumpLevel(commits) {
  return commits.reduce((level, commit) => (LEVELS.indexOf(commit.level) > LEVELS.indexOf(level) ? commit.level : level), "patch");
}

export function nextVersion(current, level) {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(current);
  if (!match) throw new Error(`Versão fora do formato X.Y.Z: ${current}`);
  const [major, minor, patch] = match.slice(1).map(Number);
  if (level === "major") return `${major + 1}.0.0`;
  if (level === "minor") return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
}

/** As cinco cópias da versão, lidas de um conteúdo por arquivo. */
export function versionsIn(files) {
  const lock = JSON.parse(files["package-lock.json"]);
  return {
    "package.json": JSON.parse(files["package.json"]).version,
    "package-lock.json": lock.version,
    "package-lock.json (packages)": lock.packages?.[""]?.version,
    "src-tauri/tauri.conf.json": JSON.parse(files["src-tauri/tauri.conf.json"]).version,
    "src-tauri/Cargo.toml": /^version = "(.+)"/m.exec(files["src-tauri/Cargo.toml"])?.[1],
    "src-tauri/Cargo.lock": /\[\[package\]\]\nname = "prancha"\nversion = "(.+)"/.exec(files["src-tauri/Cargo.lock"].replace(/\r\n/g, "\n"))?.[1],
  };
}

/** Troca a versão nos cinco arquivos, sem mexer em mais nada. */
export function withVersion(files, raw, to) {
  const from = raw.replace(/\./g, "\\.");
  const swap = (text, pattern) => {
    const next = text.replace(pattern, (whole) => whole.replace(raw, to));
    if (next === text) throw new Error(`Não achei a versão ${raw} para trocar.`);
    return next;
  };
  return {
    "package.json": swap(files["package.json"], new RegExp(`"version": "${from}"`)),
    "package-lock.json": swap(swap(files["package-lock.json"], new RegExp(`"version": "${from}"`)), new RegExp(`("packages": \\{\\s*"": \\{[^}]*?)"version": "${from}"`)),
    "src-tauri/tauri.conf.json": swap(files["src-tauri/tauri.conf.json"], new RegExp(`"version": "${from}"`)),
    "src-tauri/Cargo.toml": swap(files["src-tauri/Cargo.toml"], new RegExp(`^version = "${from}"`, "m")),
    "src-tauri/Cargo.lock": swap(files["src-tauri/Cargo.lock"], new RegExp(`name = "prancha"\\r?\\nversion = "${from}"`)),
  };
}

/** Abre a entrada da versão no topo do changelog, com as seções para preencher. */
export function withChangelogEntry(text, version, date) {
  if (parseChangelog(text).some((entry) => entry.version === version)) return text;
  const entry = [`## [${version}] - ${date}`, "", "Resumo da versão em uma ou duas frases.", "", "### Novidades", "", "- ", "", "### Melhorias", "", "- ", "", "### Correções", "", "- ", "", ""].join("\n");
  const at = text.indexOf("\n## [");
  return at < 0 ? `${text.trimEnd()}\n\n${entry}` : `${text.slice(0, at + 1)}${entry}${text.slice(at + 1)}`;
}

/** Tudo o que a verificação do PR confere. Devolve a lista de problemas, vazia quando está tudo certo. */
export function review({ commits, baseVersion, headVersions, changelog, released = [] }) {
  const problems = [];
  const parsed = [];
  for (const { sha, message } of commits) {
    const commit = parseCommit(message);
    if (commit) parsed.push(commit);
    else problems.push(`O commit ${sha.slice(0, 7)} não segue o formato \`tipo: descrição\`: "${message.split("\n")[0]}". Tipos aceitos: ${TYPES.join(", ")}.`);
  }
  const level = bumpLevel(parsed);
  const expected = nextVersion(baseVersion, level);
  const versions = Object.entries(headVersions);
  const head = headVersions["package.json"];
  for (const [file, version] of versions) {
    if (version !== head) problems.push(`A versão em ${file} é ${version ?? "(não achei)"}, e a do package.json é ${head}.`);
  }
  if (!commits.length) problems.push("O PR não tem commits além da main.");
  else if (head !== expected) problems.push(`Pelos commits, a mudança é ${level}: a versão tem que ir de ${baseVersion} para ${expected}, e está em ${head}. Rode \`npm run version:bump\`.`);
  if (released.includes(head)) problems.push(`A ${head} já foi publicada. Traga a main para o branch e rode \`npm run version:bump\` de novo.`);
  const entry = parseChangelog(changelog).find((e) => e.version === head);
  if (!entry) problems.push(`O CHANGELOG.md não tem a entrada \`## [${head}] - AAAA-MM-DD\`.`);
  else if (parseChangelog(changelog)[0].version !== head) problems.push(`A entrada da ${head} tem que ser a primeira do CHANGELOG.md.`);
  else if (!entry.sections.some((s) => s.items.some((item) => item.trim()))) problems.push(`A entrada da ${head} no CHANGELOG.md está sem itens.`);
  else if (entry.sections.some((s) => s.items.some((item) => !item.trim())) || entry.summary === "Resumo da versão em uma ou duas frases.") problems.push(`A entrada da ${head} no CHANGELOG.md ainda tem partes do modelo para preencher.`);
  return { problems, level, expected };
}

const FILES = ["package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock"];
const git = (...args) => execFileSync("git", args, { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });

/** Commits do branch que não estão na base, sem os merges. */
function commitsSince(base) {
  const out = git("log", "--no-merges", "--format=%H%x00%B%x1e", `${base}..HEAD`);
  return out
    .split("\x1e")
    .map((chunk) => chunk.trim())
    .filter(Boolean)
    .map((chunk) => {
      const [sha, message] = chunk.split("\x00");
      return { sha, message: message.trim() };
    });
}

const readAt = (ref) => Object.fromEntries(FILES.map((file) => [file, git("show", `${ref}:${file}`)]));
const readHere = () => Object.fromEntries(FILES.map((file) => [file, readFileSync(join(ROOT, file), "utf8")]));
const released = () =>
  git("tag", "--list", "v*")
    .split("\n")
    .map((tag) => tag.trim().replace(/^v/, ""))
    .filter(Boolean);

function main([command, arg]) {
  if (command === "title") {
    const commit = parseCommit(arg ?? "");
    if (!commit) {
      console.error(`::error::O título do PR não segue o formato \`tipo: descrição\`. Tipos aceitos: ${TYPES.join(", ")}.`);
      process.exit(1);
    }
    console.log(`Título ok: ${commit.level}.`);
    return;
  }
  if (command === "released") {
    const before = versionsIn(readAt(`${arg}^1`))["package.json"];
    const after = versionsIn(readAt(arg))["package.json"];
    if (before === after) {
      console.error(`::error::Esta mudança entrou na main sem versão nova (continua ${after}), então não vai ser publicada. Abra um PR com \`npm run version:bump\` e as notas no CHANGELOG.md.`);
      process.exit(1);
    }
    console.log(`Versão nova: ${before} → ${after}.`);
    return;
  }
  const base = arg ?? "origin/main";
  const baseVersion = versionsIn(readAt(base))["package.json"];
  const commits = commitsSince(base);
  if (command === "bump") {
    const parsed = commits.map((c) => parseCommit(c.message)).filter(Boolean);
    const target = nextVersion(baseVersion, bumpLevel(parsed));
    const here = readHere();
    const current = versionsIn(here)["package.json"];
    if (current !== target) {
      const next = withVersion(here, current, target);
      for (const file of FILES) writeFileSync(join(ROOT, file), next[file]);
    }
    const changelogPath = join(ROOT, "CHANGELOG.md");
    const changelog = readFileSync(changelogPath, "utf8");
    // Uma entrada aberta para a versão antiga deste branch vira a da versão nova.
    const inBase = parseChangelog(git("show", `${base}:CHANGELOG.md`)).some((e) => e.version === current);
    const renamed = current !== target && !inBase ? changelog.replace(`## [${current}] - `, `## [${target}] - `) : changelog;
    writeFileSync(changelogPath, withChangelogEntry(renamed, target, new Date().toISOString().slice(0, 10)));
    console.log(`${baseVersion} → ${target} (${bumpLevel(parsed)}). Escreva as notas no CHANGELOG.md.`);
    return;
  }
  if (command === "check") {
    const { problems, level, expected } = review({ commits, baseVersion, headVersions: versionsIn(readHere()), changelog: readFileSync(join(ROOT, "CHANGELOG.md"), "utf8"), released: released() });
    for (const problem of problems) console.error(`::error::${problem}`);
    if (problems.length) process.exit(1);
    console.log(`Versão ok: ${baseVersion} → ${expected} (${level}), ${commits.length} commit(s).`);
    return;
  }
  console.error("Uso: node scripts/version.mjs check|bump [base] | title <título> | released <ref>");
  process.exit(2);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main(process.argv.slice(2));
