// Janelas, indicador e seção de atualizações com o backend simulado. A atualização de verdade,
// entre duas versões instaladas, é coberta por tests/update/run.mjs.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { Channel } from "@tauri-apps/api/core";
import { App } from "../../src/app/App";
import { parseNotes, shortSummary, type ReleaseInfo, type UpdateProgress, type UpdatesSnapshot } from "../../src/domain/updates";
// @ts-expect-error script em JS puro, sem tipos
import { assertSignedVersion, buildManifest, packageBaseUrl } from "../../scripts/updater-manifest.mjs";

const BODY = `Lançada em 4 de março de 2026.

Resumo curto da versão.

## Novidades

- **Leitura** nova com \`G\`

## Correções

- Item corrigido

## Arquivos

| Tipo | Arquivo | SHA-256 |
|---|---|---|
| Instalador | \`a.exe\` | \`abc\` |
`;

const release: ReleaseInfo = { version: "1.3.0", name: "Prancha v1.3.0", date: "2026-03-04T12:00:00Z", notes: BODY, url: "https://exemplo.test/v1.3.0" };

const base: UpdatesSnapshot = {
  currentVersion: "1.2.0",
  available: null,
  declinedVersion: null,
  failedVersion: null,
  welcome: null,
  updatedAt: null,
  lastCheckAt: null,
  needsAdmin: false,
};

let snapshot: UpdatesSnapshot;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;
let checkResult: () => UpdatesSnapshot | Promise<UpdatesSnapshot>;
let onInstall: (channel: Channel<UpdateProgress>) => Promise<void>;
let onboarding: { status: string; step: number; comicId: null };
let home: unknown;

const count = (cmd: string) => calls.filter((c) => c.cmd === cmd).length;

function start() {
  mockWindows("main");
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    switch (cmd) {
      case "get_settings":
        return { onboarding };
      case "get_home":
        return home;
      case "open_comic":
        throw { code: "not_found", message: "O item não foi encontrado na biblioteca." };
      case "list_comics":
      case "list_collections":
      case "list_tags":
      case "pending_pdf_covers":
        return [];
      case "updates_snapshot":
        return snapshot;
      case "updates_check":
        return checkResult();
      case "updates_install":
        return onInstall(args.onProgress as Channel<UpdateProgress>);
      case "updates_history":
        return { releases: [release, { ...release, version: "1.2.0", notes: "Lançada em 1 de janeiro de 2026.\n\nA anterior." }], stale: false };
      default:
        return null;
    }
  });
  render(<App />);
}

const offer = () => screen.findByRole("dialog", { name: "Uma nova versão do Prancha está disponível!" });
const indicator = () => screen.findByRole("button", { name: "v1.3.0 disponível" });
const nothingOpens = async () => {
  await waitFor(() => expect(count("updates_check")).toBeGreaterThan(0));
  await new Promise((r) => setTimeout(r, 30));
  expect(screen.queryByRole("dialog")).toBeNull();
};

beforeEach(() => {
  calls = [];
  snapshot = { ...base };
  checkResult = () => ({ ...base, available: release });
  onInstall = () => new Promise(() => undefined);
  onboarding = { status: "completed", step: 4, comicId: null };
  home = { total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] };
});

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("notas da release", () => {
  it("lê data, resumo e seções como texto, sem os arquivos", () => {
    expect(parseNotes(BODY)).toEqual({
      released: "4 de março de 2026",
      summary: "Resumo curto da versão.",
      sections: [
        { title: "Novidades", items: ["Leitura nova com G"] },
        { title: "Correções", items: ["Item corrigido"] },
      ],
    });
  });

  it("aceita notas em outro formato sem inventar nada", () => {
    expect(parseNotes("")).toEqual({ released: null, summary: "", sections: [] });
    expect(parseNotes("# Prancha v1.0.0\n\nTexto [com link](https://x.test).\n\n## Vazia\n")).toEqual({ released: null, summary: "Texto com link.", sections: [] });
  });

  it("o resumo curto usa o sumário ou, sem ele, o primeiro item", () => {
    expect(shortSummary(parseNotes(BODY))).toBe("Resumo curto da versão.");
    expect(shortSummary(parseNotes("## Correções\n- Só isto\n- Outro"))).toBe("Só isto");
    expect(shortSummary(parseNotes(""))).toBe("");
  });
});

describe("janela de nova versão", () => {
  it("aparece depois da verificação automática, uma só vez, com as versões e o resumo", async () => {
    start();
    const dialog = within(await offer());
    expect(dialog.getByText("v1.2.0")).toBeTruthy();
    expect(dialog.getByText("v1.3.0")).toBeTruthy();
    expect(dialog.getByText("Preparamos melhorias e novidades para sua experiência de leitura.")).toBeTruthy();
    expect(dialog.getByText("Resumo curto da versão.")).toBeTruthy();
    expect(count("updates_check")).toBe(1);
    expect(count("updates_install")).toBe(0);
  });

  it("sem versão nova, nada aparece", async () => {
    checkResult = () => base;
    start();
    await nothingOpens();
    expect(screen.queryByText(/disponível/)).toBeNull();
  });

  it("erro na verificação é silencioso", async () => {
    checkResult = () => {
      throw { code: "network", message: "sem conexão" };
    };
    start();
    await nothingOpens();
    expect(screen.queryByText(/sem conexão/)).toBeNull();
  });

  it("Depois fecha, grava a recusa e deixa o indicador, que reabre a janela", async () => {
    start();
    const user = userEvent.setup();
    await offer();
    await user.click(screen.getByRole("button", { name: "Depois" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(calls.find((c) => c.cmd === "updates_decline")?.args.version).toBe("1.3.0");
    expect(count("updates_install")).toBe(0);
    await user.click(await indicator());
    await offer();
  });

  it("navegar não reabre depois de recusar", async () => {
    start();
    const user = userEvent.setup();
    await offer();
    await user.click(screen.getByRole("button", { name: "Depois" }));
    await user.click(screen.getByRole("button", { name: "Minha biblioteca" }));
    await user.click(screen.getByRole("button", { name: "Início" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(count("updates_check")).toBe(1);
  });

  it("versão já recusada abre só com o indicador", async () => {
    checkResult = () => ({ ...base, available: release, declinedVersion: "1.3.0" });
    start();
    await indicator();
    await nothingOpens();
  });

  it("versão mais nova que a recusada volta a ser oferecida", async () => {
    checkResult = () => ({ ...base, available: release, declinedVersion: "1.2.5" });
    start();
    await offer();
  });

  it("não abre sobre o tutorial", async () => {
    onboarding = { status: "active", step: 1, comicId: null };
    start();
    await waitFor(() => expect(count("updates_check")).toBe(1));
    await indicator();
    expect(screen.queryByRole("dialog", { name: "Uma nova versão do Prancha está disponível!" })).toBeNull();
  });

  it("não abre sobre a leitura: espera a pessoa sair do leitor", async () => {
    const comic = { id: 7, title: "Teste", fileName: "t.cbz", format: "cbz", pageCount: 12, coverKind: "placeholder", coverRev: 1, favorite: false, status: "reading", missing: false, addedAt: 1, lastOpenedAt: 2, progress: { pageIndex: 3, pageCount: 12, scrollRatio: 0, updatedAt: 3 } };
    home = { total: 1, continueReading: [comic], recentlyAdded: [comic], favorites: [], recentlyOpened: [comic] };
    let finishCheck!: (s: UpdatesSnapshot) => void;
    checkResult = () => new Promise((resolve) => (finishCheck = resolve));
    start();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Continuar leitura" }));
    const back = await screen.findByRole("button", { name: "Voltar à biblioteca" });
    await waitFor(() => expect(count("updates_check")).toBe(1));

    finishCheck({ ...base, available: release });
    await new Promise((r) => setTimeout(r, 30));
    expect(screen.queryByRole("dialog")).toBeNull();

    await user.click(back);
    await offer();
  });

  it("avisa da confirmação do Windows quando o pacote pede", async () => {
    checkResult = () => ({ ...base, available: release, needsAdmin: true });
    start();
    expect(within(await offer()).getByText(/o Windows vai pedir sua confirmação/)).toBeTruthy();
  });
});

describe("download e instalação", () => {
  it("mostra o percentual real, a conferência e a instalação, sem deixar fechar", async () => {
    let channel!: Channel<UpdateProgress>;
    onInstall = (c) => {
      channel = c;
      return new Promise(() => undefined);
    };
    start();
    const user = userEvent.setup();
    await offer();
    await user.click(screen.getByRole("button", { name: "Atualizar agora" }));
    await screen.findByRole("dialog", { name: "Baixando a atualização" });
    expect(screen.queryByRole("button", { name: "Fechar" })).toBeNull();

    channel.onmessage({ phase: "downloading", downloaded: 50, total: 200 });
    await screen.findByText("25%");
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("25");

    channel.onmessage({ phase: "verifying" });
    await screen.findByRole("dialog", { name: "Conferindo o arquivo" });
    expect(screen.queryByRole("button", { name: "Cancelar" })).toBeNull();

    channel.onmessage({ phase: "installing" });
    await screen.findByRole("dialog", { name: "Preparando a instalação" });
    expect(screen.getByText(/fecha e abre de novo sozinho/)).toBeTruthy();
    expect(screen.queryByText(/O Prancha está atualizado/)).toBeNull();
  });

  it("falha mostra a mensagem e permite tentar de novo", async () => {
    onInstall = () => Promise.reject({ code: "signature", message: "A atualização não passou na verificação de segurança." });
    start();
    const user = userEvent.setup();
    await offer();
    await user.click(screen.getByRole("button", { name: "Atualizar agora" }));
    const failed = await screen.findByRole("dialog", { name: "Não foi possível atualizar" });
    expect(within(failed).getByRole("alert").textContent).toContain("verificação de segurança");

    onInstall = () => new Promise(() => undefined);
    await user.click(within(failed).getByRole("button", { name: "Tentar novamente" }));
    await screen.findByRole("dialog", { name: "Baixando a atualização" });
    expect(count("updates_install")).toBe(2);
  });

  it("cancelar fecha sem erro, como quem escolheu depois", async () => {
    let reject!: (reason: unknown) => void;
    onInstall = () => new Promise((_, r) => (reject = r));
    start();
    const user = userEvent.setup();
    await offer();
    await user.click(screen.getByRole("button", { name: "Atualizar agora" }));
    await user.click(await screen.findByRole("button", { name: "Cancelar" }));
    reject({ code: "cancelled", message: "Download cancelado." });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(count("updates_cancel")).toBe(1);
    expect(calls.find((c) => c.cmd === "updates_decline")?.args.version).toBe("1.3.0");
  });

  it("instalação que não terminou é informada ao abrir", async () => {
    snapshot = { ...base, failedVersion: "1.3.0" };
    checkResult = () => ({ ...base, available: release });
    start();
    const failed = await screen.findByRole("dialog", { name: "Não foi possível atualizar" });
    expect(within(failed).getByRole("alert").textContent).toContain("A instalação da versão v1.3.0 não foi concluída.");
    expect(screen.queryByText("O Prancha está atualizado!")).toBeNull();
  });
});

describe("janela de novidades", () => {
  const updated: UpdatesSnapshot = { ...base, currentVersion: "1.3.0", welcome: { version: "1.3.0", release }, updatedAt: 1 };

  it("agradece e mostra as notas guardadas, sem precisar de internet", async () => {
    snapshot = updated;
    checkResult = () => {
      throw { code: "network", message: "sem conexão" };
    };
    start();
    const dialog = within(await screen.findByRole("dialog", { name: "O Prancha está atualizado!" }));
    expect(dialog.getByText("Você está utilizando a versão v1.3.0.")).toBeTruthy();
    expect(dialog.getByText("Obrigado por manter o Prancha atualizado e fazer parte da nossa evolução!")).toBeTruthy();
    expect(dialog.getByText("Resumo curto da versão.")).toBeTruthy();
    expect(dialog.getByText("Item corrigido")).toBeTruthy();
    expect(dialog.queryByText(/SHA-256/)).toBeNull();
    expect(count("updates_history")).toBe(0);
  });

  it("Começar a ler grava que foi vista e ela não volta com a verificação", async () => {
    snapshot = updated;
    checkResult = () => updated;
    start();
    const user = userEvent.setup();
    await screen.findByRole("dialog", { name: "O Prancha está atualizado!" });
    await user.click(screen.getByRole("button", { name: "Começar a ler" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(calls.find((c) => c.cmd === "updates_notes_shown")?.args.version).toBe("1.3.0");
  });

  it("sem notas guardadas, busca as publicadas", async () => {
    snapshot = { ...updated, welcome: { version: "1.3.0", release: null } };
    checkResult = () => snapshot;
    start();
    await screen.findByText("Item corrigido");
    expect(count("updates_history")).toBe(1);
  });

  it("vem antes de uma versão ainda mais nova", async () => {
    snapshot = updated;
    checkResult = () => ({ ...updated, available: { ...release, version: "1.4.0" } });
    start();
    const user = userEvent.setup();
    await screen.findByRole("dialog", { name: "O Prancha está atualizado!" });
    expect(screen.queryByRole("dialog", { name: "Uma nova versão do Prancha está disponível!" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Começar a ler" }));
    await offer();
  });
});

describe("seção Atualizações", () => {
  const openSection = async (user: ReturnType<typeof userEvent.setup>) => {
    await user.click(await screen.findByRole("button", { name: "Configurações" }));
    return screen.findByRole("region", { name: "Atualizações" });
  };

  it("mostra a versão instalada, o histórico com as notas e a verificação manual", async () => {
    snapshot = { ...base, updatedAt: 1_700_000_000_000 };
    checkResult = () => snapshot;
    start();
    const user = userEvent.setup();
    const section = within(await openSection(user));
    await section.findByText("v1.3.0");
    expect(section.getAllByText("v1.2.0").length).toBe(2);
    expect(section.getByText(/^Atualizada em /)).toBeTruthy();
    expect(section.getByText("instalada")).toBeTruthy();
    expect(section.getByText("4 de março de 2026")).toBeTruthy();
    expect(section.getByText("Item corrigido")).toBeTruthy();
    expect(section.getAllByRole("button", { name: "Ver no GitHub" }).length).toBe(2);

    await user.click(section.getByRole("button", { name: "Verificar atualizações" }));
    await section.findByText("O Prancha está na versão mais recente.");
    expect(calls.filter((c) => c.cmd === "updates_history").at(-1)?.args.force).toBe(true);
  });

  it("verificação manual que falha diz o motivo, e havendo versão nova oferece atualizar", async () => {
    checkResult = () => base;
    start();
    const user = userEvent.setup();
    const section = within(await openSection(user));
    await waitFor(() => expect(count("updates_check")).toBe(1));

    checkResult = () => {
      throw { code: "network", message: "Não foi possível falar com o GitHub." };
    };
    await user.click(section.getByRole("button", { name: "Verificar atualizações" }));
    await section.findByText("Não foi possível falar com o GitHub.");
    expect(screen.queryByRole("dialog")).toBeNull();

    checkResult = () => ({ ...base, available: release });
    await user.click(section.getByRole("button", { name: "Verificar atualizações" }));
    await section.findByText("Nova versão: v1.3.0");
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.click(section.getByRole("button", { name: "Atualizar agora" }));
    await offer();
  });
});

describe("latest.json", () => {
  const signed = (version: string) => btoa(`untrusted comment: x\nabc\ntrusted comment: timestamp:1\tfile:a.exe\tversion:${version}\ndef\n`);
  const encode = (text: string) => new TextEncoder().encode(text);
  const folder = (version: string, missing: string[] = []) => (name: string) => {
    if (missing.includes(name)) return null;
    return encode(name.endsWith(".sig") ? signed(version) : "abc");
  };
  const input = (read: unknown) => ({ version: "1.3.0", notes: "notas", date: "2026-03-04", read, baseUrl: "https://x.test/v1.3.0", nsis: "a.exe", msi: "a.msi" });

  it("descreve cada pacote com endereço, assinatura e SHA-256", () => {
    const manifest = buildManifest(input(folder("1.3.0")));
    expect(manifest.version).toBe("1.3.0");
    expect(manifest.pub_date).toBe("2026-03-04T12:00:00Z");
    const main = { url: "https://x.test/v1.3.0/a.exe", signature: signed("1.3.0"), sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" };
    expect(manifest.platforms["windows-x86_64"]).toEqual(main);
    expect(manifest.platforms["windows-x86_64-nsis"]).toEqual(main);
    expect(manifest.platforms["windows-x86_64-msi"].url).toBe("https://x.test/v1.3.0/a.msi");
  });

  it("não sai sem pacote, sem assinatura ou com assinatura de outra versão", () => {
    expect(() => buildManifest(input(folder("1.3.0", ["a.exe"])))).toThrow(/Pacote não encontrado/);
    expect(() => buildManifest(input(folder("1.3.0", ["a.msi.sig"])))).toThrow(/Assinatura não encontrada/);
    expect(() => buildManifest(input(folder("1.2.9")))).toThrow(/não declara a versão 1\.3\.0/);
    expect(() => assertSignedVersion(signed("1.3.01"), "1.3.0", "a.exe")).toThrow();
  });

  it("os pacotes ficam na release da versão, no repositório do endpoint", () => {
    expect(packageBaseUrl("https://github.com/CAKeysama/prancha/releases/latest/download/latest.json", "1.3.0")).toBe("https://github.com/CAKeysama/prancha/releases/download/v1.3.0");
    expect(() => packageBaseUrl("https://github.com/x/latest.json", "1.3.0")).toThrow();
  });
});
