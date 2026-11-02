// Tipos das atualizações e leitura das notas publicadas nas releases do GitHub, a fonte oficial delas.

export interface ReleaseInfo {
  version: string;
  name: string;
  date: string | null;
  notes: string;
  url: string;
}

export interface UpdatesSnapshot {
  currentVersion: string;
  available: ReleaseInfo | null;
  declinedVersion: string | null;
  /** O instalador abriu, mas esta versão não chegou a rodar. */
  failedVersion: string | null;
  welcome: { version: string; release: ReleaseInfo | null } | null;
  updatedAt: number | null;
  lastCheckAt: number | null;
  needsAdmin: boolean;
}

export type UpdateProgress =
  | { phase: "downloading"; downloaded: number; total: number | null }
  | { phase: "verifying" }
  | { phase: "installing" };

export interface UpdatesHistory {
  releases: ReleaseInfo[];
  /** A consulta falhou: a lista é a última guardada neste computador. */
  stale: boolean;
}

interface NoteSection {
  title: string;
  items: string[];
}

export interface ParsedNotes {
  /** Data escrita nas notas. É a data real da versão, mesmo que a release tenha sido recriada depois. */
  released: string | null;
  summary: string;
  sections: NoteSection[];
}

const plain = (text: string) =>
  text
    .replace(/\*\*|`/g, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .trim();

/** Lê as notas como texto simples, sem HTML. A seção de arquivos só interessa a quem baixa pelo site. */
export function parseNotes(body: string): ParsedNotes {
  const notes: ParsedNotes = { released: null, summary: "", sections: [] };
  let section: NoteSection | null = null;
  let skipping = false;
  for (const raw of body.split(/\r?\n/)) {
    const line = raw.trim();
    const heading = /^#{1,6}\s+(.+)$/.exec(line);
    const released = /^Lançada em (.+?)\.$/.exec(line);
    if (heading) {
      const title = plain(heading[1]!);
      skipping = title === "Arquivos";
      section = null;
      if (!skipping && !title.startsWith("Prancha v")) {
        section = { title, items: [] };
        notes.sections.push(section);
      }
    } else if (!line || skipping) {
      continue;
    } else if (released && !section) {
      notes.released = released[1]!;
    } else if (section) {
      section.items.push(plain(line.replace(/^[-*]\s+/, "")));
    } else {
      notes.summary = `${notes.summary} ${plain(line)}`.trim();
    }
  }
  notes.sections = notes.sections.filter((s) => s.items.length > 0);
  return notes;
}

/** O resumo curto da janela de nova versão: o sumário da release ou, sem ele, o primeiro item publicado. */
export function shortSummary(notes: ParsedNotes): string {
  return notes.summary || notes.sections[0]?.items[0] || "";
}
