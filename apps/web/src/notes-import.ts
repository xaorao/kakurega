export interface PickedNote {
  name: string;
  text: string;
}

/** Hermes Agent's MEMORY.md and USER.md hold entries joined by a line with only "§". */
export function splitEntries(text: string): string[] {
  const out: string[] = [];
  let cur: string[] = [];
  for (const line of text.replace(/^﻿/, "").split(/\r?\n/)) {
    if (line.trim() === "§") {
      out.push(cur.join("\n"));
      cur = [];
    } else cur.push(line);
  }
  out.push(cur.join("\n"));
  return out.map((e) => e.trim()).filter(Boolean);
}

/** Claude Code's MEMORY.md is a list of links to the notes beside it, not a note itself. */
export function isLinkIndex(text: string): boolean {
  const lines = text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  return lines.length > 0 && lines.every((l) => l.startsWith("#") || (l.replace(/^[-* ]+/, "").startsWith("[") && l.includes("](")));
}

/** What one chosen file becomes: nothing (an index), one note, or one note per entry. */
export function notesFromFile(fileName: string, text: string): PickedNote[] {
  const lower = fileName.toLowerCase();
  if (lower === "memory.md" || lower === "user.md") {
    if (lower === "memory.md" && isLinkIndex(text)) return [];
    const stem = fileName.replace(/\.md$/i, "");
    return splitEntries(text).map((t, i) => ({ name: `${stem} entry ${i + 1}`, text: t }));
  }
  const t = text.trim();
  return t ? [{ name: fileName, text: t }] : [];
}
