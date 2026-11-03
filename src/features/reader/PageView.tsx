import { memo, useEffect, useRef, useState } from "react";
import { ImageOff, RotateCw } from "lucide-react";
import type { PDFDocumentProxy, RenderTask } from "pdfjs-dist";
import { Button } from "../../components/Button";
import { Spinner } from "../../components/Feedback";
import type { Size } from "../../domain/reader";
import { renderPdfPage } from "../../services/pdf";
import { fetchPageError, type PageSrc } from "../../services/urls";

interface PageViewProps {
  src: PageSrc;
  index: number;
  /** Tamanho em px CSS em que a página deve aparecer. */
  width: number;
  height: number;
  pdf: PDFDocumentProxy | null;
  onSize: (index: number, size: Size) => void;
  /** Preenche a caixa inteira: entre tiras, a sobra de um arredondamento apareceria como uma linha. */
  flush?: boolean;
}

function PageError({ index, message, onRetry, width, height }: { index: number; message: string; onRetry: () => void; width: number; height: number }) {
  return (
    <div
      role="alert"
      className="flex flex-col items-center justify-center gap-2 rounded-md border border-border bg-surface p-4 text-center"
      style={{ width, height: Math.max(height, 180) }}
    >
      <ImageOff size={24} className="text-muted" aria-hidden />
      <p className="font-semibold">Página {index + 1} indisponível</p>
      <p className="max-w-xs text-sm text-muted">{message}</p>
      <Button size="sm" icon={RotateCw} onClick={onRetry}>
        Tentar carregar novamente
      </Button>
    </div>
  );
}

function ImagePage({ src: srcOf, index, width, height, flush, onSize }: Omit<PageViewProps, "pdf">) {
  const [token, setToken] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const src = srcOf(index, token);
  if (error) {
    return (
      <PageError
        index={index}
        message={error}
        width={width}
        height={height}
        onRetry={() => {
          setError(null);
          setLoaded(false);
          setToken(Date.now());
        }}
      />
    );
  }
  return (
    <div className="relative shrink-0" style={{ width, height }}>
      {!loaded && (
        <div className="absolute inset-0 flex items-center justify-center rounded-sm bg-surface/60">
          <Spinner size={20} label={`Carregando página ${index + 1}…`} />
        </div>
      )}
      <img
        src={src}
        alt={`Página ${index + 1}`}
        draggable={false}
        decoding="async"
        onLoad={(e) => {
          const img = e.currentTarget;
          setLoaded(true);
          onSize(index, { width: img.naturalWidth, height: img.naturalHeight });
        }}
        onError={() => void fetchPageError(src).then(setError)}
        className="block h-full w-full select-none"
        style={{ objectFit: flush ? "fill" : "contain" }}
      />
    </div>
  );
}

function PdfPage({ index, width, height, pdf, onSize }: Omit<PageViewProps, "src"> & { pdf: PDFDocumentProxy }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const renderKey = `${index}:${width}:${attempt}`;
  const [renderedKey, setRenderedKey] = useState("");
  const rendering = renderedKey !== renderKey;

  useEffect(() => {
    let task: RenderTask | null = null;
    let alive = true;
    // Pequeno atraso evita renderizar a cada passo de redimensionamento.
    const t = window.setTimeout(() => {
      if (!canvas.current) return;
      renderPdfPage(pdf, index, canvas.current, width, (rt) => (task = rt))
        .then((size) => {
          if (!alive) return;
          setRenderedKey(`${index}:${width}:${attempt}`);
          onSize(index, size);
        })
        .catch((e: unknown) => {
          if (!alive) return;
          const name = e instanceof Error ? e.name : "";
          if (name === "RenderingCancelledException") return;
          setError(e instanceof Error ? e.message : "Falha ao renderizar a página do PDF.");
        });
    }, 60);
    return () => {
      alive = false;
      window.clearTimeout(t);
      task?.cancel();
    };
  }, [pdf, index, width, attempt, onSize]);

  if (error) {
    return (
      <PageError
        index={index}
        message={`O PDF.js não conseguiu desenhar esta página (${error}).`}
        width={width}
        height={height}
        onRetry={() => {
          setError(null);
          setAttempt((a) => a + 1);
        }}
      />
    );
  }
  return (
    <div className="relative shrink-0 bg-white" style={{ width, height }}>
      {rendering && (
        <div className="absolute inset-0 flex items-center justify-center bg-surface/60">
          <Spinner size={20} label={`Desenhando página ${index + 1}…`} />
        </div>
      )}
      <canvas ref={canvas} aria-label={`Página ${index + 1}`} role="img" className="block h-full w-full" />
    </div>
  );
}

export const PageView = memo(function PageView(props: PageViewProps) {
  if (props.pdf) return <PdfPage {...props} pdf={props.pdf} />;
  return <ImagePage {...props} />;
});

/** Pré-carrega imagens de páginas vizinhas (limitado) para virar a página sem espera, e já conta o tamanho delas. */
export function usePreload(src: PageSrc, pages: number[], enabled: boolean, onSize?: (index: number, size: Size) => void) {
  const key = pages.join(",");
  useEffect(() => {
    if (!enabled) return;
    const imgs = key
      .split(",")
      .filter(Boolean)
      .map((p) => {
        const img = new Image();
        img.decoding = "async";
        img.onload = () => onSize?.(Number(p), { width: img.naturalWidth, height: img.naturalHeight });
        img.src = src(Number(p));
        return img;
      });
    return () => {
      for (const img of imgs) {
        img.onload = null;
        img.src = "";
      }
    };
  }, [src, key, enabled, onSize]);
}
