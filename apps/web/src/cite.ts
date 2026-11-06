/**
 * Where a cited line is, written the way editors take it on the command line or in "go to file" (`path:line`), so the owner
 * can open the exact line in their own editor. Null when there is nothing safe to give: no folder, a line that is not a
 * whole number from 1, or a file name that is absolute or climbs out of the folder.
 */
export function editorLocation(folder: string, file: string, line: unknown): string | null {
  if (!folder || !file || typeof line !== "number" || !Number.isInteger(line) || line < 1) return null;
  const parts = file.split(/[\\/]/);
  if (/^([A-Za-z]:|[\\/])/.test(file) || parts.includes("..") || parts.includes("")) return null;
  const windows = folder.includes("\\") && !folder.includes("/");
  const sep = windows ? "\\" : "/";
  const base = folder.replace(/[\\/]+$/, "");
  return `${base}${sep}${parts.join(sep)}:${line}`;
}
