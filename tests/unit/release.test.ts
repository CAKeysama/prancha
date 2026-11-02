import { describe, expect, it } from "vitest";
// @ts-expect-error script em JS puro, sem tipos
import { longDate, parseChangelog, parseSums, releaseNotes } from "../../scripts/changelog.mjs";
import changelog from "../../CHANGELOG.md?raw";
import cargo from "../../src-tauri/Cargo.toml?raw";
import lock from "../../package-lock.json";
import pkg from "../../package.json";
import tauri from "../../src-tauri/tauri.conf.json";

describe("versão do projeto", () => {
  it("é a mesma em todos os arquivos que a guardam e está no changelog", () => {
    expect(tauri.version).toBe(pkg.version);
    expect(lock.version).toBe(pkg.version);
    expect(/^version = "(.+)"/m.exec(cargo)?.[1]).toBe(pkg.version);
    expect(parseChangelog(changelog)[0].version).toBe(pkg.version);
  });
});

describe("atualizador", () => {
  it("a configuração publicada só aceita pacote assinado, por HTTPS, das releases oficiais", () => {
    const updater = tauri.plugins.updater;
    expect(updater.endpoints).toEqual(["https://github.com/CAKeysama/prancha/releases/latest/download/latest.json"]);
    expect(updater.requireSignedVersion).toBe(true);
    expect(updater).not.toHaveProperty("dangerousInsecureTransportProtocol");
    expect(updater).not.toHaveProperty("allowDowngrades");
    expect(tauri.bundle.createUpdaterArtifacts).toBe(true);
  });

  it("a interface não fala com a internet: a CSP não libera endereços externos", () => {
    expect(tauri.app.security.csp["connect-src"]).not.toMatch(/https:|github/);
  });
});

describe("changelog", () => {
  const sample = `# Histórico

texto que não é de nenhuma versão

## [1.2.0] - 2026-03-04

Resumo em
duas linhas.

### Novidades

- **Item** um
- Item dois

### Correções

- Item três

## [1.1.0] - 2026-01-02

Outra versão.
`;

  it("separa versões, datas, resumo e seções", () => {
    const [newest, older] = parseChangelog(sample);
    expect(newest).toEqual({
      version: "1.2.0",
      date: "2026-03-04",
      summary: "Resumo em duas linhas.",
      sections: [
        { title: "Novidades", items: ["**Item** um", "Item dois"] },
        { title: "Correções", items: ["Item três"] },
      ],
    });
    expect(older).toEqual({ version: "1.1.0", date: "2026-01-02", summary: "Outra versão.", sections: [] });
  });

  it("toda versão real tem resumo e pelo menos uma seção com itens", () => {
    for (const entry of parseChangelog(changelog)) {
      expect(entry.summary).not.toBe("");
      expect(entry.sections.length).toBeGreaterThan(0);
      for (const section of entry.sections) expect(section.items.length).toBeGreaterThan(0);
    }
  });

  it("escreve as notas da release com a data por extenso e os arquivos", () => {
    const hash = "a".repeat(64);
    const files = parseSums(`${hash} *Prancha_1.2.0_x64-setup.exe\r\n`);
    expect(files).toEqual([{ name: "Prancha_1.2.0_x64-setup.exe", sha256: hash }]);
    const notes = releaseNotes(parseChangelog(sample)[0], files);
    expect(notes).toContain("Lançada em 4 de março de 2026.");
    expect(notes).toContain("## Correções\n\n- Item três");
    expect(notes).toContain(`| Instalador para Windows (recomendado) | \`Prancha_1.2.0_x64-setup.exe\` | \`${hash}\` |`);
    expect(longDate("2026-11-01")).toBe("1 de novembro de 2026");
  });
});
