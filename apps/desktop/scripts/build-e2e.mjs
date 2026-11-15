// Compila o app em modo depuração (frontend embutido) e copia o executável para
// tests/.tmp/bin (na raiz do repositório), para que `cargo test` não o substitua por um binário de desenvolvimento.
import { execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const app = join(import.meta.dirname, "..");
const root = join(app, "..", "..");
execSync("npx tauri build --debug --no-bundle", { cwd: app, stdio: "inherit" });
execSync("cargo run -q -p prancha-core --example make_fixtures -- tests/.tmp/fixtures", { cwd: root, stdio: "inherit" });
const target = process.env.CARGO_TARGET_DIR ?? join(root, "target");
mkdirSync(join(root, "tests", ".tmp", "bin"), { recursive: true });
copyFileSync(join(target, "debug", "prancha.exe"), join(root, "tests", ".tmp", "bin", "prancha.exe"));
console.log("Executável E2E em tests/.tmp/bin/prancha.exe");
