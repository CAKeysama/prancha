// Atalhos de teclado do leitor. Só funcionam com o leitor em foco (nada é global no sistema).

export type ShortcutAction =
  | "next"
  | "prev"
  | "first"
  | "last"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset"
  | "fitPage"
  | "fitWidth"
  | "fitHeight"
  | "cycleMode"
  | "toggleDirection"
  | "fullscreen"
  | "toggleControls"
  | "pageIndex"
  | "help"
  | "back";

export type ShortcutMap = Record<ShortcutAction, string[]>;

export const SHORTCUT_LABELS: Record<ShortcutAction, string> = {
  next: "Próxima página",
  prev: "Página anterior",
  first: "Primeira página",
  last: "Última página",
  zoomIn: "Aumentar zoom",
  zoomOut: "Diminuir zoom",
  zoomReset: "Zoom 100%",
  fitPage: "Ajustar à página",
  fitWidth: "Ajustar à largura",
  fitHeight: "Ajustar à altura",
  cycleMode: "Alternar modo de exibição",
  toggleDirection: "Alternar direção de leitura",
  fullscreen: "Entrar ou sair da tela cheia",
  toggleControls: "Mostrar ou ocultar controles",
  pageIndex: "Abrir índice de páginas",
  help: "Mostrar atalhos",
  back: "Voltar à biblioteca",
};

/**
 * "ArrowRight" e "ArrowLeft" são relativos à tela: com leitura da direita para a esquerda,
 * o leitor inverte o significado das setas horizontais (ver `resolveArrow`).
 */
export const DEFAULT_SHORTCUTS: ShortcutMap = {
  next: ["ArrowRight", "PageDown", "Space"],
  prev: ["ArrowLeft", "PageUp", "Shift+Space"],
  first: ["Home"],
  last: ["End"],
  zoomIn: ["+", "Ctrl+="],
  zoomOut: ["-", "Ctrl+-"],
  zoomReset: ["0", "Ctrl+0"],
  fitPage: ["P"],
  fitWidth: ["W"],
  fitHeight: ["E"],
  cycleMode: ["M"],
  toggleDirection: ["D"],
  fullscreen: ["F", "F11"],
  toggleControls: ["H"],
  pageIndex: ["I"],
  help: ["?"],
  back: ["Backspace"],
};

export const SHORTCUT_ACTIONS = Object.keys(DEFAULT_SHORTCUTS) as ShortcutAction[];

const KEY_NAMES: Record<string, string> = {
  " ": "Space",
  Spacebar: "Space",
  Esc: "Escape",
};

/** Normaliza um evento de teclado para o formato "Ctrl+Shift+K". */
export function keyFromEvent(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">): string | null {
  let key = KEY_NAMES[e.key] ?? e.key;
  if (["Control", "Shift", "Alt", "Meta", "Dead", "Unidentified"].includes(key)) return null;
  if (key.length === 1) key = key.toUpperCase();
  const mods: string[] = [];
  if (e.ctrlKey || e.metaKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  // Shift só conta para teclas não imprimíveis ou Space (em "+" e "?" ele já faz parte do caractere).
  if (e.shiftKey && (key.length > 1 || key === " ")) mods.push("Shift");
  return [...mods, key].join("+");
}

export function formatKey(key: string): string {
  return key
    .replace("ArrowRight", "→")
    .replace("ArrowLeft", "←")
    .replace("ArrowUp", "↑")
    .replace("ArrowDown", "↓")
    .replace("Space", "Espaço")
    .replace("PageDown", "Page Down")
    .replace("PageUp", "Page Up")
    .replace("Backspace", "Backspace")
    .replace("Escape", "Esc");
}

export function actionForKey(map: ShortcutMap, key: string): ShortcutAction | null {
  for (const action of SHORTCUT_ACTIONS) {
    if (map[action].includes(key)) return action;
  }
  return null;
}

/** Em leitura RTL, a seta para a esquerda avança e a da direita volta. */
export function resolveArrow(action: ShortcutAction | null, key: string, rtl: boolean): ShortcutAction | null {
  if (!rtl || (action !== "next" && action !== "prev")) return action;
  if (key === "ArrowLeft") return action === "prev" ? "next" : "prev";
  if (key === "ArrowRight") return action === "next" ? "prev" : "next";
  return action;
}

/** Devolve a ação que já usa a tecla (para avisar conflitos ao reconfigurar). */
export function conflictFor(map: ShortcutMap, key: string, except: ShortcutAction): ShortcutAction | null {
  for (const action of SHORTCUT_ACTIONS) {
    if (action !== except && map[action].includes(key)) return action;
  }
  return null;
}

export function sanitizeShortcuts(value: unknown): ShortcutMap {
  const out: ShortcutMap = { ...DEFAULT_SHORTCUTS };
  if (!value || typeof value !== "object") return out;
  const v = value as Record<string, unknown>;
  for (const action of SHORTCUT_ACTIONS) {
    const keys = v[action];
    if (Array.isArray(keys) && keys.every((k) => typeof k === "string") && keys.length <= 4) {
      out[action] = keys as string[];
    }
  }
  return out;
}
