import { againPoint, editPoint, type BranchPoint } from "../branching";
import { carriesFiles, fileNotes, fingerprint, pastedFiles, room, sizeText, MAX_BYTES } from "../attach";
import { findMatches, stepTo, type Match } from "../find";
import { batch, createEffect, createResource, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { api, ApiError, streamChat, type AttachmentRead, type AttachmentSummary, type ChatHit, type ChatTurn, type KeptChat, type GatewayProofResult, type ModelProof, type ResearchPage, type ResearchProposal, type SearchHit, type SearchProposal } from "../api";
import { t } from "../i18n";
import { Markdown } from "../components/Markdown";
import { HistoryPicker } from "../components/HistoryPicker";
import { draftKey, exchangeWasSaved, opensHistoryPicker, forgetDraft, forgetSent, historyKey, historyStep, idleHistory, loadDraft, rememberSent, retagSent, saveDraft, sentHistory, stopsReply } from "../composer";
import { meterLine, noteNotes, pageNotes, savedLine, summaryNotes } from "../meter";

interface Shown extends ChatTurn {
  attached?: string[];
  foot?: string[];
  notes?: { id: string; text: string; why: string; untrusted: boolean; project?: string | null }[];
}

export function Chat() {
  const [turns, setTurns] = createSignal<Shown[]>([]);
  const [text, setTextRaw] = createSignal(loadDraft(draftKey(null)));
  const [busy, setBusy] = createSignal(false);
  const [picking, setPicking] = createSignal(false);
  const [recovered, setRecovered] = createSignal(false);
  const [live, setLive] = createSignal("");
  let abort: AbortController | null = null;
  const [error, setError] = createSignal("");
  const [session, setSession] = createSignal<string | null>(null);
  // the project this conversation is about; its notes are offered, and no other project's
  const [project, setProject] = createSignal("");
  const [confirmForget, setConfirmForget] = createSignal<string | null>(null);
  // The box holds the draft of the conversation on screen. Each conversation keeps its own while the page is open.
  const setText = (v: string) => {
    setTextRaw(v);
    saveDraft(draftKey(session()), v);
  };
  // Bumped whenever another conversation is put on screen, so a reply that arrives late never lands in the wrong one.
  let view = 0;
  // Follow new text only while the owner is at the bottom of the page. Scrolling up stops it; sending, opening a
  // conversation or Jump to the latest starts it again (OpenCode session/timeline/interaction.ts, pin and unpin).
  let pinned = true;
  // Where Up and Down are in the messages sent from this page.
  let hist = idleHistory();
  let box: HTMLTextAreaElement | undefined;
  // What the model is sent in place of the oldest messages of the open conversation, so the owner can read it.
  const [summaryText, setSummaryText] = createSignal("");
  // The other versions of the open conversation (branched at the same message), and the message it was branched at.
  const [branches, setBranches] = createSignal<{ id: string; title: string; fork_at: number | null; turns: number }[]>([]);
  const [forkAt, setForkAt] = createSignal<number | null>(null);
  const [copiedAt, setCopiedAt] = createSignal<number | null>(null);
  // Suggestions read from the open conversation, when the owner asked; each waits for a decision.
  const [suggested, setSuggested] = createSignal<{ id: string; text: string; why: string; untrusted: boolean }[]>([]);
  const [notesBusy, setNotesBusy] = createSignal(false);
  const [notesInfo, setNotesInfo] = createSignal("");
  // Files attached to the message being written: read on this computer, held in memory, sent with the next message.
  const [files, setFiles] = createSignal<AttachmentSummary[]>([]);
  const [fileInfo, setFileInfo] = createSignal("");
  const [reading, setReading] = createSignal("");
  const [shown, setShown] = createSignal<Record<string, AttachmentRead | "loading">>({});
  const [dragging, setDragging] = createSignal(false);
  // Find in the open conversation: matches are read from what is on the screen, so they always agree with it.
  const [findText, setFindText] = createSignal("");
  const [findAt, setFindAt] = createSignal<number | null>(null);
  const [found, setFound] = createSignal<Match[]>([]);
  let listEl: HTMLUListElement | undefined;
  const bodies = () => Array.from(listEl?.querySelectorAll<HTMLElement>(".msg-body") ?? []);
  const rangeFor = (el: HTMLElement, start: number, end: number): Range | null => {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let pos = 0;
    let range: Range | null = null;
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const len = (n.nodeValue ?? "").length;
      if (!range && start < pos + len) {
        range = document.createRange();
        range.setStart(n, start - pos);
      }
      if (range && end <= pos + len) {
        range.setEnd(n, end - pos);
        return range;
      }
      pos += len;
    }
    return null;
  };
  const paintFind = (matches: Match[], current: number | null) => {
    const reg = (globalThis as unknown as { CSS?: { highlights?: Map<string, unknown> } }).CSS?.highlights;
    const H = (globalThis as unknown as { Highlight?: new (...r: Range[]) => unknown }).Highlight;
    if (!reg || !H) return;
    const els = bodies();
    const ranges = matches.map((m) => {
      const el = els[m.turn];
      return el ? rangeFor(el, m.start, m.end) : null;
    });
    reg.set("find", new H(...ranges.filter((r): r is Range => r !== null)));
    const cur = current === null ? null : (ranges[current] ?? null);
    if (cur) reg.set("find-current", new H(cur));
    else reg.delete("find-current");
    if (cur) cur.startContainer.parentElement?.scrollIntoView({ block: "center" });
  };
  const runFind = (q: string, keep = false) => {
    setFindText(q);
    const matches = findMatches(bodies().map((b) => b.textContent ?? ""), q);
    setFound(matches);
    const at = matches.length === 0 ? null : keep && findAt() !== null && findAt()! < matches.length ? findAt() : 0;
    setFindAt(at);
    paintFind(matches, at);
  };
  const stepFind = (step: 1 | -1) => {
    const at = stepTo(found().length, findAt(), step);
    setFindAt(at);
    paintFind(found(), at);
  };
  // New messages and streamed text change what is on the screen, so an open search is read again after each change.
  createEffect(() => {
    turns();
    live();
    if (findText().trim() !== "") queueMicrotask(() => runFind(findText(), true));
  });
  const clearFind = () => {
    setFindText("");
    setFound([]);
    setFindAt(null);
    const reg = (globalThis as unknown as { CSS?: { highlights?: Map<string, unknown> } }).CSS?.highlights;
    reg?.delete("find");
    reg?.delete("find-current");
  };
  let picker: HTMLInputElement | undefined;
  // what each attached file is (its name and bytes), so the same file is not attached twice
  const attachedKeys = new Map<string, string>();
  const addFiles = async (incoming: File[]) => {
    if (incoming.length === 0) return;
    const { take, over } = room(files().length, incoming);
    const problems: string[] = over > 0 ? [t("files.tooMany", { n: over })] : [];
    for (const f of take) {
      if (f.size > MAX_BYTES) {
        problems.push(t("files.tooBig", { name: f.name, max: sizeText(MAX_BYTES) }));
        continue;
      }
      const key = await fingerprint(f);
      if ([...attachedKeys.values()].includes(key)) {
        problems.push(t("files.duplicate", { name: f.name }));
        continue;
      }
      setReading(t("files.reading", { name: f.name }));
      try {
        const a = await api.attachFile(f);
        attachedKeys.set(a.id, key);
        setFiles([...files(), a]);
      } catch (err) {
        problems.push(t("files.error", { name: f.name, why: err instanceof ApiError ? err.message : t("chat.error") }));
      }
    }
    setReading("");
    setFileInfo(problems.join(" "));
  };
  const dropFile = async (id: string) => {
    attachedKeys.delete(id);
    setFiles(files().filter((f) => f.id !== id));
    setShown((m) => {
      const { [id]: _gone, ...rest } = m;
      return rest;
    });
    try {
      await api.removeAttachment(id);
    } catch {
      /* it expires by itself; it is already gone from this message */
    }
  };
  const toggleRead = async (id: string) => {
    if (shown()[id]) {
      setShown((m) => {
        const { [id]: _gone, ...rest } = m;
        return rest;
      });
      return;
    }
    setShown((m) => ({ ...m, [id]: "loading" }));
    try {
      const r = await api.attachment(id);
      setShown((m) => ({ ...m, [id]: r }));
    } catch (err) {
      setShown((m) => {
        const { [id]: _gone, ...rest } = m;
        return rest;
      });
      setFileInfo(err instanceof ApiError ? err.message : t("chat.error"));
    }
  };
  const loadSuggested = async (id: string) => {
    try {
      const r = await api.suggestedNotes(id);
      if (session() === id) setSuggested(r.suggestions);
    } catch {
      /* the suggestions are a convenience; the conversation is unaffected */
    }
  };
  const findNotes = async () => {
    const id = session();
    if (!id || notesBusy()) return;
    setNotesBusy(true);
    setNotesInfo("");
    try {
      const r = await api.suggestNotes(id);
      await loadSuggested(id);
      const left = r.dropped.instruction_like + r.dropped.secret;
      setNotesInfo(r.added === 0 ? t("chat.notesNone") : t("chat.notesFound", { n: r.added }) + (left > 0 ? " " + t("chat.notesDropped", { n: left }) : ""));
    } catch (err) {
      setNotesInfo(err instanceof ApiError ? err.message : t("chat.notesError"));
    } finally {
      setNotesBusy(false);
    }
  };
  const decideNote = async (sid: string, keepIt: boolean) => {
    const id = session();
    if (!id) return;
    try {
      if (keepIt) await api.acceptSuggestedNote(id, sid);
      else await api.dismissSuggestedNote(id, sid);
      setSuggested((list) => list.filter((x) => x.id !== sid));
      setNotesInfo(keepIt ? t("chat.notesKept") : "");
    } catch (err) {
      setNotesInfo(err instanceof ApiError ? err.message : t("chat.notesError"));
    }
  };
  const show = (next: string | null, nextTurns: Shown[], summary = "") => {
    if (busy()) abort?.abort();
    view++;
    hist = idleHistory();
    pinned = true;
    batch(() => {
      setSession(next);
      setTurns(nextTurns);
      setSummaryText(summary);
      setBranches([]);
      setForkAt(null);
      setSuggested([]);
      setNotesInfo("");
      setTextRaw(loadDraft(draftKey(next)));
    });
  };
  // After a reply, the cursor goes back to the box, unless the owner has moved on to something else or is on a phone.
  const returnFocus = () => {
    if (window.matchMedia("(max-width: 767px)").matches) return;
    const a = document.activeElement;
    if (a && a !== document.body && !box?.form?.contains(a)) return;
    box?.focus({ preventScroll: !pinned });
  };
  const [unseen, setUnseen] = createSignal(false);
  const atBottom = () => document.documentElement.scrollHeight - window.innerHeight - window.scrollY <= 80;
  let lastY = window.scrollY;
  const onScroll = () => {
    const y = window.scrollY;
    if (atBottom()) {
      pinned = true;
      setUnseen(false);
    } else if (y < lastY - 2) {
      // only the owner moves the page up; text growing below never unpins
      pinned = false;
    }
    lastY = y;
  };
  window.addEventListener("scroll", onScroll, { passive: true });
  onCleanup(() => window.removeEventListener("scroll", onScroll));
  // A file dropped anywhere on the page is attached here; without this the browser would open it in place of the page and the
  // message being written would be lost. Only drags that carry files count, so dragging selected text shows no drop target.
  onMount(() => {
    const over = (e: DragEvent) => {
      if (!carriesFiles(e.dataTransfer?.types)) return;
      e.preventDefault();
      setDragging(true);
    };
    const leave = (e: DragEvent) => {
      if (!e.relatedTarget) setDragging(false);
    };
    const drop = (e: DragEvent) => {
      if (!carriesFiles(e.dataTransfer?.types)) return;
      e.preventDefault();
      setDragging(false);
      if (!busy()) void addFiles(Array.from(e.dataTransfer?.files ?? []));
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") setDragging(false);
    };
    document.addEventListener("dragover", over);
    document.addEventListener("dragleave", leave);
    document.addEventListener("drop", drop);
    document.addEventListener("keydown", esc);
    onCleanup(() => {
      document.removeEventListener("dragover", over);
      document.removeEventListener("dragleave", leave);
      document.removeEventListener("drop", drop);
      document.removeEventListener("keydown", esc);
    });
  });
  const toEnd = () => window.scrollTo({ top: document.documentElement.scrollHeight, behavior: "auto" });
  let frame = 0;
  createEffect(
    on(
      [turns, live],
      () => {
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(() => {
          if (pinned) toEnd();
          else if (!atBottom()) setUnseen(true);
        });
      },
      { defer: true },
    ),
  );
  onCleanup(() => cancelAnimationFrame(frame));
  const jump = () => {
    pinned = true;
    setUnseen(false);
    toEnd();
  };
  const [showArchived, setShowArchived] = createSignal(false);
  // the source is an object so that `false` (do not show archived) still counts as a value to fetch for
  const [kept, { refetch, mutate }] = createResource<KeptChat[], { archived: boolean }>(() => ({ archived: showArchived() }), async ({ archived }) => {
    try {
      return (await api.keptChats(archived)).sessions;
    } catch {
      return [];
    }
  });
  const keep = async () => {
    setError("");
    try {
      const r = await api.keepChat("", project().trim() || undefined);
      const carried = text();
      forgetDraft(draftKey(session()));
      retagSent(draftKey(session()), draftKey(r.id));
      setSession(r.id);
      saveDraft(draftKey(r.id), carried);
      await refetch();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("chat.error"));
    }
  };
  // search across kept conversations: the newest question wins, an answer to an older one is dropped
  const [chatQuery, setChatQuery] = createSignal("");
  const [chatHits, setChatHits] = createSignal<ChatHit[] | null>(null);
  const [renaming, setRenaming] = createSignal("");
  const [renameText, setRenameText] = createSignal("");
  const [keptProblem, setKeptProblem] = createSignal("");
  let searchSeq = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  const onSearch = (value: string) => {
    setChatQuery(value);
    clearTimeout(searchTimer);
    const mine = ++searchSeq;
    if (!value.trim()) {
      setChatHits(null);
      return;
    }
    searchTimer = setTimeout(async () => {
      try {
        const r = await api.searchChats(value.trim());
        if (mine === searchSeq) setChatHits(r.results);
      } catch {
        if (mine === searchSeq) setChatHits([]);
      }
    }, 200);
  };
  const doRename = async (id: string) => {
    setKeptProblem("");
    try {
      await api.renameChat(id, renameText());
      setRenaming("");
      await refetch();
      if (chatQuery().trim()) onSearch(chatQuery());
    } catch (e) {
      setKeptProblem(e instanceof ApiError ? e.message : t("chat.renameError"));
    }
  };
  const openKept = async (id: string) => {
    setError("");
    try {
      const r = await api.keptChat(id);
      setProject(r.project ?? "");
      show(id, r.turns.map(({ role, content }) => ({ role, content })), r.summary?.text ?? "");
      setBranches(r.branches ?? []);
      setSuggested([]);
      setNotesInfo("");
      void loadSuggested(id);
      setForkAt(r.parent ? (r.fork_at ?? null) : null);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("chat.error"));
    }
  };
  // Branch the open kept conversation at a message. The original stays as it is; the branch is opened, and the message goes
  // back into the box (edit) or is sent again (answer again).
  const branchAt = async (p: BranchPoint | null, again: boolean) => {
    const from = session();
    if (!p || !from || busy()) return;
    setError("");
    try {
      const f = await api.forkChat(from, p.at);
      await openKept(f.id);
      setText(p.text);
      await refetch();
      if (again) await send();
      else box?.focus();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("chat.branchError"));
    }
  };
  const copyTurn = async (i: number, content: string) => {
    try {
      await navigator.clipboard.writeText(content);
      setCopiedAt(i);
      setTimeout(() => setCopiedAt((c) => (c === i ? null : c)), 1500);
    } catch {
      /* the browser refused the clipboard; nothing was copied and nothing is claimed */
    }
  };
  const flagKept = async (id: string, what: "pin" | "archive", on: boolean) => {
    setKeptProblem("");
    try {
      if (what === "pin") await api.pinChat(id, on);
      else await api.archiveChat(id, on);
      await refetch();
    } catch (e) {
      setKeptProblem(e instanceof ApiError ? e.message : t("chat.renameError"));
    }
  };
  const forget = async (id: string) => {
    if (confirmForget() !== id) {
      setConfirmForget(id);
      return;
    }
    setConfirmForget(null);
    try {
      await api.forgetChat(id);
      // the list and the screen change together: the forgotten conversation is never listed once its text is gone
      batch(() => {
        mutate((list) => (list ?? []).filter((k) => k.id !== id));
        forgetDraft(draftKey(id));
        forgetSent(draftKey(id));
        hist = idleHistory();
        if (session() === id) show(null, []);
      });
      await refetch();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("chat.error"));
    }
  };
  const [proofModel, setProofModel] = createSignal("Qwen/Qwen3.8-27B");
  const [proof, setProof] = createSignal<ModelProof | null>(null);
  const [gateway, setGateway] = createSignal<GatewayProofResult | null>(null);
  const [proofBusy, setProofBusy] = createSignal(false);
  const [proofError, setProofError] = createSignal("");
  const checkProof = async () => {
    setProofBusy(true);
    setProofError("");
    setProof(null);
    try {
      setProof(await api.attestModel(proofModel().trim()));
    } catch (err) {
      setProofError(err instanceof ApiError ? err.message : t("proof.error"));
    } finally {
      setProofBusy(false);
    }
  };
  const checkGateway = async () => {
    setProofBusy(true);
    setProofError("");
    setGateway(null);
    try {
      setGateway(await api.attestGateway());
    } catch (err) {
      setProofError(err instanceof ApiError ? err.message : t("proof.error"));
    } finally {
      setProofBusy(false);
    }
  };
  const [urls, setUrls] = createSignal("");
  const [proposal, setProposal] = createSignal<ResearchProposal | null>(null);
  const [pages, setPages] = createSignal<ResearchPage[]>([]);
  const [webBusy, setWebBusy] = createSignal(false);
  const [webError, setWebError] = createSignal("");
  const propose = async () => {
    setWebBusy(true);
    setWebError("");
    setProposal(null);
    try {
      const list = urls().split(/\s+/).filter(Boolean);
      setProposal(await api.researchPropose(list));
    } catch (err) {
      setWebError(err instanceof ApiError ? err.message : t("web.error"));
    } finally {
      setWebBusy(false);
    }
  };
  const fetchApproved = async () => {
    const p = proposal();
    if (!p) return;
    setWebBusy(true);
    setWebError("");
    try {
      const r = await api.researchFetch(p.id, p.digest);
      setPages([...pages(), ...r.pages]);
      setProposal(null);
      setUrls("");
    } catch (err) {
      setWebError(err instanceof ApiError ? err.message : t("web.error"));
    } finally {
      setWebBusy(false);
    }
  };
  const [searchOn, setSearchOn] = createSignal<{ available: boolean; provider?: string } | null>(null);
  api.searchStatus().then(setSearchOn).catch(() => setSearchOn({ available: false }));
  const [words, setWords] = createSignal("");
  const [recency, setRecency] = createSignal("");
  const windows = { day: "web.recency.day", week: "web.recency.week", month: "web.recency.month", year: "web.recency.year" } as const;
  const windowText = (w: string | null) => (w && w in windows ? t(windows[w as keyof typeof windows]) : "");
  const [searchProposal, setSearchProposal] = createSignal<SearchProposal | null>(null);
  const [hits, setHits] = createSignal<SearchHit[]>([]);
  const [searchNote, setSearchNote] = createSignal<{ why: string; engines: string[] } | null>(null);
  const showSearch = async () => {
    setWebBusy(true);
    setWebError("");
    setSearchProposal(null);
    setHits([]);
    setSearchNote(null);
    try {
      setSearchProposal(await api.searchPropose(words(), recency()));
    } catch (err) {
      setWebError(err instanceof ApiError ? err.message : t("web.error"));
    } finally {
      setWebBusy(false);
    }
  };
  const sendSearch = async () => {
    const p = searchProposal();
    if (!p) return;
    setWebBusy(true);
    setWebError("");
    try {
      const answer = await api.searchRun(p.id, p.digest);
      setHits(answer.results);
      setSearchNote(answer.results.length === 0 ? { why: answer.empty_because ?? "", engines: answer.engines_failed } : null);
      setSearchProposal(null);
    } catch (err) {
      setWebError(err instanceof ApiError ? err.message : t("web.error"));
    } finally {
      setWebBusy(false);
    }
  };
  const send = async (e?: Event) => {
    e?.preventDefault();
    const message = text().trim();
    if (!message || busy()) return;
    const myView = view;
    const sentKey = draftKey(session());
    rememberSent(message, sentKey);
    hist = idleHistory();
    pinned = true;
    setUnseen(false);
    const history: ChatTurn[] = turns().slice(-18).map(({ role, content }) => ({ role, content }));
    const turnsBefore = turns().length;
    const attached = files();
    setTurns([...turns(), { role: "user", content: message, attached: attached.map((f) => f.name) }]);
    setText("");
    setBusy(true);
    setError("");
    setRecovered(false);
    setLive("");
    abort = new AbortController();
    try {
      const r = await streamChat({ message, history, ...(attached.length > 0 ? { attachments: attached.map((f) => f.id) } : {}), sources: pages().filter((p) => p.page_id && !p.looks_like_instructions).map((p) => p.page_id as string), ...(session() ? { session: session(), project: project().trim() } : project().trim() ? { project: project().trim() } : {}) }, (d) => setLive((x) => x + d), abort.signal);
      if (myView !== view) {
        if ("stopped" in r) saveDraft(sentKey, message);
        return;
      }
      if ("stopped" in r) {
        setTurns(turns().slice(0, -1));
        setText(message);
        return;
      }
      const foot = [r.route.simulated ? t("chat.test") : r.route.stays_here === false ? (r.route.host ? t("chat.remote", { host: r.route.host }) : t("chat.unknownPlace")) : t("chat.private"), t("chat.notesUsed", { n: r.notes_used })];
      if (r.project) foot.push(t("chat.projectUsed", { name: r.project }));
      if (r.instructions_used) foot.push(t("chat.instructionsUsed"));
      if (r.usage) {
        const m = meterLine(r.usage);
        foot.push(t(m.key, { n: m.n, max: m.max ?? 0, pct: m.pct ?? 0 }));
        if (m.near) foot.push(t("chat.contextNear"));
        for (const s of summaryNotes(r.usage)) foot.push(t(s.key, { n: s.n }));
        if (r.usage.history_turns_dropped > 0) foot.push(t("chat.dropped", { n: r.usage.history_turns_dropped }));
      }
      if (session()) foot.push(t(savedLine(r)));
      for (const p of pageNotes(r)) foot.push(t(p.key, { n: p.n }));
      for (const p of fileNotes(r.files)) foot.push(t(p.key, { n: p.n }));
      if (r.notes_withheld > 0) foot.push(t("chat.withheld", { n: r.notes_withheld }));
      for (const p of noteNotes(r)) foot.push(t(p.key, { n: p.n }));
      if (r.usage?.summary_text) setSummaryText(r.usage.summary_text);
      setTurns([...turns(), { role: "assistant", content: r.reply, foot, notes: r.notes }]);
      // the files went with this message; the next one starts without them
      if (attached.length > 0) setFiles((cur) => cur.filter((f) => !attached.some((a) => a.id === f.id)));
      if (session()) await refetch();
    } catch (err) {
      // a message whose reply was cut short by switching conversations goes back to the draft it was written in
      if (myView !== view) {
        saveDraft(sentKey, message);
        return;
      }
      // The service may have saved this exchange before the connection dropped. Sending the message again would save it twice,
      // so a kept conversation is read back first: if it holds exactly this exchange, it is shown instead of the message returning.
      const id = session();
      const aborted = err instanceof DOMException && err.name === "AbortError";
      if (id && !aborted) {
        try {
          const saved = await api.keptChat(id);
          if (myView === view && exchangeWasSaved(turnsBefore, message, saved.turns)) {
            setTurns(saved.turns.map(({ role, content }) => ({ role, content })));
            setSummaryText(saved.summary?.text ?? "");
            setRecovered(true);
            if (session()) await refetch();
            return;
          }
        } catch {
          /* cannot tell: treat it as not saved, like any failed send */
        }
      }
      setTurns(turns().slice(0, -1));
      setText(message);
      if (!aborted) setError(err instanceof ApiError ? err.message : t("chat.error"));
    } finally {
      setBusy(false);
      setLive("");
      abort = null;
      if (myView === view) returnFocus();
    }
  };
  return (
    <section class="stack wide" aria-labelledby="h-chat">
      <h1 id="h-chat">{t("chat.title")}</h1>
      <p class="lead">{t("chat.lead")}</p>
      <p class="muted">{session() ? t("chat.noteKept") : t("chat.note")}</p>
      <div class="stack">
        <label for="chat-project">{t("chat.project")}</label>
        <input id="chat-project" value={project()} maxlength="80" placeholder={t("chat.projectHint")} disabled={busy()} onInput={(e) => setProject(e.currentTarget.value)} />
        <p class="muted">{t("chat.projectExplain")}</p>
      </div>
      <div class="card stack" role="region" aria-label={t("chat.keptTitle")}>
        <div class="actions">
          <Show when={!session()}><button type="button" onClick={keep}>{t("chat.keep")}</button></Show>
          <Show when={session()}><span class="chip ok">{t("chat.keptNow")}</span> <button type="button" onClick={() => show(null, [])}>{t("chat.newChat")}</button></Show>
        </div>
        <Show when={session()}>
          <div class="stack notes-find">
            <p class="muted">{t("chat.notesHelp")}</p>
            <div class="actions">
              <button type="button" disabled={notesBusy() || busy() || turns().length === 0} onClick={() => void findNotes()}>{notesBusy() ? t("chat.notesFinding") : t("chat.notesFind")}</button>
              <Show when={notesInfo()}><span role="status" class="muted">{notesInfo()}</span></Show>
            </div>
            <Show when={suggested().length > 0}>
              <ul class="plain stack" aria-label={t("chat.notesTitle")}>
                <For each={suggested()}>
                  {(n) => (
                    <li class="card stack">
                      <p class="mem-text">{n.text}</p>
                      <p class="muted">{t("chat.notesFrom")} ({n.why})</p>
                      <div class="actions">
                        <button type="button" class="primary" onClick={() => void decideNote(n.id, true)}>{t("chat.notesKeep")}</button>
                        <button type="button" onClick={() => void decideNote(n.id, false)}>{t("chat.notesDismiss")}</button>
                      </div>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </div>
        </Show>
        <Show when={(kept() ?? []).length > 0}>
          <p><strong>{t("chat.keptTitle")}</strong></p>
          <label class="row-check"><input type="checkbox" checked={showArchived()} onChange={(e) => setShowArchived(e.currentTarget.checked)} /> <span>{t("chat.showArchived")}</span></label>
          <label class="sr" for="chat-search">{t("chat.search")}</label>
          <input id="chat-search" type="search" maxlength="200" placeholder={t("chat.search")} value={chatQuery()} onInput={(e) => onSearch(e.currentTarget.value)} />
          <Show when={keptProblem()}><p role="alert" class="bad">{keptProblem()}</p></Show>
          <Show when={chatHits()}>
            {(hits) => (
              <ul class="plain stack" aria-label={t("chat.searchResults")}>
                <For each={hits()} fallback={<li class="muted">{t("chat.searchNone")}</li>}>
                  {(h) => (
                    <li class="stack">
                      <div class="actions"><strong>{h.title || t("chat.untitled")}</strong><button type="button" onClick={() => openKept(h.id)}>{t("chat.open")}</button></div>
                      <For each={h.snippets}>{(sn) => <p class="muted">{sn.role}: {sn.text}</p>}</For>
                    </li>
                  )}
                </For>
              </ul>
            )}
          </Show>
          <Show when={!chatHits()}>
          <ul class="plain stack">
            <For each={kept() ?? []}>
              {(k) => (
                <li class="actions">
                  <Show when={renaming() !== k.id} fallback={
                    <span class="confirm" role="group">
                      <label class="sr" for={`rename-${k.id}`}>{t("chat.renameLabel")}</label>
                      <input id={`rename-${k.id}`} value={renameText()} maxlength="80" onInput={(e) => setRenameText(e.currentTarget.value)} />
                      <button type="button" class="primary" onClick={() => void doRename(k.id)}>{t("chat.renameSave")}</button>
                      <button type="button" onClick={() => setRenaming("")}>{t("mem.cancel")}</button>
                    </span>
                  }>
                    <span>{k.pinned ? <span class="chip">{t("chat.pinned")}</span> : null} {k.archived ? <span class="chip">{t("chat.archived")}</span> : null} {k.title || t("chat.untitled")} · {t("chat.turns", { n: k.turns })}</span>
                    <button type="button" onClick={() => { setRenaming(k.id); setRenameText(k.title); }}>{t("chat.rename")}</button>
                  </Show>
                  <button type="button" onClick={() => openKept(k.id)}>{t("chat.open")}</button>
                  <button type="button" onClick={() => void flagKept(k.id, "pin", !k.pinned)}>{k.pinned ? t("chat.unpin") : t("chat.pin")}</button>
                  <button type="button" onClick={() => void flagKept(k.id, "archive", !k.archived)}>{k.archived ? t("chat.unarchive") : t("chat.archive")}</button>
                  <button type="button" onClick={() => forget(k.id)}>{confirmForget() === k.id ? t("chat.forgetSure") : t("chat.forget")}</button>
                </li>
              )}
            </For>
          </ul>
          </Show>
        </Show>
      </div>
      <details class="card stack" id="model-proof">
        <summary>{t("proof.title")}</summary>
        <p class="muted">{t("proof.lead")}</p>
        <label for="proof-model">{t("proof.model")}</label>
        <input id="proof-model" type="text" value={proofModel()} onInput={(e) => setProofModel(e.currentTarget.value)} />
        <div class="actions">
          <button type="button" disabled={proofBusy() || proofModel().trim().length === 0} onClick={checkProof}>{proofBusy() ? t("proof.checking") : t("proof.check")}</button>
          <button type="button" disabled={proofBusy()} onClick={checkGateway}>{t("proof.checkGateway")}</button>
          <Show when={proofError()}><span role="alert" class="bad">{proofError()}</span></Show>
        </div>
        <Show when={gateway()}>
          {(g) => (
            <div class="stack" role="region" aria-label={t("proof.gatewayResult")}>
              <p><span class={`chip ${g().verified ? "ok" : "bad"}`}>{g().verified ? t("proof.verified") : t("proof.notVerified")}</span> {t("proof.gateway")}</p>
              <Show when={g().gateway}>
                {(d) => (
                  <ul class="plain">
                    <li>{t("proof.tcb", { v: d().tcb })}</li>
                    <li>{d().tls_bound ? t("proof.tlsYes") : t("proof.tlsNo")}</li>
                    <li>{t("proof.signer")} <code>{d().signer}</code></li>
                    <li>{t("proof.nonce")} <code>{d().nonce}</code></li>
                  </ul>
                )}
              </Show>
              <p><strong>{t("proof.means")}</strong></p>
              <ul><For each={g().means ?? []}>{(m) => <li>{m}</li>}</For></ul>
              <p><strong>{t("proof.notMeans")}</strong></p>
              <ul><For each={g().does_not_mean ?? []}>{(m) => <li>{m}</li>}</For></ul>
            </div>
          )}
        </Show>
        <Show when={proof()}>
          {(p) => (
            <div class="stack" role="region" aria-label={t("proof.result")}>
              <p><span class={`chip ${p().verified ? "ok" : "bad"}`}>{p().verified ? t("proof.verified") : t("proof.notVerified")}</span> <code>{p().model}</code></p>
              <For each={p().deployments ?? []}>
                {(d) => (
                  <ul class="plain">
                    <li>{t("proof.tcb", { v: d.tcb })}</li>
                    <li>{d.gpu_verified ? t("proof.gpuYes") : t("proof.gpuNo")}</li>
                    <li>{t("proof.signer")} <code>{d.signer}</code></li>
                    <Show when={d.compose_hash}><li>{t("proof.compose")} <code>{d.compose_hash}</code></li></Show>
                    <li>{t("proof.nonce")} <code>{d.nonce}</code></li>
                  </ul>
                )}
              </For>
              <p><strong>{t("proof.means")}</strong></p>
              <ul><For each={p().means ?? []}>{(m) => <li>{m}</li>}</For></ul>
              <p><strong>{t("proof.notMeans")}</strong></p>
              <ul><For each={p().does_not_mean ?? []}>{(m) => <li>{m}</li>}</For></ul>
            </div>
          )}
        </Show>
      </details>
      <details class="card stack" id="web-research">
        <summary>{t("web.title")}</summary>
        <p class="muted">{t("web.lead")}</p>
        <Show when={searchOn()?.available}>
          <label for="web-search">{t("web.searchWords")}</label>
          <input id="web-search" type="text" maxlength="200" value={words()} onInput={(e) => setWords(e.currentTarget.value)} />
          <label for="web-recency">{t("web.recency")}</label>
          <select id="web-recency" value={recency()} onChange={(e) => setRecency(e.currentTarget.value)}>
            <option value="">{t("web.recencyAny")}</option>
            <For each={Object.keys(windows)}>{(w) => <option value={w}>{windowText(w)}</option>}</For>
          </select>
          <div class="actions">
            <button type="button" disabled={webBusy() || words().trim().length === 0} onClick={showSearch}>{t("web.searchShow")}</button>
          </div>
          <Show when={searchProposal()}>
            {(p) => (
              <div class="stack" role="region" aria-label={t("web.searchReview")}>
                <p><strong>{t("web.searchSends", { site: p().will_send_to })}</strong></p>
                <p><code>{p().query}</code></p>
                <Show when={p().recency}><p>{t("web.recencyShown", { window: windowText(p().recency) })}</p></Show>
                <p class="muted">{p().stays_here}</p>
                <div class="actions">
                  <button class="primary" type="button" disabled={webBusy()} onClick={sendSearch}>{t("web.searchSend")}</button>
                  <button type="button" onClick={() => setSearchProposal(null)}>{t("web.cancel")}</button>
                </div>
              </div>
            )}
          </Show>
          <Show when={searchNote()}>
            {(n) => (
              <div role="status" class="stack">
                <p><strong>{t("web.searchNone")}</strong> {n().why}</p>
                <Show when={n().engines.length > 0}><p class="muted">{n().engines.join(" · ")}</p></Show>
              </div>
            )}
          </Show>
          <Show when={hits().length > 0}>
            <p><strong>{t("web.searchResults")}</strong></p>
            <ul class="plain">
              <For each={hits()}>
                {(h) => (
                  <li>
                    <code>{h.url}</code>{h.withheld ? ` — ${t("web.withheld")}` : ` · ${h.title}`}
                    <Show when={h.snippet}><span class="muted"> — {h.snippet}</span></Show>
                    <button type="button" onClick={() => setUrls((urls() ? urls() + "\n" : "") + h.url)}>{t("web.searchUse")}</button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
        <label for="web-urls">{t("web.urls")}</label>
        <textarea id="web-urls" rows="3" value={urls()} onInput={(e) => setUrls(e.currentTarget.value)} />
        <div class="actions">
          <button type="button" disabled={webBusy() || urls().trim().length === 0} onClick={propose}>{t("web.show")}</button>
          <Show when={webError()}><span role="alert" class="bad">{webError()}</span></Show>
        </div>
        <Show when={proposal()}>
          {(p) => (
            <div class="stack" role="region" aria-label={t("web.review")}>
              <p><strong>{t("web.willRequest")}</strong></p>
              <ul class="plain">
                <For each={p().items}>{(i) => <li class={i.allowed ? "" : "muted"}><code>{i.url}</code> {i.allowed ? "" : `— ${t("web.refused")}: ${i.reason}`}</li>}</For>
              </ul>
              <p class="muted">{p().stays_here}</p>
              <div class="actions">
                <button class="primary" type="button" disabled={webBusy()} onClick={fetchApproved}>{t("web.fetch")}</button>
                <button type="button" onClick={() => setProposal(null)}>{t("web.cancel")}</button>
              </div>
            </div>
          )}
        </Show>
        <Show when={pages().length > 0}>
          <p><strong>{t("web.pages")}</strong></p>
          <ul class="plain">
            <For each={pages()}>
              {(p) => (
                <li>
                  <code>{p.final_url ?? p.url}</code> {p.title ? `· ${p.title}` : ""}
                  {p.error ? ` — ${t("web.failed")}: ${p.error}` : ""}
                  {p.looks_like_instructions ? ` — ${t("web.withheld")}` : ""}
                </li>
              )}
            </For>
          </ul>
          <button type="button" onClick={() => setPages([])}>{t("web.forget")}</button>
        </Show>
      </details>
      {/* The tools come before the conversation, so the message box sits directly under the newest message. */}
      <Show when={summaryText()}>
        <details class="card stack summary-shown"><summary>{t("chat.summaryShow")}</summary><p class="muted">{t("chat.summaryExplain")}</p><p class="mem-text">{summaryText()}</p></details>
      </Show>
      <Show when={session() && (branches().length > 0 || forkAt() !== null)}>
        <nav class="card stack branches" aria-label={t("chat.branches")}>
          <strong>{t("chat.branches")}</strong>
          <p class="muted">{forkAt() !== null ? t("chat.branchFrom", { n: forkAt()! }) + " " : ""}{t("chat.branchesHelp")}</p>
          <ul class="plain row-wrap">
            <For each={branches()}>{(b) => <li><button type="button" class="linklike" onClick={() => void openKept(b.id)}>{t("chat.branchOf", { title: b.title, n: b.turns })}</button></li>}</For>
          </ul>
        </nav>
      </Show>
      <Show when={turns().length > 0}>
        <div class="find-bar" role="search">
          <label class="sr" for="chat-find">{t("chatfind.label")}</label>
          <input id="chat-find" type="search" maxlength="200" placeholder={t("chatfind.label")} value={findText()} onInput={(e) => runFind(e.currentTarget.value)} onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              stepFind(e.shiftKey ? -1 : 1);
            } else if (e.key === "Escape" && findText() !== "") {
              e.stopPropagation();
              clearFind();
            }
          }} />
          <Show when={findText().trim() !== ""}>
            <span class="muted" role="status">{found().length === 0 ? t("chatfind.none") : t("chatfind.count", { n: (findAt() ?? 0) + 1, of: found().length })}</span>
            <button type="button" class="linklike" disabled={found().length === 0} onClick={() => stepFind(-1)}>{t("chatfind.prev")}</button>
            <button type="button" class="linklike" disabled={found().length === 0} onClick={() => stepFind(1)}>{t("chatfind.next")}</button>
          </Show>
        </div>
      </Show>
      <ul ref={listEl} class="plain stack chat" aria-live="polite">
        <For each={turns()} fallback={<li class="muted">{t("chat.empty")}</li>}>
          {(m, i) => (
            <li class={`card stack msg ${m.role}`}>
              <strong>{m.role === "user" ? t("chat.you") : t("chat.assistant")}</strong>
              <div class="msg-body">{m.role === "assistant" ? <Markdown text={m.content} /> : <p class="mem-text">{m.content}</p>}</div>
              <Show when={(m.attached ?? []).length > 0}><p class="muted">{t("files.attached", { names: m.attached!.join(", ") })}</p></Show>
              <Show when={m.foot}>{(f) => <p class="muted">{f().join(" · ")}</p>}</Show>
              <div class="actions msg-actions">
                <button type="button" class="linklike" onClick={() => void copyTurn(i(), m.content)}>{copiedAt() === i() ? t("chat.copied") : t("chat.copy")}</button>
                <Show when={session() && !busy() && editPoint(turns(), i())}>
                  <button type="button" class="linklike" onClick={() => void branchAt(editPoint(turns(), i()), false)}>{t("chat.edit")}</button>
                </Show>
                <Show when={session() && !busy() && againPoint(turns(), i())}>
                  <button type="button" class="linklike" onClick={() => void branchAt(againPoint(turns(), i()), true)}>{t("chat.again")}</button>
                </Show>
              </div>
              <Show when={(m.notes ?? []).length > 0}>
                <details class="notes-used">
                  <summary>{t("chat.whichNotes", { n: m.notes!.length })}</summary>
                  <ul class="plain"><For each={m.notes}>{(n) => <li><span>{n.text}</span> <span class="muted">({n.why}{n.untrusted ? `; ${t("mem.untrusted")}` : ""})</span></li>}</For></ul>
                </details>
              </Show>
            </li>
          )}
        </For>
        <Show when={busy() && live()}><li class="card stack msg assistant"><strong>{t("chat.assistant")}</strong><Markdown text={live()} /></li></Show>
        <Show when={busy() && !live()}><li class="muted" role="status">{t("chat.thinking")}</li></Show>
      </ul>
      <Show when={unseen()}><button type="button" class="jump" onClick={jump}>{t("chat.jump")}</button></Show>
      <form class="card stack" onSubmit={send}>
        <label for="chat-input">{t("chat.input")}</label>
        <div
          class={dragging() ? "stack attach drop" : "stack attach"}
        >
          <div class="actions">
            <input ref={picker} type="file" multiple hidden aria-label={t("files.attach")} onChange={(e) => {
              void addFiles(Array.from(e.currentTarget.files ?? []));
              e.currentTarget.value = "";
            }} />
            <button type="button" disabled={busy() || !!reading()} onClick={() => picker?.click()}>{t("files.attach")}</button>
            <span class="muted">{t("files.drop")}</span>
            <Show when={dragging()}><strong role="status">{t("files.dropNow")}</strong></Show>
            <Show when={reading()}><span role="status" class="muted">{reading()}</span></Show>
          </div>
          <p class="muted">{t("files.attachHelp")}</p>
          <Show when={fileInfo()}><p role="alert" class="bad">{fileInfo()}</p></Show>
          <Show when={files().length > 0}>
            <ul class="plain stack" aria-label={t("files.attach")}>
              <For each={files()}>
                {(f) => (
                  <li class="card stack file-chip">
                    <p><strong>{f.name}</strong> <span class="chip">{f.kind_label}</span> <span class="muted">{sizeText(f.bytes)} · {t("files.chars", { n: f.chars })} · {t("files.passages", { n: f.passages })}</span></p>
                    <Show when={f.truncated}><p class="muted">{t("files.truncated")}</p></Show>
                    <For each={f.notes}>{(n) => <p class="muted">{n}</p>}</For>
                    <Show when={f.flagged.instruction_like > 0}><p class="bad">{t("files.flaggedInstructions", { n: f.flagged.instruction_like })}</p></Show>
                    <Show when={f.flagged.secret > 0}><p class="bad">{t("files.flaggedSecrets", { n: f.flagged.secret })}</p></Show>
                    <Show when={f.passages === 0}><p class="muted">{t("files.noText")}</p></Show>
                    <div class="actions">
                      <button type="button" class="linklike" onClick={() => void toggleRead(f.id)}>{shown()[f.id] && shown()[f.id] !== "loading" ? t("files.hide") : t("files.show")}</button>
                      <button type="button" class="linklike" onClick={() => void dropFile(f.id)}>{t("files.remove")}</button>
                    </div>
                    <Show when={shown()[f.id] && shown()[f.id] !== "loading"}>
                      <ul class="plain stack read-passages">
                        <For each={(shown()[f.id] as AttachmentRead).read.slice(0, 60)}>
                          {(p) => (
                            <li>
                              <code>{p.anchor}</code>
                              <For each={p.lines.slice(0, 40)}>
                                {(l) => (
                                  <p class={l.flagged ? "mem-text muted struck" : "mem-text"}>
                                    {l.text.length > 300 ? l.text.slice(0, 300) + "…" : l.text}
                                    <Show when={l.flagged}> <span class="chip">{t(l.flagged === "secret" ? "files.passageFlagSecret" : "files.passageFlag")}</span></Show>
                                  </p>
                                )}
                              </For>
                            </li>
                          )}
                        </For>
                      </ul>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </div>
        <textarea
          ref={box}
          id="chat-input"
          rows="3"
          value={text()}
          onInput={(e) => setText(e.currentTarget.value)}
          onPaste={(e) => {
            const pasted = pastedFiles(e.clipboardData?.items);
            if (pasted.length === 0) return;
            e.preventDefault();
            if (!busy()) void addFiles(pasted);
          }}
          onKeyDown={(e) => {
            if (stopsReply(e, busy())) {
              e.preventDefault();
              abort?.abort();
              return;
            }
            if (opensHistoryPicker(e, e.currentTarget.value, e.currentTarget.selectionStart, sentHistory().length)) {
              e.preventDefault();
              setPicking(true);
              return;
            }
            const dir = historyKey(e);
            if (dir) {
              const el = e.currentTarget;
              if (el.selectionStart !== el.selectionEnd) return;
              const step = historyStep(sentHistory(), hist, dir, el.value, el.selectionStart);
              if (step) {
                e.preventDefault();
                hist = step.state;
                setText(step.text);
                el.value = step.text;
                el.setSelectionRange(step.cursor, step.cursor);
              }
              return;
            }
            // Enter that confirms an input-method composition (Hindi, Chinese, Japanese) must not send the message.
            if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.keyCode !== 229) {
              e.preventDefault();
              void send();
            }
          }}
        />
        <div class="actions">
          <button class="primary" type="submit" disabled={busy() || text().trim().length === 0}>{t("chat.send")}</button>
          <Show when={busy()}><button type="button" onClick={() => abort?.abort()}>{t("chat.stop")}</button></Show>
          <button type="button" disabled={busy() || turns().length === 0} onClick={() => { setTurns([]); setError(""); forgetSent(draftKey(session())); hist = idleHistory(); }}>{t("chat.clear")}</button>
          <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
          <Show when={recovered()}><span role="status" class="muted">{t("chat.recovered")}</span></Show>
        </div>
        <p class="muted">{t("chat.pick.hint")}</p>
        <Show when={picking()}>
          <HistoryPicker
            entries={sentHistory()}
            onPick={(m) => {
              setPicking(false);
              hist = idleHistory();
              setText(m);
              queueMicrotask(() => {
                box?.focus();
                box?.setSelectionRange(m.length, m.length);
              });
            }}
            onClose={() => {
              setPicking(false);
              queueMicrotask(() => box?.focus());
            }}
          />
        </Show>
      </form>
    </section>
  );
}
