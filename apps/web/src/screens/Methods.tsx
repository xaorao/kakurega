import { LoadState } from "../components/LoadState";
import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type Method, type MethodVersion, type BundledMethod, type PackCatalog, type PackCatalogEntry } from "../api";
import { t, type Key, locale } from "../i18n";
import { hueOf, initials } from "../monogram";
import { when } from "../when";

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

/** The texts a method had before, newest first; any one can be brought back, and then waits for approval again. */
function History(props: { m: Method; onChanged: (message: string) => void }) {
  const [versions, setVersions] = createSignal<MethodVersion[] | null>(null);
  const [problem, setProblem] = createSignal("");
  const load = async () => {
    setProblem("");
    try {
      setVersions((await api.methodHistory(props.m.name)).versions);
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("met.error"));
    }
  };
  const earlier = () => (versions() ?? []).filter((v) => !v.current);
  return (
    <details class="history" onToggle={(e) => { if (e.currentTarget.open && versions() === null) void load(); }}>
      <summary>{t("met.history")}</summary>
      <Show when={versions()} fallback={<p class="muted">{t("review.loading")}</p>}>
        <Show when={earlier().length > 0} fallback={<p class="muted">{t("met.history.none")}</p>}>
          <ol class="plain stack">
            <For each={earlier()}>
              {(v) => (
                <li class="stack">
                  <p class="muted">{t("met.history.saved", { when: when(v.saved_at, locale()) })}{v.was_approved ? ` · ${t("met.history.wasApproved")}` : ""}</p>
                  <Show when={v.description !== props.m.description}><p>{v.description}</p></Show>
                  <pre class="plain-pre">{v.body}</pre>
                  <div class="actions">
                    <button type="button" onClick={async () => { setProblem(""); try { await api.restoreMethod(props.m.name, v.hash); setVersions(null); props.onChanged(t("met.history.restored")); } catch (e) { setProblem(e instanceof ApiError ? e.message : t("met.error")); } }}>{t("met.history.restore")}</button>
                  </div>
                </li>
              )}
            </For>
          </ol>
        </Show>
      </Show>
      <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
    </details>
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
      <Show when={open()}><History m={props.m} onChanged={props.onChanged} /></Show>
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
  // a publisher's signed catalog: read here, and each pack is then added from the exact file it lists
  const [catalogText, setCatalogText] = createSignal("");
  const [catalog, setCatalog] = createSignal<PackCatalog | null>(null);
  const openCatalog = async (file: File | undefined) => {
    if (!file) return;
    setProblem("");
    setCatalog(null);
    if (file.size > 300_000) { setProblem(t("pack.cat.tooLarge")); return; }
    const text = await file.text();
    try {
      setCatalog(await api.packCatalog(text));
      setCatalogText(text);
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("pack.error"));
    }
  };
  const addFromCatalog = (entry: PackCatalogEntry, file: File | undefined) => {
    if (!file) return;
    void act(async () => {
      if (file.size > 300_000) throw new ApiError(0, "PACK_REFUSED", t("pack.cat.tooLarge"));
      // the file's text exactly as it is: the catalog lists its SHA-256
      const r = await api.packImport(await file.text(), catalogText());
      setCatalog(await api.packCatalog(catalogText()));
      return importedSaid(r);
    });
  };
  return (
    <details class="card stack" id="packs-fold" aria-labelledby="h-pack">
      <summary><h3 id="h-pack">{t("pack.heading")}</h3></summary>
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
        <button class="primary" type="button" disabled={!pack().trim()} onClick={() => void act(async () => { const r = await api.packImport(pack().trim()); setPack(""); return importedSaid(r); })}>{t("pack.import")}</button>
      </div>
      <h4>{t("pack.cat.heading")}</h4>
      <p class="muted">{t("pack.cat.lead")}</p>
      <input id="pack-catalog" type="file" aria-label={t("pack.cat.open")} onChange={(e) => { void openCatalog(e.currentTarget.files?.[0]); e.currentTarget.value = ""; }} />
      <Show when={catalog()}>
        {(c) => (
          <div class="stack" id="pack-catalog-list">
            <p>{t("pack.cat.from", { publisher: c().publisher || c().key_id.slice(0, 20), date: when(c().not_after, locale(), { dateOnly: true }) })}</p>
            <ul class="plain stack" aria-label={t("pack.cat.heading")}>
              <For each={c().packs}>
                {(e) => (
                  <li class="card stack">
                    <p><strong>{e.name}</strong> {e.version} <span class="chip">{t(`pack.cat.state.${e.state}` as Key, { have: e.have ?? "" })}</span></p>
                    <Show when={e.description}><p>{e.description}</p></Show>
                    <p class="muted">{t("pack.cat.holds", { list: e.methods.join(", ") })}</p>
                    <p class="muted">{t("pack.cat.file", { file: e.file })} <code>{e.sha256.slice(0, 16)}</code></p>
                    <Show when={e.state === "new" || e.state === "update_available"}>
                      <label class="muted" for={`pack-cat-${e.name}`}>{t("pack.cat.choose", { file: e.file })}</label>
                      <input id={`pack-cat-${e.name}`} type="file" onChange={(ev) => { addFromCatalog(e, ev.currentTarget.files?.[0]); ev.currentTarget.value = ""; }} />
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </div>
        )}
      </Show>
      <Show when={(data()?.imported ?? []).length > 0}>
        <p class="muted">{t("pack.imported.list")}: {(data()?.imported ?? []).map((i) => `${i.name} ${i.version}`).join(", ")}</p>
      </Show>
      <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
    </details>
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
                <p><span class="chip">{p.job.startsWith("chat:") ? t("met.prop.fromChat") : t("met.prop.fromReview")}</span></p>
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
    <section class="stack" id="bundled-methods" aria-labelledby="h-bundled">
      <h3 id="h-bundled">{t("met.bundled.heading", { n: props.list.length })}</h3>
      <p class="muted">{t("met.bundled.lead")}</p>
      <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
      <For each={groups()}>
        {(g) => (
          <div class="rt-group">
            <h4>{g.group}</h4>
            <ul class="plain gallery">
              <For each={g.items}>
                {(b) => (
                  <li class="skill-card" data-bundled={b.name}>
                    <span class={`monogram hue-${hueOf(b.name)}`} aria-hidden="true">{initials(b.name)}</span>
                    <span class="skill-text"><strong>{b.name}</strong><span class="muted">{b.description}</span></span>
                    <span class="skill-action">
                      <Show when={!b.added} fallback={<span class="chip">{t("met.bundled.have")}</span>}>
                        <button type="button" aria-label={t("met.bundled.addLabel", { name: b.name })} onClick={() => void add(b.name)}>{t("met.bundled.add")}</button>
                      </Show>
                    </span>
                  </li>
                )}
              </For>
            </ul>
          </div>
        )}
      </For>
    </section>
  );
}

/** What an import did, in words: the methods it brought, and the outside programs it only suggests. */
const importedSaid = (r: { pack: string; version: string; imported: string[]; suggested_programs?: string[] }) => {
  const said = t("pack.imported", { n: r.imported.length, name: r.pack, version: r.version });
  const programs = r.suggested_programs ?? [];
  // the outside-tools list on the same screen shows what the pack suggests at once
  if (programs.length > 0) window.dispatchEvent(new CustomEvent("kk:pack-suggested"));
  return programs.length > 0 ? `${said} ${t("pack.suggests", { n: programs.length, names: programs.join(", ") })}` : said;
};

export function Methods() {
  const [data, { refetch }] = createResource(() => api.methods().catch(() => null));
  const [status, setStatus] = createSignal("");
  const [writing, setWriting] = createSignal(false);
  // said once the list shows the change
  const changed = (message: string) => { void (async () => { await refetch(); setStatus(message); })(); };
  return (
    <div class="card stack" aria-labelledby="h-met">
      <h2 id="h-met">{t("met.heading")}</h2>
      <p class="muted">{t("met.lead")}</p>
      <p class="muted">{t("met.limits")}</p>
      <Show when={status()}><p role="status" class="ok">{status()}</p></Show>
      <Proposals onChanged={changed} />
      <Show when={data()} fallback={<LoadState loading={data.loading} retry={() => void refetch()} />}>
        {(d) => (
          <>
            <Show when={d().methods.length === 0}><p class="muted">{t("met.none")}</p></Show>
            <ul class="plain stack"><For each={d().methods}>{(m) => <Row m={m} onChanged={changed} />}</For></ul>
            <Show when={writing()} fallback={<div class="actions"><button type="button" id="met-new-open" onClick={() => { setWriting(true); queueMicrotask(() => document.getElementById("met-new-name")?.focus()); }}>{t("met.write")}</button></div>}>
              <section class="card stack" aria-labelledby="h-met-new">
                <h3 id="h-met-new">{t("met.new")}</h3>
                <Form onDone={(m) => { setWriting(false); changed(m); }} onCancel={() => setWriting(false)} />
              </section>
            </Show>
            <Show when={(d().bundled ?? []).length > 0}><Bundled list={d().bundled ?? []} onChanged={changed} /></Show>
            {/* packs from publishers come last and folded: most people write their own or take one from the shelf */}
            <Packs onChanged={changed} />
          </>
        )}
      </Show>
    </div>
  );
}
