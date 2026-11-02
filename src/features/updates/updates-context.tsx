import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Channel } from "@tauri-apps/api/core";
import { useNav } from "../../app/navigation";
import { useSettings } from "../../app/settings-context";
import { useToast } from "../../components/Toast";
import type { UpdateProgress, UpdatesSnapshot } from "../../domain/updates";
import { ApiError, api, errorMessage } from "../../services/api";
import { useOnboarding } from "../onboarding/onboarding-context";

type UpdateStage =
  | { name: "idle" }
  | { name: "progress"; progress: UpdateProgress }
  | { name: "failed"; version: string; message: string };

interface UpdatesApi {
  snapshot: UpdatesSnapshot | null;
  stage: UpdateStage;
  offerOpen: boolean;
  failedOpen: boolean;
  welcomeOpen: boolean;
  openOffer: () => void;
  later: () => void;
  install: () => void;
  cancelDownload: () => void;
  check: () => Promise<UpdatesSnapshot>;
  closeWelcome: () => void;
}

const Ctx = createContext<UpdatesApi | null>(null);

export function useUpdates(): UpdatesApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("useUpdates fora do provedor");
  return v;
}

/** Procura atualização uma vez a cada abertura, sem segurar a interface, e só instala se a pessoa pedir. */
export function UpdatesProvider({ children }: { children: ReactNode }) {
  const { loaded } = useSettings();
  const { route } = useNav();
  const { wizardVisible } = useOnboarding();
  const toast = useToast();
  const [snapshot, setSnapshot] = useState<UpdatesSnapshot | null>(null);
  const [stage, setStage] = useState<UpdateStage>({ name: "idle" });
  const [offerWanted, setOfferWanted] = useState(false);
  const started = useRef(false);
  const checking = useRef<Promise<UpdatesSnapshot> | null>(null);
  const installing = useRef(false);

  const check = useCallback(() => {
    checking.current ??= api
      .updatesCheck()
      .then((next) => {
        // A janela de novidades já fechada não volta por causa de uma consulta que estava em andamento.
        setSnapshot((prev) => (next && prev && !prev.welcome ? { ...next, welcome: null } : next));
        return next;
      })
      .finally(() => {
        checking.current = null;
      });
    return checking.current;
  }, []);

  useEffect(() => {
    if (!loaded || started.current) return;
    started.current = true;
    void (async () => {
      try {
        const first = await api.updatesSnapshot();
        setSnapshot(first);
        if (first.failedVersion) {
          const message = `A instalação da versão v${first.failedVersion} não foi concluída. A versão atual continua funcionando.`;
          setStage({ name: "failed", version: first.failedVersion, message });
        }
        const next = await check();
        const version = next.available?.version;
        if (version && next.declinedVersion !== version) setOfferWanted(true);
      } catch {
        // Sem conexão ou GitHub fora do ar: o aplicativo segue normal e o detalhe fica no log do backend.
      }
    })();
  }, [loaded, check]);

  const decline = useCallback((version: string) => {
    setOfferWanted(false);
    setStage({ name: "idle" });
    setSnapshot((s) => s && { ...s, declinedVersion: version, failedVersion: null });
    void api.updatesDecline(version).catch(() => undefined);
  }, []);

  const later = useCallback(() => {
    const version = stage.name === "failed" ? stage.version : snapshot?.available?.version;
    if (version) decline(version);
  }, [stage, snapshot, decline]);

  const install = useCallback(() => {
    const version = stage.name === "failed" ? stage.version : snapshot?.available?.version;
    if (!version || installing.current) return;
    installing.current = true;
    setOfferWanted(false);
    setStage({ name: "progress", progress: { phase: "downloading", downloaded: 0, total: null } });
    const channel = new Channel<UpdateProgress>();
    channel.onmessage = (progress) => setStage({ name: "progress", progress });
    // Se der certo, esta promessa não termina: o instalador fecha o aplicativo.
    void api
      .updatesInstall(channel)
      .catch((e) => {
        const code = e instanceof ApiError ? e.code : "";
        if (code === "cancelled") {
          decline(version);
        } else if (code === "up_to_date") {
          setStage({ name: "idle" });
          toast.success("O Prancha já está na versão mais recente");
        } else {
          setStage({ name: "failed", version, message: errorMessage(e) });
        }
        void api.updatesSnapshot().then(setSnapshot).catch(() => undefined);
      })
      .finally(() => {
        installing.current = false;
      });
  }, [stage, snapshot, decline, toast]);

  const closeWelcome = useCallback(() => {
    const version = snapshot?.welcome?.version;
    if (!version) return;
    setSnapshot((s) => s && { ...s, welcome: null });
    void api.updatesNotesShown(version).catch(() => undefined);
  }, [snapshot]);

  const value = useMemo<UpdatesApi>(() => {
    // Nada se abre sozinho sobre a leitura ou o tutorial: espera a pessoa sair deles.
    const allowed = route.name !== "reader" && !wizardVisible;
    const welcomeOpen = allowed && Boolean(snapshot?.welcome);
    return {
      snapshot,
      stage,
      welcomeOpen,
      failedOpen: allowed && !welcomeOpen && stage.name === "failed",
      offerOpen: allowed && !welcomeOpen && stage.name === "idle" && offerWanted && Boolean(snapshot?.available),
      openOffer: () => setOfferWanted(true),
      later,
      install,
      cancelDownload: () => void api.updatesCancel().catch(() => undefined),
      check,
      closeWelcome,
    };
  }, [route.name, wizardVisible, snapshot, stage, offerWanted, later, install, check, closeWelcome]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
