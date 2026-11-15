import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** Sai da tela cheia ao fechar a leitura. Fica fora do leitor porque trocar de capítulo recria ele. */
export function useLeaveFullscreenOnClose() {
  useEffect(
    () => () => {
      try {
        void getCurrentWindow()
          .setFullscreen(false)
          .catch(() => undefined);
      } catch {
        /* fora do Tauri */
      }
    },
    [],
  );
}
