import { useEffect, useRef, useState, type ReactNode } from "react";
import { EyeOff, Keyboard, RotateCcw, X } from "lucide-react";
import { Button, IconButton } from "../../components/Button";
import { ConfirmDialog, Dialog } from "../../components/Dialog";
import { Kbd } from "../../components/Feedback";
import { Segmented, Select, Switch } from "../../components/Form";
import { cn } from "../../components/cn";
import { FIT_LABELS, MODE_LABELS } from "../../domain/reader";
import type { ControlsBehavior, FitMode, ReaderMode, ReadingDirection, WheelBehavior } from "../../domain/settings";
import { SHORTCUT_ACTIONS, SHORTCUT_LABELS, formatKey, type ShortcutMap } from "../../domain/shortcuts";
import { useSettings } from "../../app/settings-context";
import type { PageSrc } from "../../services/urls";

/** Painel lateral do leitor (não cobre a página inteira). */
export function SidePanel({ title, onClose, children, side = "right" }: { title: string; onClose: () => void; children: ReactNode; side?: "left" | "right" }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("button, input, select")?.focus();
  }, []);
  return (
    <aside
      ref={ref}
      aria-label={title}
      className={cn(
        "p-enter absolute top-0 bottom-0 z-30 flex w-[min(340px,90vw)] flex-col border-border bg-elevated shadow-overlay",
        side === "right" ? "right-0 border-l" : "left-0 border-r",
      )}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="flex items-center justify-between border-b border-border px-4 py-2.5">
        <h2 className="font-semibold">{title}</h2>
        <IconButton icon={X} label={`Fechar ${title.toLowerCase()}`} size="sm" onClick={onClose} />
      </div>
      <div className="flex-1 overflow-y-auto px-4 py-4">{children}</div>
    </aside>
  );
}

export function ReaderSettingsPanel({
  mode,
  onMode,
  onClose,
  onResetProgress,
  onShowShortcuts,
  onHideControls,
  hideKey,
}: {
  mode: ReaderMode;
  onMode: (mode: ReaderMode) => void;
  onClose: () => void;
  onResetProgress: () => void;
  onShowShortcuts: () => void;
  onHideControls: () => void;
  hideKey?: string;
}) {
  const { settings, update } = useSettings();
  const [confirm, setConfirm] = useState(false);
  return (
    <SidePanel title="Configurações de leitura" onClose={onClose}>
      <div className="space-y-5">
        {/* Opções longas ficam num menu: só a escolha atual aparece. */}
        <Select label="Modo de página" value={mode} onChange={(e) => onMode(e.target.value as ReaderMode)}>
          {(["single", "double", "vertical", "webtoon"] as ReaderMode[]).map((m) => (
            <option key={m} value={m}>
              {MODE_LABELS[m]}
            </option>
          ))}
        </Select>
        <Select
          label="Direção de leitura"
          value={settings["reader.direction"]}
          onChange={(e) => void update("reader.direction", e.target.value as ReadingDirection)}
        >
          <option value="ltr">Esquerda → direita</option>
          <option value="rtl">Direita → esquerda (mangá)</option>
        </Select>
        {mode !== "webtoon" && (
          <Segmented<FitMode>
            label="Ajuste da imagem"
            value={settings["reader.fit"]}
            onChange={(v) => void update("reader.fit", v)}
            size="sm"
            options={(["page", "width", "height"] as FitMode[]).map((f) => ({ value: f, label: FIT_LABELS[f].replace("Ajustar à ", "") }))}
          />
        )}
        {mode === "double" && (
          <Switch
            label="Capa sozinha"
            description="Na página dupla, mostra a primeira página sozinha para alinhar os pares."
            checked={settings["reader.coverAlone"]}
            onChange={(v) => void update("reader.coverAlone", v)}
          />
        )}
        <Segmented<WheelBehavior>
          label="Roda do mouse"
          value={settings["reader.wheel"]}
          onChange={(v) => void update("reader.wheel", v)}
          size="sm"
          options={[
            { value: "page", label: "Vira a página" },
            { value: "scroll", label: "Só rola" },
          ]}
        />
        <Segmented<ControlsBehavior>
          label="Controles do leitor"
          value={settings["reader.controls"]}
          onChange={(v) => void update("reader.controls", v)}
          size="sm"
          options={[
            { value: "autohide", label: "Ocultar sozinhos" },
            { value: "always", label: "Sempre visíveis" },
          ]}
        />
        <div className="flex flex-wrap gap-2 border-t border-border pt-4">
          <Button size="sm" icon={Keyboard} onClick={onShowShortcuts}>
            Atalhos de teclado
          </Button>
          <Button size="sm" icon={EyeOff} onClick={onHideControls} title={hideKey ? `Atalho: ${hideKey}` : undefined}>
            Ocultar controles
          </Button>
          <Button size="sm" icon={RotateCcw} onClick={() => setConfirm(true)}>
            Reiniciar progresso
          </Button>
        </div>
      </div>
      <ConfirmDialog
        open={confirm}
        title="Reiniciar o progresso desta HQ?"
        confirmLabel="Reiniciar progresso"
        onCancel={() => setConfirm(false)}
        onConfirm={() => {
          setConfirm(false);
          onResetProgress();
        }}
      >
        A leitura volta para a página 1 e a posição salva é substituída.
      </ConfirmDialog>
    </SidePanel>
  );
}

export function PageIndexPanel({
  pageSrc,
  count,
  current,
  isPdf,
  onSelect,
  onClose,
}: {
  pageSrc: PageSrc;
  count: number;
  current: number[];
  isPdf: boolean;
  onSelect: (page: number) => void;
  onClose: () => void;
}) {
  const [go, setGo] = useState("");
  const list = useRef<HTMLDivElement>(null);
  // Centraliza a página atual só ao abrir; depois a rolagem é do usuário.
  const [initialPage] = useState(() => current[0]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${initialPage}"]`)?.scrollIntoView({ block: "center" });
  }, [initialPage]);
  const n = Number(go);
  const valid = Number.isInteger(n) && n >= 1 && n <= count;
  const invalid = go !== "" && !valid;
  const submit = () => {
    if (valid) {
      onSelect(n - 1);
      setGo("");
    }
  };
  return (
    <SidePanel title="Índice de páginas" onClose={onClose} side="left">
      <form
        className="mb-4 flex items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <div className="flex-1">
          <label htmlFor="goto-page" className="mb-1 block text-sm font-medium">
            Ir para a página (1–{count})
          </label>
          <input
            id="goto-page"
            inputMode="numeric"
            value={go}
            onChange={(e) => setGo(e.target.value.replace(/\D/g, ""))}
            aria-invalid={invalid || undefined}
            className="h-8 w-full rounded-md border border-border-control bg-bg px-2.5 focus-visible:outline-2 focus-visible:outline-offset-0 focus-visible:outline-accent"
          />
        </div>
        <Button type="submit" disabled={!go || invalid}>
          Ir
        </Button>
      </form>
      <div ref={list} className="grid grid-cols-3 gap-2">
        {Array.from({ length: count }, (_, i) => {
          const active = current.includes(i);
          return (
            <button
              key={i}
              type="button"
              data-index={i}
              onClick={() => onSelect(i)}
              aria-current={active ? "page" : undefined}
              aria-label={`Ir para a página ${i + 1}`}
              className={cn(
                "flex flex-col items-center gap-1 rounded-md border p-1 text-xs",
                active ? "border-accent bg-accent-subtle text-accent-fg" : "border-border hover:bg-hover",
              )}
            >
              <span className="flex aspect-[2/3] w-full items-center justify-center overflow-hidden rounded-sm bg-surface">
                {isPdf ? (
                  <span className="text-lg font-semibold text-muted">{i + 1}</span>
                ) : (
                  <img src={pageSrc(i)} alt="" loading="lazy" decoding="async" className="h-full w-full object-contain" />
                )}
              </span>
              <span>{i + 1}</span>
            </button>
          );
        })}
      </div>
    </SidePanel>
  );
}

export function ShortcutsDialog({ open, onClose, shortcuts }: { open: boolean; onClose: () => void; shortcuts: ShortcutMap }) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Atalhos do leitor"
      description="Os atalhos funcionam apenas com o leitor aberto. Você pode alterá-los em Configurações › Leitura."
      footer={<Button onClick={onClose}>Fechar</Button>}
    >
      <table className="w-full text-left">
        <thead className="sr-only">
          <tr>
            <th>Ação</th>
            <th>Teclas</th>
          </tr>
        </thead>
        <tbody>
          {SHORTCUT_ACTIONS.map((a) => (
            <tr key={a} className="border-b border-border last:border-b-0">
              <td className="py-1.5 pr-4">{SHORTCUT_LABELS[a]}</td>
              <td className="py-1.5">
                <span className="flex flex-wrap justify-end gap-1">
                  {shortcuts[a].length ? shortcuts[a].map((k) => <Kbd key={k}>{formatKey(k)}</Kbd>) : <span className="text-sm text-muted">sem atalho</span>}
                </span>
              </td>
            </tr>
          ))}
          <tr className="border-b border-border">
            <td className="py-1.5 pr-4">Zoom com o mouse</td>
            <td className="py-1.5 text-right">
              <Kbd>Ctrl</Kbd> + roda do mouse
            </td>
          </tr>
          <tr>
            <td className="py-1.5 pr-4">Fechar painéis ou sair da tela cheia</td>
            <td className="py-1.5 text-right">
              <Kbd>Esc</Kbd>
            </td>
          </tr>
        </tbody>
      </table>
      <p className="mt-3 text-sm text-muted">Na leitura da direita para a esquerda, as setas ← e → seguem a direção da página.</p>
    </Dialog>
  );
}
