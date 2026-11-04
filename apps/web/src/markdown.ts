/** A small, safe Markdown reader for assistant answers. It produces a tree of plain data, never HTML: the screen builds
 * elements from the tree, so nothing a model writes can become markup, a script, an image request or a hidden link. */

export type Inline =
  | { t: "text"; v: string }
  | { t: "code"; v: string }
  | { t: "bold"; v: Inline[] }
  | { t: "em"; v: Inline[] }
  | { t: "link"; v: string; href: string; host: string };

export type Block =
  | { t: "p"; v: Inline[] }
  | { t: "h"; v: Inline[] }
  | { t: "code"; lang: string; v: string }
  | { t: "ul"; v: Inline[][] }
  | { t: "ol"; v: Inline[][] }
  | { t: "quote"; v: Inline[] };

export const MAX_INPUT = 200_000;

/** Only plain https links with a real host are made clickable; everything else stays visible text. */
export function safeHref(raw: string): { href: string; host: string } | null {
  if (raw.length > 2000 || /[\s\u0000-\u001f\u007f<>"'`\\]/.test(raw)) return null;
  let u: URL;
  try {
    u = new URL(raw);
  } catch {
    return null;
  }
  if (u.protocol !== "https:" || !u.hostname || u.username || u.password) return null;
  return { href: u.href, host: u.hostname };
}

export function inline(src: string): Inline[] {
  const out: Inline[] = [];
  let text = "";
  const flush = () => {
    if (text) out.push({ t: "text", v: text });
    text = "";
  };
  let i = 0;
  while (i < src.length) {
    const rest = src.slice(i);
    let m: RegExpExecArray | null;
    if ((m = /^`([^`\n]+)`/.exec(rest))) {
      flush();
      out.push({ t: "code", v: m[1] ?? "" });
      i += m[0].length;
    } else if ((m = /^\*\*([^*\n]+)\*\*/.exec(rest))) {
      flush();
      out.push({ t: "bold", v: inline(m[1] ?? "") });
      i += m[0].length;
    } else if ((m = /^\*([^*\s][^*\n]*)\*/.exec(rest)) && !/^\*\s/.test(rest)) {
      flush();
      out.push({ t: "em", v: inline(m[1] ?? "") });
      i += m[0].length;
    } else if (rest.startsWith("![")) {
      // images are never loaded: keep the text, drop the request
      const close = /^!\[([^\]\n]*)\]\(([^)\n]*)\)/.exec(rest);
      if (close) {
        text += `[image: ${close[1] ?? ""}]`;
        i += close[0].length;
      } else {
        text += rest[0];
        i += 1;
      }
    } else if ((m = /^\[([^\]\n]+)\]\(([^)\n]+)\)/.exec(rest))) {
      const link = safeHref((m[2] ?? "").trim());
      if (link) {
        flush();
        out.push({ t: "link", v: m[1] ?? "", href: link.href, host: link.host });
      } else {
        text += `${m[1] ?? ""} (${m[2] ?? ""})`;
      }
      i += m[0].length;
    } else {
      text += rest[0];
      i += 1;
    }
  }
  flush();
  return out;
}

export function parse(input: string): Block[] {
  const lines = input.slice(0, MAX_INPUT).replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let para: string[] = [];
  const endPara = () => {
    if (para.length) blocks.push({ t: "p", v: inline(para.join(" ")) });
    para = [];
  };
  for (let n = 0; n < lines.length; n++) {
    const line = lines[n] ?? "";
    const fence = /^\s*```\s*([\w+-]*)\s*$/.exec(line);
    if (fence) {
      endPara();
      const body: string[] = [];
      n++;
      while (n < lines.length && !/^\s*```\s*$/.test(lines[n] ?? "")) {
        body.push(lines[n] ?? "");
        n++;
      }
      blocks.push({ t: "code", lang: (fence[1] ?? "").slice(0, 20), v: body.join("\n") });
      continue;
    }
    if (!line.trim()) {
      endPara();
      continue;
    }
    const h = /^#{1,6}\s+(.*)$/.exec(line);
    if (h) {
      endPara();
      blocks.push({ t: "h", v: inline(h[1] ?? "") });
      continue;
    }
    const ul = /^\s*[-*+]\s+(.*)$/.exec(line);
    const ol = /^\s*\d{1,3}[.)]\s+(.*)$/.exec(line);
    if (ul || ol) {
      endPara();
      const kind = ul ? "ul" : "ol";
      const items: Inline[][] = [];
      let k = n;
      for (; k < lines.length; k++) {
        const l = lines[k] ?? "";
        const m = kind === "ul" ? /^\s*[-*+]\s+(.*)$/.exec(l) : /^\s*\d{1,3}[.)]\s+(.*)$/.exec(l);
        if (!m) break;
        items.push(inline(m[1] ?? ""));
      }
      blocks.push({ t: kind, v: items });
      n = k - 1;
      continue;
    }
    const q = /^>\s?(.*)$/.exec(line);
    if (q) {
      endPara();
      blocks.push({ t: "quote", v: inline(q[1] ?? "") });
      continue;
    }
    para.push(line.trim());
  }
  endPara();
  return blocks;
}
