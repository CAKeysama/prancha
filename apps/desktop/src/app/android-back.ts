import { useEffect, useRef } from "react";
import { onBackButtonPress } from "@tauri-apps/api/app";
import { api } from "@prancha/shared/services/api";
import { useToast } from "../components/Toast";
import { useNav } from "./navigation";
import { IS_ANDROID } from "./platform";

/** Quanto tempo o segundo Voltar na tela inicial tem para sair do app. */
const EXIT_WINDOW_MS = 2000;

/** Um menu, folha ou caixa de diálogo aberta: o Voltar fecha ela antes de sair da tela. */
function openOverlay(): Element | null {
  return document.querySelector('[role="menu"], [aria-modal="true"], [role="dialog"]');
}

/**
 * O Voltar do Android (botão ou gesto), na ordem do design aprovado: fecha o que estiver aberto por cima, volta
 * uma tela, volta ao Início e, por fim, pede um segundo toque para sair. Sem isso o WebView fecharia o app no
 * primeiro Voltar, porque a navegação do Prancha não usa o histórico do navegador.
 */
export function useAndroidBack() {
  const nav = useNav();
  const toast = useToast();
  const state = useRef({ nav, toast, armedAt: 0 });
  useEffect(() => {
    state.current.nav = nav;
    state.current.toast = toast;
  });

  useEffect(() => {
    if (!IS_ANDROID) return;
    const onBack = () => {
      const s = state.current;
      const overlay = openOverlay();
      if (overlay) {
        // Os menus e as caixas de diálogo já fecham com Esc: o Voltar vale o mesmo.
        const target = overlay.matches('[role="menu"]') ? overlay : (document.activeElement ?? document.body);
        target.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
        return;
      }
      if (s.nav.canGoBack) {
        // O leitor grava a posição ao desmontar (useProgressSaver), como no botão Voltar dele.
        s.nav.back();
        return;
      }
      if (s.nav.route.name !== "home") {
        s.nav.navigate({ name: "home" });
        return;
      }
      if (Date.now() - s.armedAt < EXIT_WINDOW_MS) {
        s.armedAt = 0;
        void api.leaveApp().catch(() => undefined);
        return;
      }
      s.armedAt = Date.now();
      s.toast.info("Toque em Voltar de novo para sair");
    };
    let listener: { unregister: () => Promise<void> } | null = null;
    let cancelled = false;
    void onBackButtonPress(onBack)
      .then((l) => {
        if (cancelled) void l.unregister();
        else listener = l;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      void listener?.unregister();
    };
  }, []);
}
