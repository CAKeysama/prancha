// Compila o app em modo depuração (frontend embutido) e copia o executável para
// tests/.tmp/bin, para que `cargo test` não o substitua por um binário de desenvolvimento.
import { execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
execSync("npx tauri build --debug --no-bundle", { cwd: root, stdio: "inherit" });
execSync("cargo run -q --manifest-path src-tauri/Cargo.toml --example make_fixtures -- tests/.tmp/fixtures", { cwd: root, stdio: "inherit" });
const target = process.env.CARGO_TARGET_DIR ?? join(root, "src-tauri", "target");
mkdirSync(join(root, "tests", ".tmp", "bin"), { recursive: true });
copyFileSync(join(target, "debug", "prancha.exe"), join(root, "tests", ".tmp", "bin", "prancha.exe"));
console.log("Executável E2E em tests/.tmp/bin/prancha.exe");
