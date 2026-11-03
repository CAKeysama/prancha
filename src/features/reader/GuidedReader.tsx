import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";
import { ChevronLeft, ChevronRight, Pencil, X } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { Spinner } from "../../components/Feedback";
import { cn } from "../../components/cn";
import { useToast } from "../../components/Toast";
import { computePop, fitPage, isDoubtful, pageOrigin, type GuidedPage, type GuidedRegion } from "../../domain/guided";
import type { Size } from "../../domain/reader";
import { actionForKey, keyFromEvent, resolveArrow, type ShortcutMap } from "../../domain/shortcuts";
import { api, errorMessage } from "../../services/api";
import { renderPdfPageImage } from "../../services/pdf";
import { comicPageSrc, pageUrl } from "../../services/urls";
import { PageView, usePreload } from "./PageView";
import { RegionEditor } from "./RegionEditor";
import { useViewportSize } from "./Views";

/** Faixa inferior que os controles ocupam: o balão nunca fica embaixo deles. */
const CONTROLS_SPACE = 72;
const POP_MS = 320;
const BACK_MS = 220;
const SWIPE_PX = 48;
/** Passa um pouco do tamanho final e assenta: é o que faz o balão "saltar". */
const POP_EASING = "cubic-bezier(0.34, 1.36, 0.64, 1)";
const BACK_EASING = "cubic-bezier(0.32, 0.72, 0, 1)";
const POP_SHADOW = "drop-shadow(0 0 1px rgb(0 0 0 / 0.7)) drop-shadow(0 14px 26px rgb(0 0 0 / 0.5))";
const NO_SHADOW = "drop-shadow(0 0 0 rgb(0 0 0 / 0)) drop-shadow(0 0 0 rgb(0 0 0 / 0))";
const WHOLE_RECT: [number, number][] = [[0, 0], [1, 0], [1, 1], [0, 1]];

async function loadPage(comicId: number, page: number, rtl: boolean, pdf: PDFDocumentProxy | null): Promise<GuidedPage> {
  const stored = await api.guidedPage(comicId, page, rtl);
  if (stored) return stored;
  // PDF sem análise guardada: quem desenha a página é o PDF.js.
  if (!pdf) throw new Error("A página não pôde ser analisada.");
  return api.guidedAnalyzeImage(comicId, page, rtl, await renderPdfPageImage(pdf, page));
}

/**
 * Cópia do balão por cima da página. Fica montada no tamanho final, para não perder nitidez, e parada
 * sobre o original quando inativa: assim entra e volta sem emenda, e qualquer gesto pode interromper.
 */
function PoppedBalloon({ region, page, view, src, active }: { region: GuidedRegion; page: Size; view: Size; src: string; active: boolean }) {
  const pop = computePop(region, page, view, CONTROLS_SPACE);
  const outline = region.outline.length ? region.outline : WHOLE_RECT;
  const points = outline.map(([x, y]) => `${x * 100}% ${y * 100}%`).join(",");
  return (
    <div
      data-testid={active ? "guided-spot" : undefined}
      aria-hidden
      className="pointer-events-none absolute"
      style={{
        left: pop.left,
        top: pop.top,
        width: pop.width,
        height: pop.height,
        zIndex: active ? 2 : 1,
        transformOrigin: "0 0",
        transform: active ? "none" : `translate(${pop.fromX}px, ${pop.fromY}px) scale(${1 / pop.scale})`,
        opacity: active ? 1 : 0,
        filter: active ? POP_SHADOW : NO_SHADOW,
        transition: active
          ? `transform ${POP_MS}ms ${POP_EASING}, filter ${POP_MS}ms ease-out, opacity 40ms linear`
          : `transform ${BACK_MS}ms ${BACK_EASING}, filter ${BACK_MS}ms ease-out, opacity 0s linear ${BACK_MS}ms`,
      }}
    >
      <div className="absolute inset-0 overflow-hidden" style={{ clipPath: region.outline.length ? `polygon(${points})` : "inset(0 round 10px)" }}>
        <img
          src={src}
          alt=""
          draggable={false}
          className="absolute max-w-none select-none"
          style={{ left: -region.x * page.width * pop.scale, top: -region.y * page.height * pop.scale, width: page.width * pop.scale, height: page.height * pop.scale }}
        />
      </div>
      {isDoubtful(region) && (
        <svg data-testid={active ? "guided-spot-doubtful" : undefined} className="absolute inset-0 size-full overflow-visible" viewBox="0 0 100 100" preserveAspectRatio="none">
          <polygon
            points={outline.map(([x, y]) => `${x * 100},${y * 100}`).join(" ")}
            fill="none"
            className="stroke-attention"
            strokeWidth={2}
            strokeDasharray="6 4"
            vectorEffect="non-scaling-stroke"
          />
        </svg>
      )}
    </div>
  );
}

function Card({ title, message, children }: { title: string; message?: string; children: React.ReactNode }) {
  return (
    <div
      role="status"
      data-controls
      className="p-enter absolute bottom-3 left-1/2 z-20 flex w-max max-w-[calc(100%-32px)] -translate-x-1/2 flex-wrap items-center justify-center gap-x-4 gap-y-2 rounded-lg border border-border bg-elevated px-4 py-2.5 text-center text-fg shadow-overlay"
    >
      <div>
        <p className="font-semibold">{title}</p>
        {message && <p className="text-sm text-muted">{message}</p>}
      </div>
      <div className="flex flex-wrap justify-center gap-2">{children}</div>
    </div>
  );
}

interface GuidedReaderProps {
  comicId: number;
  page: number;
  pageCount: number;
  pdf: PDFDocumentProxy | null;
  rtl: boolean;
  shortcuts: ShortcutMap;
  natural: Size | undefined;
  onSize: (index: number, size: Size) => void;
  onPage: (page: number) => void;
  onExit: () => void;
}

/** Cobre a área de leitura; o leitor continua montado por baixo, com a posição intacta. */
export function GuidedReader(props: GuidedReaderProps) {
  const [leaving, setLeaving] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const { onExit } = props;
  // Tira o foco do botão do leitor, que fica coberto: senão Espaço e Enter continuariam nele.
  useEffect(() => root.current?.focus(), []);
  const exit = useCallback(() => {
    setLeaving(true);
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    window.setTimeout(onExit, reduced ? 0 : BACK_MS);
  }, [onExit]);
  return (
    <div
      ref={root}
      tabIndex={-1}
      data-testid="guided-reader"
      className={cn("absolute inset-0 z-30 bg-reader outline-none transition-opacity", leaving && "pointer-events-none opacity-0")}
      style={{ transitionDuration: `${BACK_MS}ms` }}
    >
      {/* Cada página começa do zero, sem balão ativo. */}
      <GuidedStage key={props.page} {...props} leaving={leaving} onExit={exit} />
    </div>
  );
}

type Load = { status: "loading" } | { status: "ready"; data: GuidedPage } | { status: "error"; message: string };

function GuidedStage({ comicId, page, pageCount, pdf, rtl, shortcuts, natural, onSize, onPage, onExit, leaving }: GuidedReaderProps & { leaving: boolean }) {
  const toast = useToast();
  const [load, setLoad] = useState<Load>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);
  const [index, setIndex] = useState(0);
  const [editing, setEditing] = useState(false);
  const [entered, setEntered] = useState(false);
  const stage = useRef<HTMLDivElement>(null);
  const press = useRef<{ x: number; y: number } | null>(null);
  const view = useViewportSize(stage);
  const hasNextPage = page < pageCount - 1;

  useEffect(() => {
    let alive = true;
    loadPage(comicId, page, rtl, pdf)
      .then((data) => {
        if (alive) setLoad({ status: "ready", data });
      })
      .catch((e: unknown) => {
        if (alive) setLoad({ status: "error", message: errorMessage(e) });
      });
    return () => {
      alive = false;
    };
  }, [comicId, page, rtl, pdf, attempt]);

  // Deixa a próxima página pronta. PDFs ficam de fora: desenhá-los custaria mais do que a espera.
  const ready = load.status === "ready";
  useEffect(() => {
    if (ready && hasNextPage && !pdf) void api.guidedPage(comicId, page + 1, rtl).catch(() => undefined);
  }, [ready, hasNextPage, pdf, comicId, page, rtl]);
  const pageSrc = useMemo(() => comicPageSrc(comicId), [comicId]);
  usePreload(pageSrc, hasNextPage ? [page + 1] : [], !pdf);

  // Os balões precisam aparecer parados sobre a página antes de saltar: são dois quadros até o navegador registrar.
  useEffect(() => {
    if (!ready) return;
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => setEntered(true));
    });
    return () => cancelAnimationFrame(frame);
  }, [ready]);

  // PDF não tem imagem pronta para recortar: o PDF.js desenha uma vez para todos os balões da página.
  const [pdfImage, setPdfImage] = useState<string | null>(null);
  useEffect(() => {
    if (!pdf) return;
    let alive = true;
    let url: string | null = null;
    renderPdfPageImage(pdf, page)
      .then((bytes) => {
        if (!alive) return;
        url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: "image/jpeg" }));
        setPdfImage(url);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [pdf, page]);
  const balloonImage = pdf ? pdfImage : pageUrl(comicId, page);

  const regions: GuidedRegion[] = load.status === "ready" ? load.data.regions : [];
  const count = regions.length;
  const completed = ready && count > 0 && index >= count;
  const showBalloon = entered && !leaving && !!natural && !editing;

  const room = { width: Math.max(1, view.width), height: Math.max(1, view.height) };
  const fit = fitPage(natural ?? { width: 2, height: 3 }, { width: room.width, height: Math.max(1, room.height - CONTROLS_SPACE) });
  const origin = pageOrigin(fit, room, CONTROLS_SPACE);

  const next = useCallback(() => {
    if (!ready) return;
    // Toques em sequência chegam antes de `index` atualizar: nenhum se perde, e nenhum vira a página sem querer.
    if (index < count) setIndex((i) => Math.min(i + 1, count));
    else if (hasNextPage) onPage(page + 1);
  }, [ready, index, count, hasNextPage, onPage, page]);
  const prev = useCallback(() => setIndex((i) => Math.max(0, i - 1)), []);

  useEffect(() => {
    if (editing) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || document.querySelector('[aria-modal="true"]')) return;
      if (e.key === "Escape") {
        onExit();
        return;
      }
      const key = keyFromEvent(e);
      if (!key) return;
      const action = resolveArrow(actionForKey(shortcuts, key), key, rtl);
      if (key !== "Enter" && action !== "next" && action !== "prev" && action !== "guided") return;
      // Botões focados mantêm Espaço/Enter.
      if ((key === "Space" || key === "Enter") && (e.target as HTMLElement)?.closest?.("button, a")) return;
      e.preventDefault();
      if (action === "guided") onExit();
      else if (action === "prev") prev();
      else next();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [editing, shortcuts, rtl, next, prev, onExit]);

  // Mouse e toque: um gesto só, para o toque não virar dois comandos.
  const onPointerDown = (e: React.PointerEvent) => {
    press.current = (e.target as HTMLElement).closest("[data-controls]") ? null : { x: e.clientX, y: e.clientY };
  };
  const onPointerUp = (e: React.PointerEvent) => {
    const start = press.current;
    press.current = null;
    if (!start || !stage.current) return;
    const dx = e.clientX - start.x;
    let forward: boolean;
    if (Math.abs(dx) >= SWIPE_PX) {
      forward = dx < 0 !== rtl;
    } else {
      const box = stage.current.getBoundingClientRect();
      const inBackZone = (e.clientX - box.left) / box.width < 1 / 3;
      forward = inBackZone === rtl;
    }
    if (forward) next();
    else prev();
  };

  /** Depois de gravar ou desfazer correções, a página volta do backend e recomeça do primeiro balão. */
  const reloadAfter = async (work: Promise<unknown>, failureTitle: string) => {
    try {
      await work;
      const data = await loadPage(comicId, page, rtl, pdf);
      setLoad({ status: "ready", data });
      setIndex(0);
      setEditing(false);
    } catch (e) {
      toast.error(failureTitle, errorMessage(e));
    }
  };

  const nextPageButton = hasNextPage && (
    <Button size="sm" variant="primary" onClick={() => onPage(page + 1)}>
      Próxima página
    </Button>
  );
  const exitButton = (
    <Button size="sm" onClick={onExit}>
      Sair
    </Button>
  );
  const position = Math.min(index + 1, count);

  return (
    <div ref={stage} className="absolute inset-0 touch-none overflow-hidden select-none" onPointerDown={onPointerDown} onPointerUp={onPointerUp}>
      <div
        data-testid="guided-page"
        className="absolute"
        style={{ left: origin.x, top: origin.y, width: fit.width, height: fit.height }}
      >
        <PageView src={pageSrc} index={page} width={fit.width} height={fit.height} pdf={pdf} onSize={onSize} />
      </div>

      {balloonImage && regions.map((r, i) => <PoppedBalloon key={r.id} region={r} page={fit} view={room} src={balloonImage} active={showBalloon && i === index} />)}

      {load.status === "loading" && (
        <div data-controls className="p-enter absolute bottom-5 left-1/2 z-20 flex -translate-x-1/2 items-center gap-3 rounded-full border border-border bg-elevated px-4 py-1.5 text-fg shadow-overlay">
          <Spinner size={16} label="Analisando página…" />
          <IconButton icon={X} label="Sair da Leitura Guiada" size="sm" tooltipSide="top" onClick={onExit} />
        </div>
      )}

      {load.status === "error" && (
        <Card title="Não foi possível analisar esta página" message={load.message}>
          <Button
            size="sm"
            variant="primary"
            onClick={() => {
              setLoad({ status: "loading" });
              setAttempt((a) => a + 1);
            }}
          >
            Tentar de novo
          </Button>
          <Button size="sm" onClick={() => setEditing(true)}>
            Marcar manualmente
          </Button>
          {exitButton}
        </Card>
      )}

      {ready && count === 0 && (
        <Card title="Nenhum balão identificado nesta página">
          {nextPageButton}
          <Button size="sm" onClick={() => setEditing(true)}>
            Marcar balões
          </Button>
          {exitButton}
        </Card>
      )}

      {completed && (
        <Card title="Página concluída" message={hasNextPage ? undefined : "Esta é a última página da HQ."}>
          {nextPageButton}
          <Button size="sm" onClick={() => setIndex(count - 1)}>
            Rever último balão
          </Button>
          {exitButton}
        </Card>
      )}

      {ready && count > 0 && !completed && (
        <div
          data-controls
          role="toolbar"
          aria-label="Leitura Guiada"
          className={cn("absolute bottom-5 left-1/2 z-20 flex -translate-x-1/2 items-center gap-1 rounded-full border border-border bg-elevated/95 p-1 text-fg shadow-overlay backdrop-blur", rtl && "flex-row-reverse")}
        >
          <Button size="sm" variant="ghost" icon={rtl ? ChevronRight : ChevronLeft} disabled={index === 0} onClick={prev}>
            Anterior
          </Button>
          <span className="min-w-14 text-center font-mono text-xs text-muted" aria-live="polite" data-testid="guided-progress">
            {position} de {count}
          </span>
          <Button size="sm" variant="ghost" trailingIcon={rtl ? ChevronLeft : ChevronRight} onClick={next}>
            Próximo
          </Button>
          <span className="mx-0.5 h-5 w-px bg-border" aria-hidden />
          <span className="relative">
            <IconButton icon={Pencil} label={load.status === "ready" && load.data.needsReview ? "Corrigir regiões (há regiões duvidosas)" : "Corrigir regiões"} size="sm" tooltipSide="top" onClick={() => setEditing(true)} />
            {load.status === "ready" && load.data.needsReview && <span aria-hidden className="pointer-events-none absolute top-0.5 right-0.5 size-1.5 rounded-full bg-attention" />}
          </span>
          <IconButton icon={X} label="Sair da Leitura Guiada" size="sm" tooltipSide="top" onClick={onExit} />
        </div>
      )}

      {editing && (
        <div onPointerDown={(e) => e.stopPropagation()} onPointerUp={(e) => e.stopPropagation()}>
          <RegionEditor
            comicId={comicId}
            page={page}
            pdf={pdf}
            natural={natural}
            regions={regions}
            initialSelection={regions[index]?.id ?? null}
            canRestore={load.status === "ready" && load.data.source === "user"}
            onSize={onSize}
            onSave={(edited) => reloadAfter(api.guidedSaveCorrections(comicId, page, edited), "Não foi possível salvar as correções")}
            onRestore={() => reloadAfter(api.guidedClearCorrections(comicId, page), "Não foi possível restaurar a detecção")}
            onCancel={() => setEditing(false)}
          />
        </div>
      )}
    </div>
  );
}
