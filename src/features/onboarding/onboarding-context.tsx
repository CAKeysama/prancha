import { createContext, useCallback, useContext, useMemo, type ReactNode } from "react";
import type { OnboardingState } from "../../domain/settings";
import { useSettings } from "../../app/settings-context";

interface OnboardingApi {
  state: OnboardingState;
  /** O assistente (etapas 1–3) deve aparecer sobre a interface. */
  wizardVisible: boolean;
  /** A dica de leitura (etapa 4) deve aparecer no leitor. */
  readerTipVisible: boolean;
  goTo: (step: OnboardingState["step"], comicId?: number | null) => void;
  skip: () => void;
  complete: () => void;
  restart: () => void;
}

const Ctx = createContext<OnboardingApi | null>(null);

export function useOnboarding(): OnboardingApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("useOnboarding fora do provedor");
  return v;
}

/** Estado do tutorial salvo no banco; sobrevive a reinicializações e cancelamentos. */
export function OnboardingProvider({ children }: { children: ReactNode }) {
  const { settings, update, loaded } = useSettings();
  const state = settings.onboarding;
  const save = useCallback((s: OnboardingState) => void update("onboarding", s).catch(() => undefined), [update]);

  const goTo = useCallback(
    (step: OnboardingState["step"], comicId?: number | null) =>
      save({ status: "active", step, comicId: comicId === undefined ? state.comicId : comicId }),
    [save, state.comicId],
  );
  const skip = useCallback(() => save({ ...state, status: "skipped" }), [save, state]);
  const complete = useCallback(() => save({ ...state, status: "completed" }), [save, state]);
  const restart = useCallback(() => save({ status: "active", step: 1, comicId: null }), [save]);

  const value = useMemo<OnboardingApi>(
    () => ({
      state,
      wizardVisible: loaded && state.status === "active" && state.step <= 3,
      readerTipVisible: loaded && state.status === "active" && state.step === 4,
      goTo,
      skip,
      complete,
      restart,
    }),
    [state, loaded, goTo, skip, complete, restart],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
