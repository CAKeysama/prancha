// Regras da telemetria do lado da Vercel. As mesmas regras estão na função telemetry_ingest do Supabase
// (supabase/migrations/*_telemetry.sql): a Vercel recusa cedo, o banco confere de novo.
// Arquivos que começam com "_" não viram endereço na Vercel.

export const MAX_BODY_BYTES = 64 * 1024;
export const MAX_EVENTS = 200;
export const MAX_COUNT = 100_000;
export const MAX_MS = 3_600_000;

export const PLATFORMS = ["windows", "macos", "linux"];
export const CHANNELS = ["nsis", "msi", "dev", "unknown"];
export const ARCHS = ["x86_64", "aarch64", "x86", "arm"];
export const ERROR_CATEGORIES = ["network", "source", "extension", "reader", "library", "import", "database", "update", "interface", "crash", "other"];
export const METRICS = ["startup", "library_load", "explore_results", "series_open", "chapter_open", "reader_first_page"];

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const VERSION = /^\d{1,4}\.\d{1,4}\.\d{1,4}$/;
const OS_VERSION = /^\d[\d.]{0,23}$/;
const TOKEN = /^[a-z0-9][a-z0-9._:-]*$/;
const SOURCE = /^[a-z0-9][a-z0-9._-]{0,63}\/[a-z0-9][a-z0-9._-]{0,63}$/;
const DAY = /^\d{4}-\d{2}-\d{2}$/;

const ALLOWED = {
  session: ["type", "day", "launches", "uncleanExits"],
  source: ["type", "day", "source", "queries", "opens", "chapters", "errors"],
  error: ["type", "day", "hour", "category", "component", "code", "httpStatus", "count"],
  perf: ["type", "day", "metric", "count", "totalMs", "maxMs"],
};

class Invalid extends Error {}

const fail = (what) => {
  throw new Invalid(what);
};
const isObject = (v) => typeof v === "object" && v !== null && !Array.isArray(v);
const int = (v, min, max, what, fallback) => {
  if (v === undefined && fallback !== undefined) return fallback;
  if (!Number.isInteger(v) || v < min || v > max) fail(what);
  return v;
};
const token = (v, max, what) => (typeof v === "string" && v.length <= max && TOKEN.test(v) ? v : fail(what));
const onlyKeys = (obj, keys, what) => {
  for (const k of Object.keys(obj)) if (!keys.includes(k)) fail(`${what}.${k}`);
};

function day(v, now) {
  if (typeof v !== "string" || !DAY.test(v)) fail("day");
  const t = Date.parse(`${v}T00:00:00Z`);
  const today = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
  if (Number.isNaN(t) || t > today + 86_400_000 || t < today - 7 * 86_400_000) fail("day");
  return v;
}

/**
 * Confere um lote e devolve uma cópia só com os campos conhecidos. Qualquer campo a mais recusa o lote:
 * assim nada que o app não deveria mandar (título, caminho, texto) passa por engano.
 */
export function validateBatch(input, now = new Date()) {
  try {
    if (!isObject(input)) fail("batch");
    onlyKeys(input, ["schema", "batchId", "installId", "app", "events"], "batch");
    if (input.schema !== 1) fail("schema");
    if (typeof input.batchId !== "string" || !UUID.test(input.batchId)) fail("batchId");
    if (typeof input.installId !== "string" || !UUID.test(input.installId)) fail("installId");
    const app = input.app;
    if (!isObject(app)) fail("app");
    onlyKeys(app, ["version", "platform", "os", "arch", "channel"], "app");
    if (typeof app.version !== "string" || !VERSION.test(app.version)) fail("app.version");
    if (!PLATFORMS.includes(app.platform)) fail("app.platform");
    const channel = app.channel ?? "unknown";
    if (!CHANNELS.includes(channel)) fail("app.channel");
    if (app.os !== undefined && app.os !== null && (typeof app.os !== "string" || !OS_VERSION.test(app.os))) fail("app.os");
    if (app.arch !== undefined && app.arch !== null && !ARCHS.includes(app.arch)) fail("app.arch");
    if (!Array.isArray(input.events) || input.events.length > MAX_EVENTS) fail("events");

    const events = input.events.map((e) => {
      if (!isObject(e) || !(e.type in ALLOWED)) fail("event.type");
      onlyKeys(e, ALLOWED[e.type], e.type);
      const base = { type: e.type, day: day(e.day, now) };
      switch (e.type) {
        case "session":
          return { ...base, launches: int(e.launches, 0, 1000, "session.launches"), uncleanExits: int(e.uncleanExits, 0, 1000, "session.uncleanExits", 0) };
        case "source":
          if (typeof e.source !== "string" || !SOURCE.test(e.source)) fail("source.source");
          return {
            ...base,
            source: e.source,
            queries: int(e.queries, 0, MAX_COUNT, "source.queries", 0),
            opens: int(e.opens, 0, MAX_COUNT, "source.opens", 0),
            chapters: int(e.chapters, 0, MAX_COUNT, "source.chapters", 0),
            errors: int(e.errors, 0, MAX_COUNT, "source.errors", 0),
          };
        case "error":
          if (!ERROR_CATEGORIES.includes(e.category)) fail("error.category");
          return {
            ...base,
            hour: int(e.hour, 0, 23, "error.hour"),
            category: e.category,
            component: token(e.component, 40, "error.component"),
            code: token(e.code, 64, "error.code"),
            httpStatus: int(e.httpStatus, 0, 599, "error.httpStatus", 0),
            count: int(e.count, 1, MAX_COUNT, "error.count"),
          };
        default:
          if (!METRICS.includes(e.metric)) fail("perf.metric");
          return {
            ...base,
            metric: e.metric,
            count: int(e.count, 1, MAX_COUNT, "perf.count"),
            totalMs: int(e.totalMs, 0, MAX_MS * MAX_COUNT, "perf.totalMs"),
            maxMs: int(e.maxMs, 0, MAX_MS, "perf.maxMs"),
          };
      }
    });

    return {
      ok: true,
      batch: {
        schema: 1,
        batchId: input.batchId,
        installId: input.installId,
        app: { version: app.version, platform: app.platform, os: app.os ?? null, arch: app.arch ?? null, channel },
        events,
      },
    };
  } catch (e) {
    if (e instanceof Invalid) return { ok: false, error: e.message };
    throw e;
  }
}

/** Período e filtros do painel. */
export function parseReportQuery(params, now = new Date()) {
  const today = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()));
  const iso = (d) => d.toISOString().slice(0, 10);
  const from = params.get("from") ?? iso(new Date(today.getTime() - 29 * 86_400_000));
  const to = params.get("to") ?? iso(today);
  if (!DAY.test(from) || !DAY.test(to) || Number.isNaN(Date.parse(from)) || Number.isNaN(Date.parse(to))) return { ok: false, error: "period" };
  const span = (Date.parse(to) - Date.parse(from)) / 86_400_000;
  if (span < 0 || span > 366) return { ok: false, error: "period" };
  const version = params.get("version") || null;
  if (version !== null && !VERSION.test(version)) return { ok: false, error: "version" };
  const category = params.get("category") || null;
  if (category !== null && !ERROR_CATEGORIES.includes(category)) return { ok: false, error: "category" };
  return { ok: true, query: { p_from: from, p_to: to, p_version: version, p_category: category } };
}

/** Configuração do servidor. Sem as variáveis, os endereços respondem 503 e não quebram nada. */
export function serverConfig(env = process.env) {
  const url = (env.SUPABASE_URL ?? "").replace(/\/+$/, "");
  return {
    url,
    anonKey: env.SUPABASE_ANON_KEY ?? env.SUPABASE_PUBLISHABLE_KEY ?? "",
    serviceKey: env.SUPABASE_SERVICE_ROLE_KEY ?? env.SUPABASE_SECRET_KEY ?? "",
  };
}

export function json(status, body, headers = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8", "cache-control": "no-store", "x-content-type-options": "nosniff", ...headers },
  });
}

/** Lê o corpo sem passar do limite, mesmo se o cabeçalho mentir o tamanho. */
export async function readJson(request, limit = MAX_BODY_BYTES) {
  const declared = Number(request.headers.get("content-length") ?? 0);
  if (declared > limit) return { ok: false, status: 413 };
  if (!(request.headers.get("content-type") ?? "").toLowerCase().startsWith("application/json")) return { ok: false, status: 415 };
  const text = await request.text();
  if (new TextEncoder().encode(text).length > limit) return { ok: false, status: 413 };
  try {
    return { ok: true, value: JSON.parse(text) };
  } catch {
    return { ok: false, status: 400 };
  }
}

/** Chama uma função do Supabase pela API REST. O erro devolvido nunca leva a chave nem o corpo do pedido. */
export async function rpc(config, name, args, { key, bearer, fetchImpl = fetch } = {}) {
  let response;
  try {
    response = await fetchImpl(`${config.url}/rest/v1/rpc/${name}`, {
      method: "POST",
      headers: { apikey: key, authorization: `Bearer ${bearer ?? key}`, "content-type": "application/json" },
      body: JSON.stringify(args),
      signal: AbortSignal.timeout(8000),
    });
  } catch {
    return { ok: false, status: 502, code: "unavailable" };
  }
  const text = await response.text();
  let body = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = null;
  }
  if (response.ok) return { ok: true, value: body };
  const pg = body?.code ?? "";
  if (response.status === 401 && pg !== "42501") return { ok: false, status: 401, code: "unauthorized" };
  if (pg === "42501" || response.status === 403) return { ok: false, status: 403, code: "forbidden" };
  if (pg === "P0001" && String(body?.hint ?? "").startsWith("rate")) return { ok: false, status: 429, code: "rate_limited" };
  if (pg.startsWith("22")) return { ok: false, status: 400, code: "invalid" };
  return { ok: false, status: 502, code: "unavailable" };
}
