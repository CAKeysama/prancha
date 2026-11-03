import { useCallback, useEffect, useMemo, useRef, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
  Columns2,
  Eye,
  GalleryVertical,
  LayoutList,
  MessagesSquare,
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
  chapterAt,
  clampPage,
  hasTallPages,
  isContinuous,
  nextMode,
  pageIndicator,
  spreadIndexOf,
  zoomIn,
  zoomOut,
  type ReaderChapter,
  type Size,
} from "../../domain/reader";
import { actionForKey, formatKey, keyFromEvent, resolveArrow, type ShortcutAction } from "../../domain/shortcuts";
import type { FitMode, ReaderMode } from "../../domain/settings";
import type { ReaderSession } from "../../domain/types";
import { api, ApiError, errorMessage } from "../../services/api";
import { openPdf } from "../../services/pdf";
import { pageUrl, type PageSrc } from "../../services/urls";
import { useSettings } from "../../app/settings-context";
import { useNav } from "../../app/navigation";
import { useOnboarding } from "../onboarding/onboarding-context";
import { ReaderTip } from "../onboarding/ReaderTip";
import { useLeaveFullscreenOnClose } from "./fullscreen";
import { GuidedReader } from "./GuidedReader";
import { PageIndexPanel, ReaderSettingsPanel, ShortcutsDialog } from "./ReaderPanels";
import { PagedView, VerticalView, type VerticalHandle } from "./Views";
import { usePreload } from "./PageView";
import { useProgressSaver } from "./useProgressSaver";

const MODE_ICONS: Record<ReaderMode, typeof Square> = { single: RectangleVertical, double: Columns2, vertical: LayoutList, webtoon: GalleryVertical };
const FIT_ICONS: Record<"page" | "width", typeof Square> = { page: Square, width: MoveHorizontal };

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable);
}

function SideButton({ side, icon: Icon, label, onClick, disabled, visible }: { side: "left-3" | "right-3"; icon: typeof Square; label: string; onClick: () => void; disabled: boolean; visible: boolean }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "absolute top-1/2 z-10 flex size-11 -translate-y-1/2 items-center justify-center rounded-full border border-border bg-elevated/90 text-fg shadow-overlay transition-opacity hover:bg-elevated disabled:opacity-0",
        side,
        visible ? "opacity-100" : "pointer-events-none opacity-0",
      )}
    >
      <Icon size={22} aria-hidden />
    </button>
  );
}

/** O que o leitor mostra. Vem da biblioteca ou de um capítulo de uma fonte, e o leitor é o mesmo. */
export interface ReaderContent {
  title: string;
  pageCount: number;
  start: { pageIndex: number; scrollRatio: number };
  pageSrc: PageSrc;
  saveProgress: (page: number, count: number, ratio: number) => Promise<unknown>;
  resetProgress: (page: number) => Promise<void>;
  chapters: ReaderChapter[];
  backLabel: string;
  backTitle: string;
  /** Modo guardado para esta obra. Sem ele, vale o das configurações. */
  chosenMode: ReaderMode | null;
  webtoonHint: boolean;
  /** Sem isto, a escolha do modo vale para todas as leituras. */
  saveMode?: (mode: ReaderMode) => void;
  /** A cada página que passa. */
  onPage?: (page: number) => void;
  /** Ainda vem mais um capítulo depois da última página. */
  waitingForNext?: boolean;
  /** O que aparece depois da última página. */
  ending?: ReactNode;
  /** Abrem um capítulo que ainda não está na sequência. */
  previousChapter?: () => void;
  nextChapter?: () => void;
}

export function ReaderScreen({ id }: { id: number }) {
  const nav = useNav();
  const [session, setSession] = useState<ReaderSession | null>(null);
  const [error, setError] = useState<{ code: string; message: string } | null>(null);
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [pageCount, setPageCount] = useState(0);
  const zoomState = useState(1);
  useLeaveFullscreenOnClose();

  const content = useMemo<ReaderContent | null>(
    () =>
      session && {
        title: session.comic.title,
        pageCount,
        start: session.start,
        pageSrc: (index, token) => pageUrl(session.comic.id, index, token),
        saveProgress: (page, count, ratio) => api.saveProgress(session.comic.id, page, count, ratio),
        resetProgress: () => api.resetProgress(session.comic.id),
        chapters: [{ title: null, first: 0, count: pageCount, guidedComicId: session.comic.id }],
        backLabel: "Biblioteca",
        backTitle: "Voltar à biblioteca",
        chosenMode: null,
        webtoonHint: false,
      },
    [session, pageCount],
  );

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
  if (!session || !content || pageCount === 0) {
    return (
      <div className="flex h-full items-center justify-center bg-reader text-fg">
        <Spinner size={22} label={session?.comic.format === "pdf" || !session ? "Abrindo HQ…" : "Preparando páginas…"} />
      </div>
    );
  }
  return <Reader key={session.comic.id} content={content} pdf={pdf} zoomState={zoomState} />;
}

export function Reader({ content, pdf, zoomState: [zoom, setZoom] }: { content: ReaderContent; pdf: PDFDocumentProxy | null; zoomState: [number, Dispatch<SetStateAction<number>>] }) {
  const nav = useNav();
  const toast = useToast();
  const { settings, update } = useSettings();
  const onboarding = useOnboarding();
  const { pageCount, chapters, saveMode, onPage, waitingForNext, nextChapter, previousChapter } = content;
  const direction = settings["reader.direction"];
  const fit = settings["reader.fit"];
  const rtl = direction === "rtl";
  const shortcuts = settings["reader.shortcuts"];

  const [page, setPage] = useState(() => clampPage(content.start.pageIndex, pageCount));
  const [sizes, setSizes] = useState<ReadonlyMap<number, Size>>(new Map());
  const [chosenMode, setChosenMode] = useState(content.chosenMode);
  // Só vira: uma obra em tiras não volta a parecer outra coisa no meio da leitura.
  const [tall, setTall] = useState(false);
  if (!tall && hasTallPages(sizes)) setTall(true);
  const mode: ReaderMode = chosenMode ?? (tall || content.webtoonHint ? "webtoon" : settings["reader.mode"]);
  const continuous = isContinuous(mode);
  const [panel, setPanel] = useState<null | "settings" | "index">(null);
  const [help, setHelp] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [pinnedHidden, setPinnedHidden] = useState(false);
  const [endReached, setEndReached] = useState(false);
  const [guided, setGuided] = useState(false);
  const verticalRef = useRef<VerticalHandle>(null);
  const hideTimer = useRef<number | undefined>(undefined);
  const [activity, setActivity] = useState(0);
  const [initialRatio] = useState(content.start.scrollRatio);

  const { schedule, flush } = useProgressSaver(content.saveProgress, pageCount);

  const chapterStarts = useMemo(() => new Map(chapters.slice(1).map((c) => [c.first, c.title ?? ""])), [chapters]);
  const startSet = useMemo(() => new Set(chapterStarts.keys()), [chapterStarts]);
  const spreads = useMemo(() => buildSpreads(pageCount, mode, settings["reader.coverAlone"], sizes, startSet), [pageCount, mode, settings, sizes, startSet]);
  const spreadIdx = spreadIndexOf(spreads, page);
  const spread = spreads[spreadIdx] ?? [page];
  const isLast = spreadIdx >= spreads.length - 1;
  const chapter = chapterAt(chapters, page);
  const chapterIndex = chapters.indexOf(chapter);

  const onSize = useCallback((i: number, s: Size) => {
    setSizes((m) => {
      const prev = m.get(i);
      if (prev && prev.width === s.width && prev.height === s.height) return m;
      const n = new Map(m);
      n.set(i, s);
      return n;
    });
  }, []);

  // Uma obra em tiras continua assim nos próximos capítulos, sem precisar descobrir de novo.
  useEffect(() => {
    if (tall && chosenMode === null) saveMode?.("webtoon");
  }, [tall, chosenMode, saveMode]);

  useEffect(() => {
    onPage?.(page);
  }, [page, onPage]);

  // Grava a posição ao mudar de página (modos paginados).
  useEffect(() => {
    if (!continuous) schedule(page, 0);
  }, [page, continuous, schedule]);

  // Pré-carrega as próximas duas visões e a anterior.
  const neighbors = useMemo(() => {
    if (continuous) return [];
    return [spreads[spreadIdx + 1], spreads[spreadIdx + 2], spreads[spreadIdx - 1]].flat().filter((p): p is number => p !== undefined);
  }, [spreads, spreadIdx, continuous]);
  usePreload(content.pageSrc, neighbors, !pdf, onSize);

  const goToPage = useCallback(
    (p: number) => {
      const target = clampPage(p, pageCount);
      setEndReached(false);
      setPage(target);
      if (continuous) verticalRef.current?.scrollToPage(target, 0);
    },
    [pageCount, continuous],
  );

  const atLastPage = continuous ? page >= pageCount - 1 : isLast;

  const next = useCallback(() => {
    if (!atLastPage) {
      goToPage(continuous ? page + 1 : spreads[spreadIdx + 1]![0]!);
      return;
    }
    if (waitingForNext) return;
    setEndReached(true);
    if (!continuous && settings["reader.end"] === "library") {
      void flush().then(() => nav.back());
    }
  }, [atLastPage, continuous, page, spreads, spreadIdx, goToPage, waitingForNext, settings, flush, nav]);

  const prev = useCallback(() => {
    if (continuous) {
      goToPage(page - 1);
      return;
    }
    if (spreadIdx > 0) goToPage(spreads[spreadIdx - 1]![0]!);
  }, [continuous, page, spreadIdx, spreads, goToPage]);

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

  // Trocar de capítulo recria o leitor, e a tela cheia continua.
  useEffect(() => {
    try {
      void getCurrentWindow()
        .isFullscreen()
        .then(setFullscreen)
        .catch(() => undefined);
    } catch {
      /* fora do Tauri */
    }
  }, []);

  const setFit = (f: FitMode) => {
    setZoom(1);
    void update("reader.fit", f);
  };
  const setMode = (m: ReaderMode) => {
    setChosenMode(m);
    if (saveMode) saveMode(m);
    else void update("reader.mode", m);
  };
  const togglePanel = (name: "settings" | "index") => setPanel((p) => (p === name ? null : name));

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
          return goToPage(chapter.first);
        case "last":
          return goToPage(chapter.first + chapter.count - 1);
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
          return togglePanel("index");
        case "guided":
          if (chapter.guidedComicId === null) return;
          setPanel(null);
          return setGuided(true);
        case "help":
          return setHelp(true);
        case "back":
          return back();
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [next, prev, goToPage, chapter, mode, rtl, toggleFullscreen, back, update],
  );

  useEffect(() => {
    if (guided) return;
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
      if (continuous && ["ArrowUp", "ArrowDown", "Space", "Shift+Space"].includes(key)) return;
      const action = resolveArrow(actionForKey(shortcuts, key), key, rtl && !continuous);
      if (!action) return;
      // Botões e links focados mantêm Espaço/Enter.
      if ((key === "Space" || key === "Enter") && (e.target as HTMLElement)?.closest?.("button, a")) return;
      e.preventDefault();
      showControls();
      run(action);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [guided, shortcuts, rtl, continuous, panel, fullscreen, run, toggleFullscreen, showControls]);

  // Ctrl + roda do mouse: zoom.
  useEffect(() => {
    if (guided) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      setZoom((z) => (e.deltaY < 0 ? zoomIn(z) : zoomOut(z)));
    };
    window.addEventListener("wheel", onWheel, { passive: false });
    return () => window.removeEventListener("wheel", onWheel);
  }, [guided, setZoom]);

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
    void content
      .resetProgress(page)
      .then(() => {
        goToPage(chapter.first);
        toast.success("Progresso reiniciado");
      })
      .catch((e) => toast.error("Não foi possível reiniciar o progresso", errorMessage(e)));
  };

  const indicator = pageIndicator((continuous ? [page] : spread).map((p) => p - chapter.first), chapter.count);
  const firstShown = continuous ? page : spread[0]!;
  const chapterBefore = chapters[chapterIndex - 1];
  const chapterAfter = chapters[chapterIndex + 1];
  const toPreviousChapter = () => {
    if (chapterBefore) goToPage(chapterBefore.first);
    else if (previousChapter) void flush().then(previousChapter);
  };
  const toNextChapter = () => {
    if (chapterAfter) goToPage(chapterAfter.first);
    else if (nextChapter) void flush().then(nextChapter);
  };
  const ModeIcon = MODE_ICONS[mode];
  const tour = tipActive ? "ring-2 ring-accent ring-offset-2 ring-offset-elevated rounded-md" : "";
  const prevLabel = "Página anterior";
  const nextLabel = "Próxima página";
  const k = (a: ShortcutAction) => (shortcuts[a][0] ? formatKey(shortcuts[a][0]!) : undefined);

  const sideButtons = !continuous && (
    <>
      <SideButton side="left-3" icon={ChevronLeft} label={rtl ? nextLabel : prevLabel} onClick={rtl ? next : prev} disabled={!rtl && spreadIdx === 0} visible={visible} />
      <SideButton side="right-3" icon={ChevronRight} label={rtl ? prevLabel : nextLabel} onClick={rtl ? prev : next} disabled={rtl && spreadIdx === 0} visible={visible} />
    </>
  );

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-reader text-fg" onMouseMove={showControls} aria-label={`Leitor: ${content.title}`}>
      {/* Barra superior */}
      <header
        inert={guided}
        className={cn(
          "absolute inset-x-0 top-0 z-20 flex items-center gap-2 border-b border-border bg-elevated/95 px-3 py-1.5 backdrop-blur transition-transform duration-200",
          visible ? "translate-y-0" : "-translate-y-full",
        )}
        onMouseEnter={() => window.clearTimeout(hideTimer.current)}
      >
        <span className={tour} data-tour="reader-back">
          <Button variant="ghost" size="sm" icon={ArrowLeft} onClick={back} title={`${content.backTitle} (${k("back") ?? ""})`}>
            {content.backLabel}
          </Button>
        </span>
        <div className="min-w-0 flex-1 px-2">
          <p className="truncate text-sm font-semibold">{chapter.title ? `${content.title} · ${chapter.title}` : content.title}</p>
          <p className="text-xs text-muted" aria-live="polite">
            {indicator}
          </p>
        </div>
        {mode !== "webtoon" && (
          <div className="flex items-center gap-0.5 max-md:hidden" role="group" aria-label="Ajuste da imagem">
            {(["page", "width"] as FitMode[]).map((f) => (
              <IconButton key={f} icon={FIT_ICONS[f as "page" | "width"]} label={FIT_LABELS[f]} shortcut={k(f === "page" ? "fitPage" : "fitWidth")} pressed={fit === f} onClick={() => setFit(f)} size="sm" />
            ))}
          </div>
        )}
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
        {chapter.guidedComicId !== null && (
          <Button
            variant="ghost"
            size="sm"
            icon={MessagesSquare}
            title={`Ler balão por balão (${k("guided") ?? ""})`}
            onClick={() => run("guided")}
          >
            <span className="max-lg:sr-only">Leitura Guiada</span>
          </Button>
        )}
        <IconButton icon={ModeIcon} label={`Alternar modo de exibição (atual: ${MODE_LABELS[mode]})`} shortcut={k("cycleMode")} onClick={() => setMode(nextMode(mode))} size="sm" />
        <IconButton icon={ListOrdered} label="Abrir índice de páginas" shortcut={k("pageIndex")} pressed={panel === "index"} onClick={() => togglePanel("index")} size="sm" />
        <IconButton icon={Settings2} label="Configurações de leitura" pressed={panel === "settings"} onClick={() => togglePanel("settings")} size="sm" />
        <IconButton icon={fullscreen ? Minimize : Maximize} label={fullscreen ? "Sair da tela cheia" : "Entrar em tela cheia"} shortcut={k("fullscreen")} onClick={() => void toggleFullscreen()} size="sm" />
      </header>

      {/* Área de leitura (com controles fixos, reserva espaço para não cobrir a página) */}
      <div inert={guided} className={cn("relative flex-1 overflow-hidden", (!continuous || settings["reader.controls"] === "always") && "pt-[45px] pb-[49px]")}>
        {continuous ? (
          <VerticalView
            ref={verticalRef}
            pageSrc={content.pageSrc}
            webtoon={mode === "webtoon"}
            chapterStarts={chapterStarts}
            footer={content.ending}
            count={pageCount}
            sizes={sizes}
            fit={fit}
            zoom={zoom}
            pdf={pdf}
            startPage={page}
            startRatio={page === content.start.pageIndex ? initialRatio : 0}
            onSize={onSize}
            onPosition={onPosition}
          />
        ) : (
          <PagedView pageSrc={content.pageSrc} spread={spread} sizes={sizes} fit={fit} zoom={zoom} direction={direction} pdf={pdf} onSize={onSize} onWheelEdge={onWheelEdge} />
        )}
        {!continuous && firstShown === chapter.first && chapterIndex > 0 && (
          <p role="status" className="p-enter pointer-events-none absolute top-[53px] left-1/2 z-10 max-w-[80%] -translate-x-1/2 truncate rounded-full border border-border bg-elevated/90 px-3 py-1 text-xs text-muted shadow-overlay">
            A partir daqui: {chapter.title}
          </p>
        )}
        {!continuous && atLastPage && waitingForNext && <div className="absolute inset-x-0 bottom-16 z-10 flex justify-center">{content.ending}</div>}
        {sideButtons}
        {panel === "settings" && (
          <ReaderSettingsPanel
            mode={mode}
            onMode={setMode}
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
            pageSrc={(index, token) => content.pageSrc(chapter.first + index, token)}
            count={chapter.count}
            current={(continuous ? [page] : spread).map((p) => p - chapter.first)}
            isPdf={!!pdf}
            onSelect={(p) => {
              goToPage(chapter.first + p);
              setPanel(null);
            }}
            onClose={() => setPanel(null)}
          />
        )}
        {endReached && (
          <div role="status" className="p-enter absolute bottom-20 left-1/2 z-20 -translate-x-1/2 rounded-lg border border-border bg-elevated px-4 py-3 text-center shadow-overlay">
            <p className="font-semibold">Você chegou ao fim de “{content.title}”</p>
            <p className="text-sm text-muted">A leitura foi marcada como concluída.</p>
            <div className="mt-2 flex justify-center gap-2">
              <Button size="sm" onClick={() => setEndReached(false)}>
                Continuar aqui
              </Button>
              <Button size="sm" variant="primary" onClick={back}>
                {content.backTitle}
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
        inert={guided}
        className={cn(
          "absolute inset-x-0 bottom-0 z-20 flex items-center gap-3 border-t border-border bg-elevated/95 px-4 py-2 backdrop-blur transition-transform duration-200",
          visible ? "translate-y-0" : "translate-y-full",
        )}
        onMouseEnter={() => window.clearTimeout(hideTimer.current)}
      >
        <div className={cn("flex flex-1 items-center gap-3", rtl && "flex-row-reverse", tour)} data-tour="reader-nav">
          {(chapterBefore || previousChapter) && <IconButton icon={rtl ? ChevronsRight : ChevronsLeft} label="Capítulo anterior" variant="secondary" size="sm" onClick={toPreviousChapter} />}
          <Button size="sm" icon={rtl ? ChevronRight : ChevronLeft} onClick={prev} disabled={!continuous ? spreadIdx === 0 : page === 0} title={`${prevLabel} (${k("prev") ?? ""})`}>
            <span className="max-sm:sr-only">{prevLabel}</span>
          </Button>
          <label htmlFor="reader-progress" className="sr-only">
            Página atual
          </label>
          <input
            id="reader-progress"
            type="range"
            min={1}
            max={chapter.count}
            value={firstShown - chapter.first + 1}
            onChange={(e) => goToPage(chapter.first + Number(e.target.value) - 1)}
            dir={rtl ? "rtl" : "ltr"}
            aria-valuetext={indicator}
            className="h-1.5 flex-1 cursor-pointer accent-[var(--color-accent)]"
          />
          <Button size="sm" trailingIcon={rtl ? ChevronLeft : ChevronRight} onClick={next} title={`${nextLabel} (${k("next") ?? ""})`}>
            <span className="max-sm:sr-only">{nextLabel}</span>
          </Button>
          {(chapterAfter || nextChapter) && <IconButton icon={rtl ? ChevronsLeft : ChevronsRight} label="Próximo capítulo" variant="secondary" size="sm" onClick={toNextChapter} />}
        </div>
      </footer>

      {guided && chapter.guidedComicId !== null && (
        <GuidedReader
          comicId={chapter.guidedComicId}
          page={page - chapter.first}
          pageCount={chapter.count}
          pdf={pdf}
          rtl={rtl}
          shortcuts={shortcuts}
          natural={sizes.get(page)}
          onSize={(i, s) => onSize(chapter.first + i, s)}
          onPage={(p) => goToPage(chapter.first + p)}
          onExit={() => setGuided(false)}
        />
      )}
      {tipActive && <ReaderTip shortcuts={shortcuts} />}
      <ShortcutsDialog open={help} onClose={() => setHelp(false)} shortcuts={shortcuts} />
    </div>
  );
}
