// Telemetria na interface: consentimento, convite, revogação e o cliente que repassa falhas ao backend.
// O backend (telemetry.rs) tem os próprios testes; aqui ele é simulado por mockIPC.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../../src/app/App";
import type { TelemetryStatus } from "../../src/domain/telemetry";
import { reportError, setTelemetryEnabled, telemetryEnabled } from "../../src/services/telemetry";
import { INVITE_DELAY_MS } from "../../src/features/telemetry/TelemetryInvite";

let settings: Record<string, unknown>;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;
let telemetry: TelemetryStatus;
let failStatus: boolean;
let failRecord: boolean;

function status(patch: Partial<TelemetryStatus> = {}): TelemetryStatus {
  return { consent: "unset", decidedAt: null, installId: null, pendingEvents: 0, lastSentAt: null, endpoint: true, forgetPending: false, ...patch };
}

beforeEach(() => {
  settings = { onboarding: { status: "completed", step: 4, comicId: null } };
  calls = [];
  telemetry = status();
  failStatus = false;
  failRecord = false;
  mockWindows("main");
  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      switch (cmd) {
        case "get_settings":
          return settings;
        case "get_home":
          return { total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] };
        case "list_comics":
        case "list_collections":
        case "pending_pdf_covers":
          return [];
        case "telemetry_status":
          if (failStatus) throw { code: "internal", message: "falhou" };
          return telemetry;
        case "telemetry_set_consent":
          telemetry = args.granted
            ? status({ consent: "granted", decidedAt: 1, installId: "8c1f0d6e-1d2b-4c3a-9e8f-0123456789ab" })
            : status({ consent: "denied", decidedAt: 2 });
          return telemetry;
        case "telemetry_reset_id":
          telemetry = { ...telemetry, installId: "11111111-2222-4333-8444-555555555555" };
          return telemetry;
        case "telemetry_forget":
          telemetry = status({ consent: "denied", decidedAt: 3 });
          return telemetry;
        case "telemetry_record":
          if (failRecord) throw { code: "internal", message: "rede" };
          return (args.events as unknown[]).length;
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.useRealTimers();
});

const sent = (cmd: string) => calls.filter((c) => c.cmd === cmd);

async function openPrivacy() {
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: "Configurações" }));
  const nav = await screen.findByRole("navigation", { name: "Categorias das configurações" });
  await user.click(within(nav).getByRole("button", { name: "Privacidade" }));
  return user;
}

describe("Configurações › Privacidade", () => {
  it("começa desligada, explica o que é e o que não é coletado", async () => {
    await openPrivacy();
    expect(await screen.findByTestId("telemetry-state")).toHaveTextContent("Desligada");
    expect(screen.getByRole("switch", { name: "Enviar estatísticas de uso" })).not.toBeChecked();
    expect(screen.getByText("O que é enviado")).toBeInTheDocument();
    expect(screen.getByText("O que nunca é enviado")).toBeInTheDocument();
    expect(screen.getByText(/Títulos, capas, buscas/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Gerar outro" })).toBeDisabled();
    expect(sent("telemetry_set_consent")).toHaveLength(0);
  });

  it("aceitar liga, mostra o identificador e revogar desliga na hora", async () => {
    const user = await openPrivacy();
    const toggle = await screen.findByRole("switch", { name: "Enviar estatísticas de uso" });
    await user.click(toggle);
    await waitFor(() => expect(screen.getByTestId("telemetry-state")).toHaveTextContent("Ligada"));
    expect(sent("telemetry_set_consent").at(-1)?.args).toEqual({ granted: true });
    expect(screen.getByText("8c1f0d6e-1d2b-4c3a-9e8f-0123456789ab")).toBeInTheDocument();
    expect(telemetryEnabled()).toBe(true);

    await user.click(screen.getByRole("button", { name: "Gerar outro" }));
    expect(await screen.findByText("11111111-2222-4333-8444-555555555555")).toBeInTheDocument();

    await user.click(screen.getByRole("switch", { name: "Enviar estatísticas de uso" }));
    await waitFor(() => expect(screen.getByTestId("telemetry-state")).toHaveTextContent("Desligada"));
    expect(sent("telemetry_set_consent").at(-1)?.args).toEqual({ granted: false });
    expect(telemetryEnabled()).toBe(false);
    expect(screen.getByText(/Desligada por você/)).toBeInTheDocument();
  });

  it("apagar os dados pede confirmação e desliga", async () => {
    telemetry = status({ consent: "granted", decidedAt: 1, installId: "8c1f0d6e-1d2b-4c3a-9e8f-0123456789ab" });
    const user = await openPrivacy();
    await user.click(await screen.findByRole("button", { name: "Apagar dados" }));
    const dialog = await screen.findByRole("dialog", { name: "Apagar os dados enviados?" });
    expect(sent("telemetry_forget")).toHaveLength(0);
    await user.click(within(dialog).getByRole("button", { name: "Apagar" }));
    await waitFor(() => expect(sent("telemetry_forget")).toHaveLength(1));
    await waitFor(() => expect(screen.getByTestId("telemetry-state")).toHaveTextContent("Desligada"));
  });

  it("sem resposta do backend, a tela abre e a telemetria fica desligada", async () => {
    failStatus = true;
    await openPrivacy();
    expect(await screen.findByTestId("telemetry-state")).toHaveTextContent("Desligada");
    expect(telemetryEnabled()).toBe(false);
  });
});

describe("Convite", () => {
  it("aparece uma vez depois do tutorial, e recusar grava a escolha", async () => {
    const user = userEvent.setup();
    render(<App />);
    const dialog = await screen.findByRole("dialog", { name: "Ajudar a melhorar o Prancha?" }, { timeout: INVITE_DELAY_MS + 2000 });
    expect(within(dialog).getByText("O que nunca é enviado")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Agora não" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(sent("telemetry_set_consent").at(-1)?.args).toEqual({ granted: false });
  }, 10000);

  it("não aparece durante o tutorial, depois de decidir nem em builds que não enviam", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    for (const [setup, why] of [
      [() => (settings = { onboarding: { status: "active", step: 1, comicId: null } }), "tutorial"],
      [() => (telemetry = status({ consent: "denied", decidedAt: 1 })), "já recusou"],
      [() => (telemetry = status({ endpoint: false })), "build de desenvolvimento"],
    ] as const) {
      setup();
      render(<App />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(INVITE_DELAY_MS + 1000);
      });
      expect(screen.queryByRole("dialog", { name: "Ajudar a melhorar o Prancha?" }), why).not.toBeInTheDocument();
      cleanup();
      settings = { onboarding: { status: "completed", step: 4, comicId: null } };
      telemetry = status();
    }
    expect(sent("telemetry_set_consent")).toHaveLength(0);
  });
});

describe("Cliente de falhas da interface", () => {
  it("desligado não chama o backend; ligado agrupa e envia só identificadores", async () => {
    vi.useFakeTimers();
    reportError("interface", "window", "uncaught_error");
    await vi.advanceTimersByTimeAsync(6000);
    expect(sent("telemetry_record")).toHaveLength(0);

    setTelemetryEnabled(true);
    reportError("interface", "window", "uncaught_error");
    reportError("reader", "pdf", "render_failed");
    await vi.advanceTimersByTimeAsync(6000);
    const record = sent("telemetry_record");
    expect(record).toHaveLength(1);
    const events = record[0]!.args.events as Record<string, unknown>[];
    expect(events.map((e) => e.code)).toEqual(["uncaught_error", "render_failed"]);
    expect(Object.keys(events[0]!).sort()).toEqual(["category", "code", "component", "count", "day", "hour", "type"]);

    // Revogar descarta o que ainda não tinha saído.
    reportError("interface", "window", "uncaught_error");
    setTelemetryEnabled(false);
    await vi.advanceTimersByTimeAsync(6000);
    expect(sent("telemetry_record")).toHaveLength(1);
  });

  it("falha ao repassar não quebra nada", async () => {
    vi.useFakeTimers();
    failRecord = true;
    setTelemetryEnabled(true);
    reportError("interface", "window", "uncaught_error");
    await vi.advanceTimersByTimeAsync(6000);
    expect(sent("telemetry_record")).toHaveLength(1);
  });
});
