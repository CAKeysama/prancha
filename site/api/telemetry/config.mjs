// GET /api/telemetry/config: o endereço do Supabase e a chave pública (publishable/anon), para o login do painel.
// A chave pública não dá acesso a nada da telemetria: as tabelas ficam fora da API e as funções conferem o papel.
import { json, serverConfig } from "../_telemetry.mjs";

export function GET(_request, { env = process.env } = {}) {
  const config = serverConfig(env);
  if (!config.url || !config.anonKey) return json(503, { error: "not_configured" });
  return json(200, { supabaseUrl: config.url, anonKey: config.anonKey });
}
