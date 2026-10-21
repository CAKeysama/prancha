import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  Columns2,
  Eye,
  LayoutList,
  Maximize,
  Minimize,
  MoveHorizontal,
  RectangleVertical,
  Settings2,
  Square,
  ZoomIn,
  ZoomOut,
  ListOrdered,
} from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { ErrorState, Spinner } from "../../components/Feedback";
import { cn } from "../../components/cn";
import { useToast } from "../../components/Toast";
import {
  FIT_LABELS,
  MODE_LABELS,
  buildSpreads,
  clampPage,
  nextMode,
  pageIndicator,
  spreadIndexOf,
  zoomIn,
  zoomOut,
  type Size,
} from "../../domain/reader";
import { actionForKey, formatKey, keyFromEvent, resolveArrow, type ShortcutAction } from "../../domain/shortcuts";
import type { FitMode, ReaderMode } from "../../domain/settings";
import type { ReaderSession } from "../../domain/types";
import { api, ApiError, errorMessage } from "../../services/api";
import { openPdf } from "../../services/pdf";
import { useSettings } from "../../app/settings-context";
import { useNav } from "../../app/navigation";
import { useOnboarding } from "../onboarding/onboarding-context";
import { ReaderTip } from "../onboarding/ReaderTip";
import { PageIndexPanel, ReaderSettingsPanel, ShortcutsDialog } from "./ReaderPanels";
import { PagedView, VerticalView, type VerticalHandle, type ViewHandle } from "./Views";
import { usePreload } from "./PageView";
import { useProgressSaver } from "./useProgressSaver";

const MODE_ICONS: Record<ReaderMode, typeof Square> = { single: RectangleVertical, double: Columns2, vertical: LayoutList };
const FIT_ICONS: Record<"page" | "width", typeof Square> = { page: Square, width: MoveHorizontal };

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable);
}

export function ReaderScreen({ id }: { id: number }) {
  const nav = useNav();
  const [session, setSession] = useState<ReaderSession | null>(null);
  const [error, setError] = useState<{ code: string; message: string } | null>(null);
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [pageCount, setPageCount] = useState(0);

  useEffect(() => {
    let alive = true;
    let doc: PDFDocumentProxy | null = null;
    api
      .openComic(id)
      .then(async (s) => {
        if (!alive) return;
        if (s.comic.format === "pdf") {
          try {
            doc = await openPdf(id);
          } catch (e) {
            if (alive) setError({ code: "corrupt", message: `O PDF não pôde ser aberto pelo leitor (${e instanceof Error ? e.message : String(e)}).` });
            return;
          }
          if (!alive) {
            void doc.loadingTask.destroy();
            return;
          }
          if (doc.numPages !== s.pageCount) void api.setPdfPageCount(id, doc.numPages).catch(() => undefined);
          setPdf(doc);
          setPageCount(doc.numPages);
        } else {
          setPageCount(s.pageCount);
        }
        setSession(s);
      })
      .catch((e: unknown) => {
        if (alive) setError({ code: e instanceof ApiError ? e.code : "unknown", message: errorMessage(e) });
      });
    return () => {
      alive = false;
      if (doc) void doc.loadingTask.destroy();
    };
  }, [id]);

  if (error) {
    return (
      <div className="flex h-full flex-col bg-bg">
        <div className="flex items-center gap-2 border-b border-border px-4 py-2">
          <Button variant="ghost" icon={ArrowLeft} onClick={nav.back}>
            Voltar à biblioteca
          </Button>
        </div>
        <div className="flex flex-1 items-center justify-center p-6">
          <ErrorState
            title="Não foi possível abrir esta HQ"
            message={error.message}
            action={
              <div className="flex gap-2">
                <Button onClick={nav.back}>Voltar</Button>
                <Button variant="primary" onClick={() => nav.navigate({ name: "comic", id }, { replace: true })}>
                  Ver detalhes da HQ
                </Button>
              </div>
            }
          />
        </div>
      </div>
    );
  }
  if (!session || pageCount === 0) {
    return (
      <div className="flex h-full items-center justify-center bg-reader text-fg">
        <Spinner size={22} label={session?.comic.format === "pdf" || !session ? "Abrindo HQ…" : "Preparando páginas…"} />
      </div>
    );
  }
  return <Reader key={session.comic.id} session={session} pageCount={pageCount} pdf={pdf} />;
}

function Reader({ session, pageCount, pdf }: { session: ReaderSession; pageCount: number; pdf: PDFDocumentProxy | null }) {
  const nav = useNav();
  const toast = useToast();
  const { settings, update } = useSettings();
  const onboarding = useOnboarding();
  const comic = session.comic;
  const mode = settings["reader.mode"];
  const direction = settings["reader.direction"];
  const fit = settings["reader.fit"];
  const rtl = direction === "rtl";
  const shortcuts = settings["reader.shortcuts"];

  const [page, setPage] = useState(() => clampPage(session.start.pageIndex, pageCount));
  const [zoom, setZoom] = useState(1);
  const [sizes, setSizes] = useState<ReadonlyMap<number, Size>>(new Map());
  const [panel, setPanel] = useState<null | "settings" | "index">(null);
  const [help, setHelp] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [pinnedHidden, setPinnedHidden] = useState(false);
  const [endReached, setEndReached] = useState(false);
  const verticalRef = useRef<VerticalHandle>(null);
  const pagedRef = useRef<ViewHandle>(null);
  const hideTimer = useRef<number | undefined>(undefined);
  const [activity, setActivity] = useState(0);
  const [initialRatio] = useState(session.start.scrollRatio);

  const { schedule, flush } = useProgressSaver(comic.id, pageCount);

  const spreads = useMemo(() => buildSpreads(pageCount, mode, settings["reader.coverAlone"], sizes), [pageCount, mode, settings, sizes]);
  const spreadIdx = spreadIndexOf(spreads, page);
  const spread = spreads[spreadIdx] ?? [page];
  const isLast = spreadIdx >= spreads.length - 1;

  const onSize = useCallback((i: number, s: Size) => {
    setSizes((m) => {
      const prev = m.get(i);
      if (prev && prev.width === s.width && prev.height === s.height) return m;
      const n = new Map(m);
      n.set(i, s);
      return n;
    });
  }, []);

  // Grava a posição ao mudar de página (modos paginados).
  useEffect(() => {
    if (mode !== "vertical") schedule(page, 0);
  }, [page, mode, schedule]);

  // Pré-carrega as próximas duas visões e a anterior.
  const neighbors = useMemo(() => {
    if (mode === "vertical") return [];
    return [spreads[spreadIdx + 1], spreads[spreadIdx + 2], spreads[spreadIdx - 1]].flat().filter((p): p is number => p !== undefined);
  }, [spreads, spreadIdx, mode]);
  usePreload(comic.id, neighbors, !pdf);

  const goToPage = useCallback(
    (p: number) => {
      const target = clampPage(p, pageCount);
      setEndReached(false);
      setPage(target);
      if (mode === "vertical") verticalRef.current?.scrollToPage(target, 0);
    },
    [pageCount, mode],
  );

  const next = useCallback(() => {
    if (mode === "vertical") {
      if (page >= pageCount - 1) setEndReached(true);
      else goToPage(page + 1);
      return;
    }
    if (isLast) {
      setEndReached(true);
      if (settings["reader.end"] === "library") {
        void flush().then(() => nav.back());
      }
      return;
    }
    goToPage(spreads[spreadIdx + 1]![0]!);
  }, [mode, page, pageCount, isLast, spreads, spreadIdx, goToPage, settings, flush, nav]);

  const prev = useCallback(() => {
    if (mode === "vertical") {
      goToPage(page - 1);
      return;
    }
    if (spreadIdx > 0) goToPage(spreads[spreadIdx - 1]![0]!);
  }, [mode, page, spreadIdx, spreads, goToPage]);

  const back = useCallback(() => {
    void flush();
    nav.back();
  }, [flush, nav]);

  const toggleFullscreen = useCallback(async () => {
    try {
      const w = getCurrentWindow();
      const fs = await w.isFullscreen();
      await w.setFullscreen(!fs);
      setFullscreen(!fs);
    } catch (e) {
      toast.error("Não foi possível alternar a tela cheia", errorMessage(e));
    }
  }, [toast]);

  // Sai da tela cheia ao fechar o leitor.
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

  const setFit = (f: FitMode) => {
    setZoom(1);
    void update("reader.fit", f);
  };
  const setMode = (m: ReaderMode) => void update("reader.mode", m);

  // ---- Controles: ocultar automaticamente, com forma clara de reaparecer ----
  const tipActive = onboarding.readerTipVisible;
  const autoHide = settings["reader.controls"] === "autohide" && !panel && !help && !tipActive;
  const showControls = useCallback(() => {
    if (pinnedHidden) return;
    setControlsVisible(true);
    setActivity((a) => a + 1);
  }, [pinnedHidden]);
  // Oculta após alguns segundos sem atividade (o temporizador recomeça a cada movimento).
  useEffect(() => {
    window.clearTimeout(hideTimer.current);
    if (!autoHide || pinnedHidden) return;
    hideTimer.current = window.setTimeout(() => setControlsVisible(false), 2800);
    return () => window.clearTimeout(hideTimer.current);
  }, [autoHide, pinnedHidden, activity]);
  const visible = (controlsVisible && !pinnedHidden) || !!panel || help || tipActive;
  const toggleControls = () => {
    setPinnedHidden((h) => {
      const nextHidden = !h;
      if (!nextHidden) setControlsVisible(true);
      return nextHidden;
    });
  };

  // ---- Teclado ----
  const run = useCallback(
    (action: ShortcutAction) => {
      switch (action) {
        case "next":
          return next();
        case "prev":
          return prev();
        case "first":
          return goToPage(0);
        case "last":
          return goToPage(pageCount - 1);
        case "zoomIn":
          return setZoom((z) => zoomIn(z));
        case "zoomOut":
          return setZoom((z) => zoomOut(z));
        case "zoomReset":
          return setZoom(1);
        case "fitPage":
          return setFit("page");
        case "fitWidth":
          return setFit("width");
        case "fitHeight":
          return setFit("height");
        case "cycleMode":
          return setMode(nextMode(mode));
        case "toggleDirection":
          return void update("reader.direction", rtl ? "ltr" : "rtl");
        case "fullscreen":
          return void toggleFullscreen();
        case "toggleControls":
          return toggleControls();
        case "pageIndex":
          return setPanel((p) => (p === "index" ? null : "index"));
        case "help":
          return setHelp(true);
        case "back":
          return back();
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [next, prev, goToPage, pageCount, mode, rtl, toggleFullscreen, back, update],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || isTyping(e.target) || document.querySelector('[aria-modal="true"]')) return;
      if (e.key === "Escape") {
        if (panel) setPanel(null);
        else if (fullscreen) void toggleFullscreen();
        return;
      }
      const key = keyFromEvent(e);
      if (!key) return;
      // No modo vertical, setas para cima/baixo e espaço rolam normalmente.
      if (mode === "vertical" && ["ArrowUp", "ArrowDown", "Space", "Shift+Space"].includes(key)) return;
      const action = resolveArrow(actionForKey(shortcuts, key), key, rtl && mode !== "vertical");
      if (!action) return;
      // Botões e links focados mantêm Espaço/Enter.
      if ((key === "Space" || key === "Enter") && (e.target as HTMLElement)?.closest?.("button, a")) return;
      e.preventDefault();
      showControls();
      run(action);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [shortcuts, rtl, mode, panel, fullscreen, run, toggleFullscreen, showControls]);

  // Ctrl + roda do mouse: zoom.
  useEffect(() => {
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      setZoom((z) => (e.deltaY < 0 ? zoomIn(z) : zoomOut(z)));
    };
    window.addEventListener("wheel", onWheel, { passive: false });
    return () => window.removeEventListener("wheel", onWheel);
  }, []);

  const wheelLock = useRef(0);
  const onWheelEdge = useCallback(
    (delta: number, atStart: boolean, atEnd: boolean) => {
      if (settings["reader.wheel"] !== "page" || Math.abs(delta) < 4) return false;
      const now = Date.now();
      if (now - wheelLock.current < 350) return true;
      if (delta > 0 && atEnd) {
        wheelLock.current = now;
        next();
        return true;
      }
      if (delta < 0 && atStart) {
        wheelLock.current = now;
        prev();
        return true;
      }
      return false;
    },
    [settings, next, prev],
  );

  const onPosition = useCallback(
    (p: number, ratio: number) => {
      setPage(p);
      schedule(p, ratio, 1000);
    },
    [schedule],
  );

  const resetProgress = () => {
    void api
      .resetProgress(comic.id)
      .then(() => {
        goToPage(0);
        toast.success("Progresso reiniciado");
      })
      .catch((e) => toast.error("Não foi possível reiniciar o progresso", errorMessage(e)));
  };

  const indicator = mode === "vertical" ? pageIndicator([page], pageCount) : pageIndicator(spread, pageCount);
  const ModeIcon = MODE_ICONS[mode];
  const tour = tipActive ? "ring-2 ring-accent ring-offset-2 ring-offset-elevated rounded-md" : "";
  const prevLabel = "Página anterior";
  const nextLabel = "Próxima página";
  const k = (a: ShortcutAction) => (shortcuts[a][0] ? formatKey(shortcuts[a][0]!) : undefined);

  const sideButtons = mode !== "vertical" && (
    <>
      <button
        type="button"
        aria-label={rtl ? nextLabel : prevLabel}
        title={rtl ? nextLabel : prevLabel}
        onClick={rtl ? next : prev}
        disabled={!rtl && spreadIdx === 0}
        className={cn(
          "absolute top-1/2 left-3 z-10 flex size-11 -translate-y-1/2 items-center justify-center rounded-full border border-border bg-elevated/90 text-fg shadow-overlay transition-opacity hover:bg-elevated disabled:opacity-0",
          visible ? "opacity-100" : "pointer-events-none opacity-0",
        )}
      >
        <ChevronLeft size={22} aria-hidden />
      </button>
      <button
        type="button"
        aria-label={rtl ? prevLabel : nextLabel}
        title={rtl ? prevLabel : nextLabel}
        onClick={rtl ? prev : next}
        disabled={rtl && spreadIdx === 0}
        className={cn(
          "absolute top-1/2 right-3 z-10 flex size-11 -translate-y-1/2 items-center justify-center rounded-full border border-border bg-elevated/90 text-fg shadow-overlay transition-opacity hover:bg-elevated disabled:opacity-0",
          visible ? "opacity-100" : "pointer-events-none opacity-0",
        )}
      >
        <ChevronRight size={22} aria-hidden />
      </button>
    </>
  );

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-reader text-fg" onMouseMove={showControls} aria-label={`Leitor: ${comic.title}`}>
      {/* Barra superior */}
      <header
        className={cn(
          "absolute inset-x-0 top-0 z-20 flex items-center gap-2 border-b border-border bg-elevated/95 px-3 py-1.5 backdrop-blur transition-transform duration-200",
          visible ? "translate-y-0" : "-translate-y-full",
        )}
        onMouseEnter={() => window.clearTimeout(hideTimer.current)}
      >
        <span className={tipActive ? tour : ""} data-tour="reader-back">
          <Button variant="ghost" size="sm" icon={ArrowLeft} onClick={back} title={`Voltar à biblioteca (${k("back") ?? ""})`}>
            Biblioteca
          </Button>
        </span>
        <div className="min-w-0 flex-1 px-2">
          <p className="truncate text-sm font-semibold">{comic.title}</p>
          <p className="text-xs text-muted" aria-live="polite">
            {indicator}
          </p>
        </div>
        <div className="flex items-center gap-0.5 max-md:hidden" role="group" aria-label="Ajuste da imagem">
          {(["page", "width"] as FitMode[]).map((f) => (
            <IconButton key={f} icon={FIT_ICONS[f as "page" | "width"]} label={FIT_LABELS[f]} shortcut={k(f === "page" ? "fitPage" : "fitWidth")} pressed={fit === f} onClick={() => setFit(f)} size="sm" />
          ))}
        </div>
        <div className={cn("flex items-center gap-0.5", tour)} role="group" aria-label="Zoom" data-tour="reader-zoom">
          <IconButton icon={ZoomOut} label="Diminuir zoom" shortcut={k("zoomOut")} onClick={() => setZoom(zoomOut)} size="sm" />
          <button
            type="button"
            onClick={() => setZoom(1)}
            title="Voltar o zoom para 100%"
            className="h-7 w-12 rounded-md text-center font-mono text-xs text-muted hover:bg-hover hover:text-fg"
            aria-label={`Zoom atual ${Math.round(zoom * 100)}%. Clique para voltar a 100%`}
          >
            {Math.round(zoom * 100)}%
          </button>
          <IconButton icon={ZoomIn} label="Aumentar zoom" shortcut={k("zoomIn")} onClick={() => setZoom(zoomIn)} size="sm" />
        </div>
        <span className="mx-1 h-5 w-px bg-border" aria-hidden />
        <IconButton icon={ModeIcon} label={`Alternar modo de exibição (atual: ${MODE_LABELS[mode]})`} shortcut={k("cycleMode")} onClick={() => setMode(nextMode(mode))} size="sm" />
        <IconButton icon={ListOrdered} label="Abrir índice de páginas" shortcut={k("pageIndex")} pressed={panel === "index"} onClick={() => setPanel((p) => (p === "index" ? null : "index"))} size="sm" />
        <IconButton icon={Settings2} label="Configurações de leitura" pressed={panel === "settings"} onClick={() => setPanel((p) => (p === "settings" ? null : "settings"))} size="sm" />
        <IconButton icon={fullscreen ? Minimize : Maximize} label={fullscreen ? "Sair da tela cheia" : "Entrar em tela cheia"} shortcut={k("fullscreen")} onClick={() => void toggleFullscreen()} size="sm" />
      </header>

      {/* Área de leitura (com controles fixos, reserva espaço para não cobrir a página) */}
      <div className={cn("relative flex-1 overflow-hidden", (mode !== "vertical" || settings["reader.controls"] === "always") && "pt-[45px] pb-[49px]")}>
        {mode === "vertical" ? (
          <VerticalView
            ref={verticalRef}
            comicId={comic.id}
            count={pageCount}
            sizes={sizes}
            fit={fit}
            zoom={zoom}
            pdf={pdf}
            startPage={page}
            startRatio={page === session.start.pageIndex ? initialRatio : 0}
            onSize={onSize}
            onPosition={onPosition}
          />
        ) : (
          <PagedView ref={pagedRef} comicId={comic.id} spread={spread} sizes={sizes} fit={fit} zoom={zoom} direction={direction} pdf={pdf} onSize={onSize} onWheelEdge={onWheelEdge} />
        )}
        {sideButtons}
        {panel === "settings" && (
          <ReaderSettingsPanel
            onClose={() => setPanel(null)}
            onResetProgress={resetProgress}
            onShowShortcuts={() => {
              setPanel(null);
              setHelp(true);
            }}
            onHideControls={() => {
              setPanel(null);
              toggleControls();
            }}
            hideKey={k("toggleControls")}
          />
        )}
        {panel === "index" && (
          <PageIndexPanel
            comicId={comic.id}
            count={pageCount}
            current={mode === "vertical" ? [page] : spread}
            isPdf={!!pdf}
            onSelect={(p) => {
              goToPage(p);
              setPanel(null);
            }}
            onClose={() => setPanel(null)}
          />
        )}
        {endReached && (
          <div role="status" className="p-enter absolute bottom-20 left-1/2 z-20 -translate-x-1/2 rounded-lg border border-border bg-elevated px-4 py-3 text-center shadow-overlay">
            <p className="font-semibold">Você chegou ao fim de “{comic.title}”</p>
            <p className="text-sm text-muted">A leitura foi marcada como concluída.</p>
            <div className="mt-2 flex justify-center gap-2">
              <Button size="sm" onClick={() => setEndReached(false)}>
                Continuar aqui
              </Button>
              <Button size="sm" variant="primary" onClick={back}>
                Voltar à biblioteca
              </Button>
            </div>
          </div>
        )}
        {!visible && (
          <button
            type="button"
            onClick={() => {
              setPinnedHidden(false);
              setControlsVisible(true);
            }}
            className="absolute top-2 right-2 z-20 inline-flex items-center gap-1.5 rounded-full border border-border bg-elevated/80 px-3 py-1 text-xs font-medium text-fg opacity-70 shadow-overlay transition-opacity hover:opacity-100 focus-visible:opacity-100"
          >
            <Eye size={13} aria-hidden /> Mostrar controles{k("toggleControls") ? ` (${k("toggleControls")})` : ""}
          </button>
        )}
      </div>

      {/* Barra inferior */}
      <footer
        className={cn(
          "absolute inset-x-0 bottom-0 z-20 flex items-center gap-3 border-t border-border bg-elevated/95 px-4 py-2 backdrop-blur transition-transform duration-200",
          visible ? "translate-y-0" : "translate-y-full",
        )}
        onMouseEnter={() => window.clearTimeout(hideTimer.current)}
      >
        <div className={cn("flex flex-1 items-center gap-3", rtl && "flex-row-reverse", tour)} data-tour="reader-nav">
          <Button size="sm" icon={rtl ? ChevronRight : ChevronLeft} onClick={prev} disabled={mode !== "vertical" ? spreadIdx === 0 : page === 0} title={`${prevLabel} (${k("prev") ?? ""})`}>
            <span className="max-sm:sr-only">{prevLabel}</span>
          </Button>
          <label htmlFor="reader-progress" className="sr-only">
            Página atual
          </label>
          <input
            id="reader-progress"
            type="range"
            min={1}
            max={pageCount}
            value={(mode === "vertical" ? page : spread[0]!) + 1}
            onChange={(e) => goToPage(Number(e.target.value) - 1)}
            dir={rtl ? "rtl" : "ltr"}
            aria-valuetext={indicator}
            className="h-1.5 flex-1 cursor-pointer accent-[var(--color-accent)]"
          />
          <Button size="sm" trailingIcon={rtl ? ChevronLeft : ChevronRight} onClick={next} title={`${nextLabel} (${k("next") ?? ""})`}>
            <span className="max-sm:sr-only">{nextLabel}</span>
          </Button>
        </div>
      </footer>

      {tipActive && <ReaderTip shortcuts={shortcuts} />}
      <ShortcutsDialog open={help} onClose={() => setHelp(false)} shortcuts={shortcuts} />
    </div>
  );
}
