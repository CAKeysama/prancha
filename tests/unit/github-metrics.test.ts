// Métricas públicas do GitHub (site/lib/github-metrics.mjs e a função da Vercel em site/api/github-metrics.mjs).
// O GitHub é simulado: nenhum teste faz chamada de rede.
import { describe, expect, it, vi } from "vitest";
import {
  ERROR_BACKOFF_MS,
  GitHubError,
  JOB_DETAIL_RUNS,
  STALE_MS,
  TTL_MS,
  collectMetrics,
  createClient,
  createMetricsCache,
  jobTiming,
  summarizeReleases,
  summarizeRuns,
  // @ts-expect-error script em JS puro, sem tipos
} from "../../site/lib/github-metrics.mjs";
// @ts-expect-error script em JS puro, sem tipos
import { handle, respond } from "../../site/api/github-metrics.mjs";

const TOKEN = "ghp_segredoQueNaoPodeVazar123";
const NOW = Date.parse("2026-11-09T15:00:00Z");

const asset = (name: string, download_count: number) => ({ name, download_count, size: 1000, browser_download_url: `https://github.com/CAKeysama/prancha/releases/download/x/${name}` });
const release = (tag: string, published_at: string, extra: Record<string, unknown> = {}) => ({
  tag_name: tag,
  name: `Prancha ${tag}`,
  draft: false,
  prerelease: false,
  published_at,
  html_url: `https://github.com/CAKeysama/prancha/releases/tag/${tag}`,
  assets: [asset(`Prancha_${tag.slice(1)}_x64-setup.exe`, 10), asset(`Prancha_${tag.slice(1)}_x64_pt-BR.msi`, 5), asset("latest.json", 900), asset("SHA256SUMS.txt", 7), asset(`Prancha_${tag.slice(1)}_x64-setup.exe.sig`, 300)],
  ...extra,
});

let nextId = 1;
const run = (path: string, conclusion: string | null, created: string, minutes = 5, extra: Record<string, unknown> = {}) => ({
  id: nextId++,
  name: path.includes("ci") ? "Verificações" : "Publicar versão",
  path: `.github/workflows/${path}`,
  event: "push",
  head_branch: "main",
  status: conclusion ? "completed" : "in_progress",
  conclusion,
  created_at: created,
  run_started_at: created,
  updated_at: new Date(Date.parse(created) + minutes * 60000).toISOString(),
  html_url: `https://github.com/CAKeysama/prancha/actions/runs/${nextId}`,
  ...extra,
});

function json(body: unknown, status = 200, headers: Record<string, string> = {}) {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", ...headers } });
}

/** GitHub de mentira: responde releases, execuções e jobs, e guarda cada chamada. */
function fakeGitHub({ releases = [release("v1.0.0", "2026-11-08T10:00:00Z")], runs = [run("ci.yml", "success", "2026-11-09T10:00:00Z")], fail }: { releases?: unknown[]; runs?: unknown[]; fail?: (url: URL, init: RequestInit) => Response | undefined } = {}) {
  const calls: { url: URL; headers: Record<string, string> }[] = [];
  const fetch = vi.fn(async (input: URL, init: RequestInit) => {
    const url = new URL(String(input));
    calls.push({ url, headers: init.headers as Record<string, string> });
    const failed = fail?.(url, init);
    if (failed) return failed;
    if (url.pathname.endsWith("/releases")) return json(releases);
    if (url.pathname.endsWith("/actions/runs")) return json({ workflow_runs: url.searchParams.get("page") === "1" ? runs : [] });
    if (url.pathname.endsWith("/jobs"))
      return json({
        jobs: [
          { started_at: "2026-11-09T10:00:30Z", completed_at: "2026-11-09T10:03:00Z", conclusion: "success" },
          { started_at: "2026-11-09T10:00:20Z", completed_at: "2026-11-09T10:04:20Z", conclusion: "success" },
        ],
      });
    return json({ message: "Not Found" }, 404);
  });
  return { fetch, calls };
}

describe("releases e downloads", () => {
  it("conta só os instaladores das versões publicadas, da mais nova para a mais antiga", () => {
    const s = summarizeReleases([
      release("v0.1.0", "2026-10-01T00:00:00Z"),
      release("v0.3.0", "2026-11-01T00:00:00Z"),
      release("v0.2.0", "2026-10-15T00:00:00Z"),
      release("v0.4.0", "2026-11-02T00:00:00Z", { draft: true }),
      release("v0.5.0-beta", "2026-11-03T00:00:00Z", { prerelease: true }),
    ]);
    expect(s.count).toBe(3);
    expect(s.latest.version).toBe("0.3.0");
    expect(s.releases.map((r: { version: string }) => r.version)).toEqual(["0.3.0", "0.2.0", "0.1.0"]);
    // latest.json, assinaturas e SHA256SUMS.txt não são downloads de instalador.
    expect(s.releases[0].downloads).toBe(15);
    expect(s.releases[0].assets.map((a: { kind: string }) => a.kind)).toEqual(["exe", "msi"]);
    expect(s.downloads).toEqual({ total: 45, byKind: { exe: 30, msi: 15 } });
    expect(s.releases[0].share).toBeCloseTo(1 / 3);
  });

  it("sem nenhuma versão, devolve zeros em vez de quebrar", () => {
    expect(summarizeReleases([])).toEqual({ latest: null, count: 0, downloads: { total: 0, byKind: { exe: 0, msi: 0 } }, releases: [] });
  });
});

describe("execuções do CI", () => {
  it("separa a espera por máquina da execução e ignora jobs pulados", () => {
    const t = jobTiming({ created_at: "2026-11-09T10:00:00Z" }, [
      { started_at: "2026-11-09T10:00:20Z", completed_at: "2026-11-09T10:02:00Z", conclusion: "success" },
      { started_at: "2026-11-09T10:00:40Z", completed_at: "2026-11-09T10:05:20Z", conclusion: "success" },
      { started_at: "2026-11-09T10:09:00Z", completed_at: "2026-11-09T10:09:00Z", conclusion: "skipped" },
    ]);
    expect(t).toEqual({ queueSeconds: 20, executionSeconds: 300 });
    expect(jobTiming({ created_at: "2026-11-09T10:00:00Z" }, [])).toBeNull();
  });

  it("calcula a taxa de sucesso só com aprovadas e falhas, e a duração média", () => {
    const runs = [
      run("ci.yml", "success", "2026-11-09T10:00:00Z", 4),
      run("ci.yml", "success", "2026-11-09T11:00:00Z", 6),
      run("ci.yml", "failure", "2026-11-08T10:00:00Z", 8),
      run("ci.yml", "cancelled", "2026-11-08T11:00:00Z", 1),
      run("ci.yml", null, "2026-11-09T14:00:00Z"),
      run("release.yml", "timed_out", "2026-11-07T10:00:00Z", 30),
    ];
    const ci = summarizeRuns(runs, new Map(), { now: NOW, days: 30 });
    const main = ci.workflows.find((w: { path: string }) => w.path.endsWith("ci.yml"));
    expect(main).toMatchObject({ runs: 5, success: 2, failure: 1, other: 1, running: 1, avgDurationSeconds: 360, medianDurationSeconds: 360 });
    expect(main.successRate).toBeCloseTo(2 / 3);
    expect(ci.summary.failure).toBe(2);
    expect(ci.summary.successRate).toBe(0.5);
    // Uma linha por dia, inclusive sem execução, terminando hoje.
    expect(ci.daily).toHaveLength(30);
    expect(ci.daily.at(-1)).toEqual({ date: "2026-11-09", success: 2, failure: 0, avgDurationSeconds: 300 });
    expect(main.daily.find((d: { date: string }) => d.date === "2026-11-07")).toMatchObject({ success: 0, failure: 0 });
    expect(ci.recent[0].outcome).toBe("running");
    // Sem detalhe dos jobs, a espera fica desconhecida em vez de virar zero.
    expect(ci.summary.avgQueueSeconds).toBeNull();
  });

  it("sem execuções, as médias ficam vazias", () => {
    const ci = summarizeRuns([], new Map(), { now: NOW });
    expect(ci.summary).toMatchObject({ runs: 0, successRate: null, avgDurationSeconds: null });
    expect(ci.workflows).toEqual([]);
  });
});

describe("cliente do GitHub", () => {
  it("manda o token só no cabeçalho de autorização e nunca o devolve", async () => {
    const gh = fakeGitHub();
    const out = await collectMetrics({ fetch: gh.fetch, token: TOKEN, now: NOW });
    expect(gh.calls.every((c) => c.url.origin === "https://api.github.com")).toBe(true);
    expect(gh.calls.every((c) => c.headers.Authorization === `Bearer ${TOKEN}`)).toBe(true);
    expect(gh.calls.every((c) => !c.url.search.includes(TOKEN))).toBe(true);
    expect(JSON.stringify(out)).not.toContain(TOKEN);
    const res = await handle(new Request("https://prancha.test/api/github-metrics"), async () => ({ data: out, stale: false }));
    expect(await res.text()).not.toContain(TOKEN);
  });

  it("sem token, não pede o detalhe dos jobs para caber no limite público", async () => {
    const gh = fakeGitHub();
    const out = await collectMetrics({ fetch: gh.fetch, now: NOW });
    expect(gh.calls.some((c) => c.url.pathname.endsWith("/jobs"))).toBe(false);
    expect(gh.calls.every((c) => !("Authorization" in c.headers))).toBe(true);
    expect(out.ci.summary.sampled).toBe(0);
    expect(out.releases.latest.version).toBe("1.0.0");
  });

  it("com token, detalha no máximo as execuções mais recentes", async () => {
    const runs = Array.from({ length: JOB_DETAIL_RUNS + 10 }, (_, i) => run("ci.yml", "success", new Date(NOW - (i + 1) * 3600000).toISOString()));
    const gh = fakeGitHub({ runs });
    const out = await collectMetrics({ fetch: gh.fetch, token: TOKEN, now: NOW });
    expect(gh.calls.filter((c) => c.url.pathname.endsWith("/jobs"))).toHaveLength(JOB_DETAIL_RUNS);
    expect(out.ci.summary.sampled).toBe(JOB_DETAIL_RUNS);
    expect(out.ci.summary.avgQueueSeconds).toBeGreaterThanOrEqual(0);
    expect(out.ci.summary.avgExecutionSeconds).toBeGreaterThan(0);
  });

  it("com o token recusado, continua pela API pública", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const gh = fakeGitHub({ fail: (_url, init) => ((init.headers as Record<string, string>).Authorization ? json({ message: "Bad credentials" }, 401) : undefined) });
    const out = await collectMetrics({ fetch: gh.fetch, token: TOKEN, now: NOW });
    expect(out.releases.count).toBe(1);
    expect(warn).toHaveBeenCalledOnce();
    expect(String(warn.mock.calls[0])).not.toContain(TOKEN);
    warn.mockRestore();
  });

  it("reconhece o limite de requisições e guarda quando ele termina", async () => {
    const reset = Math.floor(NOW / 1000) + 600;
    const limited = createClient({ fetch: async () => json({ message: "API rate limit exceeded" }, 403, { "x-ratelimit-remaining": "0", "x-ratelimit-reset": String(reset) }) });
    await expect(limited("/releases")).rejects.toMatchObject({ rateLimited: true, resetAt: reset * 1000 });
    const secondary = createClient({ fetch: async () => json({}, 429, { "retry-after": "30" }) });
    const err = await secondary("/releases").catch((e: unknown) => e);
    expect(err).toBeInstanceOf(GitHubError);
    expect((err as { rateLimited: boolean }).rateLimited).toBe(true);
    const other = createClient({ fetch: async () => json({}, 500) });
    await expect(other("/releases")).rejects.toMatchObject({ status: 500, rateLimited: false });
    const offline = createClient({
      fetch: async () => {
        throw new TypeError("fetch failed");
      },
    });
    await expect(offline("/releases")).rejects.toThrow("falha de rede");
  });
});

describe("cache das métricas", () => {
  const sample = { generatedAt: "agora" };

  it("consulta o GitHub uma vez por período, mesmo com várias visitas ao mesmo tempo", async () => {
    let t = NOW;
    const load = vi.fn(async () => sample);
    const get = createMetricsCache({ load, now: () => t });
    const [a, b] = await Promise.all([get(), get()]);
    expect(a).toEqual({ data: sample, stale: false });
    expect(b).toEqual(a);
    await get();
    expect(load).toHaveBeenCalledTimes(1);
    t += TTL_MS + 1;
    await get();
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("com o GitHub fora, mostra o último resumo marcado como desatualizado e espera antes de tentar de novo", async () => {
    let t = NOW;
    let fail = false;
    const load = vi.fn(async () => {
      if (fail) throw new GitHubError("O GitHub respondeu 502.", { status: 502 });
      return sample;
    });
    const get = createMetricsCache({ load, now: () => t });
    await get();
    fail = true;
    t += TTL_MS + 1;
    expect(await get()).toMatchObject({ data: sample, stale: true, error: "O GitHub respondeu 502." });
    await get();
    expect(load).toHaveBeenCalledTimes(2); // não insiste durante a espera
    t += ERROR_BACKOFF_MS + 1;
    fail = false;
    expect(await get()).toEqual({ data: sample, stale: false });
    expect(load).toHaveBeenCalledTimes(3);
  });

  it("no limite de requisições, não chama o GitHub até o limite passar", async () => {
    let t = NOW;
    const load = vi.fn(async () => {
      throw new GitHubError("limite", { rateLimited: true, resetAt: NOW + 30 * 60000 });
    });
    const get = createMetricsCache({ load, now: () => t });
    expect(await get()).toMatchObject({ data: null, error: "limite", retryAt: new Date(NOW + 30 * 60000).toISOString() });
    t += 20 * 60000;
    await get();
    expect(load).toHaveBeenCalledTimes(1);
    t += 11 * 60000;
    await get();
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("não serve um resumo velho demais", async () => {
    let t = NOW;
    let fail = false;
    const get = createMetricsCache({
      load: async () => {
        if (fail) throw new Error("fora");
        return sample;
      },
      now: () => t,
    });
    await get();
    fail = true;
    t += STALE_MS + 1;
    expect((await get()).data).toBeNull();
  });
});

describe("função da Vercel", () => {
  it("responde com cache na CDN e diz quando os dados estão desatualizados", async () => {
    const ok = respond({ data: { a: 1 }, stale: false });
    expect(ok.status).toBe(200);
    expect(ok.headers.get("cache-control")).toContain(`s-maxage=${TTL_MS / 1000}`);
    expect(await ok.json()).toEqual({ a: 1, stale: false });
    const stale = respond({ data: { a: 1 }, stale: true });
    expect(stale.headers.get("cache-control")).toContain("s-maxage=60");
    expect((await stale.json()).stale).toBe(true);
  });

  it("sem dados, responde 503 com uma mensagem, em cache curto", async () => {
    const res = await handle(new Request("https://prancha.test/api/github-metrics"), async () => ({ data: null, error: "O GitHub limitou as consultas por agora.", retryAt: null }));
    expect(res.status).toBe(503);
    expect(res.headers.get("cache-control")).toContain("s-maxage=60");
    expect(await res.json()).toEqual({ error: "O GitHub limitou as consultas por agora.", retryAt: null });
  });

  it("só aceita leitura", async () => {
    const metrics = vi.fn();
    const res = await handle(new Request("https://prancha.test/api/github-metrics", { method: "POST", body: "{}" }), metrics);
    expect(res.status).toBe(405);
    expect(metrics).not.toHaveBeenCalled();
  });
});
