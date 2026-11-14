import { memo, useEffect, useRef, useState, type CSSProperties } from "react";
import { ImageOff, RotateCw } from "lucide-react";
import type { PDFDocumentProxy, RenderTask } from "pdfjs-dist";
import { Button } from "../../components/Button";
import { Spinner } from "../../components/Feedback";
import type { Size } from "../../domain/reader";
import { renderPdfPage } from "../../services/pdf";
import { fetchPageError, type PageSrc } from "../../services/urls";

/** Espera curta só aparece se demorar: a página já pronta (pré-carregada, em cache) entra sem piscar. */
export const APPEAR_LATE: CSSProperties = { animation: "p-fade-in 140ms ease-out 250ms both" };

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

/** Fundo discreto no lugar da página. Sem anúncio: na rolagem, várias páginas carregam ao mesmo tempo. */
function PageLoading() {
  return (
    <div aria-hidden className="absolute inset-0 flex items-center justify-center rounded-sm bg-surface/40 text-muted" style={APPEAR_LATE}>
      <Spinner size={18} />
    </div>
  );
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
        Tentar de novo
      </Button>
    </div>
  );
}

function ImagePage({ src: srcOf, index, width, height, flush, onSize }: Omit<PageViewProps, "pdf">) {
  // Cada nova tentativa pede a imagem de novo, sem passar pelo cache.
  const [token, setToken] = useState(0);
  const src = srcOf(index, token);
  // Guardados junto com o endereço: a resposta atrasada de uma tentativa antiga não vale para a nova.
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null);
  const [failure, setFailure] = useState<{ src: string; message: string } | null>(null);
  if (failure?.src === src) {
    return <PageError index={index} message={failure.message} width={width} height={height} onRetry={() => setToken(Date.now())} />;
  }
  const loaded = loadedSrc === src;
  return (
    <div className="relative shrink-0" style={{ width, height }}>
      {!loaded && <PageLoading />}
      <img
        src={src}
        alt={`Página ${index + 1}`}
        aria-busy={loaded ? undefined : true}
        draggable={false}
        decoding="async"
        onLoad={(e) => {
          const img = e.currentTarget;
          setLoadedSrc(src);
          onSize(index, { width: img.naturalWidth, height: img.naturalHeight });
        }}
        onError={() => void fetchPageError(src).then((message) => setFailure({ src, message }))}
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
  // Quem avisa o tamanho pode mudar a cada render de quem usa: isso não deve recomeçar o desenho.
  const onSizeRef = useRef(onSize);
  useEffect(() => {
    onSizeRef.current = onSize;
  });

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
          onSizeRef.current(index, size);
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
  }, [pdf, index, width, attempt]);

  if (error) {
    return (
      <PageError
        index={index}
        message={`Não foi possível desenhar esta página do PDF (${error}).`}
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
      {rendering && <PageLoading />}
      <canvas ref={canvas} aria-label={`Página ${index + 1}`} aria-busy={rendering || undefined} role="img" className="block h-full w-full" />
    </div>
  );
}

export const PageView = memo(function PageView(props: PageViewProps) {
  if (props.pdf) return <PdfPage {...props} pdf={props.pdf} />;
  return <ImagePage {...props} />;
});

/**
 * Pré-carrega imagens de páginas vizinhas (poucas) para virar a página sem espera, e já conta o tamanho delas.
 * Ao virar, só as que deixaram de ser vizinhas são canceladas: as que continuam na lista seguem baixando.
 */
export function usePreload(src: PageSrc, pages: number[], enabled: boolean, onSize?: (index: number, size: Size) => void) {
  const images = useRef<Map<string, HTMLImageElement>>(new Map());
  const onSizeRef = useRef(onSize);
  useEffect(() => {
    onSizeRef.current = onSize;
  });
  const key = enabled ? pages.join(",") : "";
  useEffect(() => {
    const live = images.current;
    // Pelo endereço: quando a sequência de capítulos cresce, `src` muda, mas as páginas já pedidas continuam as mesmas.
    const wanted = new Map(
      key
        .split(",")
        .filter(Boolean)
        .map((p) => [src(Number(p)), Number(p)] as const),
    );
    for (const [url, img] of live) {
      if (wanted.has(url)) continue;
      img.onload = null;
      img.src = "";
      live.delete(url);
    }
    for (const [url, page] of wanted) {
      if (live.has(url)) continue;
      const img = new Image();
      img.decoding = "async";
      // A página na tela passa na frente.
      img.fetchPriority = "low";
      img.onload = () => onSizeRef.current?.(page, { width: img.naturalWidth, height: img.naturalHeight });
      img.src = url;
      // Já decodificada, a página entra na tela sem o quadro em branco do decoding="async".
      img.decode?.().catch(() => undefined);
      live.set(url, img);
    }
  }, [src, key]);
  useEffect(() => {
    const live = images.current;
    return () => {
      for (const img of live.values()) {
        img.onload = null;
        img.src = "";
      }
      live.clear();
    };
  }, []);
}
