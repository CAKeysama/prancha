// GET /api/telemetry/report: números agregados para o painel. Repassa o token da pessoa ao Supabase:
// quem decide se ela é administradora é a função telemetry_report, não esta. Nenhuma chave privilegiada aqui.
import { json, parseReportQuery, rpc, serverConfig } from "../_telemetry.mjs";

export async function GET(request, { env = process.env, fetchImpl = fetch } = {}) {
  const config = serverConfig(env);
  if (!config.url || !config.anonKey) return json(503, { error: "not_configured" });
  const auth = request.headers.get("authorization") ?? "";
  const token = /^Bearer ([A-Za-z0-9._-]{20,4096})$/.exec(auth)?.[1];
  if (!token) return json(401, { error: "unauthorized" });
  const parsed = parseReportQuery(new URL(request.url).searchParams);
  if (!parsed.ok) return json(400, { error: "invalid", field: parsed.error });
  const result = await rpc(config, "telemetry_report", parsed.query, { key: config.anonKey, bearer: token, fetchImpl });
  if (!result.ok) return json(result.status, { error: result.code });
  return json(200, result.value);
}
