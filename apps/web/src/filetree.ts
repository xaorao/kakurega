// The Source tab's file list: filtered by what is typed, grouped by folder, and cut to a number that can be drawn at once.

export interface TreeFile {
  path: string;
}

export interface Group<T extends TreeFile> {
  /** The folder the files are in, "" for the top level. */
  dir: string;
  files: T[];
}

/** The most files drawn at once; the rest are reached by typing. */
export const MAX_SHOWN = 400;

/** The files whose path holds every word typed, in any case. Nothing typed keeps them all. */
export function filterFiles<T extends TreeFile>(files: readonly T[], query: string): T[] {
  const words = query.toLowerCase().split(/\s+/).filter((w) => w !== "");
  if (words.length === 0) return [...files];
  return files.filter((f) => {
    const p = f.path.toLowerCase();
    return words.every((w) => p.includes(w));
  });
}

/** Files grouped by their folder, folders in path order, the top level first, files in name order inside each. */
export function groupByDir<T extends TreeFile>(files: readonly T[]): Group<T>[] {
  const by = new Map<string, T[]>();
  for (const f of files) {
    const i = f.path.lastIndexOf("/");
    const dir = i < 0 ? "" : f.path.slice(0, i);
    const list = by.get(dir);
    if (list) list.push(f);
    else by.set(dir, [f]);
  }
  return [...by.entries()]
    .sort(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : a < b ? -1 : a > b ? 1 : 0))
    .map(([dir, list]) => ({ dir, files: list.sort((x, y) => (x.path < y.path ? -1 : x.path > y.path ? 1 : 0)) }));
}

/** The name without its folder. */
export function baseName(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? path : path.slice(i + 1);
}

/** The label the highlighter knows a file by: its extension, lower case, or null when it has none. */
export function languageOf(path: string): string | null {
  const name = baseName(path);
  const i = name.lastIndexOf(".");
  return i <= 0 || i === name.length - 1 ? null : name.slice(i + 1).toLowerCase();
}

/** The numbers of the lines of `text`, one per line, for a gutter. A text ending in a newline has no empty last line. */
export function lineCount(text: string): number {
  if (text === "") return 0;
  const n = text.split("\n").length;
  return text.endsWith("\n") ? n - 1 : n;
}
