// Índice da pesquisa das configurações: cada opção com as palavras que alguém usaria para procurá-la,
// não só o título que aparece na tela. Os `id`s são as âncoras `setting-<id>` da tela.

export type SettingsCategory = "aparencia" | "leitura" | "biblioteca" | "notificacoes" | "armazenamento" | "atualizacoes" | "integracoes" | "privacidade" | "geral";
/** `plugins` aponta para a tela Plugins, onde moram as opções de leitura online. */
export type SettingPlace = SettingsCategory | "plugins";

export const SETTINGS_CATEGORIES: ReadonlyArray<{ id: SettingsCategory; label: string }> = [
  { id: "aparencia", label: "Aparência" },
  { id: "leitura", label: "Leitura" },
  { id: "biblioteca", label: "Biblioteca" },
  { id: "notificacoes", label: "Notificações" },
  { id: "armazenamento", label: "Armazenamento" },
  { id: "atualizacoes", label: "Atualizações" },
  { id: "integracoes", label: "Integrações" },
  { id: "privacidade", label: "Privacidade" },
  { id: "geral", label: "Geral" },
];

export const PLACE_LABELS: Record<SettingPlace, string> = {
  ...(Object.fromEntries(SETTINGS_CATEGORIES.map((c) => [c.id, c.label])) as Record<SettingsCategory, string>),
  plugins: "Plugins",
};

export interface SettingEntry {
  id: string;
  place: SettingPlace;
  title: string;
  description: string;
  keywords: string[];
}

export const SETTINGS_INDEX: readonly SettingEntry[] = [
  { id: "tema", place: "aparencia", title: "Tema", description: "Escuro, claro ou seguindo o sistema.", keywords: ["modo escuro", "modo claro", "noturno", "dark", "light", "cores", "visual"] },
  { id: "densidade", place: "aparencia", title: "Densidade da biblioteca", description: "Confortável ou compacta.", keywords: ["espaçamento", "compacta", "confortável"] },
  { id: "visualizacao", place: "aparencia", title: "Visualização da biblioteca", description: "Grade ou lista.", keywords: ["grade", "lista", "exibição", "layout"] },
  { id: "miniaturas", place: "aparencia", title: "Tamanho das miniaturas", description: "Pequeno, médio ou grande.", keywords: ["capas", "thumbnail", "imagem"] },

  { id: "modo", place: "leitura", title: "Modo de página", description: "Página única, dupla, vertical ou webtoon.", keywords: ["página dupla", "webtoon", "manhwa", "rolagem", "layout", "leitor"] },
  { id: "direcao", place: "leitura", title: "Direção de leitura", description: "Direita para a esquerda é o padrão de mangás.", keywords: ["mangá", "esquerda", "direita", "rtl", "ltr", "leitor"] },
  { id: "ajuste", place: "leitura", title: "Ajuste padrão da imagem", description: "Encaixa a página na tela, na largura ou na altura.", keywords: ["zoom", "largura", "altura", "encaixar", "leitor"] },
  { id: "capa-sozinha", place: "leitura", title: "Capa sozinha na página dupla", description: "Mostra a primeira página isolada para alinhar os pares.", keywords: ["capa", "pares", "impresso", "leitor"] },
  { id: "roda", place: "leitura", title: "Roda do mouse", description: "Vira a página ou só rola.", keywords: ["mouse", "scroll", "rolagem", "virar página", "leitor"] },
  { id: "fim", place: "leitura", title: "Ao avançar depois da última página", description: "Permanece no leitor ou volta à biblioteca.", keywords: ["fim", "final", "terminar", "última página", "leitor"] },
  { id: "controles", place: "leitura", title: "Controles do leitor", description: "Ocultar sozinhos ou sempre visíveis.", keywords: ["barra", "botões", "esconder", "ocultar", "interface"] },
  { id: "atalhos", place: "leitura", title: "Atalhos de teclado", description: "Teclas usadas dentro do leitor.", keywords: ["teclado", "tecla", "atalho", "shortcut", "leitor"] },

  { id: "duplicados", place: "biblioteca", title: "Arquivos duplicados", description: "Ignorar ou importar mesmo assim.", keywords: ["duplicata", "repetido", "cópia", "importar", "importação"] },
  { id: "ordenacao", place: "biblioteca", title: "Ordenação padrão da biblioteca", description: "Título, data de adição, último acesso ou progresso.", keywords: ["ordem", "ordenar", "classificar", "crescente", "decrescente"] },
  { id: "verificar", place: "biblioteca", title: "Verificar arquivos", description: "Procura HQs cujos arquivos foram movidos ou excluídos.", keywords: ["ausente", "faltando", "movido", "excluído", "perdido"] },
  { id: "capas", place: "biblioteca", title: "Atualizar capas", description: "Recria todas as miniaturas a partir dos arquivos.", keywords: ["capas", "miniaturas", "recriar", "regenerar", "thumbnail"] },

  { id: "notif-capitulos", place: "notificacoes", title: "Capítulos novos", description: "Avisa quando uma obra acompanhada recebe capítulos.", keywords: ["notificação", "aviso", "capítulo", "lançamento", "acompanhar", "novidade"] },
  { id: "notif-atualizacoes", place: "notificacoes", title: "Avisos de nova versão", description: "Versão disponível, instalada ou que não terminou de instalar.", keywords: ["notificação", "aviso", "versão", "update"] },
  { id: "notif-downloads", place: "notificacoes", title: "Downloads concluídos", description: "Capítulos baixados para a biblioteca.", keywords: ["notificação", "aviso", "baixar", "download"] },
  { id: "notif-falhas", place: "notificacoes", title: "Falhas de download", description: "Capítulos que não puderam ser baixados.", keywords: ["notificação", "erro", "falha", "download"] },
  { id: "notif-sincronizacao", place: "notificacoes", title: "Problemas de sincronização", description: "Obras acompanhadas que não puderam ser verificadas.", keywords: ["notificação", "sincronizar", "fonte", "erro", "sistema"] },
  { id: "notif-biblioteca", place: "notificacoes", title: "Avisos da biblioteca", description: "Importações com problemas e arquivos que sumiram.", keywords: ["notificação", "importação", "arquivo ausente", "biblioteca"] },
  { id: "notif-nativas", place: "notificacoes", title: "Notificações do sistema operacional", description: "Avisos fora da janela do Prancha.", keywords: ["notificação nativa", "windows", "desktop", "área de trabalho", "pop-up", "silenciar", "não perturbe"] },

  { id: "pasta", place: "armazenamento", title: "Pasta de dados do aplicativo", description: "Onde ficam o catálogo, as capas e o cache.", keywords: ["pasta", "diretório", "dados", "backup", "local"] },
  { id: "uso", place: "armazenamento", title: "Espaço usado", description: "Tamanho do catálogo, das miniaturas e do cache de páginas.", keywords: ["espaço", "disco", "tamanho", "uso"] },
  { id: "limpar-miniaturas", place: "armazenamento", title: "Limpar miniaturas em cache", description: "Remove as capas geradas; elas são recriadas.", keywords: ["cache", "limpar", "liberar espaço", "capas"] },
  { id: "limpar-paginas", place: "armazenamento", title: "Limpar cache de páginas", description: "Remove as páginas extraídas de arquivos CBR e CB7.", keywords: ["cache", "limpar", "liberar espaço", "cbr", "cb7"] },

  { id: "versao", place: "atualizacoes", title: "Versão instalada", description: "A versão do Prancha neste computador.", keywords: ["versão", "número"] },
  { id: "procurar", place: "atualizacoes", title: "Procurar nova versão", description: "Verifica se há atualização e instala.", keywords: ["atualizar", "atualização", "update", "nova versão", "instalar"] },
  { id: "historico", place: "atualizacoes", title: "Versões publicadas", description: "As notas de cada versão.", keywords: ["histórico", "novidades", "changelog", "notas", "o que mudou"] },

  { id: "myanimelist", place: "integracoes", title: "MyAnimeList", description: "Conecta sua conta para associar as obras e atualizar os capítulos lidos.", keywords: ["mal", "myanimelist", "anime list", "conta", "login", "entrar", "conectar", "desconectar", "sincronizar", "lista", "progresso", "tracker", "rastreador"] },
  { id: "mal-sincronizacao", place: "integracoes", title: "Sincronização com o MyAnimeList", description: "Atualizações pendentes, falhas e tentar de novo.", keywords: ["mal", "fila", "pendente", "erro", "falha", "sincronização", "tentar de novo"] },

  { id: "telemetria", place: "privacidade", title: "Estatísticas de uso", description: "Telemetria voluntária: versão, fontes usadas, falhas e tempos, sem conteúdo nem dados pessoais.", keywords: ["telemetria", "privacidade", "dados", "coleta", "estatísticas", "métricas", "analytics", "consentimento", "rastreamento", "anônimo"] },
  { id: "telemetria-identificador", place: "privacidade", title: "Identificador da instalação", description: "Troca o identificador aleatório usado pela telemetria.", keywords: ["identificador", "id", "renovar", "trocar", "anônimo", "telemetria"] },
  { id: "telemetria-apagar", place: "privacidade", title: "Apagar os dados enviados", description: "Desliga a telemetria e pede ao servidor que apague o que esta instalação enviou.", keywords: ["apagar", "excluir", "remover", "dados", "lgpd", "privacidade", "telemetria"] },

  { id: "idioma", place: "geral", title: "Idioma", description: "Português do Brasil.", keywords: ["língua", "language", "português", "tradução"] },
  { id: "tutorial", place: "geral", title: "Tutorial de primeira execução", description: "Mostra de novo o passo a passo para adicionar e ler uma HQ.", keywords: ["ajuda", "passo a passo", "guia", "introdução", "começar"] },
  { id: "sugerir", place: "geral", title: "Sugerir uma melhoria", description: "Escreva sua ideia e envie pelo GitHub.", keywords: ["sugestão", "ideia", "feedback", "pedido", "issue", "bug", "problema"] },
  { id: "visao-geral", place: "geral", title: "Visão geral do projeto", description: "Versões, downloads e verificações do Prancha no GitHub.", keywords: ["overview", "métricas", "estatísticas", "downloads", "releases", "ci", "github", "projeto"] },
  { id: "licencas", place: "geral", title: "Licenças de terceiros", description: "Componentes de código aberto usados pelo Prancha.", keywords: ["licença", "código aberto", "open source", "créditos"] },
  { id: "sobre", place: "geral", title: "Sobre o Prancha", description: "Versão, formatos aceitos e componentes.", keywords: ["sobre", "formatos", "versão", "informações"] },

  { id: "fontes-online", place: "plugins", title: "Fontes de leitura", description: "Plugin para procurar e ler obras de repositórios de extensões.", keywords: ["online", "internet", "extensões", "repositórios", "explorar", "plugin"] },
];

const STOP_WORDS = new Set(["a", "o", "e", "as", "os", "ao", "aos", "de", "da", "do", "das", "dos", "em", "na", "no", "nas", "nos", "para", "por", "com", "um", "uma"]);

/** Sem maiúsculas nem acentos: “Atualização” e “atualizacao” são a mesma busca. */
export function normalize(text: string): string {
  return text.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "");
}

/** Plural simples do português: “atualizações” vira “atualizacao”; “temas”, “tema”. */
export function singular(word: string): string {
  if (word.endsWith("oes")) return `${word.slice(0, -3)}ao`;
  if (word.length > 3 && word.endsWith("s")) return word.slice(0, -1);
  return word;
}

type Word = readonly [raw: string, singular: string];

function words(text: string): Word[] {
  return normalize(text)
    .split(/[^a-z0-9+]+/)
    .filter(Boolean)
    .map((w) => [w, singular(w)] as const);
}

/** Pesos por campo: o título vale mais que uma palavra-chave, que vale mais que a categoria e a descrição. */
const PREPARED = SETTINGS_INDEX.map((entry) => ({
  entry,
  fields: [
    { weight: 4, words: words(entry.title) },
    { weight: 3, words: words(entry.keywords.join(" ")) },
    { weight: 2, words: words(PLACE_LABELS[entry.place]) },
    { weight: 1, words: words(entry.description) },
  ],
}));

/** Toda palavra digitada precisa começar alguma palavra da opção. Ordem: relevância, depois a ordem da tela. */
export function searchSettings(query: string): SettingEntry[] {
  const terms = words(query).filter(([raw]) => !STOP_WORDS.has(raw));
  if (!terms.length) return [];
  const scored: Array<{ entry: SettingEntry; score: number }> = [];
  for (const { entry, fields } of PREPARED) {
    let score = 0;
    for (const [raw, one] of terms) {
      const best = Math.max(0, ...fields.filter((f) => f.words.some(([w, s]) => w.startsWith(raw) || s.startsWith(one))).map((f) => f.weight));
      if (!best) {
        score = 0;
        break;
      }
      score += best;
    }
    if (score) scored.push({ entry, score });
  }
  return scored.sort((a, b) => b.score - a.score).map((s) => s.entry);
}
