import { describe, expect, it } from "vitest";
// @ts-expect-error script em JS puro, sem tipos
import { bumpLevel, nextVersion, parseCommit, review, versionsIn, withChangelogEntry, withVersion } from "../../scripts/version.mjs";

const files = {
  "package.json": '{\n  "name": "prancha",\n  "version": "1.2.3",\n  "devDependencies": { "x": "1.2.3" }\n}\n',
  "package-lock.json": '{\n  "name": "prancha",\n  "version": "1.2.3",\n  "packages": {\n    "": {\n      "name": "prancha",\n      "version": "1.2.3"\n    },\n    "node_modules/x": {\n      "version": "1.2.3"\n    }\n  }\n}\n',
  "src-tauri/tauri.conf.json": '{\n  "version": "1.2.3"\n}\n',
  "src-tauri/Cargo.toml": '[package]\nname = "prancha"\nversion = "1.2.3"\n\n[dependencies]\nx = { version = "1.2.3" }\n',
  "src-tauri/Cargo.lock": '[[package]]\nname = "outro"\nversion = "1.2.3"\n\n[[package]]\nname = "prancha"\nversion = "1.2.3"\n',
};

const entry = (version: string) => `# Histórico\n\n## [${version}] - 2026-11-09\n\nResumo.\n\n### Correções\n\n- Algo.\n`;

describe("rótulo dos commits", () => {
  it("feat sobe a minor, ! e BREAKING CHANGE sobem a major, o resto sobe a patch", () => {
    expect(parseCommit("feat: notificações")?.level).toBe("minor");
    expect(parseCommit("feat(leitura): progresso")?.level).toBe("minor");
    expect(parseCommit("fix: corrige o filtro")?.level).toBe("patch");
    expect(parseCommit("chore(release): versão 1.0.0")?.level).toBe("patch");
    expect(parseCommit("refactor!: troca o banco")?.level).toBe("major");
    expect(parseCommit("fix: algo\n\nBREAKING CHANGE: a biblioteca antiga não abre")?.level).toBe("major");
  });

  it("recusa o que não diz o tipo", () => {
    expect(parseCommit("versão 0.6.0")).toBeNull();
    expect(parseCommit("explorar: mostra as fontes")).toBeNull();
    expect(parseCommit("feat:sem espaço")).toBeNull();
    expect(parseCommit("Feat: maiúscula")).toBeNull();
    expect(parseCommit("feat: ")).toBeNull();
  });

  it("o maior nível ganha", () => {
    expect(bumpLevel([{ level: "patch" }, { level: "minor" }, { level: "patch" }])).toBe("minor");
    expect(bumpLevel([{ level: "minor" }, { level: "major" }])).toBe("major");
    expect(bumpLevel([])).toBe("patch");
  });

  it("calcula a próxima versão", () => {
    expect(nextVersion("2.5.1", "major")).toBe("3.0.0");
    expect(nextVersion("2.4.1", "minor")).toBe("2.5.0");
    expect(nextVersion("2.5.0", "patch")).toBe("2.5.1");
  });
});

describe("arquivos da versão", () => {
  it("troca só a versão do Prancha nos cinco arquivos", () => {
    const next = withVersion(files, "1.2.3", "1.3.0");
    const versions = versionsIn(next);
    expect(new Set(Object.values(versions))).toEqual(new Set(["1.3.0"]));
    expect(next["package.json"]).toContain('"x": "1.2.3"');
    expect(next["package-lock.json"]).toContain('"node_modules/x": {\n      "version": "1.2.3"');
    expect(next["src-tauri/Cargo.toml"]).toContain('x = { version = "1.2.3" }');
    expect(next["src-tauri/Cargo.lock"]).toContain('name = "outro"\nversion = "1.2.3"');
  });

  it("abre a entrada no topo do changelog, uma vez só", () => {
    const text = withChangelogEntry(entry("1.2.3"), "1.3.0", "2026-11-10");
    expect(text.indexOf("## [1.3.0] - 2026-11-10")).toBeLessThan(text.indexOf("## [1.2.3]"));
    expect(withChangelogEntry(text, "1.3.0", "2026-11-10")).toBe(text);
  });
});

describe("verificação do PR", () => {
  const base = { baseVersion: "1.2.3", headVersions: versionsIn(withVersion(files, "1.2.3", "1.3.0")), changelog: entry("1.3.0") };

  it("aceita a versão que os commits pedem", () => {
    const result = review({ ...base, commits: [{ sha: "a".repeat(40), message: "feat: algo novo" }, { sha: "b".repeat(40), message: "fix: algo" }] });
    expect(result.problems).toEqual([]);
    expect(result.expected).toBe("1.3.0");
  });

  it("não deixa uma minor sair como patch", () => {
    const result = review({ ...base, headVersions: versionsIn(withVersion(files, "1.2.3", "1.2.4")), changelog: entry("1.2.4"), commits: [{ sha: "a".repeat(40), message: "feat: algo novo" }] });
    expect(result.problems.join()).toContain("tem que ir de 1.2.3 para 1.3.0");
  });

  it("aponta commit sem rótulo, versão já publicada, arquivos diferentes e changelog faltando", () => {
    const headVersions = { ...base.headVersions, "src-tauri/Cargo.toml": "1.2.3" };
    const result = review({ ...base, headVersions, changelog: entry("1.2.3"), released: ["1.3.0"], commits: [{ sha: "c".repeat(40), message: "feat: algo" }, { sha: "d".repeat(40), message: "versão nova" }] });
    const text = result.problems.join("\n");
    expect(text).toContain("ddddddd");
    expect(text).toContain("já foi publicada");
    expect(text).toContain("src-tauri/Cargo.toml");
    expect(text).toContain("não tem a entrada");
  });

  it("recusa a entrada do changelog ainda com o modelo", () => {
    const changelog = withChangelogEntry(entry("1.2.3"), "1.3.0", "2026-11-10");
    const result = review({ ...base, changelog, commits: [{ sha: "a".repeat(40), message: "feat: algo" }] });
    expect(result.problems.join()).toMatch(/modelo|sem itens/);
  });
});
