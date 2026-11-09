// O menu do botão direito sai do app inteiro, menos dos campos de texto. Seleção, digitação e atalhos continuam.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../../src/app/App";
import { disableContextMenu, isEditableTarget } from "../../src/app/context-menu";
import type { ComicSummary } from "../../src/domain/types";

const comic: ComicSummary = {
  id: 7,
  title: "Teste em Quadrinhos",
  fileName: "teste.cbz",
  format: "cbz",
  series: null,
  issueNumber: null,
  volume: null,
  writer: null,
  pageCount: 12,
  coverKind: "first_page",
  coverRev: 1,
  coverError: null,
  favorite: false,
  status: "reading",
  missing: false,
  addedAt: 1,
  lastOpenedAt: 2,
  progress: { pageIndex: 3, pageCount: 12, scrollRatio: 0, updatedAt: 3 },
};

/** Dispara o clique com o botão direito como o WebView dispara e diz se o menu teria aparecido. */
function rightClick(element: Element): boolean {
  const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2 });
  element.dispatchEvent(event);
  return !event.defaultPrevented;
}

function mount<T extends HTMLElement>(html: string): T {
  const host = document.createElement("div");
  host.innerHTML = html;
  document.body.appendChild(host);
  return host.firstElementChild as T;
}

afterEach(() => {
  cleanup();
  document.body.innerHTML = "";
});

describe("campos de texto", () => {
  it("reconhece onde a pessoa digita", () => {
    expect(isEditableTarget(mount("<input />"))).toBe(true);
    expect(isEditableTarget(mount('<input type="search" />'))).toBe(true);
    expect(isEditableTarget(mount('<input type="password" />'))).toBe(true);
    expect(isEditableTarget(mount("<textarea></textarea>"))).toBe(true);
    expect(isEditableTarget(mount('<div contenteditable="true"><b>texto</b></div>').querySelector("b"))).toBe(true);
  });

  it("botões, caixas de marcar e campos desligados não são edição", () => {
    expect(isEditableTarget(mount('<input type="checkbox" />'))).toBe(false);
    expect(isEditableTarget(mount('<input type="range" />'))).toBe(false);
    expect(isEditableTarget(mount("<input disabled />"))).toBe(false);
    expect(isEditableTarget(mount('<div contenteditable="false">x</div>'))).toBe(false);
    expect(isEditableTarget(mount("<button>Abrir</button>"))).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });
});

describe("bloqueio na janela", () => {
  let off: () => void;
  beforeEach(() => {
    off = disableContextMenu();
  });
  afterEach(() => off());

  it("vale para qualquer elemento, sem depender do componente", () => {
    // Capa, imagem do leitor, painel, item de lista, link, botão e área vazia.
    for (const html of ['<img alt="capa" />', "<canvas></canvas>", "<nav><a href='#'>Início</a></nav>", "<ul><li>Obra</li></ul>", "<button>Ler</button>", "<div></div>", "<p>texto</p>"]) {
      expect(rightClick(mount(html)), html).toBe(false);
    }
  });

  it("vale mesmo quando um componente para a propagação", () => {
    const panel = mount("<section><span>painel</span></section>");
    panel.addEventListener("contextmenu", (e) => e.stopPropagation());
    expect(rightClick(panel.querySelector("span")!)).toBe(false);
  });

  it("deixa o menu de edição nos campos de texto", () => {
    expect(rightClick(mount('<input type="search" value="batman" />'))).toBe(true);
    expect(rightClick(mount("<textarea>nota</textarea>"))).toBe(true);
  });

  it("não mexe na seleção, nos cliques nem nos atalhos", () => {
    const text = mount("<p>Um texto para selecionar</p>");
    for (const type of ["mousedown", "mouseup", "click", "selectstart", "dblclick"]) {
      const event = new MouseEvent(type, { bubbles: true, cancelable: true });
      text.dispatchEvent(event);
      expect(event.defaultPrevented, type).toBe(false);
    }
    const key = new KeyboardEvent("keydown", { key: "c", ctrlKey: true, bubbles: true, cancelable: true });
    text.dispatchEvent(key);
    expect(key.defaultPrevented).toBe(false);
    const range = document.createRange();
    range.selectNodeContents(text);
    window.getSelection()!.removeAllRanges();
    window.getSelection()!.addRange(range);
    rightClick(text);
    expect(window.getSelection()!.toString()).toBe("Um texto para selecionar");
  });

  it("desligar devolve o comportamento original", () => {
    off();
    expect(rightClick(mount("<div></div>"))).toBe(true);
    off = disableContextMenu();
  });
});

describe("no aplicativo", () => {
  beforeEach(() => {
    mockWindows("main");
    mockIPC((cmd) => {
      switch (cmd) {
        case "get_settings":
          return { onboarding: { status: "completed", step: 4, comicId: 1 } };
        case "get_home":
          return { total: 1, continueReading: [comic], recentlyAdded: [comic], favorites: [], recentlyOpened: [comic] };
        case "list_comics":
          return [comic];
        case "pending_pdf_covers":
        case "list_collections":
        case "list_tags":
          return [];
        default:
          return null;
      }
    }, { shouldMockEvents: true });
  });
  // Desmonta antes de tirar o simulador, para os ouvintes do Tauri saírem direito.
  afterEach(() => {
    cleanup();
    clearMocks();
  });

  it("some dos cards, das capas e da navegação, e fica no campo de pesquisa", async () => {
    const user = userEvent.setup();
    render(<App />);
    const cards = await screen.findAllByText("Teste em Quadrinhos");
    expect(rightClick(cards[0]!)).toBe(false);
    for (const cover of document.querySelectorAll("img")) expect(rightClick(cover)).toBe(false);
    expect(rightClick(screen.getByRole("button", { name: "Configurações" }))).toBe(false);
    expect(rightClick(document.body)).toBe(false);

    await user.click(screen.getAllByRole("button", { name: "Minha biblioteca" })[0]!);
    const search = await screen.findByRole("searchbox");
    await user.type(search, "abc");
    expect(search).toHaveValue("abc");
    expect(rightClick(search)).toBe(true);
    // Atalho de teclado dentro do campo continua com o navegador.
    fireEvent.keyDown(search, { key: "a", ctrlKey: true });
    expect(search).toHaveValue("abc");
  });

  it("volta ao normal quando o app sai da tela", () => {
    const { unmount } = render(<App />);
    unmount();
    expect(rightClick(document.body)).toBe(true);
  });
});
