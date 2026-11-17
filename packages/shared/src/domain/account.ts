// Conta do Prancha e sincronização entre aparelhos: tipos que vêm de crates/prancha-core/src/account e src/sync.

export type AccountState = "signed_out" | "signed_in" | "expired";
export type AccountProvider = "google" | "discord";

export interface AccountStatus {
  state: AccountState;
  email: string | null;
  name: string | null;
  provider: string | null;
  signedInAt: number | null;
  /** Este aparelho na conta. */
  deviceId: string;
}

export interface SyncStatus {
  running: boolean;
  lastSyncAt: number | null;
  lastError: string | null;
  /** Obras, pastas e catálogo esperando para subir. */
  pending: number;
}

/** Como a nuvem devolve cada aparelho (`sync_devices`). */
export interface AccountDevice {
  id: string;
  name: string;
  platform: string;
  app_version: string | null;
  last_seen_at: string;
}

export const PROVIDER_LABELS: Record<AccountProvider, string> = { google: "Google", discord: "Discord" };

export function providerLabel(provider: string | null): string {
  return provider && provider in PROVIDER_LABELS ? PROVIDER_LABELS[provider as AccountProvider] : (provider ?? "—");
}

export function platformLabel(platform: string): string {
  return ({ windows: "Windows", android: "Android", linux: "Linux", macos: "macOS" } as Record<string, string>)[platform] ?? platform;
}
