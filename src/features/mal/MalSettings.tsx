// Configurações › Integrações: conectar e desconectar a conta do MyAnimeList e ver a fila de sincronização.
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Link2, LogOut, RefreshCw, Trash2, X } from "lucide-react";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/Dialog";
import { useToast } from "../../components/Toast";
import { cn } from "../../components/cn";
import { formatDate } from "../../domain/progress";
import type { MalStatus } from "../../domain/mal";
import { ApiError, api, errorMessage } from "../../services/api";
import { onMalChanged } from "../../services/events";

const BADGE: Record<MalStatus["state"], { text: string; className: string }> = {
  connected: { text: "Conectada", className: "bg-success-subtle text-success" },
  expired: { text: "Sessão expirada", className: "bg-attention-subtle text-attention" },
  disconnected: { text: "Desconectada", className: "bg-hover text-muted" },
};

/** O cartão do MyAnimeList. `anchor` envolve as partes que a pesquisa das configurações encontra. */
export function MalSettings({ anchor }: { anchor: (id: string, children: ReactNode) => ReactNode }) {
  const toast = useToast();
  const [status, setStatus] = useState<MalStatus | null>(null);
  const [busy, setBusy] = useState<"connect" | "disconnect" | "retry" | "discard" | null>(null);
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);

  const load = useCallback(() => {
    api
      .malStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);
  useEffect(() => {
    load();
    return onMalChanged(load);
  }, [load]);

  const connect = async () => {
    setBusy("connect");
    try {
      const s = await api.malConnect();
      setStatus(s);
      toast.success("MyAnimeList conectado", s.userName ? `Conta ${s.userName}.` : undefined);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === "cancelled")) toast.error("Não foi possível conectar", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const run = async (kind: "disconnect" | "retry" | "discard", action: () => Promise<unknown>, done: string) => {
    setBusy(kind);
    try {
      await action();
      toast.success(done);
      load();
    } catch (e) {
      toast.error("Não foi possível concluir", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const state = status?.state ?? "disconnected";
  const connected = state !== "disconnected";
  const badge = BADGE[state];

  let text: string;
  if (!status) text = "Carregando…";
  else if (!status.available) text = "Esta versão do Prancha não tem a integração com o MyAnimeList configurada.";
  else if (busy === "connect") text = "Continue no navegador: entre no MyAnimeList e autorize o Prancha. Esta tela atualiza sozinha.";
  else if (state === "connected") text = `Conectada como ${status.userName ?? "—"}${status.connectedAt ? ` desde ${formatDate(status.connectedAt)}` : ""}.`;
  else if (state === "expired") text = `A sessão de ${status.userName ?? "sua conta"} expirou. Conecte de novo para retomar a sincronização; nada pendente foi perdido.`;
  else text = "Desconectada. O Prancha não consulta nem envia nada ao MyAnimeList.";

  return (
    <>
      {anchor(
        "myanimelist",
        <div className="rounded-md border border-border bg-surface p-4">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="min-w-56 flex-1">
              <p className="flex items-center gap-2 font-medium">
                <Link2 size={16} aria-hidden /> MyAnimeList
                <span className={cn("rounded-full px-2 py-0.5 text-xs font-semibold", badge.className)} data-testid="mal-state">
                  {badge.text}
                </span>
              </p>
              <p className="mt-0.5 text-sm text-muted" aria-live="polite">
                {text}
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              {busy === "connect" ? (
                <Button icon={X} onClick={() => void api.malConnectCancel()}>
                  Cancelar
                </Button>
              ) : (
                state !== "connected" && (
                  <Button variant="primary" icon={Link2} disabled={!status?.available || busy !== null} onClick={() => void connect()}>
                    {state === "expired" ? "Conectar de novo" : "Conectar conta"}
                  </Button>
                )
              )}
              {connected && busy !== "connect" && (
                <Button variant="danger" icon={LogOut} disabled={busy !== null} loading={busy === "disconnect"} onClick={() => setConfirmDisconnect(true)}>
                  Desconectar
                </Button>
              )}
            </div>
          </div>
          <p className="mt-3 border-t border-border pt-3 text-sm text-muted">
            Com a conta conectada, cada obra das fontes é associada à obra do MyAnimeList ao ser aberta, e o número de capítulos lidos é atualizado quando você termina um capítulo no leitor. Abrir uma obra ou um capítulo não muda nada no MAL. O Prancha nunca marca uma obra como concluída e não diminui um progresso mais adiantado no MAL. Você entra pelo site do MyAnimeList; o Prancha não vê sua senha e guarda o acesso cifrado neste computador.
          </p>
        </div>,
      )}
      {anchor(
        "mal-sincronizacao",
        <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-2">
          <div className="min-w-56 flex-1">
            <p className="font-medium">Sincronização</p>
            <p className="text-sm text-muted">
              {!connected
                ? "Nada para sincronizar sem uma conta conectada."
                : status && status.pending > 0
                  ? `${status.pending} ${status.pending === 1 ? "atualização esperando" : "atualizações esperando"} a vez${status.pendingError ? `. Última tentativa: ${status.pendingError}` : "."}`
                  : status?.lastSyncAt
                    ? `Tudo sincronizado. Última atualização em ${formatDate(status.lastSyncAt)}.`
                    : "Tudo em dia."}
            </p>
            {status && status.failures.length > 0 && (
              <ul className="mt-2 space-y-1" aria-label="Atualizações que não foram enviadas">
                {status.failures.map((f) => (
                  <li key={f.malId} className="text-sm">
                    <span className="font-medium text-danger">{f.title ?? `Obra ${f.malId}`}</span>
                    <span className="text-muted">
                      {" "}
                      · capítulo {f.chapters}
                      {f.error ? ` · ${f.error}` : ""}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
          {connected && (
            <div className="flex flex-wrap gap-2">
              <Button icon={RefreshCw} disabled={busy !== null || !status || (status.pending === 0 && status.failures.length === 0)} loading={busy === "retry"} onClick={() => void run("retry", api.malRetry, "Sincronizando agora")}>
                Tentar agora
              </Button>
              {status && status.failures.length > 0 && (
                <Button variant="ghost" icon={Trash2} disabled={busy !== null} loading={busy === "discard"} onClick={() => void run("discard", api.malDiscardFailed, "Atualizações descartadas")}>
                  Descartar falhas
                </Button>
              )}
            </div>
          )}
        </div>,
      )}
      <ConfirmDialog
        open={confirmDisconnect}
        title="Desconectar o MyAnimeList?"
        confirmLabel="Desconectar"
        destructive
        busy={busy === "disconnect"}
        onCancel={() => setConfirmDisconnect(false)}
        onConfirm={() => {
          setConfirmDisconnect(false);
          void run("disconnect", api.malDisconnect, "MyAnimeList desconectado");
        }}
      >
        O acesso guardado neste computador será apagado{status && status.pending + status.failures.length > 0 ? `, junto com ${status.pending + status.failures.length} atualização(ões) que ainda não foram enviadas` : ""}. As associações das obras ficam, para quando você conectar de novo. Sua lista no MyAnimeList não muda.
      </ConfirmDialog>
    </>
  );
}
