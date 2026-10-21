// Gera src/generated/licenses.json com as dependências distribuídas no aplicativo
// (crates Rust de produção e pacotes npm de produção) e o aviso de licença do UnRAR.
// Uso: npm run licenses
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function rustDeps() {
  const out = execFileSync(
    "cargo",
    ["tree", "--manifest-path", join(root, "src-tauri", "Cargo.toml"), "-e", "normal", "--prefix", "none", "-f", "{p}|{l}"],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  const seen = new Map();
  for (const line of out.split(/\r?\n/)) {
    const [pkg, license] = line.split("|");
    if (!pkg) continue;
    const m = pkg.trim().match(/^(\S+) v(\S+)/);
    if (!m || m[1] === "prancha") continue;
    const lic = (license ?? "").replace(/\(\*\)\s*$/, "").trim();
    seen.set(`${m[1]}@${m[2]}`, { name: m[1], version: m[2], license: lic || "desconhecida" });
  }
  return [...seen.values()].sort((a, b) => a.name.localeCompare(b.name));
}

function jsDeps() {
  const out = execFileSync("npm", ["ls", "--omit=dev", "--all", "--json", "--long"], { cwd: root, encoding: "utf8", shell: true, maxBuffer: 64 * 1024 * 1024 });
  const tree = JSON.parse(out);
  const seen = new Map();
  const walk = (deps) => {
    for (const [name, info] of Object.entries(deps ?? {})) {
      const key = `${name}@${info.version}`;
      if (seen.has(key) || !info.version) continue;
      let license = typeof info.license === "string" ? info.license : "desconhecida";
      const pj = join(info.path ?? join(root, "node_modules", name), "package.json");
      if (license === "desconhecida" && existsSync(pj)) {
        const p = JSON.parse(readFileSync(pj, "utf8"));
        license = typeof p.license === "string" ? p.license : (p.license?.type ?? license);
      }
      seen.set(key, { name, version: info.version, license });
      walk(info.dependencies);
    }
  };
  walk(tree.dependencies);
  return [...seen.values()].sort((a, b) => a.name.localeCompare(b.name));
}

function unrarLicense() {
  const registry = join(process.env.CARGO_HOME ?? join(homedir(), ".cargo"), "registry", "src");
  for (const index of existsSync(registry) ? readdirSync(registry) : []) {
    const dir = join(registry, index);
    const sys = readdirSync(dir).filter((d) => d.startsWith("unrar_sys-")).sort().pop();
    if (sys) {
      const file = join(dir, sys, "vendor", "unrar", "license.txt");
      if (existsSync(file)) return readFileSync(file, "latin1").replace(/\r\n/g, "\n").trim();
    }
  }
  return "Texto da licença do UnRAR não encontrado. Consulte https://www.rarlab.com/license.htm";
}

const data = { generatedAt: new Date().toISOString(), rust: rustDeps(), js: jsDeps(), unrar: unrarLicense() };
mkdirSync(join(root, "src", "generated"), { recursive: true });
writeFileSync(join(root, "src", "generated", "licenses.json"), JSON.stringify(data, null, 1));
console.log(`licenses.json: ${data.rust.length} crates, ${data.js.length} pacotes npm`);
