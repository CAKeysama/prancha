import { createContext, useCallback, useContext, useMemo, type ReactNode } from "react";
import { ONBOARDING_STEPS, type OnboardingState } from "../../domain/settings";
import { useSettings } from "../../app/settings-context";

interface OnboardingApi {
  state: OnboardingState;
  /** A configuração inicial está aberta sobre o app. */
  wizardVisible: boolean;
  /** A dica dos controles deve aparecer no leitor (uma vez, depois de concluir a configuração). */
  readerTipVisible: boolean;
  goTo: (step: number) => void;
  skip: () => void;
  complete: () => void;
  dismissReaderTip: () => void;
  /** Abre a configuração inicial de novo, pelas Configurações. Não apaga nenhuma escolha. */
  restart: () => void;
}

const Ctx = createContext<OnboardingApi | null>(null);

export function useOnboarding(): OnboardingApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("useOnboarding fora do provedor");
  return v;
}

/** Estado da configuração inicial salvo no banco: sobrevive a reinicializações e não volta em atualizações. */
export function OnboardingProvider({ children }: { children: ReactNode }) {
  const { settings, update, loaded } = useSettings();
  const state = settings.onboarding;
  const save = useCallback((s: OnboardingState) => void update("onboarding", s).catch(() => undefined), [update]);

  const goTo = useCallback(
    (step: number) => save({ ...state, status: "active", step: Math.min(Math.max(0, step), ONBOARDING_STEPS.length - 1) }),
    [save, state],
  );
  const skip = useCallback(() => save({ ...state, status: "skipped", readerTip: false }), [save, state]);
  const complete = useCallback(() => save({ ...state, status: "completed", readerTip: true }), [save, state]);
  const dismissReaderTip = useCallback(() => save({ ...state, readerTip: false }), [save, state]);
  const restart = useCallback(() => save({ status: "active", step: 0, readerTip: false, flow: 2 }), [save]);

  const value = useMemo<OnboardingApi>(() => {
    const wizardVisible = loaded && state.status === "active";
    return {
      state,
      wizardVisible,
      readerTipVisible: loaded && !wizardVisible && state.readerTip,
      goTo,
      skip,
      complete,
      dismissReaderTip,
      restart,
    };
  }, [state, loaded, goTo, skip, complete, dismissReaderTip, restart]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
