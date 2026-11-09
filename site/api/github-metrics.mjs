// GET /api/github-metrics: resumo público das releases, dos downloads e do CI do Prancha.
// O token do GitHub é opcional e só existe aqui, na variável de ambiente GITHUB_TOKEN da Vercel.
// Duas camadas de cache: a CDN da Vercel guarda a resposta (s-maxage) e a função guarda o último resumo na memória.
import { TTL_MS, collectMetrics, createMetricsCache } from "../lib/github-metrics.mjs";

const getMetrics = createMetricsCache({ load: () => collectMetrics({ token: process.env.GITHUB_TOKEN ?? "" }) });

const CDN_SECONDS = TTL_MS / 1000;

export function respond(result) {
  const headers = { "Content-Type": "application/json; charset=utf-8", "X-Content-Type-Options": "nosniff" };
  if (!result.data) {
    // Uma falha fica pouco tempo na CDN: evita martelar o GitHub sem esconder a volta dos dados.
    headers["Cache-Control"] = "public, max-age=0, s-maxage=60";
    return new Response(JSON.stringify({ error: result.error, retryAt: result.retryAt ?? null }), { status: 503, headers });
  }
  headers["Cache-Control"] = result.stale
    ? "public, max-age=60, s-maxage=60, stale-while-revalidate=600"
    : `public, max-age=300, s-maxage=${CDN_SECONDS}, stale-while-revalidate=86400`;
  return new Response(JSON.stringify({ ...result.data, stale: result.stale }), { status: 200, headers });
}

export async function handle(request, metrics = getMetrics) {
  if (request.method !== "GET" && request.method !== "HEAD") {
    return new Response(JSON.stringify({ error: "Método não permitido." }), { status: 405, headers: { Allow: "GET, HEAD", "Content-Type": "application/json; charset=utf-8" } });
  }
  return respond(await metrics());
}

export default { fetch: (request) => handle(request) };
