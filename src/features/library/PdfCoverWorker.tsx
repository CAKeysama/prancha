import { useEffect, useRef } from "react";
import { api } from "../../services/api";
import { renderPdfCover } from "../../services/pdf";
import { useLibraryVersion } from "../../app/library-events";

/**
 * Gera em segundo plano as capas de PDFs pendentes (o PDF.js roda na interface).
 * Processa uma de cada vez para não disputar recursos com a leitura.
 */
export function PdfCoverWorker() {
  const { version } = useLibraryVersion();
  const running = useRef(false);
  const failed = useRef(new Set<number>());

  useEffect(() => {
    if (running.current) return;
    running.current = true;
    void (async () => {
      try {
        for (;;) {
          const ids = (await api.pendingPdfCovers()).filter((id) => !failed.current.has(id));
          const id = ids[0];
          if (id === undefined) break;
          try {
            const { image, pageCount } = await renderPdfCover(id);
            await api.savePdfCover(id, image, pageCount);
          } catch (e) {
            failed.current.add(id);
            const msg = e instanceof Error ? e.message : String(e);
            await api.reportPdfCoverFailure(id, `o PDF.js não conseguiu desenhar a primeira página (${msg})`).catch(() => undefined);
          }
        }
      } catch {
        /* backend indisponível: tenta na próxima mudança */
      } finally {
        running.current = false;
      }
    })();
  }, [version]);
  return null;
}
