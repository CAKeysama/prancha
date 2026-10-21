import type { ComicSummary, ReadStatus } from "./types";

export const STATUS_LABELS: Record<ReadStatus, string> = {
  unread: "Não iniciado",
  reading: "Em andamento",
  completed: "Concluído",
};

/** Total de páginas conhecido: o do progresso salvo tem prioridade (é o que o leitor confirmou). */
export function knownPageCount(c: Pick<ComicSummary, "pageCount" | "progress">): number | null {
  const fromProgress = c.progress?.pageCount ?? 0;
  if (fromProgress > 0) return fromProgress;
  return c.pageCount && c.pageCount > 0 ? c.pageCount : null;
}

/**
 * Texto do indicador de progresso. Nunca inventa uma posição:
 * itens nunca abertos mostram "Não iniciado" e totais desconhecidos não são exibidos.
 */
export function progressLabel(c: Pick<ComicSummary, "status" | "pageCount" | "progress">): string {
  if (c.status === "completed") return "Leitura concluída";
  if (c.status === "unread" || !c.progress) return "Não iniciado";
  const total = knownPageCount(c);
  const page = c.progress.pageIndex + 1;
  return total ? `Parou na página ${page} de ${total}` : `Parou na página ${page}`;
}

/** Fração lida (0–1) ou `null` quando não há dado real para mostrar. */
export function progressRatio(c: Pick<ComicSummary, "status" | "pageCount" | "progress">): number | null {
  if (c.status === "completed") return 1;
  if (c.status === "unread" || !c.progress) return null;
  const total = knownPageCount(c);
  if (!total) return null;
  return Math.min(1, Math.max(0, (c.progress.pageIndex + 1) / total));
}

export function readActionLabel(c: Pick<ComicSummary, "status">): string {
  if (c.status === "reading") return "Continuar leitura";
  if (c.status === "completed") return "Ler novamente";
  return "Ler";
}

export function subtitle(c: Pick<ComicSummary, "series" | "issueNumber" | "volume">): string | null {
  const parts: string[] = [];
  if (c.series) parts.push(c.series);
  if (c.volume) parts.push(`vol. ${c.volume}`);
  if (c.issueNumber) parts.push(`#${c.issueNumber}`);
  return parts.length ? parts.join(" · ") : null;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  const value = bytes / 1024 ** i;
  return `${value.toLocaleString("pt-BR", { maximumFractionDigits: i === 0 ? 0 : 1 })} ${units[i]}`;
}

export function formatDate(ms: number | null | undefined): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString("pt-BR", { dateStyle: "medium", timeStyle: "short" });
}
