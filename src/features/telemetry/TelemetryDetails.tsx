import { Check, X } from "lucide-react";
import { TELEMETRY_COLLECTED, TELEMETRY_LIMITS, TELEMETRY_NOT_COLLECTED, TELEMETRY_PURPOSE } from "../../domain/telemetry";

/** O aviso de privacidade, o mesmo no convite e nas configurações. */
export function TelemetryDetails({ compact }: { compact?: boolean }) {
  return (
    <div className="space-y-3 text-sm">
      <p>{TELEMETRY_PURPOSE}</p>
      <div className={compact ? "grid gap-3" : "grid gap-4 md:grid-cols-2"}>
        <div>
          <p className="font-medium">O que é enviado</p>
          <ul className="mt-1 space-y-1">
            {TELEMETRY_COLLECTED.map((item) => (
              <li key={item} className="flex gap-2 text-muted">
                <Check size={16} className="mt-0.5 shrink-0 text-success" aria-hidden />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </div>
        <div>
          <p className="font-medium">O que nunca é enviado</p>
          <ul className="mt-1 space-y-1">
            {TELEMETRY_NOT_COLLECTED.map((item) => (
              <li key={item} className="flex gap-2 text-muted">
                <X size={16} className="mt-0.5 shrink-0 text-danger" aria-hidden />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>
      <p className="text-muted">{TELEMETRY_LIMITS}</p>
    </div>
  );
}
