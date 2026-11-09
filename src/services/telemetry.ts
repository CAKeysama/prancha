// Falhas da interface para a telemetria. Nunca lança, nunca espera e não faz nada com a telemetria desligada:
// o backend confere o consentimento de novo e descarta o que não for um identificador técnico.
import { api } from "./api";

type Category = "interface" | "reader" | "library" | "other";

const FLUSH_MS = 5000;
const MAX_QUEUE = 50;

let enabled = false;
let queue: Record<string, unknown>[] = [];
let timer: ReturnType<typeof setTimeout> | null = null;

/** Liga ou desliga o envio. Desligar descarta o que ainda não foi passado ao backend. */
export function setTelemetryEnabled(on: boolean) {
  enabled = on;
  if (!on) {
    queue = [];
    if (timer) clearTimeout(timer);
    timer = null;
  }
}

export function telemetryEnabled() {
  return enabled;
}

function utcNow() {
  const d = new Date();
  return { day: d.toISOString().slice(0, 10), hour: d.getUTCHours() };
}

function flush() {
  timer = null;
  if (!enabled || queue.length === 0) return;
  const events = queue;
  queue = [];
  api.telemetryRecord(events).catch(() => undefined);
}

/** Só identificadores fixos do código, nunca mensagens: `reportError("interface", "window", "uncaught")`. */
export function reportError(category: Category, component: string, code: string) {
  if (!enabled || queue.length >= MAX_QUEUE) return;
  const { day, hour } = utcNow();
  queue.push({ type: "error", day, hour, category, component, code, count: 1 });
  timer ??= setTimeout(flush, FLUSH_MS);
}

/** Erros que escaparam da interface: conta só que aconteceram e onde, sem a mensagem. */
export function installGlobalErrorReporting(target: Window = window) {
  const onError = () => reportError("interface", "window", "uncaught_error");
  const onRejection = () => reportError("interface", "window", "unhandled_rejection");
  target.addEventListener("error", onError);
  target.addEventListener("unhandledrejection", onRejection);
  return () => {
    target.removeEventListener("error", onError);
    target.removeEventListener("unhandledrejection", onRejection);
  };
}

export function resetTelemetryClient() {
  setTelemetryEnabled(false);
}
