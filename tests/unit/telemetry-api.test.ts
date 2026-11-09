// Funções da Vercel da telemetria (site/api): validação, limites, chaves e mapeamento de erros do Supabase.
// O Supabase é simulado pelo fetch; as regras do banco têm testes próprios em supabase/tests (Postgres de verdade).
import { describe, expect, it, vi } from "vitest";
// @ts-expect-error módulo em JS puro, sem tipos
import { MAX_BODY_BYTES, parseReportQuery, validateBatch } from "../../site/api/_telemetry.mjs";
// @ts-expect-error módulo em JS puro, sem tipos
import * as ingest from "../../site/api/telemetry/ingest.mjs";
// @ts-expect-error módulo em JS puro, sem tipos
import * as forget from "../../site/api/telemetry/forget.mjs";
// @ts-expect-error módulo em JS puro, sem tipos
import * as report from "../../site/api/telemetry/report.mjs";
// @ts-expect-error módulo em JS puro, sem tipos
import * as config from "../../site/api/telemetry/config.mjs";

const NOW = new Date("2026-11-09T15:00:00Z");
const SERVICE = "sb_secret_servico_nao_vaza_1234567890";
const ANON = "sb_publishable_publica_1234567890";
const ENV = { SUPABASE_URL: "https://proj.supabase.co/", SUPABASE_SERVICE_ROLE_KEY: SERVICE, SUPABASE_ANON_KEY: ANON };

function batch(events: unknown[] = [], extra: Record<string, unknown> = {}) {
  return {
    schema: 1,
    batchId: "10000000-0000-4000-8000-000000000001",
    installId: "20000000-0000-4000-8000-000000000001",
    app: { version: "0.9.0", platform: "windows", arch: "x86_64", channel: "nsis" },
    events,
    ...extra,
  };
}

const post = (body: unknown, headers: Record<string, string> = {}) =>
  new Request("https://prancha-beta.vercel.app/api/telemetry/ingest", {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: typeof body === "string" ? body : JSON.stringify(body),
  });

function supabase(status: number, body: unknown) {
  return vi.fn(async (_url: string, _init: RequestInit) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
}

describe("validateBatch", () => {
  it("aceita um lote válido e devolve só os campos conhecidos", () => {
    const r = validateBatch(
      batch([
        { type: "session", day: "2026-11-09", launches: 1 },
        { type: "source", day: "2026-11-09", source: "mangadex/pt-br", queries: 3 },
        { type: "error", day: "2026-11-09", hour: 14, category: "network", component: "sources", code: "timeout", count: 2 },
        { type: "perf", day: "2026-11-08", metric: "startup", count: 1, totalMs: 900, maxMs: 900 },
      ]),
      NOW,
    );
    expect(r.ok).toBe(true);
    expect(r.batch.events[1]).toEqual({ type: "source", day: "2026-11-09", source: "mangadex/pt-br", queries: 3, opens: 0, chapters: 0, errors: 0 });
  });

  it.each([
    ["campo a mais no lote", batch([], { ip: "1.2.3.4" })],
    ["campo a mais no evento", batch([{ type: "error", day: "2026-11-09", hour: 1, category: "network", component: "x", code: "y", count: 1, message: "Falha ao abrir C:\\Users\\ana" }])],
    ["título de obra", batch([{ type: "title", day: "2026-11-09", title: "One Piece" }])],
    ["caminho como fonte", batch([{ type: "source", day: "2026-11-09", source: "C:\\Users\\ana/x", queries: 1 }])],
    ["URL como fonte", batch([{ type: "source", day: "2026-11-09", source: "https://site.com/x", queries: 1 }])],
    ["e-mail como código", batch([{ type: "error", day: "2026-11-09", hour: 1, category: "network", component: "x", code: "ana@example.com", count: 1 }])],
    ["texto livre como código", batch([{ type: "error", day: "2026-11-09", hour: 1, category: "network", component: "x", code: "Falha de rede", count: 1 }])],
    ["categoria desconhecida", batch([{ type: "error", day: "2026-11-09", hour: 1, category: "senha", component: "x", code: "y", count: 1 }])],
    ["data antiga", batch([{ type: "session", day: "2026-10-01", launches: 1 }])],
    ["data futura", batch([{ type: "session", day: "2026-11-12", launches: 1 }])],
    ["contagem negativa", batch([{ type: "perf", day: "2026-11-09", metric: "startup", count: 1, totalMs: -1, maxMs: 1 }])],
    ["versão inválida", batch([], { app: { version: "0.9.0-beta", platform: "windows" } })],
    ["plataforma desconhecida", batch([], { app: { version: "0.9.0", platform: "android" } })],
    ["identificador que não é UUID", batch([], { installId: "ana@example.com" })],
    ["schema desconhecido", batch([], { schema: 2 })],
    ["eventos demais", batch(Array.from({ length: 201 }, () => ({ type: "session", day: "2026-11-09", launches: 1 })))],
  ])("recusa %s", (_why, input) => {
    expect(validateBatch(input, NOW).ok).toBe(false);
  });
});

describe("POST /api/telemetry/ingest", () => {
  it("sem as variáveis de ambiente responde 503 e não chama o Supabase", async () => {
    const fetchImpl = supabase(200, {});
    const res = await ingest.POST(post(batch()), { env: {}, fetchImpl });
    expect(res.status).toBe(503);
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("grava pelo telemetry_ingest com a chave de serviço e não devolve a chave", async () => {
    const fetchImpl = supabase(200, { accepted: 1, duplicate: false });
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOW);
    const res = await ingest.POST(post(batch([{ type: "session", day: "2026-11-09", launches: 1 }])), { env: ENV, fetchImpl });
    vi.useRealTimers();
    expect(res.status).toBe(200);
    const text = await res.text();
    expect(JSON.parse(text)).toEqual({ accepted: 1, duplicate: false });
    expect(text).not.toContain(SERVICE);
    expect(res.headers.get("cache-control")).toBe("no-store");
    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(url).toBe("https://proj.supabase.co/rest/v1/rpc/telemetry_ingest");
    expect((init.headers as Record<string, string>).apikey).toBe(SERVICE);
    expect(JSON.parse(String(init.body)).batch.installId).toBe("20000000-0000-4000-8000-000000000001");
  });

  it("recusa antes do Supabase o que não passa na validação, o corpo grande e o tipo errado", async () => {
    const fetchImpl = supabase(200, {});
    expect((await ingest.POST(post(batch([{ type: "title", day: "2026-11-09" }])), { env: ENV, fetchImpl })).status).toBe(400);
    expect((await ingest.POST(post("x".repeat(MAX_BODY_BYTES + 1)), { env: ENV, fetchImpl })).status).toBe(413);
    expect((await ingest.POST(post(batch(), { "content-type": "text/plain" }), { env: ENV, fetchImpl })).status).toBe(415);
    expect((await ingest.POST(post("{não é json"), { env: ENV, fetchImpl })).status).toBe(400);
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("traduz os erros do banco: frequência vira 429, validação vira 400, falha vira 502", async () => {
    const rate = await ingest.POST(post(batch()), { env: ENV, fetchImpl: supabase(400, { code: "P0001", hint: "rate_install", message: "telemetry: rate" }) });
    expect(rate.status).toBe(429);
    expect(rate.headers.get("retry-after")).toBe("3600");
    expect((await ingest.POST(post(batch()), { env: ENV, fetchImpl: supabase(400, { code: "22023" }) })).status).toBe(400);
    expect((await ingest.POST(post(batch()), { env: ENV, fetchImpl: supabase(500, { message: "boom" }) })).status).toBe(502);
    const offline = vi.fn(async () => {
      throw new TypeError("fetch failed");
    });
    const res = await ingest.POST(post(batch()), { env: ENV, fetchImpl: offline });
    expect(res.status).toBe(502);
    expect(await res.text()).not.toContain("fetch failed");
  });

  it("GET não é aceito", async () => {
    expect(ingest.GET().status).toBe(405);
  });
});

describe("POST /api/telemetry/forget", () => {
  it("só aceita o identificador e chama telemetry_forget", async () => {
    const fetchImpl = supabase(200, { removed: 4 });
    const req = (body: unknown) => new Request("https://x/api/telemetry/forget", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
    expect((await forget.POST(req({ installId: "x" }), { env: ENV, fetchImpl })).status).toBe(400);
    expect((await forget.POST(req({ installId: "20000000-0000-4000-8000-000000000001", extra: 1 }), { env: ENV, fetchImpl })).status).toBe(400);
    const ok = await forget.POST(req({ installId: "20000000-0000-4000-8000-000000000001" }), { env: ENV, fetchImpl });
    expect(ok.status).toBe(200);
    expect(fetchImpl.mock.calls[0]![0]).toBe("https://proj.supabase.co/rest/v1/rpc/telemetry_forget");
  });
});

describe("GET /api/telemetry/report", () => {
  const get = (query = "", auth?: string) => new Request(`https://x/api/telemetry/report${query}`, { headers: auth ? { authorization: auth } : {} });
  const TOKEN = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.assinatura_do_token";

  it("sem login responde 401 e não chama o Supabase", async () => {
    const fetchImpl = supabase(200, {});
    expect((await report.GET(get(), { env: ENV, fetchImpl })).status).toBe(401);
    expect((await report.GET(get("", "Basic abc"), { env: ENV, fetchImpl })).status).toBe(401);
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("repassa o token da pessoa com a chave pública, nunca a de serviço", async () => {
    const fetchImpl = supabase(200, { totals: { installs: 3 } });
    const res = await report.GET(get("?from=2026-10-01&to=2026-11-09&version=0.9.0", `Bearer ${TOKEN}`), { env: ENV, fetchImpl });
    expect(res.status).toBe(200);
    const init = fetchImpl.mock.calls[0]![1];
    const headers = init.headers as Record<string, string>;
    expect(headers.apikey).toBe(ANON);
    expect(headers.authorization).toBe(`Bearer ${TOKEN}`);
    expect(JSON.stringify(init)).not.toContain(SERVICE);
    expect(JSON.parse(String(init.body))).toEqual({ p_from: "2026-10-01", p_to: "2026-11-09", p_version: "0.9.0", p_category: null });
  });

  it("quem não é administrador recebe 403", async () => {
    const res = await report.GET(get("", `Bearer ${TOKEN}`), { env: ENV, fetchImpl: supabase(403, { code: "42501", message: "telemetry: forbidden" }) });
    expect(res.status).toBe(403);
    expect(await res.json()).toEqual({ error: "forbidden" });
  });

  it("token vencido vira 401, para o painel pedir login de novo", async () => {
    const res = await report.GET(get("", `Bearer ${TOKEN}`), { env: ENV, fetchImpl: supabase(401, { code: "PGRST301", message: "JWT expired" }) });
    expect(res.status).toBe(401);
  });

  it("confere o período e os filtros", () => {
    expect(parseReportQuery(new URLSearchParams("from=2026-11-09&to=2026-11-01"), NOW).ok).toBe(false);
    expect(parseReportQuery(new URLSearchParams("from=2024-01-01&to=2026-11-01"), NOW).ok).toBe(false);
    expect(parseReportQuery(new URLSearchParams("version=latest"), NOW).ok).toBe(false);
    expect(parseReportQuery(new URLSearchParams("category=senha"), NOW).ok).toBe(false);
    expect(parseReportQuery(new URLSearchParams(""), NOW)).toEqual({ ok: true, query: { p_from: "2026-10-11", p_to: "2026-11-09", p_version: null, p_category: null } });
  });
});

describe("GET /api/telemetry/config", () => {
  it("entrega só o endereço e a chave pública", async () => {
    const res = config.GET(new Request("https://x/api/telemetry/config"), { env: ENV });
    const text = await res.text();
    expect(JSON.parse(text)).toEqual({ supabaseUrl: "https://proj.supabase.co", anonKey: ANON });
    expect(text).not.toContain(SERVICE);
  });
});
