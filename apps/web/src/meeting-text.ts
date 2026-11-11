/** A note holds this many bytes at most. */
export const NOTE_BYTES = 4000;

/** The longest start of the text, cut at a line end where there is one, that fits in that many bytes. */
export function fitBytes(text: string, max: number): string {
  const enc = new TextEncoder();
  if (enc.encode(text).length <= max) return text;
  let out = "";
  for (const line of text.split("\n")) {
    const next = out + line + "\n";
    if (enc.encode(next).length > max) break;
    out = next;
  }
  if (out) return out.trimEnd();
  let cut = text.slice(0, max);
  while (enc.encode(cut).length > max) cut = cut.slice(0, -1);
  return cut;
}

/** Subtitle files carry counters and timings between the spoken lines; those are dropped so the model reads speech. */
export function plainTranscript(name: string, text: string): string {
  if (!/\.(vtt|srt)$/i.test(name)) return text;
  return text
    .split(/\r?\n/)
    .filter((l) => !/^\s*(WEBVTT.*|\d+|NOTE.*|[\d:.,]+\s*-->\s*[\d:.,]+.*)\s*$/.test(l))
    .filter((l) => l.trim().length > 0)
    .join("\n");
}
