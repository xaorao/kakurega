// The pure parts of attaching files to a message: how a file is described and what is said about it after an answer.

import type { Key } from "./i18n";

export interface FilesReport {
  used: number;
  trimmed: number;
  left_out: number;
  withheld_instructions: number;
  withheld_secrets: number;
}

/** The most files one message carries, the same figure as the service. */
export const MAX_FILES = 5;
/** The most bytes one file may have, the same figure as the service. A larger one is refused here, before it is sent. */
export const MAX_BYTES = 8 * 1024 * 1024;

export function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 102.4) / 10} KB`;
  return `${Math.round(bytes / 104857.6) / 10} MB`;
}

/** What to say after an answer about the files that went with the question, as language keys with a number. Nothing when none went. */
export function fileNotes(f: FilesReport | undefined): { key: Key; n: number }[] {
  if (!f || (f.used === 0 && f.left_out === 0 && f.withheld_instructions === 0 && f.withheld_secrets === 0)) return [];
  const out: { key: Key; n: number }[] = [];
  if (f.used > 0) out.push({ key: "files.used", n: f.used });
  if (f.trimmed > 0) out.push({ key: "files.trimmed", n: f.trimmed });
  if (f.left_out > 0) out.push({ key: "files.leftOut", n: f.left_out });
  if (f.withheld_instructions > 0) out.push({ key: "files.withheldInstructions", n: f.withheld_instructions });
  if (f.withheld_secrets > 0) out.push({ key: "files.withheldSecrets", n: f.withheld_secrets });
  return out;
}

/** The files that may still be added to a message of `have` files, in the order given. */
export function room(have: number, incoming: readonly File[]): { take: File[]; over: number } {
  const free = Math.max(0, MAX_FILES - have);
  return { take: incoming.slice(0, free), over: Math.max(0, incoming.length - free) };
}
