// The text of a conversation as it is exported to a file: what was said, in order, without the page's own notes and controls.

export interface Turn {
  role: "user" | "assistant";
  content: string;
}

export const MAX_BYTES = 500 * 1024;

/** "You: ..." and "Assistant: ..." blocks separated by blank lines. Control characters other than line breaks and tabs are dropped. */
export function transcriptText(turns: readonly Turn[]): string {
  const clean = (s: string) => s.replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, "");
  return turns.map((t) => `${t.role === "user" ? "You" : "Assistant"}: ${clean(t.content).trimEnd()}`).join("\n\n") + "\n";
}

/** Whether a file name is one the service accepts: 1 to 60 letters, digits, dots, dashes or underscores, not starting with a dot. */
export function validExportName(name: string): boolean {
  return name.length >= 1 && name.length <= 60 && !name.startsWith(".") && /^[A-Za-z0-9._-]+$/.test(name) && (!name.includes(".") || /\.(md|txt|json)$/.test(name) && !/^\.?(md|txt|json)$/.test(name));
}

/** True when the text is too large to send in one piece, counted in bytes the way the service counts. */
export function tooLarge(text: string): boolean {
  return new TextEncoder().encode(text).length > MAX_BYTES;
}

export type ExportFormat = "lines" | "markdown" | "json";

const dropControls = (s: string) => s.replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, "");

/** Markdown with a heading for each speaker. The words are the conversation's own, unchanged but for control characters. */
export function transcriptMarkdown(turns: readonly Turn[]): string {
  return turns.map((t) => `## ${t.role === "user" ? "You" : "Assistant"}\n\n${dropControls(t.content).trimEnd()}`).join("\n\n") + "\n";
}

/** The conversation as JSON: a version marker and the turns, each with its role and text. */
export function transcriptJson(turns: readonly Turn[]): string {
  return JSON.stringify({ kakurega_transcript: 1, turns: turns.map((t) => ({ role: t.role, content: dropControls(t.content) })) }, null, 2) + "\n";
}

/** The text of the chosen format. */
export function transcriptIn(format: ExportFormat, turns: readonly Turn[]): string {
  return format === "json" ? transcriptJson(turns) : format === "markdown" ? transcriptMarkdown(turns) : transcriptText(turns);
}

/** The name to ask for: a JSON export always ends in .json (a typed .md or .txt is replaced), the others keep the name as typed. */
export function exportFileName(name: string, format: ExportFormat): string {
  if (format !== "json") return name;
  return name.replace(/\.(md|txt|json)$/, "") + ".json";
}
