// Cópia recursiva simples. Evita `fs.cpSync`, que no Node 22 derruba o processo
// (0xC0000409) em caminhos do Windows com caracteres não ASCII, como "Área de Trabalho".
import { copyFileSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";

export function copyDir(from, to) {
  mkdirSync(to, { recursive: true });
  for (const entry of readdirSync(from, { withFileTypes: true })) {
    const src = join(from, entry.name);
    const dest = join(to, entry.name);
    if (entry.isDirectory()) copyDir(src, dest);
    else if (entry.isFile()) copyFileSync(src, dest);
  }
}
