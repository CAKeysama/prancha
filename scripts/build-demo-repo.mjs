// Publica o repositório de demonstração: compila a extensão, gera o acervo (capas e páginas originais) e assina tudo.
// Uso: node scripts/build-demo-repo.mjs [--out pasta] [--base-url endereço] [--key arquivo]
//
// A chave privada nunca fica no repositório. O padrão é ~/.tauri/prancha-demo-repo.key, criada na primeira vez.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { crc32, deflateSync } from 'node:zlib';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const MANIFEST = join(ROOT, 'extensions', 'Cargo.toml');
const REPOSITORY_ID = 'prancha.demonstracao';
const EXTENSION_ID = 'prancha.demonstracao.acervo';

function options() {
  const defaults = {
    out: join(ROOT, 'extensions', 'demo-repo'),
    'base-url': 'https://raw.githubusercontent.com/CAKeysama/prancha/main/extensions/demo-repo',
    key: join(homedir(), '.tauri', 'prancha-demo-repo.key'),
  };
  const args = process.argv.slice(2);
  for (let i = 0; i < args.length; i += 2) {
    const name = args[i].replace(/^--/, '');
    if (!(name in defaults) || args[i + 1] === undefined) throw new Error(`Opção inválida: ${args[i]}`);
    defaults[name] = args[i + 1];
  }
  return { out: resolve(defaults.out), baseUrl: defaults['base-url'].replace(/\/+$/, ''), key: resolve(defaults.key) };
}

// ---------------------------------------------------------------------------
// Imagens: PNG com paleta, desenhado aqui. Nada vem de terceiros.
// ---------------------------------------------------------------------------

const PALETTES = [
  ['#f6f1e5', '#1f2a44', '#e4572e', '#f3a712', '#76b0a4', '#29335c'],
  ['#eef5f7', '#14324a', '#1b7f8c', '#f2c14e', '#7fb7be', '#0b4f6c'],
  ['#f5f2ea', '#3a3226', '#7a9e7e', '#c8553d', '#d9b44a', '#588157'],
  ['#f4eef2', '#2e2a3a', '#8e6c9e', '#e8a1a1', '#6a8eae', '#4b3f72'],
  ['#0f1630', '#f1f1e6', '#f7c548', '#5c7cfa', '#30446b', '#e8e3f4'],
  ['#f8f4f0', '#402a23', '#d97b5a', '#8fb996', '#e3c16f', '#6d4c41'],
];

const SEGMENTS = ['abcdef', 'bc', 'abged', 'abgcd', 'fgbc', 'afgcd', 'afgedc', 'abc', 'abcdefg', 'abcdfg'];

function random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function hash(text) {
  let value = 2166136261;
  for (const char of text) value = Math.imul(value ^ char.codePointAt(0), 16777619);
  return value >>> 0;
}

class Canvas {
  constructor(width, height, palette) {
    Object.assign(this, { width, height, palette, pixels: new Uint8Array(width * height) });
  }

  rect(x, y, w, h, color) {
    for (let row = Math.max(0, y); row < Math.min(this.height, y + h); row++) {
      this.pixels.fill(color, row * this.width + Math.max(0, x), row * this.width + Math.min(this.width, x + w));
    }
  }

  circle(cx, cy, r, color, clip) {
    for (let y = Math.max(clip.y, cy - r); y < Math.min(clip.y + clip.h, cy + r); y++) {
      const half = Math.floor(Math.sqrt(r * r - (y - cy) * (y - cy)));
      const from = Math.max(clip.x, cx - half);
      const to = Math.min(clip.x + clip.w, cx + half);
      if (to > from) this.pixels.fill(color, y * this.width + from, y * this.width + to);
    }
  }

  digits(text, x, y, size, color) {
    const thick = Math.max(2, Math.round(size / 5));
    let left = x;
    for (const char of text) {
      const on = SEGMENTS[Number(char)] ?? '';
      const place = { a: [0, 0, size, thick], b: [size - thick, 0, thick, size], c: [size - thick, size, thick, size], d: [0, size * 2 - thick, size, thick], e: [0, size, thick, size], f: [0, 0, thick, size], g: [0, size - thick / 2, size, thick] };
      for (const segment of on) {
        const [dx, dy, w, h] = place[segment];
        this.rect(left + dx, Math.round(y + dy), w, h, color);
      }
      left += size + thick * 2;
    }
  }

  png() {
    const chunk = (type, data) => {
      const head = Buffer.alloc(8);
      head.writeUInt32BE(data.length, 0);
      head.write(type, 4, 'latin1');
      const crc = Buffer.alloc(4);
      crc.writeUInt32BE(crc32(Buffer.concat([head.subarray(4), data])), 0);
      return Buffer.concat([head, data, crc]);
    };
    const header = Buffer.alloc(13);
    header.writeUInt32BE(this.width, 0);
    header.writeUInt32BE(this.height, 4);
    header.set([8, 3, 0, 0, 0], 8);
    const palette = Buffer.from(this.palette.flatMap((color) => [1, 3, 5].map((at) => parseInt(color.slice(at, at + 2), 16))));
    const rows = Buffer.alloc((this.width + 1) * this.height);
    for (let y = 0; y < this.height; y++) rows.set(this.pixels.subarray(y * this.width, (y + 1) * this.width), y * (this.width + 1) + 1);
    return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', header), chunk('PLTE', palette), chunk('IDAT', deflateSync(rows, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
  }
}

function scene(canvas, panel, next) {
  const { x, y, w, h } = panel;
  canvas.rect(x, y, w, h, 2 + Math.floor(next() * 4));
  canvas.circle(x + Math.floor(w * (0.2 + next() * 0.6)), y + Math.floor(h * (0.15 + next() * 0.25)), Math.floor(Math.min(w, h) * (0.1 + next() * 0.1)), 3, panel);
  const ground = y + Math.floor(h * (0.62 + next() * 0.15));
  const hill = 2 + Math.floor(next() * 4);
  canvas.circle(x + Math.floor(w * next()), ground + h, h + Math.floor(h * 0.15), hill, panel);
  canvas.rect(x, ground, w, y + h - ground, 5);
  const people = 1 + Math.floor(next() * 2);
  for (let i = 0; i < people; i++) {
    const cx = x + Math.floor(w * (i + 0.3 + next() * 0.4) / people);
    const size = Math.floor(h * 0.09);
    canvas.circle(cx, ground - size * 3, size, 1, panel);
    canvas.rect(cx - size, ground - size * 2, size * 2, size * 2, 1);
  }
}

function page(work, chapter, number, palette) {
  const canvas = new Canvas(720, 1040, palette);
  const next = random(hash(`${work.id}/${chapter.id}/${number}`));
  const margin = 28;
  const split = 400 + Math.floor(next() * 120);
  const column = 240 + Math.floor(next() * 240);
  const panels = [
    { x: margin, y: margin, w: 720 - margin * 2, h: split - margin },
    { x: margin, y: split + 16, w: column - margin, h: 960 - split - 16 },
    { x: column + 16, y: split + 16, w: 720 - margin - column - 16, h: 960 - split - 16 },
  ];
  for (const panel of panels) {
    canvas.rect(panel.x - 4, panel.y - 4, panel.w + 8, panel.h + 8, 1);
    scene(canvas, panel, next);
  }
  canvas.digits(`${chapter.number}${String(number).padStart(2, '0')}`, margin, 982, 20, 1);
  return canvas.png();
}

function cover(work, palette) {
  const canvas = new Canvas(600, 880, palette);
  const next = random(hash(work.id));
  const whole = { x: 0, y: 0, w: 600, h: 880 };
  canvas.rect(0, 0, 600, 880, 5);
  canvas.circle(300, 340, 210 + Math.floor(next() * 40), 3, whole);
  canvas.circle(120 + Math.floor(next() * 360), 700, 300, 2, whole);
  canvas.rect(0, 760, 600, 120, 1);
  canvas.circle(300, 400, 70, 1, whole);
  canvas.rect(230, 470, 140, 300, 1);
  return canvas.png();
}

function icon() {
  const palette = PALETTES[0];
  const canvas = new Canvas(64, 64, palette);
  const whole = { x: 0, y: 0, w: 64, h: 64 };
  canvas.rect(0, 0, 64, 64, 5);
  canvas.circle(32, 26, 16, 3, whole);
  canvas.rect(0, 44, 64, 20, 1);
  return canvas.png();
}

// ---------------------------------------------------------------------------

function run(command, args, env = {}) {
  execFileSync(command, args, { stdio: 'inherit', env: { ...process.env, ...env } });
}

function main() {
  const { out, baseUrl, key } = options();
  const host = new URL(baseUrl).hostname;
  const cargo = process.env.CARGO ?? 'cargo';
  const target = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : join(ROOT, 'extensions', 'target');

  run(cargo, ['build', '--release', '--manifest-path', MANIFEST, '-p', 'prancha-ext']);
  run(cargo, ['build', '--release', '--target', 'wasm32-unknown-unknown', '--manifest-path', MANIFEST, '-p', 'prancha-extension-demo'], { PRANCHA_DEMO_BASE_URL: baseUrl });
  const tool = join(target, 'release', process.platform === 'win32' ? 'prancha-ext.exe' : 'prancha-ext');

  if (!existsSync(key)) {
    mkdirSync(dirname(key), { recursive: true });
    run(tool, ['keygen', key]);
  }

  const work = mkdtempSync(join(tmpdir(), 'prancha-demo-'));
  try {
    run(tool, ['component', join(target, 'wasm32-unknown-unknown', 'release', 'prancha_extension_demo.wasm'), join(work, 'component.wasm')]);
    writeFileSync(
      join(work, 'repo.json'),
      JSON.stringify({
        id: REPOSITORY_ID,
        name: 'Demonstração do Prancha',
        baseUrl,
        description: 'Repositório do próprio projeto, com obras originais criadas só para demonstrar as extensões.',
        website: 'https://github.com/CAKeysama/prancha',
      }),
    );
    writeFileSync(
      join(work, 'extension.json'),
      JSON.stringify({
        artifact: 'component.wasm',
        id: EXTENSION_ID,
        name: 'Acervo de demonstração',
        version: '1.0.0',
        description: 'Obras curtas criadas pelo projeto Prancha. Não é uma fonte comercial.',
        authors: ['Projeto Prancha'],
        languages: ['pt-BR'],
        minAppVersion: '0.2.0',
        icon: `data:image/png;base64,${icon().toString('base64')}`,
        supportedKinds: ['comic', 'manga'],
        contentRating: 'general',
        sources: [{ id: 'acervo', name: 'Acervo de demonstração', language: 'pt-BR', contentRating: 'general' }],
        capabilities: { network: true, storage: false, authentication: false, allowedHosts: [host] },
      }),
    );

    const catalog = join(out, 'catalogo');
    rmSync(catalog, { recursive: true, force: true });
    mkdirSync(catalog, { recursive: true });
    const works = JSON.parse(readFileSync(join(ROOT, 'extensions', 'demo', 'obras.json'), 'utf8'));
    works.forEach((entry, position) => {
      const palette = PALETTES[position % PALETTES.length];
      const folder = join(catalog, entry.id);
      mkdirSync(folder, { recursive: true });
      writeFileSync(join(folder, 'capa.png'), cover(entry, palette));
      for (const chapter of entry.chapters) {
        mkdirSync(join(folder, chapter.id), { recursive: true });
        for (let number = 1; number <= chapter.pages; number++) {
          writeFileSync(join(folder, chapter.id, `${String(number).padStart(2, '0')}.png`), page(entry, chapter, number, palette));
        }
      }
    });
    const published = works.map(({ chapters, ...entry }) => {
      const dated = chapters.map(({ published: date, ...chapter }) => ({ ...chapter, publishedAt: Date.parse(`${date}T12:00:00Z`) }));
      return { ...entry, updatedAt: Math.max(...dated.map((chapter) => chapter.publishedAt)), chapters: dated };
    });
    writeFileSync(join(catalog, 'obras.json'), `${JSON.stringify(published, null, 2)}\n`);

    run(tool, ['publish', '--key', key, '--repo', join(work, 'repo.json'), '--out', out, join(work, 'extension.json')]);
    run(tool, ['verify', out]);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

main();
