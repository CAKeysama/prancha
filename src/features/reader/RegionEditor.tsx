import { useEffect, useMemo, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";
import { ArrowDown, ArrowUp, Plus, RotateCcw, Trash2 } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { ConfirmDialog } from "../../components/Dialog";
import { Select } from "../../components/Form";
import { cn } from "../../components/cn";
import {
  HANDLES,
  KIND_LABELS,
  REGION_KINDS,
  edited,
  fitPage,
  isDoubtful,
  moveInList,
  moveRect,
  newRegion,
  rectFromDrag,
  resizeRect,
  type GuidedRegion,
  type Handle,
  type Rect,
  type RegionKind,
} from "../../domain/guided";
import type { Size } from "../../domain/reader";
import { comicPageSrc } from "../../services/urls";
import { PageView } from "./PageView";
import { SidePanel } from "./ReaderPanels";
import { useViewportSize } from "./Views";

const LIST_WIDTH = 340;
const PAGE_MARGIN = 24;
const NUDGE = 0.005;

const HANDLE_STYLE: Record<Handle, string> = {
  nw: "left-0 top-0 cursor-nwse-resize",
  n: "left-1/2 top-0 cursor-ns-resize",
  ne: "left-full top-0 cursor-nesw-resize",
  e: "left-full top-1/2 cursor-ew-resize",
  se: "left-full top-full cursor-nwse-resize",
  s: "left-1/2 top-full cursor-ns-resize",
  sw: "left-0 top-full cursor-nesw-resize",
  w: "left-0 top-1/2 cursor-ew-resize",
};

type Drag =
  | { type: "move"; id: string; startX: number; startY: number; origin: Rect }
  | { type: "resize"; id: string; handle: Handle; startX: number; startY: number; origin: Rect }
  | { type: "draw"; startX: number; startY: number; x: number; y: number };

interface RegionEditorProps {
  comicId: number;
  page: number;
  pdf: PDFDocumentProxy | null;
  natural: Size | undefined;
  regions: GuidedRegion[];
  initialSelection: string | null;
  canRestore: boolean;
  onSize: (index: number, size: Size) => void;
  onSave: (regions: GuidedRegion[]) => Promise<void>;
  onRestore: () => Promise<void>;
  onCancel: () => void;
}

/** Correção manual sobre a própria página: selecionar, arrastar, redimensionar, desenhar. */
export function RegionEditor({ comicId, page, pdf, natural, regions, initialSelection, canRestore, onSize, onSave, onRestore, onCancel }: RegionEditorProps) {
  const pageSrc = useMemo(() => comicPageSrc(comicId), [comicId]);
  const [draft, setDraft] = useState(regions);
  const [selected, setSelected] = useState(initialSelection);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const [drawing, setDrawing] = useState<Rect | null>(null);
  const area = useRef<HTMLDivElement>(null);
  const sheet = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const view = useViewportSize(area);
  const fit = fitPage(natural ?? { width: 2, height: 3 }, {
    width: Math.max(1, view.width - PAGE_MARGIN * 2),
    height: Math.max(1, view.height - PAGE_MARGIN * 2),
  });

  const close = () => (dirty ? setConfirmCancel(true) : onCancel());
  // O foco pode estar na página, fora do painel lateral.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector('[aria-modal="true"]')) return;
      if (dirty) setConfirmCancel(true);
      else onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dirty, onCancel]);

  const change = (next: GuidedRegion[]) => {
    setDraft(next);
    setDirty(true);
  };
  const patch = (id: string, values: Partial<GuidedRegion>) => change(draft.map((r) => (r.id === id ? edited(r, values) : r)));
  const remove = (id: string) => {
    change(draft.filter((r) => r.id !== id));
    if (selected === id) setSelected(null);
  };
  const add = (rect: Rect) => {
    const region = newRegion(rect, page, draft.length);
    change([...draft, region]);
    setSelected(region.id);
  };

  const pointer = (e: React.PointerEvent) => {
    const box = sheet.current!.getBoundingClientRect();
    return { x: (e.clientX - box.left) / box.width, y: (e.clientY - box.top) / box.height };
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const { x, y } = pointer(e);
    const target = e.target as HTMLElement;
    const id = target.closest<HTMLElement>("[data-region]")?.dataset.region;
    const handle = target.dataset.handle as Handle | undefined;
    const region = draft.find((r) => r.id === id);
    if (region) {
      setSelected(region.id);
      drag.current = handle
        ? { type: "resize", id: region.id, handle, startX: x, startY: y, origin: region }
        : { type: "move", id: region.id, startX: x, startY: y, origin: region };
    } else {
      setSelected(null);
      drag.current = { type: "draw", startX: x, startY: y, x, y };
    }
    sheet.current!.setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const { x, y } = pointer(e);
    if (d.type === "draw") {
      drag.current = { ...d, x, y };
      setDrawing(rectFromDrag(d.startX, d.startY, x, y));
    } else if (d.type === "move") {
      const { x: nx, y: ny } = moveRect(d.origin, x - d.startX, y - d.startY);
      patch(d.id, { x: nx, y: ny });
    } else {
      patch(d.id, resizeRect(d.origin, d.handle, x - d.startX, y - d.startY));
    }
  };

  const onPointerUp = () => {
    const d = drag.current;
    drag.current = null;
    if (d?.type === "draw") {
      const rect = rectFromDrag(d.startX, d.startY, d.x, d.y);
      setDrawing(null);
      if (rect) add(rect);
    }
  };

  const onBoxKey = (e: React.KeyboardEvent, region: GuidedRegion) => {
    const step = e.shiftKey ? NUDGE * 4 : NUDGE;
    const delta: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      remove(region.id);
    } else if (delta[e.key]) {
      e.preventDefault();
      const { x, y } = moveRect(region, delta[e.key]![0], delta[e.key]![1]);
      patch(region.id, { x, y });
    }
  };

  const guard = (work: () => Promise<void>) => () => {
    setBusy(true);
    // Em caso de falha o rascunho continua aqui, para tentar de novo.
    void work().finally(() => setBusy(false));
  };

  return (
    <div className="absolute inset-0 z-40 bg-reader">
      <div ref={area} className="absolute inset-y-0 left-0 flex items-center justify-center overflow-hidden" style={{ right: LIST_WIDTH }}>
        <div
          ref={sheet}
          data-testid="guided-editor-page"
          className="relative shrink-0 cursor-crosshair touch-none select-none"
          style={{ width: fit.width, height: fit.height }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        >
          <PageView src={pageSrc} index={page} width={fit.width} height={fit.height} pdf={pdf} onSize={onSize} />
          {draft.map((r, n) => (
            <div
              key={r.id}
              data-region={r.id}
              role="button"
              tabIndex={0}
              aria-pressed={selected === r.id}
              aria-label={`Região ${n + 1}: ${KIND_LABELS[r.kind]}${isDoubtful(r) ? ", duvidosa" : ""}`}
              onFocus={() => setSelected(r.id)}
              onKeyDown={(e) => onBoxKey(e, r)}
              className={cn(
                "absolute cursor-move rounded-[3px] border-2 focus-visible:outline-none",
                isDoubtful(r) ? "border-dashed border-attention" : "border-accent",
                selected === r.id ? "bg-accent/15" : "bg-accent/5",
              )}
              style={{ left: `${r.x * 100}%`, top: `${r.y * 100}%`, width: `${r.w * 100}%`, height: `${r.h * 100}%` }}
            >
              <span className="absolute top-0 left-0 flex h-5 min-w-5 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full bg-accent-emphasis px-1 text-xs font-semibold text-on-emphasis">
                {n + 1}
              </span>
              {selected === r.id &&
                HANDLES.map((h) => (
                  <span
                    key={h}
                    data-handle={h}
                    aria-hidden
                    className={cn("absolute size-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-accent bg-elevated", HANDLE_STYLE[h])}
                  />
                ))}
            </div>
          ))}
          {drawing && (
            <div
              aria-hidden
              className="pointer-events-none absolute rounded-[3px] border-2 border-accent bg-accent/15"
              style={{ left: `${drawing.x * 100}%`, top: `${drawing.y * 100}%`, width: `${drawing.w * 100}%`, height: `${drawing.h * 100}%` }}
            />
          )}
        </div>
      </div>

      <SidePanel title="Corrigir regiões" onClose={close}>
        <div className="flex h-full flex-col gap-3">
          <p className="text-sm text-muted">Arraste na página para desenhar uma região. A ordem da lista é a ordem de leitura.</p>
          <Button size="sm" icon={Plus} onClick={() => add({ x: 0.35, y: 0.4, w: 0.3, h: 0.15 })}>
            Adicionar região
          </Button>
          <ol className="flex-1 space-y-1.5 overflow-y-auto" aria-label="Sequência de leitura">
            {draft.map((r, n) => (
              <li
                key={r.id}
                data-testid="guided-editor-row"
                onClick={() => setSelected(r.id)}
                className={cn("flex items-center gap-1 rounded-md border px-1.5 py-1", selected === r.id ? "border-accent bg-accent-subtle" : "border-border")}
              >
                <span className="w-5 text-center font-mono text-xs text-muted">{n + 1}</span>
                <Select
                  label={`Tipo da região ${n + 1}`}
                  hideLabel
                  className="min-w-0 flex-1"
                  value={r.kind}
                  onChange={(e) => patch(r.id, { kind: e.target.value as RegionKind })}
                >
                  {REGION_KINDS.map((k) => (
                    <option key={k} value={k}>
                      {KIND_LABELS[k]}
                    </option>
                  ))}
                </Select>
                <IconButton icon={ArrowUp} label={`Ler a região ${n + 1} antes`} size="sm" disabled={n === 0} onClick={() => change(moveInList(draft, n, -1))} />
                <IconButton icon={ArrowDown} label={`Ler a região ${n + 1} depois`} size="sm" disabled={n === draft.length - 1} onClick={() => change(moveInList(draft, n, 1))} />
                <IconButton icon={Trash2} label={`Excluir a região ${n + 1}`} size="sm" onClick={() => remove(r.id)} />
              </li>
            ))}
          </ol>
          <div className="flex flex-col gap-2 border-t border-border pt-3">
            <Button variant="primary" loading={busy} disabled={busy} onClick={guard(() => onSave(draft))}>
              Confirmar sequência
            </Button>
            {canRestore && (
              <Button size="sm" variant="ghost" icon={RotateCcw} disabled={busy} onClick={guard(onRestore)}>
                Restaurar detecção automática
              </Button>
            )}
          </div>
        </div>
      </SidePanel>

      <ConfirmDialog
        open={confirmCancel}
        title="Descartar as correções?"
        confirmLabel="Descartar"
        cancelLabel="Continuar corrigindo"
        destructive
        onConfirm={onCancel}
        onCancel={() => setConfirmCancel(false)}
      >
        As mudanças feitas nesta página ainda não foram salvas.
      </ConfirmDialog>
    </div>
  );
}
