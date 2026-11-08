import { Code } from "./Code";
import { createMemo, createSignal, For, Match, Show, Switch } from "solid-js";
import { parse, tableToTsv, type Align, type Block, type Inline } from "../markdown";
import { t } from "../i18n";

function Inlines(props: { v: Inline[] }) {
  return (
    <For each={props.v}>
      {(n) => (
        <Switch>
          <Match when={n.t === "text" && n}>{(x) => <>{x().v}</>}</Match>
          <Match when={n.t === "code" && n}>{(x) => <code>{x().v}</code>}</Match>
          <Match when={n.t === "bold" && n}>{(x) => <strong><Inlines v={x().v} /></strong>}</Match>
          <Match when={n.t === "em" && n}>{(x) => <em><Inlines v={x().v} /></em>}</Match>
          <Match when={n.t === "link" && n}>
            {(x) => (
              <>
                <a href={x().href} target="_blank" rel="noopener noreferrer nofollow">{x().v}</a>
                <span class="muted"> ({x().host})</span>
              </>
            )}
          </Match>
        </Switch>
      )}
    </For>
  );
}

function CodeBlock(props: { lang: string; v: string }) {
  const [copied, setCopied] = createSignal(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(props.v);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* copying is a convenience; the text is still selectable */
    }
  };
  return (
    <div class="codeblock">
      <div class="actions"><span class="muted">{props.lang || t("md.code")}</span><button type="button" class="link" onClick={copy}>{copied() ? t("md.copied") : t("md.copy")}</button></div>
      <pre tabindex="0"><Code lang={props.lang} text={props.v} /></pre>
    </div>
  );
}

const alignClass = (a: Align) => (a === "right" ? "al-right" : a === "center" ? "al-center" : undefined);

function TableBlock(props: { b: Extract<Block, { t: "table" }> }) {
  const [copied, setCopied] = createSignal(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(tableToTsv(props.b));
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* copying is a convenience; the cells are still selectable */
    }
  };
  return (
    <div class="tablebox">
      <div class="actions"><button type="button" class="link" onClick={copy}>{copied() ? t("md.copied") : t("md.copyTable")}</button></div>
      <div class="tablewrap" role="region" aria-label={t("md.table")} tabindex="0">
        <table>
          <thead>
            <tr><For each={props.b.head}>{(h, i) => <th scope="col" class={alignClass(props.b.align[i()] ?? null)}><Inlines v={h} /></th>}</For></tr>
          </thead>
          <tbody>
            <For each={props.b.rows}>{(r) => <tr><For each={r}>{(c, i) => <td class={alignClass(props.b.align[i()] ?? null)}><Inlines v={c} /></td>}</For></tr>}</For>
          </tbody>
        </table>
      </div>
    </div>
  );
}

/** Renders assistant text as formatted, safe elements. No HTML is ever inserted; see markdown.ts. */
export function Markdown(props: { text: string }) {
  const blocks = createMemo<Block[]>(() => parse(props.text));
  return (
    <div class="md">
      <For each={blocks()}>
        {(b) => (
          <Switch>
            <Match when={b.t === "p" && b}>{(x) => <p><Inlines v={x().v} /></p>}</Match>
            <Match when={b.t === "h" && b}>{(x) => <p><strong><Inlines v={x().v} /></strong></p>}</Match>
            <Match when={b.t === "quote" && b}>{(x) => <blockquote><Inlines v={x().v} /></blockquote>}</Match>
            <Match when={b.t === "code" && b}>{(x) => <CodeBlock lang={x().lang} v={x().v} />}</Match>
            <Match when={b.t === "table" && b}>{(x) => <TableBlock b={x() as Extract<Block, { t: "table" }>} />}</Match>
            <Match when={b.t === "ul" && b}>{(x) => <ul><For each={x().v}>{(li) => <li><Inlines v={li} /></li>}</For></ul>}</Match>
            <Match when={b.t === "ol" && b}>{(x) => <ol><For each={x().v}>{(li) => <li><Inlines v={li} /></li>}</For></ol>}</Match>
          </Switch>
        )}
      </For>
      <Show when={blocks().length === 0}><span /></Show>
    </div>
  );
}
