// Ícones que o lucide não tem, no mesmo traço (24×24, stroke 2, pontas redondas) para não destoar dos outros.
import { createLucideIcon } from "lucide-react";

/** Dois tiques: lido. É o mesmo desenho do `CheckCheck` do lucide, repetido aqui para os três ficarem iguais. */
export const ReadTicks = createLucideIcon("prancha-read-ticks", [
  ["path", { d: "M18 6 7 17l-5-5", key: "a" }],
  ["path", { d: "m22 10-7.5 7.5L13 16", key: "b" }],
]);

/** Dois tiques atravessados por uma diagonal: marcar como não lido. */
export const UnreadTicks = createLucideIcon("prancha-unread-ticks", [
  ["path", { d: "M18 6 7 17l-5-5", key: "a" }],
  ["path", { d: "m22 10-7.5 7.5L13 16", key: "b" }],
  ["path", { d: "M3 3 21 21", key: "c" }],
]);

/** Um tique e uma seta para baixo: marcar os anteriores como lidos. */
export const ReadPrevious = createLucideIcon("prancha-read-previous", [
  ["path", { d: "M2 9.5 5.5 13 12 6", key: "a" }],
  ["path", { d: "M18 4v15", key: "b" }],
  ["path", { d: "m14 15 4 4 4-4", key: "c" }],
]);
