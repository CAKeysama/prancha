import type { SortKey } from "./types";
import { DEFAULT_SHORTCUTS, type ShortcutMap, sanitizeShortcuts } from "./shortcuts";

type ThemePreference = "dark" | "light" | "system";
/** Família da interface. As páginas das obras nunca recebem fonte nenhuma. */
export type InterfaceFont = "hanken" | "atkinson" | "plex" | "system";
export type ReaderMode = "single" | "double" | "vertical" | "webtoon";
export type ReadingDirection = "ltr" | "rtl";
export type FitMode = "page" | "width" | "height";
export type WheelBehavior = "page" | "scroll";
type EndBehavior = "stay" | "library";
export type ControlsBehavior = "autohide" | "always";
type LibraryView = "grid" | "list";
type ThumbSize = "small" | "medium" | "large";
type Density = "comfortable" | "compact";
export type DuplicatePolicy = "skip" | "allow";

/** Etapas da configuração inicial, na ordem em que aparecem. */
export const ONBOARDING_STEPS = ["welcome", "appearance", "type", "sound", "reading", "start"] as const;
export type OnboardingStep = (typeof ONBOARDING_STEPS)[number];

export interface OnboardingState {
  /** active: a configuração inicial ainda está aberta; completed/skipped: não reaparece sozinha, nem em atualizações. */
  status: "active" | "completed" | "skipped";
  /** Índice em `ONBOARDING_STEPS`: quem fecha o app no meio volta para a mesma etapa. */
  step: number;
  /** A dica dos controles aparece uma vez, na primeira leitura depois de concluir. */
  readerTip: boolean;
  /** Versão do fluxo. Sem ela, o estado é do tutorial antigo (0.11 e anteriores). */
  flow: 2;
}

export interface Settings {
  "appearance.theme": ThemePreference;
  "appearance.font": InterfaceFont;
  "appearance.density": Density;
  "appearance.thumbSize": ThumbSize;
  "library.view": LibraryView;
  "library.sort": SortKey;
  "library.sortDesc": boolean;
  "library.duplicates": DuplicatePolicy;
  "reader.mode": ReaderMode;
  "reader.direction": ReadingDirection;
  "reader.fit": FitMode;
  "reader.coverAlone": boolean;
  "reader.wheel": WheelBehavior;
  "reader.end": EndBehavior;
  "reader.controls": ControlsBehavior;
  "reader.shortcuts": ShortcutMap;
  /** Plugin de fontes de leitura: sem ele, nada de Explorar, extensões ou repositórios. */
  "plugins.online": boolean;
  /** O que entra na central de notificações. */
  "notifications.chapters": boolean;
  "notifications.updates": boolean;
  "notifications.downloadsCompleted": boolean;
  "notifications.downloadsFailed": boolean;
  "notifications.sync": boolean;
  "notifications.library": boolean;
  /** Notificações do sistema operacional, separadas da central: desligar não apaga o histórico. */
  "notifications.native": boolean;
  "notifications.nativeDownloads": boolean;
  /** Silêncio temporário das notificações do sistema (ms desde a época), ou nada. */
  "notifications.nativeMutedUntil": number | null;
  /** Sons da interface. Começam desligados: quem já usava o app não ouve nada sem escolher. */
  "sound.enabled": boolean;
  /** Volume dos sons, de 0 a 100. */
  "sound.volume": number;
  onboarding: OnboardingState;
}

export type SettingKey = keyof Settings;

export const DEFAULT_SETTINGS: Settings = {
  "appearance.theme": "dark",
  "appearance.font": "hanken",
  "appearance.density": "comfortable",
  "appearance.thumbSize": "medium",
  "library.view": "grid",
  "library.sort": "title",
  "library.sortDesc": false,
  "library.duplicates": "skip",
  "reader.mode": "single",
  "reader.direction": "ltr",
  "reader.fit": "page",
  "reader.coverAlone": true,
  "reader.wheel": "page",
  "reader.end": "stay",
  "reader.controls": "autohide",
  "reader.shortcuts": DEFAULT_SHORTCUTS,
  "plugins.online": false,
  "notifications.chapters": true,
  "notifications.updates": true,
  "notifications.downloadsCompleted": true,
  "notifications.downloadsFailed": true,
  "notifications.sync": true,
  "notifications.library": true,
  "notifications.native": true,
  "notifications.nativeDownloads": false,
  "notifications.nativeMutedUntil": null,
  "sound.enabled": false,
  "sound.volume": 60,
  onboarding: { status: "active", step: 0, readerTip: false, flow: 2 },
};

/** O que "Restaurar padrão" dos sons devolve. */
export const SOUND_DEFAULTS = { enabled: true, volume: 60 } as const;

const ENUMS: Partial<Record<SettingKey, readonly string[]>> = {
  "appearance.theme": ["dark", "light", "system"],
  "appearance.font": ["hanken", "atkinson", "plex", "system"],
  "appearance.density": ["comfortable", "compact"],
  "appearance.thumbSize": ["small", "medium", "large"],
  "library.view": ["grid", "list"],
  "library.sort": ["title", "added", "opened", "progress"],
  "library.duplicates": ["skip", "allow"],
  "reader.mode": ["single", "double", "vertical", "webtoon"],
  "reader.direction": ["ltr", "rtl"],
  "reader.fit": ["page", "width", "height"],
  "reader.wheel": ["page", "scroll"],
  "reader.end": ["stay", "library"],
  "reader.controls": ["autohide", "always"],
};

function sanitizeOnboarding(v: unknown): OnboardingState {
  const d = DEFAULT_SETTINGS.onboarding;
  if (!v || typeof v !== "object") return d;
  const o = v as Record<string, unknown>;
  const status = o.status === "completed" || o.status === "skipped" || o.status === "active" ? o.status : d.status;
  // Estado do tutorial antigo: quem já concluiu ou pulou não vê nada de novo; quem estava no meio começa a configuração nova.
  if (o.flow !== 2) return { ...d, status };
  const step = typeof o.step === "number" && Number.isInteger(o.step) && o.step >= 0 && o.step < ONBOARDING_STEPS.length ? o.step : 0;
  return { status, step, readerTip: o.readerTip === true, flow: 2 };
}

/** Mescla valores salvos com os padrões, descartando o que for inválido. */
export function sanitizeSettings(raw: Record<string, unknown>): Settings {
  const out: Settings = { ...DEFAULT_SETTINGS };
  for (const key of Object.keys(DEFAULT_SETTINGS) as SettingKey[]) {
    if (!(key in raw)) continue;
    const value = raw[key];
    const allowed = ENUMS[key];
    if (allowed) {
      if (typeof value === "string" && allowed.includes(value)) (out as unknown as Record<string, unknown>)[key] = value;
    } else if (key === "reader.shortcuts") {
      out[key] = sanitizeShortcuts(value);
    } else if (key === "onboarding") {
      out[key] = sanitizeOnboarding(value);
    } else if (key === "sound.volume") {
      if (typeof value === "number" && Number.isFinite(value)) out[key] = Math.round(Math.min(100, Math.max(0, value)));
    } else if (key === "notifications.nativeMutedUntil") {
      out[key] = typeof value === "number" && Number.isFinite(value) ? value : null;
    } else if (typeof DEFAULT_SETTINGS[key] === "boolean") {
      if (typeof value === "boolean") (out as unknown as Record<string, unknown>)[key] = value;
    }
  }
  return out;
}

export function resolveTheme(pref: ThemePreference, systemDark: boolean): "dark" | "light" {
  if (pref === "system") return systemDark ? "dark" : "light";
  return pref;
}

export const THUMB_WIDTHS: Record<ThumbSize, number> = { small: 132, medium: 168, large: 212 };
