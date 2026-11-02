// URLs do protocolo `prancha://` (no Windows o WebView2 o expõe como http://prancha.localhost/).

import { convertFileSrc } from "@tauri-apps/api/core";

let base: string | null = null;

function protocolBase(): string {
  if (base === null) {
    try {
      base = convertFileSrc("", "prancha");
    } catch {
      base = "http://prancha.localhost/";
    }
    if (!base.endsWith("/")) base += "/";
  }
  return base;
}

export const coverUrl = (id: number, rev: number) => `${protocolBase()}cover/${id}?v=${rev}`;
export const pageUrl = (id: number, index: number, token = 0) =>
  `${protocolBase()}page/${id}/${index}${token ? `?r=${token}` : ""}`;
export const fileUrl = (id: number) => `${protocolBase()}file/${id}`;
export const onlineCoverUrl = (id: number) => `${protocolBase()}online-cover/${id}`;

/** Lê a mensagem de erro do protocolo (o <img> não expõe o motivo da falha). */
export async function fetchPageError(url: string): Promise<string> {
  try {
    const res = await fetch(url);
    if (res.ok) return "A página não pôde ser exibida (imagem em formato não suportado pelo visualizador).";
    const text = await res.text();
    return text || `Erro ${res.status} ao carregar a página.`;
  } catch {
    return "Não foi possível carregar a página.";
  }
}
