import { describe, expect, it } from "vitest";
// @ts-expect-error script em JS puro, sem tipos
import { longDate, parseChangelog, parseSums, releaseNotes } from "../scripts/changelog.mjs";
import changelog from "../CHANGELOG.md?raw";
import cargo from "../Cargo.toml?raw";
import lock from "../package-lock.json";
import pkg from "../package.json";
import tauri from "../apps/desktop/src-tauri/tauri.conf.json";
import android from "../apps/android/src-tauri/tauri.conf.json";
// @ts-expect-error script em JS puro, sem tipos
import { addAndroid, apkName } from "../scripts/updater-manifest.mjs";

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
    expect(android.app.security.csp["connect-src"]).not.toMatch(/https:|github/);
  });

  it("o app Android confere o APK com a mesma chave e as mesmas releases do desktop", () => {
    expect(android.plugins.updater.pubkey).toBe(tauri.plugins.updater.pubkey);
    expect(android.plugins.updater.endpoints).toEqual(tauri.plugins.updater.endpoints);
    // A versão do APK vem do package.json, a mesma do desktop (ver o teste "versão do projeto").
    expect(android.version).toBe("../../../package.json");
  });

  it("acrescenta o APK ao latest.json sem mexer nos pacotes do Windows", () => {
    // Assinatura feita com uma chave descartável e `--app-version 9.9.9` (a mesma dos testes de crates/prancha-app/src/updates.rs).
    const signature =
      "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUVndiOGF5QzkvQ0tocGpQVVk3UldmYkM2cU5qQVhBWHVhbENFeEhNdmNuTlcyWktVN0Q0clpNRjJNQVIycnkvU0RnaGdqcXo0WFAwQ2srNnFoOHdNQm9hamp2WlllR3dJPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0ODQyMzM3CWZpbGU6cGF5bG9hZC5hcGsJdmVyc2lvbjo5LjkuOQp0OUM4VDU4RlpVby94ejJRRHFlUjd5VlVtK1E1U3VBemx3bTZpenVGY2YveWNjRW9FczFabUZ2Rk1JV1J2R2NUc0hHUUtoYzlWa3VDeE44L3kwdU9Cdz09Cg==";
    const apk = apkName("9.9.9");
    const files: Record<string, string> = { [apk]: "APK de teste do Prancha\n", [`${apk}.sig`]: `${signature}\n` };
    const read = (name: string) => (files[name] === undefined ? null : Buffer.from(files[name]));
    const windows = { url: "https://exemplo/Prancha_9.9.9_x64-setup.exe", signature: "x", sha256: "y" };
    const base = { version: "9.9.9", notes: "", pub_date: "2026-11-16T12:00:00Z", platforms: { "windows-x86_64": windows } };
    const baseUrl = "https://github.com/CAKeysama/prancha/releases/download/v9.9.9";
    const manifest = addAndroid(base, { read, baseUrl, apk });
    expect(apk).toBe("Prancha_9.9.9_android-arm64.apk");
    expect(manifest.platforms["windows-x86_64"]).toEqual(windows);
    expect(manifest.platforms["android-aarch64"]).toEqual({
      url: `${baseUrl}/Prancha_9.9.9_android-arm64.apk`,
      signature,
      sha256: "0966529c4da04f8d39faa8d8b4050ff3e1fa075ccf088ee84cc1c76780a30e27",
    });
    // Uma assinatura de outra versão não entra.
    expect(() => addAndroid({ ...base, version: "9.9.10" }, { read, baseUrl, apk })).toThrow(/não declara a versão 9.9.10/);
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
