import { afterEach } from "vitest";
import { clearQueryCache } from "../src/services/query-cache";
import { resetTelemetryClient } from "../src/services/telemetry";
import { resetSound } from "../src/services/sound";

// Os caches dos serviços vivem enquanto o app está aberto. Cada caso começa do zero.
afterEach(() => {
  clearQueryCache();
  resetTelemetryClient();
  resetSound();
});
