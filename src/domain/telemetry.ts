// Telemetria anônima e voluntária: tipos e os textos de privacidade, iguais na tela de Configurações e no convite.
// O que é coletado de verdade está em src-tauri/src/telemetry.rs e em docs/TELEMETRY.md; mantenha os três iguais.

export type TelemetryConsent = "unset" | "granted" | "denied";

export interface TelemetryStatus {
  consent: TelemetryConsent;
  decidedAt: number | null;
  installId: string | null;
  pendingEvents: number;
  lastSentAt: number | null;
  /** Este build envia para o servidor oficial. Builds de desenvolvimento não enviam nada. */
  endpoint: boolean;
  /** Um pedido para apagar os dados não chegou ao servidor e pode ser repetido. */
  forgetPending: boolean;
}

export const TELEMETRY_PURPOSE =
  "Ajuda a saber quais versões estão em uso, quais fontes são mais usadas ou falham e onde o Prancha trava ou fica lento. É opcional e o Prancha funciona igual sem ela.";

export const TELEMETRY_COLLECTED: readonly string[] = [
  "Versão do Prancha, sistema (Windows, macOS ou Linux), arquitetura e tipo de instalador.",
  "Quantas vezes o app abriu por dia e se a vez anterior fechou de forma inesperada.",
  "Por fonte, só o identificador técnico da extensão e da fonte: quantas buscas, obras e capítulos abertos e quantos erros.",
  "Falhas por categoria, componente e código técnico (por exemplo, “network / sources / timeout”), somadas por hora.",
  "Tempos de abertura do app, das listas de Explorar, de obras e de capítulos.",
  "Um identificador aleatório desta instalação, criado quando você aceita, que serve só para contar instalações.",
];

export const TELEMETRY_NOT_COLLECTED: readonly string[] = [
  "Títulos, capas, buscas, páginas ou qualquer conteúdo que você lê ou importa.",
  "Histórico de leitura, progresso, favoritos ou coleções.",
  "Nome de arquivos e pastas, caminhos do computador, mensagens de erro em texto.",
  "Nome, e-mail, contas, senhas, tokens, cookies ou endereços de sites.",
  "Endereço IP: o servidor não o grava junto com os dados.",
];

export const TELEMETRY_LIMITS =
  "Os dados são agregados por dia, ficam 180 dias e só a equipe do Prancha vê os totais. O identificador é aleatório, mas é o mesmo em todos os envios desta instalação até você trocá-lo ou desligar a telemetria: por isso falamos em dados pseudônimos, não em anonimato absoluto.";
