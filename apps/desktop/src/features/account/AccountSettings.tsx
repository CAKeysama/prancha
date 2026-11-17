// Configurações › Conta: entrar com Google ou Discord, ver a sincronização entre aparelhos e cuidar dos dados na nuvem.
import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  CloudDownload,
  LogIn,
  LogOut,
  RefreshCw,
  Smartphone,
  Monitor,
  Trash2,
  X,
  UserRound,
} from "lucide-react";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/Dialog";
import { useToast } from "../../components/Toast";
import { cn } from "../../components/cn";
import { formatDate } from "@prancha/shared/domain/progress";
import {
  platformLabel,
  providerLabel,
  type AccountDevice,
  type AccountProvider,
  type AccountStatus,
  type SyncStatus,
} from "@prancha/shared/domain/account";
import { ApiError, api, errorMessage } from "@prancha/shared/services/api";
import {
  onAccountChanged,
  onSyncChanged,
} from "@prancha/shared/services/events";
import { DEVICE, IS_ANDROID } from "../../app/platform";

type Busy =
  | AccountProvider
  | "sign-out"
  | "sync"
  | "export"
  | "delete"
  | `remove:${string}`
  | null;

const BADGE: Record<
  AccountStatus["state"],
  { text: string; className: string }
> = {
  signed_in: { text: "Conectada", className: "bg-success-subtle text-success" },
  expired: {
    text: "Sessão expirada",
    className: "bg-attention-subtle text-attention",
  },
  signed_out: { text: "Sem conta", className: "bg-hover text-muted" },
};

function syncText(
  account: AccountStatus | null,
  sync: SyncStatus | null,
): string {
  if (!account || account.state === "signed_out")
    return "Sem conta, nada sai deste aparelho.";
  if (account.state === "expired")
    return "A sincronização está parada até você entrar de novo. O que mudou aqui não se perde.";
  if (!sync) return "Carregando…";
  if (sync.running) return "Sincronizando agora…";
  if (sync.lastError) return `A última tentativa falhou: ${sync.lastError}`;
  if (sync.pending > 0)
    return `${sync.pending} ${sync.pending === 1 ? "mudança esperando" : "mudanças esperando"} para subir.`;
  if (sync.lastSyncAt)
    return `Tudo sincronizado. Última vez em ${formatDate(sync.lastSyncAt)}.`;
  return "A primeira sincronização começa em instantes.";
}

/** O cartão da conta. `anchor` envolve as partes que a pesquisa das configurações encontra. */
export function AccountSettings({
  anchor,
}: {
  anchor: (id: string, children: ReactNode) => ReactNode;
}) {
  const toast = useToast();
  const [account, setAccount] = useState<AccountStatus | null>(null);
  const [sync, setSync] = useState<SyncStatus | null>(null);
  const [devices, setDevices] = useState<AccountDevice[] | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [confirm, setConfirm] = useState<
    "sign-out" | "delete" | AccountDevice | null
  >(null);

  const load = useCallback(() => {
    api
      .accountStatus()
      .then(setAccount)
      .catch(() => undefined);
    api
      .syncStatus()
      .then(setSync)
      .catch(() => undefined);
  }, []);
  const loadDevices = useCallback(() => {
    api
      .accountDevices()
      .then(setDevices)
      .catch(() => setDevices(null));
  }, []);
  useEffect(() => {
    load();
    const offAccount = onAccountChanged(load);
    const offSync = onSyncChanged(() =>
      api
        .syncStatus()
        .then(setSync)
        .catch(() => undefined),
    );
    return () => {
      offAccount();
      offSync();
    };
  }, [load]);
  const signedIn = account?.state === "signed_in";
  useEffect(() => {
    // A lista só aparece com a conta conectada: sem ela, o que ficou guardado não é mostrado.
    if (signedIn) loadDevices();
  }, [signedIn, loadDevices]);

  const signIn = async (provider: AccountProvider) => {
    setBusy(provider);
    try {
      const s = await api.accountSignIn(provider);
      setAccount(s);
      toast.success("Conta conectada", s.email ?? undefined);
    } catch (e) {
      if (!(e instanceof ApiError && e.code === "cancelled"))
        toast.error("Não foi possível entrar", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const run = async (
    kind: Busy,
    action: () => Promise<unknown>,
    done?: string,
  ) => {
    setBusy(kind);
    try {
      await action();
      if (done) toast.success(done);
      load();
    } catch (e) {
      toast.error("Não foi possível concluir", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const exportData = () =>
    run("export", async () => {
      const path = await api.accountExport();
      if (path) toast.success("Dados salvos", path);
    });

  const state = account?.state ?? "signed_out";
  const badge = BADGE[state];
  const signingIn = busy === "google" || busy === "discord";

  let text: string;
  if (!account) text = "Carregando…";
  else if (signingIn)
    text =
      "Continue no navegador e entre na sua conta. Esta tela atualiza sozinha.";
  else if (state === "signed_in")
    text = `${account.name ?? account.email ?? "Conta"}${account.email && account.name ? ` (${account.email})` : ""}, pelo ${providerLabel(account.provider)}.`;
  else if (state === "expired")
    text = "A sessão expirou. Entre de novo para retomar a sincronização.";
  else
    text =
      "Entre para continuar a leitura online em outro aparelho: o capítulo, a página e as obras da biblioteca.";

  return (
    <>
      {anchor(
        "conta",
        <div className="rounded-md border border-border bg-surface p-4">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="min-w-56 flex-1">
              <p className="flex items-center gap-2 font-medium">
                <UserRound size={16} aria-hidden /> Conta do Prancha
                <span
                  className={cn(
                    "rounded-full px-2 py-0.5 text-xs font-semibold",
                    badge.className,
                  )}
                  data-testid="account-state"
                >
                  {badge.text}
                </span>
              </p>
              <p className="mt-0.5 text-sm text-muted" aria-live="polite">
                {text}
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              {signingIn ? (
                <Button icon={X} onClick={() => void api.accountCancel()}>
                  Cancelar
                </Button>
              ) : state === "signed_in" ? (
                <Button
                  variant="danger"
                  icon={LogOut}
                  disabled={busy !== null}
                  loading={busy === "sign-out"}
                  onClick={() => setConfirm("sign-out")}
                >
                  Sair
                </Button>
              ) : (
                <>
                  <Button
                    variant="primary"
                    icon={LogIn}
                    disabled={busy !== null}
                    onClick={() => void signIn("google")}
                  >
                    Entrar com Google
                  </Button>
                  <Button
                    icon={LogIn}
                    disabled={busy !== null}
                    onClick={() => void signIn("discord")}
                  >
                    Entrar com Discord
                  </Button>
                  {state === "expired" && (
                    <Button
                      variant="ghost"
                      icon={LogOut}
                      disabled={busy !== null}
                      onClick={() => setConfirm("sign-out")}
                    >
                      Sair
                    </Button>
                  )}
                </>
              )}
            </div>
          </div>
          <p className="mt-3 border-t border-border pt-3 text-sm text-muted">
            Sobem só as obras das fontes do Prancha que estão na biblioteca, o
            estado e a posição de cada capítulo, as pastas, as associações
            feitas à mão com o MyAnimeList e os repositórios e extensões
            instalados. HQs do {DEVICE} (PDF, CBZ, CBR, pastas), capítulos
            baixados, imagens e logins de sites nunca saem daqui. Obras de
            extensões do Mihon ainda não sincronizam. Você entra pelo site do
            Google ou do Discord: o Prancha não vê sua senha e guarda o acesso
            cifrado neste {DEVICE}.
          </p>
        </div>,
      )}
      {anchor(
        "conta-sincronizacao",
        <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-2">
          <div className="min-w-56 flex-1">
            <p className="font-medium">Sincronização entre aparelhos</p>
            <p
              className={cn(
                "text-sm",
                sync?.lastError && signedIn ? "text-danger" : "text-muted",
              )}
              aria-live="polite"
              data-testid="sync-state"
            >
              {syncText(account, sync)}
            </p>
          </div>
          {signedIn && (
            <Button
              icon={RefreshCw}
              disabled={busy !== null || sync?.running}
              loading={busy === "sync" || sync?.running}
              onClick={() => void run("sync", api.syncNow)}
            >
              Sincronizar agora
            </Button>
          )}
        </div>,
      )}
      {anchor(
        "conta-aparelhos",
        <div>
          <p className="font-medium">Aparelhos</p>
          {!signedIn ? (
            <p className="text-sm text-muted">
              Os computadores e celulares da conta aparecem aqui quando você
              entra.
            </p>
          ) : devices === null ? (
            <p className="text-sm text-muted">
              Os aparelhos aparecem depois da primeira sincronização.
            </p>
          ) : devices.length === 0 ? (
            <p className="text-sm text-muted">
              Nenhum aparelho sincronizou ainda.
            </p>
          ) : (
            <ul
              className="mt-1 divide-y divide-border"
              aria-label="Aparelhos da conta"
            >
              {devices.map((d) => {
                const here = d.id === account?.deviceId;
                const Icon = d.platform === "android" ? Smartphone : Monitor;
                return (
                  <li
                    key={d.id}
                    className="flex flex-wrap items-center justify-between gap-2 py-2"
                  >
                    <span className="flex items-center gap-2 text-sm">
                      <Icon size={16} aria-hidden />
                      <span className="font-medium">{d.name}</span>
                      <span className="text-muted">
                        · {platformLabel(d.platform)}
                        {d.app_version ? ` · ${d.app_version}` : ""}
                        {here
                          ? " · este aparelho"
                          : ` · visto em ${formatDate(Date.parse(d.last_seen_at))}`}
                      </span>
                    </span>
                    {!here && (
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={Trash2}
                        disabled={busy !== null}
                        loading={busy === `remove:${d.id}`}
                        onClick={() => setConfirm(d)}
                      >
                        Remover
                      </Button>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>,
      )}
      {anchor(
        "conta-dados",
        <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-2">
          <div className="min-w-56 flex-1">
            <p className="font-medium">Seus dados na nuvem</p>
            <p className="text-sm text-muted">
              {IS_ANDROID
                ? "Para baixar tudo o que a conta guarda, use o Prancha no computador. "
                : "Baixe tudo o que a conta guarda num arquivo JSON. "}
              Apagar a conta remove os dados da nuvem e o login; o que está
              neste {DEVICE} continua aqui.
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            {!IS_ANDROID && (
              <Button
                icon={CloudDownload}
                disabled={busy !== null || !signedIn}
                loading={busy === "export"}
                onClick={() => void exportData()}
              >
                Baixar meus dados
              </Button>
            )}
            <Button
              variant="danger"
              icon={Trash2}
              disabled={busy !== null || !signedIn}
              loading={busy === "delete"}
              onClick={() => setConfirm("delete")}
            >
              Apagar conta
            </Button>
          </div>
        </div>,
      )}
      <ConfirmDialog
        open={confirm === "sign-out"}
        title="Sair da conta?"
        confirmLabel="Sair"
        destructive
        busy={busy === "sign-out"}
        onCancel={() => setConfirm(null)}
        onConfirm={() => {
          setConfirm(null);
          void run("sign-out", api.accountSignOut, "Você saiu da conta");
        }}
      >
        Este {DEVICE} para de sincronizar. A biblioteca e a leitura que estão
        aqui ficam como estão, e os outros aparelhos continuam com a conta.
      </ConfirmDialog>
      <ConfirmDialog
        open={confirm === "delete"}
        title="Apagar a conta do Prancha?"
        confirmLabel="Apagar conta"
        destructive
        busy={busy === "delete"}
        onCancel={() => setConfirm(null)}
        onConfirm={() => {
          setConfirm(null);
          void run("delete", api.accountDelete, "Conta apagada");
        }}
      >
        Tudo o que a conta guarda na nuvem é apagado, e não dá para desfazer. Os
        outros aparelhos saem da conta na próxima sincronização. O que está
        neste {DEVICE} não muda.
      </ConfirmDialog>
      <ConfirmDialog
        open={typeof confirm === "object" && confirm !== null}
        title="Remover este aparelho?"
        confirmLabel="Remover"
        destructive
        onCancel={() => setConfirm(null)}
        onConfirm={() => {
          if (typeof confirm !== "object" || confirm === null) return;
          const id = confirm.id;
          setConfirm(null);
          void run(
            `remove:${id}`,
            async () => {
              await api.accountRemoveDevice(id);
              loadDevices();
            },
            "Aparelho removido",
          );
        }}
      >
        {typeof confirm === "object" && confirm !== null
          ? `${confirm.name} sai da lista. Se ele sincronizar de novo, volta a aparecer; para parar de vez, saia da conta nele.`
          : null}
      </ConfirmDialog>
    </>
  );
}
