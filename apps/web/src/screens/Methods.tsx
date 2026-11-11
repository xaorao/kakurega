import { LoadState } from "../components/LoadState";
import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type Method, type BundledMethod } from "../api";
import { t } from "../i18n";

const words = (text: string) => text.split(",").map((w) => w.trim()).filter(Boolean);

/** The form that writes or changes one method. A saved method always waits for the owner's approval. */
function Form(props: { initial?: Method; onDone: (message: string) => void; onCancel?: () => void }) {
  const m = props.initial;
  const [name, setName] = createSignal(m?.name ?? "");
  const [description, setDescription] = createSignal(m?.description ?? "");
  const [triggers, setTriggers] = createSignal((m?.triggers ?? []).join(", "));
  const [avoid, setAvoid] = createSignal((m?.avoid ?? []).join(", "));
  const [body, setBody] = createSignal(m?.body ?? "");
  const [problem, setProblem] = createSignal("");
  const id = (k: string) => `met-${m?.name ?? "new"}-${k}`;
  const save = async () => {
    setProblem("");
    try {
      await api.saveMethod({ name: name().trim(), description: description().trim(), triggers: words(triggers()), avoid: words(avoid()), body: body() });
      props.onDone(t("met.saved"));
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("met.error"));
    }
  };
  return (
    <div class="stack">
      <label for={id("name")}>{t("met.name")}</label>
      <input id={id("name")} value={name()} maxlength="64" disabled={!!m} onInput={(e) => setName(e.currentTarget.value)} />
      <label for={id("desc")}>{t("met.description")}</label>
      <input id={id("desc")} value={description()} maxlength="250" onInput={(e) => setDescription(e.currentTarget.value)} />
      <label for={id("trig")}>{t("met.triggersLabel")}</label>
      <input id={id("trig")} value={triggers()} onInput={(e) => setTriggers(e.currentTarget.value)} />
      <label for={id("avoid")}>{t("met.avoidLabel")}</label>
      <input id={id("avoid")} value={avoid()} onInput={(e) => setAvoid(e.currentTarget.value)} />
      <label for={id("body")}>{t("met.body")}</label>
      <textarea id={id("body")} rows="6" maxlength="8000" value={body()} onInput={(e) => setBody(e.currentTarget.value)} />
      <div class="actions">
        <button class="primary" type="button" disabled={!name().trim() || !body().trim()} onClick={() => void save()}>{t("met.save")}</button>
        <Show when={props.onCancel}><button type="button" onClick={() => props.onCancel?.()}>{t("met.cancel")}</button></Show>
      </div>
      <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
    </div>
  );
}

function Row(props: { m: Method; onChanged: (message: string) => void }) {
  const [open, setOpen] = createSignal(false);
  const [editing, setEditing] = createSignal(false);
  const [problem, setProblem] = createSignal("");
  const run = async (fn: () => Promise<unknown>, message: string) => {
    setProblem("");
    try {
      await fn();
      props.onChanged(message);
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("met.error"));
    }
  };
  return (
    <li class="card stack">
      <h3>{props.m.name}</h3>
      <Show when={props.m.description}><p>{props.m.description}</p></Show>
      <p class={props.m.approved ? "ok" : "muted"} role="status">{props.m.approved ? t("met.approved") : t("met.waiting")}</p>
      <p class="muted">{props.m.triggers.length ? t("met.triggers", { list: props.m.triggers.join(", ") }) : t("met.noTriggers", { name: props.m.name })}</p>
      <Show when={props.m.avoid.length}><p class="muted">{t("met.avoid", { list: props.m.avoid.join(", ") })}</p></Show>
      <Show when={open() && !editing()}><pre class="plain-pre">{props.m.body}</pre></Show>
      <Show when={editing()}>
        <Form initial={props.m} onDone={(msg) => { setEditing(false); props.onChanged(msg); }} onCancel={() => setEditing(false)} />
      </Show>
      <div class="actions">
        <button type="button" onClick={() => setOpen(!open())}>{open() ? t("met.hide") : t("met.show")}</button>
        <button type="button" onClick={() => { setEditing(!editing()); setOpen(true); }}>{t("met.edit")}</button>
        <Show when={!props.m.approved && open()}>
          <button class="primary" type="button" onClick={() => void run(() => api.approveMethod(props.m.name, props.m.hash), t("met.approvedNow"))}>{t("met.approve")}</button>
        </Show>
        <Show when={props.m.approved}>
          <button type="button" onClick={() => void run(() => api.revokeMethod(props.m.name), t("met.revoked"))}>{t("met.revoke")}</button>
        </Show>
        <button type="button" onClick={() => void run(() => api.removeMethod(props.m.name), t("met.removed"))}>{t("met.remove")}</button>
      </div>
      <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
    </li>
  );
}

function Packs(props: { onChanged: (message: string) => void }) {
  const [data, { refetch }] = createResource(() => api.packs());
  const [key, setKey] = createSignal("");
  const [label, setLabel] = createSignal("");
  const [pack, setPack] = createSignal("");
  const [problem, setProblem] = createSignal("");
  const act = async (fn: () => Promise<string | void>) => {
    setProblem("");
    try {
      const message = await fn();
      await refetch();
      props.onChanged(message ?? "");
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("pack.error"));
    }
  };
  const choose = async (file: File | undefined) => {
    if (file && file.size <= 300_000) setPack(await file.text());
  };
  return (
    <section class="card stack" aria-labelledby="h-pack">
      <h3 id="h-pack">{t("pack.heading")}</h3>
      <p class="muted">{t("pack.lead")}</p>
      <h4>{t("pack.publishers")}</h4>
      <Show when={(data()?.publishers ?? []).length === 0}><p class="muted">{t("pack.noPublishers")}</p></Show>
      <ul class="plain stack">
        <For each={data()?.publishers ?? []}>
          {(p) => (
            <li class="stack">
              <p><strong>{p.label}</strong> <code>{p.key_id.slice(0, 20)}</code></p>
              <div class="actions"><button type="button" onClick={() => void act(async () => { await api.packUnpin(p.key_id); return t("pack.unpinned"); })}>{t("pack.unpin")}</button></div>
            </li>
          )}
        </For>
      </ul>
      <label for="pack-key">{t("pack.key")}</label>
      <input id="pack-key" value={key()} onInput={(e) => setKey(e.currentTarget.value)} />
      <label for="pack-label">{t("pack.label")}</label>
      <input id="pack-label" value={label()} maxlength="80" onInput={(e) => setLabel(e.currentTarget.value)} />
      <div class="actions">
        <button type="button" disabled={!key().trim() || !label().trim()} onClick={() => void act(async () => { await api.packPin(key().trim(), label().trim()); setKey(""); setLabel(""); return t("pack.pinned"); })}>{t("pack.pin")}</button>
      </div>
      <label for="pack-text">{t("pack.paste")}</label>
      <textarea id="pack-text" rows="4" value={pack()} onInput={(e) => setPack(e.currentTarget.value)} />
      <input type="file" aria-label={t("pack.paste")} onChange={(e) => void choose(e.currentTarget.files?.[0])} />
      <div class="actions">
        <button class="primary" type="button" disabled={!pack().trim()} onClick={() => void act(async () => { const r = await api.packImport(pack().trim()); setPack(""); return t("pack.imported", { n: r.imported.length, name: r.pack, version: r.version }); })}>{t("pack.import")}</button>
      </div>
      <Show when={(data()?.imported ?? []).length > 0}>
        <p class="muted">{t("pack.imported.list")}: {(data()?.imported ?? []).map((i) => `${i.name} ${i.version}`).join(", ")}</p>
      </Show>
      <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
    </section>
  );
}

function Proposals(props: { onChanged: (message: string) => void }) {
  const [data, { refetch }] = createResource(() => api.methodProposals());
  const [problem, setProblem] = createSignal("");
  const act = async (fn: () => Promise<unknown>, message: string) => {
    setProblem("");
    try {
      await fn();
      await refetch();
      props.onChanged(message);
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("met.error"));
    }
  };
  return (
    <Show when={(data()?.proposals ?? []).length > 0}>
      <section class="card stack" aria-labelledby="h-met-prop">
        <h3 id="h-met-prop">{t("met.prop.heading")}</h3>
        <p class="muted">{t("met.prop.lead")}</p>
        <ul class="plain stack">
          <For each={data()?.proposals ?? []}>
            {(p) => (
              <li class="card stack">
                <h4>{p.name}</h4>
                <Show when={p.description}><p>{p.description}</p></Show>
                <Show when={p.triggers.length}><p class="muted">{t("met.triggers", { list: p.triggers.join(", ") })}</p></Show>
                <pre class="plain-pre">{p.body}</pre>
                <div class="actions">
                  <button class="primary" type="button" onClick={() => void act(() => api.keepProposal(p.id), t("met.prop.kept"))}>{t("met.prop.keep")}</button>
                  <button type="button" onClick={() => void act(() => api.dismissProposal(p.id), t("met.prop.dismissed"))}>{t("met.prop.dismiss")}</button>
                </div>
              </li>
            )}
          </For>
        </ul>
        <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
      </section>
    </Show>
  );
}

/** The methods that ship with the program, by group. Adding one puts it in the list above, still waiting for approval. */
function Bundled(props: { list: BundledMethod[]; onChanged: (message: string) => void }) {
  const [problem, setProblem] = createSignal("");
  const groups = () => {
    const out: { group: string; items: BundledMethod[] }[] = [];
    for (const b of props.list) {
      const g = out.find((x) => x.group === b.group);
      if (g) g.items.push(b);
      else out.push({ group: b.group, items: [b] });
    }
    return out;
  };
  const add = async (name: string) => {
    setProblem("");
    try {
      await api.addBundledMethod(name);
      props.onChanged(t("met.bundled.added", { name }));
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("met.error"));
    }
  };
  return (
    <details class="team-allow" id="bundled-methods">
      <summary>{t("met.bundled.heading", { n: props.list.length })}</summary>
      <p class="muted">{t("met.bundled.lead")}</p>
      <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
      <For each={groups()}>
        {(g) => (
          <div class="rt-group">
            <h4>{g.group}</h4>
            <ul class="plain bundled-list">
              <For each={g.items}>
                {(b) => (
                  <li class="work-item" data-bundled={b.name}>
                    <span class="work-what"><strong>{b.name}</strong> <span class="muted">{b.description}</span></span>
                    <Show when={!b.added} fallback={<span class="chip">{t("met.bundled.have")}</span>}>
                      <button type="button" onClick={() => void add(b.name)}>{t("met.bundled.add")}</button>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </div>
        )}
      </For>
    </details>
  );
}

export function Methods() {
  const [data, { refetch }] = createResource(() => api.methods().catch(() => null));
  const [status, setStatus] = createSignal("");
  const changed = (message: string) => { setStatus(message); void refetch(); };
  return (
    <div class="card stack" aria-labelledby="h-met">
      <h2 id="h-met">{t("met.heading")}</h2>
      <p class="muted">{t("met.lead")}</p>
      <p class="muted">{t("met.limits")}</p>
      <Show when={status()}><p role="status" class="ok">{status()}</p></Show>
      <Proposals onChanged={changed} />
      <Packs onChanged={changed} />
      <Show when={data()} fallback={<LoadState loading={data.loading} retry={() => void refetch()} />}>
        {(d) => (
          <>
            <Show when={d().methods.length === 0}><p class="muted">{t("met.none")}</p></Show>
            <ul class="plain stack"><For each={d().methods}>{(m) => <Row m={m} onChanged={changed} />}</For></ul>
            <Show when={(d().bundled ?? []).length > 0}><Bundled list={d().bundled ?? []} onChanged={changed} /></Show>
            <h3>{t("met.new")}</h3>
            <Form onDone={changed} />
          </>
        )}
      </Show>
    </div>
  );
}
