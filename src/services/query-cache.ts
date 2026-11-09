// Cache em memória das consultas ao backend, compartilhado pelo app inteiro enquanto ele está aberto.
// Uma tela que sai e volta encontra o último valor na hora, e duas telas que pedem a mesma coisa ao mesmo tempo
// esperam uma consulta só. Quem guarda os dados de verdade continua sendo o SQLite: aqui fica só o que já foi lido.

export interface Snapshot<T> {
  data: T | undefined;
  error: string | null;
  /** Quando a última consulta deu certo (`Date.now()`), ou 0. */
  updatedAt: number;
  /** A versão da biblioteca em que o valor foi lido. */
  version: number;
  /** Há uma consulta em andamento. */
  loading: boolean;
}

interface Entry {
  snapshot: Snapshot<unknown>;
  promise: Promise<unknown> | null;
  /** A versão que a consulta em andamento vai ler. */
  pendingVersion: number;
  /** Só a consulta mais nova grava: a resposta de uma antiga não passa por cima. */
  token: number;
  usedAt: number;
}

/** Acima disto, sai o que foi usado há mais tempo e não tem ninguém olhando. */
const MAX_ENTRIES = 300;

const entries = new Map<string, Entry>();
const listeners = new Map<string, Set<() => void>>();

const EMPTY: Snapshot<never> = { data: undefined, error: null, updatedAt: 0, version: -1, loading: false };

function entryOf(key: string): Entry {
  let entry = entries.get(key);
  if (!entry) {
    entry = { snapshot: EMPTY, promise: null, pendingVersion: -1, token: 0, usedAt: Date.now() };
    entries.set(key, entry);
    evict();
  }
  entry.usedAt = Date.now();
  return entry;
}

function evict() {
  if (entries.size <= MAX_ENTRIES) return;
  const idle = [...entries].filter(([key, entry]) => !entry.promise && !listeners.get(key)?.size).sort((a, b) => a[1].usedAt - b[1].usedAt);
  for (const [key] of idle.slice(0, entries.size - MAX_ENTRIES)) entries.delete(key);
}

function update(key: string, entry: Entry, change: Partial<Snapshot<unknown>>) {
  entry.snapshot = { ...entry.snapshot, ...change };
  for (const listener of listeners.get(key) ?? []) listener();
}

/** Mantém a mesma referência quando o conteúdo não mudou: quem depende dela não desenha nem consulta de novo. */
export function sameOrNew<T>(before: T | undefined, after: T): T {
  if (before === undefined) return after;
  try {
    return JSON.stringify(before) === JSON.stringify(after) ? before : after;
  } catch {
    return after;
  }
}

export function peek<T>(key: string): Snapshot<T> {
  return (entries.get(key)?.snapshot ?? EMPTY) as Snapshot<T>;
}

export function subscribe(key: string, listener: () => void): () => void {
  let set = listeners.get(key);
  if (!set) listeners.set(key, (set = new Set()));
  set.add(listener);
  return () => {
    set.delete(listener);
    if (set.size === 0) listeners.delete(key);
  };
}

/**
 * Consulta e guarda. Um pedido igual a um que ainda está andando (mesma chave e mesma versão) recebe a mesma promessa.
 * Uma falha mantém o valor anterior e só registra o erro.
 */
export function fetchInto<T>(key: string, load: () => Promise<T>, version = 0): Promise<T> {
  const entry = entryOf(key);
  if (entry.promise && entry.pendingVersion === version) return entry.promise as Promise<T>;
  const token = ++entry.token;
  const promise = load().then(
    (data) => {
      if (entry.token === token) {
        entry.promise = null;
        update(key, entry, { data: sameOrNew(entry.snapshot.data as T | undefined, data), error: null, updatedAt: Date.now(), version, loading: false });
      }
      return data;
    },
    (error: unknown) => {
      if (entry.token === token) {
        entry.promise = null;
        update(key, entry, { error: error instanceof Error ? error.message : String(error), loading: false });
      }
      throw error;
    },
  );
  entry.promise = promise;
  entry.pendingVersion = version;
  if (!entry.snapshot.loading) update(key, entry, { loading: true });
  return promise;
}

/** Grava um valor que chegou por outro caminho (uma ação da tela, por exemplo). */
export function setData<T>(key: string, data: T, version?: number) {
  const entry = entryOf(key);
  update(key, entry, { data: sameOrNew(entry.snapshot.data as T | undefined, data), error: null, updatedAt: Date.now(), version: version ?? entry.snapshot.version });
}

/** Esquece tudo. Para os testes, que começam cada caso do zero. */
export function clearQueryCache() {
  entries.clear();
}
