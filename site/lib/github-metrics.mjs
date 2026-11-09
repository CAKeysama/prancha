// Métricas públicas do projeto: releases, downloads dos instaladores e execuções do GitHub Actions.
// Tudo vem da API pública do GitHub. Nada aqui lê ou guarda dado de quem usa o Prancha.
// Quem chama é a função api/github-metrics.mjs, na Vercel; o token, se houver, fica só na variável GITHUB_TOKEN de lá.

export const REPO = "CAKeysama/prancha";
const API = "https://api.github.com";
/** Quanto tempo um resumo vale antes de consultar o GitHub de novo. */
export const TTL_MS = 15 * 60 * 1000;
/** Até quando um resumo antigo ainda serve se o GitHub falhar. */
export const STALE_MS = 24 * 60 * 60 * 1000;
/** Depois de uma falha comum, espera este tempo antes de tentar de novo. */
export const ERROR_BACKOFF_MS = 60 * 1000;
export const CI_WINDOW_DAYS = 30;
const RUN_PAGES = 3;
/** Execuções que ganham o detalhe por job (espera e execução separadas). Cada uma custa uma chamada. */
export const JOB_DETAIL_RUNS = 20;
const JOB_CONCURRENCY = 5;
const RECENT_RUNS = 15;
const TIMEOUT_MS = 8000;

const FAILED = new Set(["failure", "timed_out", "startup_failure"]);

export class GitHubError extends Error {
  constructor(message, { status = 0, rateLimited = false, resetAt = null } = {}) {
    super(message);
    this.name = "GitHubError";
    this.status = status;
    this.rateLimited = rateLimited;
    this.resetAt = resetAt;
  }
}

/** Cliente mínimo. O token só entra no cabeçalho da chamada ao GitHub, nunca no que é devolvido. */
export function createClient({ fetch = globalThis.fetch, token = "", repo = REPO } = {}) {
  const headers = { Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28", "User-Agent": "prancha-site" };
  if (token) headers.Authorization = `Bearer ${token}`;
  return async function get(path, params = {}) {
    const url = new URL(`${API}/repos/${repo}${path}`);
    for (const [k, v] of Object.entries(params)) url.searchParams.set(k, String(v));
    let res;
    try {
      res = await fetch(url, { headers, signal: AbortSignal.timeout(TIMEOUT_MS) });
    } catch (e) {
      throw new GitHubError(`Sem resposta do GitHub (${e?.name === "TimeoutError" ? "tempo esgotado" : "falha de rede"}).`);
    }
    if (res.ok) return res.json();
    const remaining = res.headers.get("x-ratelimit-remaining");
    const retryAfter = Number(res.headers.get("retry-after"));
    if (res.status === 429 || (res.status === 403 && (remaining === "0" || retryAfter > 0))) {
      const reset = Number(res.headers.get("x-ratelimit-reset"));
      const resetAt = retryAfter > 0 ? Date.now() + retryAfter * 1000 : reset > 0 ? reset * 1000 : Date.now() + ERROR_BACKOFF_MS;
      throw new GitHubError("O GitHub limitou as consultas por agora.", { status: res.status, rateLimited: true, resetAt });
    }
    throw new GitHubError(`O GitHub respondeu ${res.status}.`, { status: res.status });
  };
}

const seconds = (from, to) => {
  const a = Date.parse(from);
  const b = Date.parse(to);
  return Number.isFinite(a) && Number.isFinite(b) && b >= a ? Math.round((b - a) / 1000) : null;
};
const average = (xs) => (xs.length ? Math.round(xs.reduce((s, x) => s + x, 0) / xs.length) : null);
const median = (xs) => {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : Math.round((s[mid - 1] + s[mid]) / 2);
};

/** Só os instaladores contam como download. Assinaturas, latest.json e SHA256SUMS.txt ficam de fora. */
export function installerKind(name) {
  if (name.endsWith("-setup.exe")) return "exe";
  if (name.endsWith(".msi")) return "msi";
  return null;
}

export function summarizeReleases(raw) {
  const releases = raw
    .filter((r) => !r.draft && !r.prerelease && r.published_at)
    .sort((a, b) => Date.parse(b.published_at) - Date.parse(a.published_at))
    .map((r) => {
      const assets = (r.assets ?? [])
        .map((a) => ({ name: a.name, kind: installerKind(a.name), downloads: a.download_count ?? 0, size: a.size ?? 0, url: a.browser_download_url }))
        .filter((a) => a.kind)
        .sort((a, b) => Number(b.kind === "exe") - Number(a.kind === "exe"));
      return {
        version: r.tag_name.replace(/^v/, ""),
        tag: r.tag_name,
        name: r.name || r.tag_name,
        publishedAt: r.published_at,
        url: r.html_url,
        downloads: assets.reduce((s, a) => s + a.downloads, 0),
        assets,
      };
    });
  const total = releases.reduce((s, r) => s + r.downloads, 0);
  const byKind = { exe: 0, msi: 0 };
  for (const r of releases) for (const a of r.assets) byKind[a.kind] += a.downloads;
  return {
    latest: releases[0] ?? null,
    count: releases.length,
    downloads: { total, byKind },
    // A fatia de cada versão no total. O GitHub só informa o total atual de cada arquivo, sem histórico por dia.
    releases: releases.map((r) => ({ ...r, share: total ? r.downloads / total : 0 })),
  };
}

/** Espera = do disparo até o primeiro job começar numa máquina. Execução = do início do primeiro job ao fim do último. */
export function jobTiming(run, jobs) {
  const ran = jobs.filter((j) => j.started_at && j.completed_at && j.conclusion !== "skipped");
  if (!ran.length) return null;
  const firstStart = ran.map((j) => j.started_at).sort()[0];
  const lastEnd = ran.map((j) => j.completed_at).sort().at(-1);
  return { queueSeconds: seconds(run.created_at, firstStart) ?? 0, executionSeconds: seconds(firstStart, lastEnd) };
}

function outcome(run) {
  if (run.status !== "completed") return "running";
  if (run.conclusion === "success") return "success";
  if (FAILED.has(run.conclusion)) return "failure";
  return "other"; // cancelada, pulada, neutra: fora da taxa de sucesso
}

function stats(runs) {
  const done = runs.filter((r) => r.outcome === "success" || r.outcome === "failure");
  const success = done.filter((r) => r.outcome === "success").length;
  const durations = done.map((r) => r.durationSeconds).filter((x) => x != null);
  const detailed = runs.filter((r) => r.executionSeconds != null);
  return {
    runs: runs.length,
    success,
    failure: done.length - success,
    other: runs.filter((r) => r.outcome === "other").length,
    running: runs.filter((r) => r.outcome === "running").length,
    successRate: done.length ? success / done.length : null,
    avgDurationSeconds: average(durations),
    medianDurationSeconds: median(durations),
    // Só das execuções com detalhe por job, que são as mais recentes.
    sampled: detailed.length,
    avgQueueSeconds: average(detailed.map((r) => r.queueSeconds)),
    avgExecutionSeconds: average(detailed.map((r) => r.executionSeconds)),
  };
}

/** Uma linha por dia da janela, inclusive os dias sem execução, para o gráfico não pular datas. */
function daily(runs, now, days) {
  const out = [];
  const today = new Date(now);
  today.setUTCHours(0, 0, 0, 0);
  for (let i = days - 1; i >= 0; i--) {
    const date = new Date(today.getTime() - i * 86400000).toISOString().slice(0, 10);
    const s = stats(runs.filter((r) => r.createdAt.slice(0, 10) === date));
    out.push({ date, success: s.success, failure: s.failure, avgDurationSeconds: s.avgDurationSeconds });
  }
  return out;
}

export function summarizeRuns(rawRuns, timings = new Map(), { now = Date.now(), days = CI_WINDOW_DAYS } = {}) {
  const runs = rawRuns
    .map((r) => {
      const t = timings.get(r.id) ?? null;
      return {
        id: r.id,
        workflow: r.name,
        path: r.path,
        event: r.event,
        branch: r.head_branch,
        outcome: outcome(r),
        conclusion: r.conclusion,
        createdAt: r.created_at,
        url: r.html_url,
        // Duração total, como o GitHub mostra na execução: inclui a espera por máquina entre os jobs.
        durationSeconds: r.status === "completed" ? seconds(r.run_started_at ?? r.created_at, r.updated_at) : null,
        queueSeconds: t?.queueSeconds ?? null,
        executionSeconds: t?.executionSeconds ?? null,
      };
    })
    .sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt));

  const groups = new Map();
  for (const r of runs) {
    if (!groups.has(r.path)) groups.set(r.path, { name: r.workflow, path: r.path, runs: [] });
    groups.get(r.path).runs.push(r);
  }
  const workflows = [...groups.values()]
    .map((g) => ({ name: g.name, path: g.path, url: `https://github.com/${REPO}/actions/workflows/${g.path.split("/").pop()}`, ...stats(g.runs), daily: daily(g.runs, now, days), recent: g.runs.slice(0, RECENT_RUNS) }))
    .sort((a, b) => b.runs - a.runs);

  return { windowDays: days, summary: stats(runs), daily: daily(runs, now, days), workflows, recent: runs.slice(0, RECENT_RUNS) };
}

async function mapLimited(items, limit, fn) {
  const out = new Array(items.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, async () => {
      while (next < items.length) {
        const i = next++;
        out[i] = await fn(items[i]);
      }
    }),
  );
  return out;
}

/** Busca e resume tudo. Sem token, pula o detalhe por job para caber no limite da API pública (60 por hora). */
export async function collectMetrics({ fetch, token = "", now = Date.now() } = {}) {
  try {
    return await collect({ fetch, token, now });
  } catch (e) {
    // Token vencido ou revogado: a página continua com a API pública, só sem o detalhe por job.
    if (token && e instanceof GitHubError && e.status === 401) {
      console.warn("GITHUB_TOKEN recusado pelo GitHub (401); usando a API pública sem token.");
      return collect({ fetch, token: "", now });
    }
    throw e;
  }
}

async function collect({ fetch, token, now }) {
  const get = createClient({ fetch, token });
  const since = new Date(now - CI_WINDOW_DAYS * 86400000).toISOString().slice(0, 10);

  const [rawReleases, rawRuns] = await Promise.all([
    get("/releases", { per_page: 100 }),
    (async () => {
      const all = [];
      for (let page = 1; page <= RUN_PAGES; page++) {
        const { workflow_runs: runs = [] } = await get("/actions/runs", { per_page: 100, page, created: `>=${since}`, exclude_pull_requests: true });
        all.push(...runs);
        if (runs.length < 100) break;
      }
      return all;
    })(),
  ]);

  const timings = new Map();
  if (token) {
    const sample = rawRuns.filter((r) => r.status === "completed").slice(0, JOB_DETAIL_RUNS);
    await mapLimited(sample, JOB_CONCURRENCY, async (run) => {
      try {
        const { jobs = [] } = await get(`/actions/runs/${run.id}/jobs`, { per_page: 100 });
        const t = jobTiming(run, jobs);
        if (t) timings.set(run.id, t);
      } catch (e) {
        if (e instanceof GitHubError && e.rateLimited) throw e;
        // Uma execução sem detalhe só fica fora da média de espera.
      }
    });
  }

  return {
    repository: `https://github.com/${REPO}`,
    generatedAt: new Date(now).toISOString(),
    releases: summarizeReleases(rawReleases),
    ci: summarizeRuns(rawRuns, timings, { now }),
  };
}

/**
 * Guarda o último resumo na memória da função. Uma consulta por vez; com o GitHub fora ou no limite,
 * devolve o resumo anterior (até STALE_MS) marcado como desatualizado, e não insiste até o limite passar.
 */
export function createMetricsCache({ load, now = () => Date.now() }) {
  let value = null;
  let fetchedAt = 0;
  let blockedUntil = 0;
  let lastError = null;
  let inflight = null;

  return async function getMetrics() {
    const t = now();
    if (value && t - fetchedAt < TTL_MS) return { data: value, stale: false };
    if (t < blockedUntil) return fallback(t);
    inflight ??= load()
      .then((v) => {
        value = v;
        fetchedAt = now();
        lastError = null;
        return { data: v, stale: false };
      })
      .catch((e) => {
        lastError = e;
        blockedUntil = e instanceof GitHubError && e.rateLimited && e.resetAt ? e.resetAt : now() + ERROR_BACKOFF_MS;
        return fallback(now());
      })
      .finally(() => {
        inflight = null;
      });
    return inflight;
  };

  function fallback(t) {
    if (value && t - fetchedAt < STALE_MS) return { data: value, stale: true, error: lastError?.message ?? null };
    const err = lastError ?? new GitHubError("Métricas indisponíveis.");
    return { data: null, stale: false, error: err.message, retryAt: blockedUntil ? new Date(blockedUntil).toISOString() : null };
  }
}
