// O menu do botão direito do WebView (Voltar, Atualizar, Salvar imagem, Imprimir, Inspecionar…) não tem nada a ver
// com o Prancha e leva a ações que quebram o app, como recarregar no meio de uma leitura. Ele sai em todas as telas
// de uma vez, por um único ouvinte no documento, e não componente a componente.
//
// Fica só nos campos de texto, onde o menu é parte da edição (colar, desfazer, ortografia). Nada mais muda: a seleção
// de texto, o Ctrl+C, os atalhos, os cliques e o foco continuam como estavam, porque só o evento `contextmenu` é
// cancelado, e sem interromper a propagação.

const TEXT_INPUT_TYPES = new Set(["", "text", "search", "email", "url", "tel", "password", "number"]);

/** Diz se o clique com o botão direito caiu num lugar onde a pessoa digita. */
export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  const field = target.closest("input, textarea, [contenteditable]");
  if (!field) return false;
  if (field instanceof HTMLInputElement) return TEXT_INPUT_TYPES.has(field.type.toLowerCase()) && !field.disabled;
  if (field instanceof HTMLTextAreaElement) return !field.disabled;
  const editable = field.getAttribute("contenteditable");
  return editable !== null && editable.toLowerCase() !== "false";
}

function blockContextMenu(event: MouseEvent) {
  if (!isEditableTarget(event.target)) event.preventDefault();
}

/** Liga o bloqueio na janela inteira. Devolve a função que desliga. */
export function disableContextMenu(target: Window = window): () => void {
  // Na captura, para valer mesmo que algum componente pare a propagação.
  target.addEventListener("contextmenu", blockContextMenu, { capture: true });
  return () => target.removeEventListener("contextmenu", blockContextMenu, { capture: true });
}
