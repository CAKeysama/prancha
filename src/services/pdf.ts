// Integração com o PDF.js: o PDF é lido pelo protocolo `prancha://file/<id>`. O primeiro pedido
// não tem intervalo e traz o arquivo inteiro; os seguintes pedem só as partes necessárias.

import { GlobalWorkerOptions, getDocument, type PDFDocumentProxy, type RenderTask } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import type { Size } from "../domain/reader";
import { fileUrl } from "./urls";

GlobalWorkerOptions.workerSrc = workerUrl;

const asset = (path: string) => new URL(path, window.location.href).href;

/** Limite de pixels do canvas para não esgotar memória com zoom alto. */
const MAX_CANVAS_PIXELS = 4096 * 4096;

export function openPdf(id: number): Promise<PDFDocumentProxy> {
  return getDocument({
    url: fileUrl(id),
    cMapUrl: asset("/pdfjs/cmaps/"),
    cMapPacked: true,
    standardFontDataUrl: asset("/pdfjs/standard_fonts/"),
    wasmUrl: asset("/pdfjs/wasm/"),
    iccUrl: asset("/pdfjs/iccs/"),
    rangeChunkSize: 1 << 20,
    disableAutoFetch: true,
    enableXfa: false,
  }).promise;
}

/**
 * Renderiza a página na largura CSS pedida (multiplicada pela densidade de pixels).
 * Devolve a tarefa para permitir cancelamento quando o usuário muda de página.
 */
export async function renderPdfPage(
  doc: PDFDocumentProxy,
  pageIndex: number,
  canvas: HTMLCanvasElement,
  cssWidth: number,
  onTask?: (t: RenderTask) => void,
): Promise<Size> {
  const page = await doc.getPage(pageIndex + 1);
  const base = page.getViewport({ scale: 1 });
  const dpr = window.devicePixelRatio || 1;
  let scale = (cssWidth / base.width) * dpr;
  const pixels = base.width * scale * base.height * scale;
  if (pixels > MAX_CANVAS_PIXELS) scale *= Math.sqrt(MAX_CANVAS_PIXELS / pixels);
  const viewport = page.getViewport({ scale });
  canvas.width = Math.floor(viewport.width);
  canvas.height = Math.floor(viewport.height);
  const task = page.render({ canvas, viewport });
  onTask?.(task);
  await task.promise;
  return { width: base.width, height: base.height };
}

async function pageToJpeg(doc: PDFDocumentProxy, pageIndex: number, pixelWidth: number, quality: number): Promise<Uint8Array> {
  const canvas = document.createElement("canvas");
  await renderPdfPage(doc, pageIndex, canvas, pixelWidth / (window.devicePixelRatio || 1));
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", quality));
  if (!blob) throw new Error("falha ao codificar a imagem");
  return new Uint8Array(await blob.arrayBuffer());
}

/** Desenha uma página como JPEG para a análise da Leitura Guiada, no tamanho que o detector usa. */
export async function renderPdfPageImage(doc: PDFDocumentProxy, pageIndex: number): Promise<Uint8Array> {
  const base = (await doc.getPage(pageIndex + 1)).getViewport({ scale: 1 });
  return pageToJpeg(doc, pageIndex, (1600 / Math.max(base.width, base.height)) * base.width, 0.92);
}

/** Gera a miniatura da primeira página válida (JPEG) e o total de páginas. */
export async function renderPdfCover(id: number): Promise<{ image: Uint8Array; pageCount: number }> {
  const doc = await openPdf(id);
  try {
    let lastError: unknown = null;
    for (let i = 0; i < Math.min(3, doc.numPages); i++) {
      try {
        return { image: await pageToJpeg(doc, i, 600, 0.9), pageCount: doc.numPages };
      } catch (e) {
        lastError = e;
      }
    }
    throw lastError ?? new Error("o PDF não tem páginas");
  } finally {
    void doc.loadingTask.destroy();
  }
}
