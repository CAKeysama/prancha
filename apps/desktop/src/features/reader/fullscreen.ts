import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "@prancha/shared/services/api";
import { IS_ANDROID } from "../../app/platform";

/**
 * Sai da tela cheia ao fechar a leitura. Fica fora do leitor porque trocar de capítulo recria ele.
 * No Android, a leitura é sempre em tela cheia (modo imersivo): as barras do sistema somem ao abrir e voltam ao
 * fechar, e deslizar da borda mostra elas por um instante.
 */
export function useLeaveFullscreenOnClose() {
  useEffect(() => {
    if (IS_ANDROID) void api.setImmersive(true).catch(() => undefined);
    return () => {
      if (IS_ANDROID) {
        void api.setImmersive(false).catch(() => undefined);
        return;
      }
      try {
        void getCurrentWindow()
          .setFullscreen(false)
          .catch(() => undefined);
      } catch {
        /* fora do Tauri */
      }
    };
  }, []);
}
