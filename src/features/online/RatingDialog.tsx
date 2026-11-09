import { useEffect, useState } from "react";
import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { ErrorState, Spinner } from "../../components/Feedback";
import { TextField } from "../../components/Form";
import { useToast } from "../../components/Toast";
import { formatDate } from "../../domain/progress";
import {
  EVIDENCE_KIND_LABELS,
  EVIDENCE_VERDICT_LABELS,
  RATING_ORIGIN_LABELS,
  RATING_STATE_HELP,
  RATING_STATE_LABELS,
  type RatingDetails,
} from "../../domain/rating";
import { api, errorMessage } from "../../services/api";
import { Fact } from "./parts";

/** Só quando a pessoa pede: por que a obra tem a classificação que tem, e como corrigir. Montado ao abrir. */
export function RatingDialog({ seriesId, title, onClose, onChanged }: { seriesId: number; title: string; onClose: () => void; onChanged: (details: RatingDetails) => void }) {
  const toast = useToast();
  const [details, setDetails] = useState<RatingDetails | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reason, setReason] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .seriesRating(seriesId)
      .then((found) => alive && setDetails(found))
      .catch((e: unknown) => alive && setError(errorMessage(e)));
    return () => {
      alive = false;
    };
  }, [seriesId]);

  const correct = async (rating: "confirmed" | "none" | null) => {
    setSaving(true);
    try {
      const after = await api.seriesSetRating(seriesId, rating, reason.trim() || null);
      setDetails(after);
      setReason("");
      onChanged(after);
    } catch (e) {
      toast.error("Não foi possível corrigir a classificação", errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const evidence = details?.evidence ?? [];
  return (
    <Dialog
      open
      title="Classificação +18"
      description={title}
      onClose={onClose}
      footer={
        details && (
          <>
            {details.manual ? (
              <Button onClick={() => void correct(null)} loading={saving}>
                Voltar ao automático
              </Button>
            ) : null}
            {details.state !== "none" && (
              <Button onClick={() => void correct("none")} loading={saving}>
                Não é +18
              </Button>
            )}
            {details.state !== "confirmed" && (
              <Button variant="danger" onClick={() => void correct("confirmed")} loading={saving}>
                Marcar como +18
              </Button>
            )}
          </>
        )
      }
    >
      {error && <ErrorState title="Não foi possível ver a classificação" message={error} />}
      {!details && !error && <Spinner label="Carregando…" />}
      {details && (
        <div>
          <dl>
            <Fact label="Estado">
              {RATING_STATE_LABELS[details.state]}
              <span className="block text-sm text-muted">{RATING_STATE_HELP[details.state]}</span>
            </Fact>
            <Fact label="Origem">
              {RATING_ORIGIN_LABELS[details.origin]}
              {details.manual && details.automaticState !== details.state && <span className="block text-sm text-muted">Automático: {RATING_STATE_LABELS[details.automaticState]}</span>}
              {details.manual?.reason && <span className="block text-sm text-muted">Motivo: {details.manual.reason}</span>}
            </Fact>
            <Fact label="Pontuação de risco">{details.score} de 100</Fact>
            <Fact label="Última análise">{formatDate(details.ratedAt)}</Fact>
          </dl>
          <h3 className="mt-4 mb-1 text-sm font-semibold">Evidências</h3>
          {evidence.length === 0 ? (
            <p className="text-sm text-muted">Nenhuma evidência encontrada nos metadados desta obra.</p>
          ) : (
            <ul className="text-sm" aria-label="Evidências">
              {evidence.map((item, at) => (
                <li key={at} className="border-b border-border py-1.5 last:border-b-0">
                  <span className="font-medium">{EVIDENCE_KIND_LABELS[item.kind] ?? item.kind}:</span> {item.value} <span className="text-muted">({EVIDENCE_VERDICT_LABELS[item.verdict] ?? item.verdict})</span>
                  <span className="block text-xs text-muted">{item.origin}</span>
                </li>
              ))}
            </ul>
          )}
          {details.history.length > 0 && (
            <>
              <h3 className="mt-4 mb-1 text-sm font-semibold">Histórico</h3>
              <ul className="text-sm text-muted" aria-label="Histórico">
                {details.history.map((change, at) => (
                  <li key={at} className="py-0.5">
                    {formatDate(change.changedAt)}: {change.previousState ? `${RATING_STATE_LABELS[change.previousState]} → ` : ""}
                    {RATING_STATE_LABELS[change.newState]} ({change.origin === "manual" ? "por você" : "automático"}){change.reason ? `, ${change.reason}` : ""}
                  </li>
                ))}
              </ul>
            </>
          )}
          <TextField className="mt-4" label="Motivo da correção (opcional)" value={reason} maxLength={500} onChange={(e) => setReason(e.target.value)} />
          <p className="mt-2 text-xs text-muted">A correção vale só para esta obra, neste computador, e continua valendo quando a fonte atualizar os dados.</p>
        </div>
      )}
    </Dialog>
  );
}
