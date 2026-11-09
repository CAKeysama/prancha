// POST /api/telemetry/forget: apaga tudo que uma instalação enviou. Só quem tem o identificador (o próprio app) pede.
import { json, readJson, rpc, serverConfig } from "../_telemetry.mjs";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export async function POST(request, { env = process.env, fetchImpl = fetch } = {}) {
  const config = serverConfig(env);
  if (!config.url || !config.serviceKey) return json(503, { error: "not_configured" });
  const body = await readJson(request, 1024);
  if (!body.ok) return json(body.status, { error: "invalid_body" });
  const installId = body.value?.installId;
  if (typeof installId !== "string" || !UUID.test(installId) || Object.keys(body.value).length !== 1) return json(400, { error: "invalid" });
  const result = await rpc(config, "telemetry_forget", { install: installId }, { key: config.serviceKey, fetchImpl });
  if (!result.ok) return json(result.status, { error: result.code });
  return json(200, { removed: result.value?.removed ?? 0 });
}

export function GET() {
  return json(405, { error: "method_not_allowed" }, { allow: "POST" });
}
