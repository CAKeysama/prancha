import "@testing-library/jest-dom/vitest";

// O jsdom não implementa estas APIs usadas pela interface.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;

if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: query.includes("dark"),
      media: query,
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => false,
    }) as unknown as MediaQueryList;
}

// Dimensões fixas para que a grade virtualizada tenha onde desenhar.
Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 1024 });
Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 768 });

window.scrollTo ??= () => undefined;
Element.prototype.scrollTo ??= () => undefined;
Element.prototype.scrollIntoView ??= () => undefined;

import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
afterEach(() => cleanup());
