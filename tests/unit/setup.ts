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
import { clearQueryCache } from "../../src/services/query-cache";
import { resetExplore } from "../../src/features/online/explore-store";
import { clearScrollMemory } from "../../src/app/scroll-memory";
import { clearSeriesMemory } from "../../src/features/online/SeriesScreen";
import { clearLibraryFilters } from "../../src/features/library/LibraryScreen";
import { clearNotificationsScreen } from "../../src/features/notifications/NotificationsScreen";
// Os caches do app vivem enquanto ele está aberto. Cada caso começa do zero.
afterEach(() => {
  cleanup();
  clearQueryCache();
  resetExplore();
  clearScrollMemory();
  clearLibraryFilters();
  clearSeriesMemory();
  clearNotificationsScreen();
});
