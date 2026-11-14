import { useState } from "react";
import { CircleAlert, CircleCheck, CircleMinus, FileUp, RefreshCw, X } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { ProgressBar, Spinner } from "../../components/Feedback";
import { Dialog } from "../../components/Dialog";
import { cn } from "../../components/cn";
import type { ImportItem } from "../../domain/types";
import { useImport, type ImportJob } from "./import-context";

const STATUS_TEXT: Record<ImportItem["status"], string> = {
  added: "Adicionado",
  relinked: "Localização atualizada",
  duplicate: "Já estava na biblioteca",
  skipped: "Ignorado",
  failed: "Falhou",
};

function ItemIcon({ status }: { status: ImportItem["status"] }) {
  if (status === "failed") return <CircleAlert size={16} className="shrink-0 text-danger" aria-hidden />;
  if (status === "added") return <CircleCheck size={16} className="shrink-0 text-success" aria-hidden />;
  if (status === "relinked") return <RefreshCw size={16} className="shrink-0 text-accent" aria-hidden />;
  return <CircleMinus size={16} className="shrink-0 text-attention" aria-hidden />;
}

function ImportResults({ items }: { items: ImportItem[] }) {
  const ordered = [...items].sort((a, b) => Number(b.status === "failed") - Number(a.status === "failed"));
  return (
    <ul className="divide-y divide-border rounded-md border border-border">
      {ordered.map((item, i) => (
        <li key={`${item.path}-${i}`} className="flex gap-2.5 px-3 py-2">
          <ItemIcon status={item.status} />
          <div className="min-w-0 flex-1">
            <p className="flex flex-wrap items-baseline gap-x-2">
              <span className="font-medium break-all">{item.name}</span>
              <span className={cn("text-xs", item.status === "failed" ? "text-danger" : "text-muted")}>
                {STATUS_TEXT[item.status]}
              </span>
            </p>
            {item.message && <p className="mt-0.5 text-sm text-muted">{item.message}</p>}
            <p className="mt-0.5 truncate font-mono text-xs text-muted" title={item.path}>
              {item.path}
            </p>
          </div>
        </li>
      ))}
    </ul>
  );
}

function summaryText(job: ImportJob): string {
  const { added, skipped, failed } = job.counts;
  const parts = [`${added} ${added === 1 ? "adicionado" : "adicionados"}`];
  if (skipped) parts.push(`${skipped} ${skipped === 1 ? "ignorado" : "ignorados"}`);
  if (failed) parts.push(`${failed} com falha`);
  return parts.join(" · ");
}

function JobCard({ job }: { job: ImportJob }) {
  const { cancel, dismiss } = useImport();
  const [details, setDetails] = useState(false);
  const running = job.phase === "scanning" || job.phase === "processing";
  const { total, processed, failed } = job.counts;
  const title =
    job.phase === "scanning"
      ? "Procurando quadrinhos…"
      : running
        ? `Importando ${Math.min(processed + 1, total)} de ${total}`
        : job.phase === "cancelled"
          ? "Importação cancelada"
          : "Importação concluída";
  // O leitor de tela ouve só a mudança de fase, não cada arquivo processado.
  const announcement = job.phase === "scanning" ? "Procurando quadrinhos" : running ? "Importando quadrinhos" : `${title}: ${summaryText(job)}`;
  return (
    <section
      aria-label="Importação de quadrinhos"
      className="p-enter w-[min(360px,calc(100vw-2rem))] rounded-lg border border-border bg-elevated shadow-overlay"
    >
      <div className="flex items-start gap-3 px-3 pt-3">
        <div className="mt-0.5 text-muted">
          {running ? <Spinner size={16} /> : failed > 0 ? <CircleAlert size={16} className="text-danger" aria-hidden /> : <FileUp size={16} aria-hidden />}
        </div>
        <div className="min-w-0 flex-1">
          <p className="font-semibold">{title}</p>
          <p className="truncate text-sm text-muted">{running && job.current ? job.current : summaryText(job)}</p>
          <span role="status" className="sr-only">
            {announcement}
          </span>
        </div>
        {!running && <IconButton icon={X} label="Fechar resumo da importação" size="sm" onClick={() => dismiss(job.id)} />}
      </div>
      <div className="px-3 pt-2.5 pb-3">
        {running && total > 0 && <ProgressBar value={processed / total} label="Progresso da importação" />}
        <div className="mt-2 flex justify-end gap-2">
          {job.items.length > 0 && (
            <Button size="sm" variant="ghost" onClick={() => setDetails(true)}>
              Ver detalhes
            </Button>
          )}
          {running && (
            <Button size="sm" onClick={() => cancel(job.id)}>
              Cancelar importação
            </Button>
          )}
        </div>
      </div>
      <Dialog
        open={details}
        onClose={() => setDetails(false)}
        title="Resultado da importação"
        description={summaryText(job)}
        size="lg"
        footer={<Button onClick={() => setDetails(false)}>Fechar</Button>}
      >
        <ImportResults items={job.items} />
      </Dialog>
    </section>
  );
}

/** Painel flutuante: a interface continua utilizável durante a importação. */
export function ImportPanel() {
  const { jobs, dragging } = useImport();
  return (
    <>
      <div className="fixed right-4 bottom-4 z-40 flex flex-col gap-2">
        {jobs.slice(-3).map((j) => (
          <JobCard key={j.id} job={j} />
        ))}
      </div>
      {dragging && (
        <div className="pointer-events-none fixed inset-0 z-[80] flex items-center justify-center bg-overlay">
          <div className="rounded-lg border-2 border-dashed border-accent bg-elevated px-10 py-8 text-center shadow-overlay">
            <FileUp size={28} className="mx-auto text-accent" aria-hidden />
            <p className="mt-2 text-lg font-semibold">Solte para adicionar à biblioteca</p>
            <p className="text-sm text-muted">Arquivos de quadrinhos ou pastas com imagens</p>
          </div>
        </div>
      )}
    </>
  );
}
