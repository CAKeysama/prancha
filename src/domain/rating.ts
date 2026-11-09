/** Classificação +18 por evidências. Quem decide o estado é o backend; aqui ficam os nomes, os selos e as explicações. */

export type RatingState = "confirmed" | "suspect" | "none" | "insufficient";
export type RatingOrigin = "repository" | "inferred" | "manual";
/** O que fazer com obras suspeitas ou sem informações quando o filtro +18 está ligado. */
export type SuspectBehavior = "hide" | "mark" | "show";

export const RATING_STATE_LABELS: Record<RatingState, string> = {
  confirmed: "Confirmado +18",
  suspect: "Suspeito",
  none: "Sem indicação de +18",
  insufficient: "Sem informações suficientes",
};

/** Curtas, para quem pergunta o que cada estado quer dizer. Nenhuma promete que a obra foi verificada. */
export const RATING_STATE_HELP: Record<RatingState, string> = {
  confirmed: "Há evidências fortes e confiáveis de conteúdo adulto.",
  suspect: "Há indícios de conteúdo adulto, mas não o bastante para confirmar.",
  none: "Nada indica conteúdo adulto. Isso não quer dizer que a obra foi verificada.",
  insufficient: "A fonte não informa o bastante para avaliar a obra.",
};

export const RATING_ORIGIN_LABELS: Record<RatingOrigin, string> = {
  repository: "Informada pela fonte",
  inferred: "Deduzida pelo Prancha",
  manual: "Corrigida por você",
};

export const EVIDENCE_KIND_LABELS: Record<string, string> = {
  rating: "Classificação da obra",
  source_rating: "Classificação da fonte",
  tag: "Categoria",
  description: "Descrição",
  title: "Título",
  alt_title: "Título alternativo",
  cover: "Capa",
  history: "Histórico",
};

export const EVIDENCE_VERDICT_LABELS: Record<string, string> = {
  adult: "indica +18",
  suggestive: "indício",
  safe: "indica obra livre",
  ignored: "desconsiderada",
  superseded: "substituída por informação mais nova",
  inconclusive: "inconclusiva",
};

export const SUSPECT_BEHAVIOR_OPTIONS: { value: SuspectBehavior; label: string }[] = [
  { value: "hide", label: "Esconder" },
  { value: "mark", label: "Mostrar com aviso" },
  { value: "show", label: "Mostrar" },
];

export interface RatingEvidence {
  kind: string;
  origin: string;
  value: string;
  reliability: number;
  points: number;
  verdict: string;
  observedAt: number;
}

export interface RatingDetails {
  state: RatingState;
  /** O que o classificador concluiu, mesmo com uma correção valendo. */
  automaticState: RatingState;
  score: number;
  origin: RatingOrigin;
  ratedAt: number | null;
  classifierVersion: number;
  evidence: RatingEvidence[];
  manual: { state: RatingState; reason: string | null; setAt: number } | null;
  history: { previousState: RatingState | null; newState: RatingState; origin: "manual" | "automatic"; reason: string | null; changedAt: number }[];
}

export interface RatingBadge {
  label: string;
  /** `strong` para +18 confirmado, `soft` para dúvida. */
  tone: "strong" | "soft";
  title: string;
}

/**
 * O selo de uma obra que está aparecendo. Uma regra só para todas as telas: +18 confirmado sempre leva selo;
 * suspeita e falta de informação levam, a não ser que a pessoa tenha escolhido mostrar sem aviso.
 * O que não aparece não chega aqui: quem esconde é o backend.
 */
export function ratingBadge(state: string | undefined, suspect: SuspectBehavior): RatingBadge | null {
  if (state === "confirmed") return { label: "+18", tone: "strong", title: RATING_STATE_LABELS.confirmed };
  if (suspect === "show") return null;
  if (state === "suspect") return { label: "+18?", tone: "soft", title: RATING_STATE_LABELS.suspect };
  if (state === "insufficient") return { label: "?", tone: "soft", title: RATING_STATE_LABELS.insufficient };
  return null;
}

export function isRatingState(value: unknown): value is RatingState {
  return value === "confirmed" || value === "suspect" || value === "none" || value === "insufficient";
}
