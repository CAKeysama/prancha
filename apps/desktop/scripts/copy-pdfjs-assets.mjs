// Copia os recursos do PDF.js (cmaps, fontes padrão, wasm) para public/pdfjs,
// para que funcionem offline e dentro da política de segurança do app.
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { copyDir } from "./copy-dir.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
// Com os workspaces do npm, o pdfjs-dist fica no node_modules da raiz do repositório: o Node acha onde estiver.
const src = dirname(createRequire(import.meta.url).resolve("pdfjs-dist/package.json"));
const dest = join(root, "public", "pdfjs");
rmSync(dest, { recursive: true, force: true });
mkdirSync(dest, { recursive: true });
for (const dir of ["cmaps", "standard_fonts", "wasm", "iccs"]) {
  if (existsSync(join(src, dir))) copyDir(join(src, dir), join(dest, dir));
}
console.log("Recursos do PDF.js copiados para public/pdfjs");
