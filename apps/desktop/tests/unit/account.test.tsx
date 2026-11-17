// Tela Conta, com o backend simulado. O login, a nuvem e a sincronização são testados no Rust, em
// crates/prancha-core/src/account/tests.rs e src/sync/tests.rs.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../../src/app/App";
import type { AccountDevice, AccountStatus, SyncStatus } from "@prancha/shared/domain/account";

type Handler = (args: Record<string, unknown>) => unknown;
let calls: Array<{ cmd: string; args: Record<string, unknown> }>;
let handlers: Record<string, Handler>;

const signedOut: AccountStatus = { state: "signed_out", email: null, name: null, provider: null, signedInAt: null, deviceId: "aaaaaaaa-0000-4000-a000-000000000001" };
const signedIn: AccountStatus = { ...signedOut, state: "signed_in", email: "ana@exemplo.com", name: "Ana", provider: "google", signedInAt: 1_700_000_000_000 };
const synced: SyncStatus = { running: false, lastSyncAt: 1_700_000_000_000, lastError: null, pending: 0 };
const phone: AccountDevice = { id: "bbbbbbbb-0000-4000-a000-000000000002", name: "Galaxy", platform: "android", app_version: "0.14.0", last_seen_at: "2026-11-16T22:00:00Z" };
const here: AccountDevice = { id: signedOut.deviceId, name: "PC", platform: "windows", app_version: "0.14.0", last_seen_at: "2026-11-16T22:00:00Z" };

beforeEach(() => {
  calls = [];
  handlers = {};
  mockWindows("main");
  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      if (handlers[cmd]) return handlers[cmd]!(args);
      switch (cmd) {
        case "get_settings":
          return { onboarding: { status: "completed", step: 4, comicId: 1 } };
        case "get_home":
          return { total: 0, continueReading: [], recentlyAdded: [], favorites: [], recentlyOpened: [] };
        case "pending_pdf_covers":
        case "list_collections":
        case "list_comics":
          return [];
        case "sync_status":
          return synced;
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
});

async function openAccount() {
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: "Configurações" }));
  await user.click(await screen.findByRole("button", { name: "Conta" }));
  return user;
}

describe("conta", () => {
  it("entra com o Google e mostra a conta, a sincronização e os aparelhos", async () => {
    let status = signedOut;
    handlers.account_status = () => status;
    handlers.account_sign_in = () => {
      status = signedIn;
      return signedIn;
    };
    handlers.account_devices = () => [here, phone];
    const user = await openAccount();
    expect(await screen.findByTestId("account-state")).toHaveTextContent("Sem conta");
    expect(screen.getByTestId("sync-state")).toHaveTextContent("nada sai deste aparelho");
    await user.click(screen.getByRole("button", { name: "Entrar com Google" }));
    await waitFor(() => expect(screen.getByTestId("account-state")).toHaveTextContent("Conectada"));
    expect(calls.find((c) => c.cmd === "account_sign_in")?.args.provider).toBe("google");
    expect(screen.getByText(/Ana \(ana@exemplo.com\), pelo Google/)).toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("sync-state")).toHaveTextContent("Tudo sincronizado"));
    const list = await screen.findByRole("list", { name: "Aparelhos da conta" });
    expect(within(list).getByText(/este aparelho/)).toBeInTheDocument();
    // Só o outro aparelho pode ser removido daqui.
    expect(within(list).getAllByRole("button", { name: "Remover" })).toHaveLength(1);
  });

  it("sair e apagar a conta pedem confirmação", async () => {
    handlers.account_status = () => signedIn;
    handlers.account_devices = () => [here];
    const user = await openAccount();
    await user.click(await screen.findByRole("button", { name: "Sair" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/A biblioteca e a leitura que estão aqui ficam/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Sair" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "account_sign_out")).toBe(true));

    await user.click(screen.getByRole("button", { name: "Apagar conta" }));
    const confirm = await screen.findByRole("dialog");
    expect(within(confirm).getByText(/não dá para desfazer/)).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === "account_delete")).toBe(false);
    await user.click(within(confirm).getByRole("button", { name: "Apagar conta" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "account_delete")).toBe(true));
  });

  it("mostra o erro da última sincronização", async () => {
    handlers.account_status = () => signedIn;
    handlers.account_devices = () => [];
    handlers.sync_status = () => ({ ...synced, lastError: "Sem conexão com a internet." });
    await openAccount();
    expect(await screen.findByText(/A última tentativa falhou: Sem conexão/)).toBeInTheDocument();
  });
});
