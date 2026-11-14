// Identidade sonora do Prancha: poucos sons curtos, sintetizados na hora pelo Web Audio, sem arquivos nem rede.
// Todos saem da mesma "madeira" (notas pinçadas em ré maior pentatônica, com o parcial agudo que dá o timbre de
// marimba) e de uma folha de papel, para soarem como uma família. Nenhum componente toca áudio por conta própria:
// tudo passa por `sound.play`, que respeita as preferências, evita sobreposição e não toca com a janela escondida.

export type SoundName = "confirm" | "add" | "remove" | "complete" | "notify" | "chapter";

/** O que cada som quer dizer, para as configurações e a configuração inicial. */
export const SOUND_LABELS: Record<SoundName, string> = {
  confirm: "Confirmar",
  add: "Adicionar",
  remove: "Remover",
  complete: "Concluir",
  notify: "Notificação",
  chapter: "Trocar de capítulo",
};

interface PlayOptions {
  /** Toca mesmo com os sons desligados: só para o botão de ouvir um exemplo. */
  preview?: boolean;
  /** Som do próprio leitor. Os outros ficam quietos durante a leitura. */
  reader?: boolean;
  /** Volume do exemplo (0 a 1), para ouvir o novo valor antes de ele chegar às preferências. */
  volume?: number;
}

/** Repetições muito próximas viram um som só: um lote de downloads não toca dez vezes. */
const COOLDOWN_MS: Record<SoundName, number> = { confirm: 120, add: 200, remove: 200, complete: 1500, notify: 2500, chapter: 400 };
const MAX_VOICES = 3;
/** Dois sons diferentes quase juntos (concluir e passar ao próximo capítulo) viram um só: fica o primeiro. */
const GAP_MS = 200;
/** Volume máximo, já discreto: o 100% das configurações não passa disto. */
const PEAK = 0.32;

// Ré maior pentatônica.
const D4 = 293.66;
const A4 = 440;
const D5 = 587.33;
const Fs5 = 739.99;
const A5 = 880;
const B5 = 987.77;
const D6 = 1174.66;

type Ctx = AudioContext;

let enabled = false;
let volume = 0.6;
let reading = false;
let ctx: Ctx | null = null;
let unavailable = false;
let master: GainNode | null = null;
let noise: AudioBuffer | null = null;
let voices = 0;
let idleTimer: number | undefined;
const lastPlayed = new Map<SoundName, number>();
let lastAny = -Infinity;

function context(): Ctx | null {
  if (ctx || unavailable) return ctx;
  const Ctor = (globalThis as { AudioContext?: typeof AudioContext }).AudioContext;
  if (!Ctor) {
    unavailable = true;
    return null;
  }
  try {
    ctx = new Ctor({ latencyHint: "interactive" });
    // Um corte suave nos agudos deixa tudo menos estridente, em qualquer caixa de som.
    const soften = ctx.createBiquadFilter();
    soften.type = "lowpass";
    soften.frequency.value = 6000;
    master = ctx.createGain();
    master.connect(soften).connect(ctx.destination);
  } catch {
    unavailable = true;
    ctx = null;
  }
  return ctx;
}

/** Uma nota pinçada: fundamental, oitava discreta e o parcial agudo que some primeiro. */
function pluck(c: Ctx, out: AudioNode, at: number, freq: number, length: number, level: number) {
  const env = c.createGain();
  env.gain.setValueAtTime(0, at);
  env.gain.linearRampToValueAtTime(level, at + 0.004);
  env.gain.exponentialRampToValueAtTime(0.0001, at + length);
  env.connect(out);
  const partials: Array<[ratio: number, gain: number, decay: number]> = [
    [1, 1, 1],
    [2, 0.18, 0.6],
    [4, 0.1, 0.25],
  ];
  for (const [ratio, gain, decay] of partials) {
    const osc = c.createOscillator();
    osc.type = "sine";
    osc.frequency.value = freq * ratio;
    const g = c.createGain();
    g.gain.setValueAtTime(gain, at);
    g.gain.exponentialRampToValueAtTime(0.0001, at + length * decay);
    osc.connect(g).connect(env);
    osc.start(at);
    osc.stop(at + length + 0.02);
  }
}

/** Uma folha virando: ruído que passa por um filtro que sobe. */
function paper(c: Ctx, out: AudioNode, at: number, length: number, level: number) {
  if (!noise) {
    noise = c.createBuffer(1, Math.ceil(c.sampleRate * 0.5), c.sampleRate);
    const data = noise.getChannelData(0);
    for (let i = 0; i < data.length; i++) data[i] = Math.random() * 2 - 1;
  }
  const src = c.createBufferSource();
  src.buffer = noise;
  const band = c.createBiquadFilter();
  band.type = "bandpass";
  band.Q.value = 0.9;
  band.frequency.setValueAtTime(900, at);
  band.frequency.exponentialRampToValueAtTime(2800, at + length);
  const env = c.createGain();
  env.gain.setValueAtTime(0, at);
  env.gain.linearRampToValueAtTime(level, at + length * 0.35);
  env.gain.exponentialRampToValueAtTime(0.0001, at + length);
  src.connect(band).connect(env).connect(out);
  src.start(at);
  src.stop(at + length + 0.02);
}

/** Desenha o som e devolve quanto ele dura, em segundos. */
function render(c: Ctx, out: AudioNode, name: SoundName, t: number): number {
  switch (name) {
    case "confirm":
      pluck(c, out, t, A5, 0.16, 0.5);
      return 0.18;
    case "add":
      pluck(c, out, t, D5, 0.18, 0.55);
      pluck(c, out, t + 0.07, A5, 0.32, 0.6);
      return 0.4;
    case "remove":
      pluck(c, out, t, A4, 0.16, 0.45);
      pluck(c, out, t + 0.07, D4, 0.3, 0.4);
      return 0.38;
    case "complete":
      pluck(c, out, t, D5, 0.2, 0.5);
      pluck(c, out, t + 0.08, Fs5, 0.22, 0.5);
      pluck(c, out, t + 0.16, A5, 0.6, 0.55);
      pluck(c, out, t + 0.16, D6, 0.5, 0.12);
      return 0.8;
    case "notify":
      pluck(c, out, t, B5, 0.3, 0.45);
      pluck(c, out, t + 0.11, Fs5, 0.45, 0.4);
      return 0.58;
    case "chapter":
      paper(c, out, t, 0.24, 0.35);
      pluck(c, out, t + 0.12, D5, 0.22, 0.18);
      return 0.36;
  }
}

function start(c: Ctx, name: SoundName, level: number) {
  if (!master || voices >= MAX_VOICES) return;
  const voice = c.createGain();
  // Volume com curva perceptiva: metade do controle soa como metade.
  voice.gain.value = PEAK * level * level;
  voice.connect(master);
  const length = render(c, voice, name, c.currentTime + 0.01);
  voices++;
  window.clearTimeout(idleTimer);
  window.setTimeout(
    () => {
      voices--;
      voice.disconnect();
      // Em silêncio, o contexto de áudio dorme e para de ocupar a thread de som.
      if (voices === 0) idleTimer = window.setTimeout(() => void c.suspend().catch(() => undefined), 4000);
    },
    (length + 0.1) * 1000,
  );
}

export const sound = {
  configure(next: { enabled: boolean; volume: number }) {
    enabled = next.enabled;
    volume = Math.min(1, Math.max(0, next.volume));
  },

  /** Durante a leitura, só o leitor faz som: um download que termina não interrompe a página. */
  setReading(on: boolean) {
    reading = on;
  },

  play(name: SoundName, options: PlayOptions = {}) {
    if (!options.preview) {
      if (!enabled || volume === 0) return;
      if (reading && !options.reader) return;
      if (typeof document !== "undefined" && document.hidden) return;
      const now = Date.now();
      if (now - (lastPlayed.get(name) ?? -Infinity) < COOLDOWN_MS[name]) return;
      if (now - lastAny < GAP_MS) return;
      lastPlayed.set(name, now);
      lastAny = now;
    }
    const level = Math.min(1, Math.max(0, options.volume ?? volume));
    if (level === 0) return;
    const c = context();
    if (!c) return;
    if (c.state === "running") {
      start(c, name, level);
      return;
    }
    // O navegador só libera áudio depois de um gesto da pessoa. Sem ele, o som é descartado, nunca guardado para depois.
    c.resume()
      .then(() => {
        if (c.state === "running") start(c, name, level);
      })
      .catch(() => undefined);
  },
};

/** Só para os testes: volta ao estado de quando o app abre. */
export function resetSound() {
  enabled = false;
  volume = 0.6;
  reading = false;
  voices = 0;
  lastPlayed.clear();
  lastAny = -Infinity;
  window.clearTimeout(idleTimer);
  ctx = null;
  master = null;
  noise = null;
  unavailable = false;
}
