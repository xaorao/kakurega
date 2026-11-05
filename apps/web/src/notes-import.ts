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

/** The file types the import card takes: Markdown, Cursor rule files and plain text. */
export function isNoteFile(fileName: string): boolean {
  return /\.(md|mdc|markdown|txt)$/i.test(fileName);
}

/** What one chosen file becomes: nothing (an index), one note, or one note per entry. */
export function notesFromFile(fileName: string, text: string): PickedNote[] {
  const lower = fileName.toLowerCase();
  if (lower === "memory.md" || lower === "user.md") {
    if (lower === "memory.md" && isLinkIndex(text)) return [];
    const stem = fileName.replace(/\.md$/i, "");
    return splitEntries(text).map((t, i) => ({ name: `${stem} entry ${i + 1}`, text: t }));
  }
  return fromFrontmatter(fileName, text);
}

/** A note with a small frontmatter (Claude Code memory files, Cursor `.mdc` rules): the description leads the text and the
 *  frontmatter itself is not part of it. A file without one is taken as it is. */
function fromFrontmatter(fileName: string, input: string): PickedNote[] {
  const t = input.replace(/^\uFEFF/, "");
  let name = fileName;
  let description = "";
  let body = t;
  if (t.startsWith("---")) {
    const lines = t.split(/\r?\n/);
    const close = lines.findIndex((l, i) => i > 0 && l.trim() === "---");
    if (close > 0) {
      for (const line of lines.slice(1, close)) {
        if (/^\s/.test(line) || !line.includes(":")) continue;
        const [k, ...rest] = line.split(":");
        const v = rest.join(":").trim().replace(/^(["'])(.*)\1$/, "$2");
        if (k?.trim() === "name" && v) name = v;
        if (k?.trim() === "description") description = v;
      }
      body = lines.slice(close + 1).join("\n");
    }
  }
  body = body.trim();
  const text = description && body ? `${description}\n\n${body}` : description || body;
  return text ? [{ name, text }] : [];
}
