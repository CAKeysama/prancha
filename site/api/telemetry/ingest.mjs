// POST /api/telemetry/ingest: recebe um lote do aplicativo, confere e grava pelo Supabase.
// Não guarda IP, cabeçalhos nem o corpo. Ver docs/TELEMETRY.md.
import { json, readJson, rpc, serverConfig, validateBatch } from "../_telemetry.mjs";

export async function POST(request, { env = process.env, fetchImpl = fetch } = {}) {
  const config = serverConfig(env);
  if (!config.url || !config.serviceKey) return json(503, { error: "not_configured" });
  const body = await readJson(request);
  if (!body.ok) return json(body.status, { error: "invalid_body" });
  const checked = validateBatch(body.value);
  if (!checked.ok) return json(400, { error: "invalid", field: checked.error });
  const result = await rpc(config, "telemetry_ingest", { batch: checked.batch }, { key: config.serviceKey, fetchImpl });
  if (!result.ok) return json(result.status, { error: result.code }, result.status === 429 ? { "retry-after": "3600" } : {});
  return json(200, { accepted: result.value?.accepted ?? 0, duplicate: Boolean(result.value?.duplicate) });
}

export function GET() {
  return json(405, { error: "method_not_allowed" }, { allow: "POST" });
}
