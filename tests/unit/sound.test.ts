// Serviço central de sons: quando toca e quando fica quieto. O som em si (Web Audio) é simulado.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { sound } from "../../src/services/sound";

class Param {
  value = 0;
  setValueAtTime() {
    return this;
  }
  linearRampToValueAtTime() {
    return this;
  }
  exponentialRampToValueAtTime() {
    return this;
  }
}
class Node {
  gain = new Param();
  frequency = new Param();
  Q = new Param();
  type = "";
  buffer: unknown = null;
  connect<T>(next: T) {
    return next;
  }
  disconnect() {}
  start() {}
  stop() {}
}
class FakeContext {
  static instances: FakeContext[] = [];
  state = "running";
  currentTime = 0;
  sampleRate = 8000;
  destination = new Node();
  notes = 0;
  constructor() {
    FakeContext.instances.push(this);
  }
  createGain() {
    return new Node();
  }
  createBiquadFilter() {
    return new Node();
  }
  createOscillator() {
    this.notes++;
    return new Node();
  }
  createBufferSource() {
    this.notes++;
    return new Node();
  }
  createBuffer(_channels: number, length: number) {
    return { getChannelData: () => new Float32Array(length) };
  }
  resume() {
    this.state = "running";
    return Promise.resolve();
  }
  suspend() {
    return Promise.resolve();
  }
}

let now = 1_000_000;
const played = () => FakeContext.instances.reduce((n, c) => n + c.notes, 0);

beforeEach(() => {
  FakeContext.instances = [];
  vi.stubGlobal("AudioContext", FakeContext);
  vi.spyOn(Date, "now").mockImplementation(() => now);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("sons da interface", () => {
  it("desligados não tocam nem criam o contexto de áudio", () => {
    sound.configure({ enabled: false, volume: 0.6 });
    sound.play("add");
    expect(FakeContext.instances).toHaveLength(0);
  });

  it("ligados tocam, e a mesma ação repetida logo em seguida vira um som só", () => {
    sound.configure({ enabled: true, volume: 0.6 });
    sound.play("add");
    const once = played();
    expect(once).toBeGreaterThan(0);
    now += 50;
    sound.play("add");
    expect(played()).toBe(once);
    now += 1000;
    sound.play("add");
    expect(played()).toBeGreaterThan(once);
  });

  it("dois sons diferentes quase juntos tocam só o primeiro", () => {
    sound.configure({ enabled: true, volume: 0.6 });
    sound.play("complete", { reader: true });
    const once = played();
    now += 20;
    sound.play("chapter", { reader: true });
    expect(played()).toBe(once);
  });

  it("durante a leitura, só os sons do leitor", () => {
    sound.configure({ enabled: true, volume: 0.6 });
    sound.setReading(true);
    sound.play("notify");
    expect(played()).toBe(0);
    sound.play("chapter", { reader: true });
    expect(played()).toBeGreaterThan(0);
  });

  it("volume zero e janela escondida ficam em silêncio", () => {
    sound.configure({ enabled: true, volume: 0 });
    sound.play("add");
    expect(played()).toBe(0);
    sound.configure({ enabled: true, volume: 0.6 });
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    sound.play("add");
    expect(played()).toBe(0);
    hidden.mockRestore();
  });

  it("o exemplo das configurações toca mesmo com os sons desligados", () => {
    sound.configure({ enabled: false, volume: 0.6 });
    sound.play("notify", { preview: true, volume: 0.4 });
    expect(played()).toBeGreaterThan(0);
  });

  it("sem Web Audio, nada quebra", () => {
    vi.stubGlobal("AudioContext", undefined);
    sound.configure({ enabled: true, volume: 0.6 });
    expect(() => sound.play("add")).not.toThrow();
  });
});
