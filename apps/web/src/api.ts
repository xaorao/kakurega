import type { ForgetResult } from "./forgotten";
let csrf = "";

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
  }
}

export function setCsrf(token: string) {
  csrf = token;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (method !== "GET") headers["x-kk-csrf"] = csrf;
  const res = await fetch(path, { method, headers, credentials: "same-origin", body: body === undefined ? undefined : JSON.stringify(body) });
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  let data: any = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = null;
  }
  if (!res.ok) throw new ApiError(res.status, data?.error?.code ?? "ERROR", data?.error?.message ?? `request failed (${res.status})`);
  return data as T;
}

export interface ExpertInfo { id: string; name: string; group: string; summary: string }
export interface RoundtableResolution { structured: boolean; agreed: string | null; disputed: string | null; decision: string | null; open_questions: string | null; text: string }
/** One line of a roundtable's stream: an expert's state, a piece of its answer, its whole answer or its failure; then the resolution. */
export type RoundtableEvent = Record<string, unknown> & { expert?: string; state?: string; delta?: string; answer?: string; failed?: string; stopped?: boolean; done?: boolean };

export type TeamTask = {
  n: number;
  expert: string;
  expert_name: string;
  after: number[];
  task: string;
  state: "waiting" | "running" | "done" | "needs_input" | "failed" | "skipped";
  result: string;
  note: string;
  question: string;
  asked: { question: string; answer: string }[];
  attempts: number;
};
export type WorkItem = { kind: "review" | "code" | "team"; id: string; title: string; state: string; since: number; last: number; quiet_for: number; quiet: boolean; can_stop: boolean };
export type TeamStatus = "planned" | "running" | "waiting" | "done" | "stopped" | "interrupted";
export type TeamRun = { id: string; goal: string; project: string | null; status: TeamStatus; working: boolean; final: string | null; final_note: string; created: number; updated: number; tasks: TeamTask[] };
export type TeamRunSummary = { id: string; goal: string; status: TeamStatus; created: number; updated: number; tasks: number; done: number; needs_input: number; not_done: number };

/** Streams a roundtable. `onEvent` receives each line; aborting `signal` closes the connection, which stops the model. */
export async function streamRoundtable(body: unknown, onEvent: (e: RoundtableEvent) => void, signal: AbortSignal): Promise<void> {
  const res = await fetch("/api/chat/roundtable", { method: "POST", headers: { "content-type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify(body), signal });
  if (!res.ok || !res.body) {
    let data: any = null;
    try {
      data = JSON.parse(await res.text());
    } catch {
      data = null;
    }
    throw new ApiError(res.status, data?.error?.code ?? "ERROR", data?.error?.message ?? `request failed (${res.status})`);
  }
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let nl: number;
    while ((nl = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, nl).trim();
      buffer = buffer.slice(nl + 1);
      if (!line) continue;
      const v = JSON.parse(line) as RoundtableEvent & { error?: { code?: string; message?: string } };
      if (v.error) throw new ApiError(502, v.error.code ?? "ERROR", v.error.message ?? "the roundtable failed");
      onEvent(v);
    }
  }
}

/** Streams a chat answer. `onDelta` receives each piece; aborting `signal` closes the connection, which stops the model. */
export async function streamChat(body: unknown, onDelta: (t: string) => void, signal: AbortSignal): Promise<ChatReply | { stopped: true }> {
  const res = await fetch("/api/chat/stream", { method: "POST", headers: { "content-type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify(body), signal });
  if (!res.ok || !res.body) {
    let data: any = null;
    try {
      data = JSON.parse(await res.text());
    } catch {
      data = null;
    }
    throw new ApiError(res.status, data?.error?.code ?? "ERROR", data?.error?.message ?? `request failed (${res.status})`);
  }
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let last: ChatReply | { stopped: true } | null = null;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let nl: number;
    while ((nl = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, nl).trim();
      buffer = buffer.slice(nl + 1);
      if (!line) continue;
      const v = JSON.parse(line);
      if (typeof v.delta === "string") onDelta(v.delta);
      else if (v.error) throw new ApiError(502, v.error.code ?? "ERROR", v.error.message ?? "the answer failed");
      else if (v.stopped) last = { stopped: true };
      else if (v.done) last = v as ChatReply;
    }
  }
  // the body is read to its end so the browser sees a finished response, not an abandoned one
  if (last) return last;
  throw new ApiError(502, "ENDED", "the answer ended before it was finished");
}

export interface Config {
  version: string;
  route: { id: string; class: string; model?: string; simulated: boolean; transport?: string; host?: string | null; stays_here?: boolean };
  roots: string[];
  test_mode: boolean;
  approvals?: { asks_every_release: boolean; signing_key_enrolled: boolean };
  signer: string;
  signer_public: string;
}
export interface ReadinessCheck { id: string; label: string; ok: boolean; required?: boolean; detail: string; fix: string; repairable?: boolean; repair?: { state: "running" | "done" | "failed"; detail: string } }
export interface Readiness { ready: boolean; checks: ReadinessCheck[] }
export interface MemoryItem { id: string; text: string; why: string; matched_terms?: string[]; origin?: string; instruction_like?: boolean; untrusted: boolean; level: string; compartments: string[]; created_at: number; valid_to: number | null; source: string; project?: string | null; uses?: number; last_used?: number | null }
export interface MemoryFilter { level?: string; origin?: string; project?: string; unused?: boolean; ending?: boolean }
export interface MemoryStats { total: number; complete: boolean; by_level: Record<string, number>; by_origin: Record<string, number>; by_project: Record<string, number>; untrusted: number; never_used: number; ending_within_14_days: number; held: number }
export interface MemoryVersion { id: string; text: string; reason: string; created_at: number; current: boolean }
export interface Suggestion { id: string; text: string; why: string; level: string; untrusted: boolean }
export interface LogEntry { seq: number; at: number; kind: string; subject: string; data: Record<string, string | number> }
export interface LogSummary { count: number; head: string | null; public_key: string; recent: LogEntry[] }
export interface ChatTurn { role: "user" | "assistant"; content: string }
export interface Method { name: string; description: string; triggers: string[]; avoid: string[]; body: string; hash: string; approved: boolean; approved_at?: number | null }
export interface BundledMethod { name: string; group: string; description: string; hash: string; added: boolean }
export interface MethodProposal { id: number; job: string; name: string; description: string; triggers: string[]; body: string }
export interface McpTool { name: string; description: string; pin: string; mode: "off" | "allow" | "ask"; effect: "read_only" | "may_change" | "unknown"; schema?: string }
export interface McpServer { name: string; program: string; args: string[]; env_names: string[] | null; tools: McpTool[] }
export interface BreakResult { planted: { key: string; password: string }; asked: string; sent: { role: string; content: string }[]; reply: string; reached_model: number; in_reply: number; leaked: boolean; attack_was_sent: boolean; attempts: number; leaks: number; route: { stays_here: boolean; host: string | null }; limits: string }
export interface HandoffProposal { id: string; text: string; sha256: string; chars: number; personal: string; tool: string; notice: string }
export interface OutboundRow { at: number; seq: number; what: "model" | "search" | "pages" | "tool" | "release" | "telegram" | "held"; to: string; detail: Record<string, number | string> }
export interface RouteOptions { changeable: boolean; current?: "local" | "near" | "outside" | "test"; local?: { model: string }; near?: { model: string | null; key_saved: boolean }; outside?: { in_use: boolean }; route: { stays_here: boolean; host: string | null; model?: string }; note: string }
export interface MeetingPoint { kind: "decision" | "action" | "question"; text: string; owner: string; due: string; quote: string; line: number | null }
export interface MeetingNotes { title: string; summary: string[]; checked: MeetingPoint[]; unchecked: MeetingPoint[]; parts: number; unreadable_parts: number; markdown: string; withheld_lines: number; swapped: number; elapsed_ms: number; route: { stays_here: boolean; host: string | null }; limits: string }
export interface ChatReply { cut_short?: boolean; swapped?: number; instructions_used?: boolean; methods_used?: string[]; project?: string | null; reply: string; route: { id: string; class: string; simulated: boolean; stays_here?: boolean; host?: string | null }; notes_used: number; notes?: { id: string; text: string; why: string; untrusted: boolean; project?: string | null }[]; notes_withheld: number; notes_left_out?: number; files?: { used: number; trimmed: number; left_out: number; withheld_instructions: number; withheld_secrets: number }; web_sources_used: number; web_sources_trimmed?: number; web_sources_left_out?: number; saved?: boolean; not_saved?: "full" | "forgotten" | "error" | null; usage?: { elapsed_ms?: number; context_tokens?: number; context_window?: number | null; estimate?: boolean; context_tokens_estimate: number; reply_tokens_estimate: number; history_turns_sent: number; history_turns_dropped: number; history_turns_summarized?: number; summary_text?: string | null; compacted_now?: boolean; compaction_failed?: string | null } }
export interface Instructions {
  global: { text: string; updated_at: number } | null;
  projects: { project: string; text: string; updated_at: number }[];
  max_chars: number;
}

export interface RoutePrice { route: string; input_per_million: number; output_per_million: number; currency: string }
export interface Usage {
  from: number;
  to: number;
  routes: { route: string; class: string; simulated: boolean; turns: number; prompt_tokens: number; measured_turns: number; reply_tokens: number; avg_ms: number; max_ms: number; reply_tokens_per_second: number; cost: { amount: number; currency: string; tokens_estimated: boolean } | null }[];
  total: { turns: number; prompt_tokens: number; reply_tokens: number; cost: { currency: string; amount: number }[] };
  note: string;
}

export interface ReviewSource {
  job: string;
  notes: number;
  suggestions: number;
}

export interface MemoryConflicts {
  pairs: { a: MemoryItem; b: MemoryItem; why: "same_subject" | "other_claim"; subject: string | null }[];
  scanned: number;
  complete: boolean;
  note: string;
}

export interface ChatHit {
  id: string;
  title: string;
  updated: number;
  project: string | null;
  snippets: { role: string; text: string }[];
}

export interface SimilarMemories { groups: MemoryItem[][]; scanned: number; complete: boolean; note: string }
export interface KeptChat { id: string; title: string; created: number; updated: number; turns: number; project?: string | null; parent?: string | null; fork_at?: number | null; pinned?: boolean; archived?: boolean }
export interface ReleaseResult { result: "verified" | "not_established" | "mismatch"; reason: string | null }
export interface ModelProof { verified: boolean; model: string; checked_at?: number; means?: string[]; does_not_mean?: string[]; deployments?: { signer: string; tcb: string; advisories: string[]; gpu_verified: boolean; compose_hash: string | null; os_image_hash: string | null; nonce: string; release?: ReleaseResult | null }[] }
export interface GatewayProofResult { verified: boolean; gateway?: { signer: string; tcb: string; advisories: string[]; compose_hash: string | null; tls_bound: boolean; tls_fingerprint: string | null; nonce: string; release?: ReleaseResult | null }; means?: string[]; does_not_mean?: string[] }
export interface Passage { file: string; start: number; focus: number; lines: string[] }
export interface ChatSent { messages: { role: string; content: string }[]; cut: boolean; excluded_notes?: { id: string; text: string; reason: string }[]; at: number; route: { class: string; simulated: boolean; host: string | null; stays_here: boolean; id: string; name?: string | null } }
export interface PrivacyFacts { model: { class: string; id: string; name?: string | null; simulated: boolean; host: string | null; stays_here: boolean }; search: { on: boolean; host: string | null }; extension: { on: boolean }; telegram: { on: boolean }; agents: { tools_open: string[]; calls_are_logged: boolean }; notes: { count: number; encrypted_at_rest: boolean }; approvals: { signing_key_enrolled: boolean }; updates: { checks_in_background: boolean } }
export interface Lease { tool: string; remaining: number; expires_at: number; issued_at: number }
export interface InboxItem { kind: "approval_waiting" | "ready_to_review" | "ready_to_deliver" | "failed" | "suggestions" | "team_planned" | "team_question" | "team_interrupted"; job?: string; recipient?: string; team?: string; title?: string; task?: number; since: number; what: string; count?: number }
export interface Schedule { id: string; repo: string; recipient: string; every_hours: number; next_at: number }
export interface ScheduleRun { schedule: string; at: number; outcome: string; job: string | null }
export interface ResearchItem { url: string; host?: string; allowed: boolean; reason?: string }
export interface ResearchProposal { id: string; digest: string; items: ResearchItem[]; will_request: string[]; stays_here: string }
export interface SearchProposal { id: string; digest: string; query: string; recency: string | null; will_send_to: string; stays_here: string }
export interface SearchAnswer { results: SearchHit[]; note: string; engines_failed: string[]; empty_because: string | null }
export interface SearchHit { title: string; snippet: string; url: string; host: string | null; withheld: boolean }
export interface ResearchPage { page_id?: string; url: string; final_url?: string; title?: string; sha256?: string; truncated?: boolean; looks_like_instructions?: boolean; error?: string }
export interface AskCitation { file: string; quote: string; verdict: "verified" | "not_found" | "unknown_file" | "bad_length"; line: number | null }
export interface AskReply { checkable: boolean; answer?: string; citations?: AskCitation[]; verified?: number; unchecked_text?: string; files_used: string[]; files_withheld: string[]; files_skipped: string[]; files_partial?: string[]; searched?: boolean; by_meaning?: boolean; narrowed_to?: number | null; no_answer?: boolean; injection_suspects: string[]; route: { simulated: boolean } }
export interface Withheld { path: string; notes: string[] }
export interface Summary {
  imported: number;
  skipped: { path: string; reason: string }[];
  withheld: Withheld[];
  injection_suspects: string[];
  route: { route_id: string; class: string; model: string; model_digest: string | null; endpoint: string; simulated: boolean }[];
  worker_exit: string | null;
  final_taint: { level: string; integrity: string; compartments: string[] } | null;
  findings: number;
  confirmed: number;
  manifest_digest: string;
  files_in_manifest: number;
}
export interface Job {
  id: string;
  repo: string;
  recipient: string;
  created: number;
  state: "running" | "ready" | "nothing_to_release" | "failed";
  note: string;
  events: { at: number; stage: string; detail: string }[];
  summary: Summary | null;
  approval: string | null;
  delivered: { name: string; digest: string; signer: string } | null;
}
export interface FileEntry { path: string; bytes: number; level: string; compartments: string[]; withheld: boolean; notes: string[] }
export interface Fix { index: number; title: string; file: string; files?: string[]; explanation: string; status: string; tests?: string; verified: boolean; diff: string; applied: boolean; state?: "not_applied" | "applying" | "applied" | "undoing" | "attention"; attention?: string | null }
export interface Finding { title: string; file: string; line: number; explanation: string; reproducer: string; language?: string; status: string; output: string; sample: string[] }
export interface AuditEntry { request: number; op: string; outcome: string; detail: string }
export interface ReleaseFile { name: string; bytes: number; sha256: string; text: string | null }
export interface Disclosure { verdict: "allow" | "review" | "block"; reasons: string[]; checked: string[]; not_checked: string[]; files_after: number; project_files: number; releases_before: number }
export interface Release {
  disclosure?: Disclosure;
  available: boolean;
  reason?: string;
  digest?: string;
  recipient?: string;
  expires?: number;
  /** The service's clock when this was answered, in Unix seconds, so a countdown does not depend on this page's clock. */
  server_time?: number;
  files?: ReleaseFile[];
  removed?: { what: string; why: string }[];
  not_confirmed: { title: string; why: string }[];
  not_established?: string[];
  personal_data?: { file: string; found: string }[];
}

export interface AttachmentSummary { id: string; name: string; kind: string; kind_label: string; bytes: number; chars: number; passages: number; notes: string[]; truncated: boolean; flagged: { instruction_like: number; secret: number } }
export interface AttachmentRead extends AttachmentSummary { read: { anchor: string; lines: { text: string; flagged: "instruction_like" | "secret" | null }[] }[] }

/** Sends one file to be read on this computer. The bytes go as they are; the name is only a label. */
async function attachFile(file: File): Promise<AttachmentSummary> {
  const res = await fetch(`/api/attachments?name=${encodeURIComponent(file.name)}`, { method: "POST", headers: { "content-type": "application/octet-stream", "x-kk-csrf": csrf }, credentials: "same-origin", body: file });
  const text = await res.text();
  let data: any = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = null;
  }
  if (!res.ok) throw new ApiError(res.status, data?.error?.code ?? "ERROR", data?.error?.message ?? `request failed (${res.status})`);
  return data as AttachmentSummary;
}

export type CodeChange = { path: string; kind: "added" | "modified" | "deleted"; diff: string; added: number; removed: number };
export type CodeTask = {
  id: string;
  task: string;
  folder: string;
  state: "running" | "ready" | "failed" | "applied" | "applying" | "undoing" | "attention";
  error: string | null;
  attention: string[];
  progress: string[];
  ended: string | null;
  summary: string | null;
  tests: { passed: boolean; output: string } | null;
  test_command: string | null;
  withheld_files: number | null;
  steps: { tool: string; label: string | null; refused: boolean; said: string }[];
  changes: CodeChange[];
  digest: string | null;
  concerns: string[];
  can_apply: boolean;
};

export const api = {
  health: () => request<{ ok: boolean }>("GET", "/api/health"),
  pair: (code: string) => request<{ csrf: string }>("POST", "/api/pair", { code }),
  session: () => request<{ paired: boolean; csrf: string }>("GET", "/api/session"),
  logout: () => request<void>("POST", "/api/logout"),
  config: () => request<Config>("GET", "/api/config"),
  experts: () => request<{ experts: ExpertInfo[]; yours?: { id: string; name: string; summary: string }[]; min: number; max: number }>("GET", "/api/experts"),
  work: () => request<{ items: WorkItem[]; count: number; quiet: number; quiet_after: number }>("GET", "/api/work"),
  teamRuns: () => request<{ runs: TeamRunSummary[] }>("GET", "/api/team"),
  teamRun: (id: string) => request<TeamRun>("GET", `/api/team/${encodeURIComponent(id)}`),
  teamPlan: (goal: string, experts: string[]) => request<TeamRun>("POST", "/api/team", experts.length > 0 ? { goal, experts } : { goal }),
  teamStart: (id: string) => request<TeamRun>("POST", `/api/team/${encodeURIComponent(id)}/start`, {}),
  teamStop: (id: string) => request<{ stopping: boolean }>("POST", `/api/team/${encodeURIComponent(id)}/stop`, {}),
  teamAnswer: (id: string, task: number, answer: string) => request<TeamRun>("POST", `/api/team/${encodeURIComponent(id)}/answer`, { task, answer }),
  teamDelete: (id: string) => request<{ deleted: string }>("POST", `/api/team/${encodeURIComponent(id)}/delete`, {}),
  models: () => request<{ current: string; installed: string[]; can_choose: boolean }>("GET", "/api/models"),
  chooseModel: (model: string) => request<{ route: { model: string } }>("POST", "/api/model", { model }),
  memories: (q?: string, f?: MemoryFilter) => {
    const p = new URLSearchParams();
    if (q) p.set("q", q);
    if (f?.level) p.set("level", f.level);
    if (f?.origin) p.set("origin", f.origin);
    if (f?.project) p.set("project", f.project);
    if (f?.unused) p.set("unused", "1");
    if (f?.ending) p.set("ending", "1");
    const qs = p.toString();
    return request<{ memories: MemoryItem[] }>("GET", `/api/memories${qs ? `?${qs}` : ""}`);
  },
  piiAllow: () => request<{ entries: { entry: string; added_at: number }[]; max_entries: number; note: string }>("GET", "/api/pii-allow"),
  breakStats: () => request<{ attempts: number; leaks: number; max_chars: number }>("GET", "/api/break"),
  breakAttempt: (attack: string, ask?: string) => request<BreakResult>("POST", "/api/break", ask ? { attack, ask } : { attack }),
  meetingNotes: (text: string, title?: string) => request<MeetingNotes>("POST", "/api/meeting-notes", title ? { text, title } : { text }),
  prompts: () => request<{ prompts: { name: string; text: string; added_at: number }[]; max: number }>("GET", "/api/prompts"),
  promptChange: (b: { name?: string; text?: string; remove?: string }) => request<{ changed: boolean; prompts: { name: string; text: string; added_at: number }[] }>("POST", "/api/prompts", b),
  handoffStatus: () => request<{ available: boolean; tool: string | null; notice: string }>("GET", "/api/handoff"),
  handoffPropose: (text: string) => request<HandoffProposal>("POST", "/api/handoff", { text }),
  handoffRun: (id: string) => request<{ reply: string; chars_sent: number }>("POST", `/api/handoff/${id}/run`, {}),
  handoffCancel: (id: string) => request<{ cancelled: boolean }>("POST", `/api/handoff/${id}/cancel`, {}),
  outbound: () => request<{ rows: OutboundRow[]; totals: Record<string, number>; looked_at: number; note: string }>("GET", "/api/outbound"),
  routeOptions: () => request<RouteOptions>("GET", "/api/route/options"),
  routeChoose: (b: { route?: "local" | "near"; model?: string; key?: string; forget_key?: boolean }) => request<RouteOptions>("POST", "/api/route/choose", b),
  hiddenNames: () => request<{ names: { name: string; added_at: number }[]; max: number; applies: boolean; note: string }>("GET", "/api/hidden-names"),
  hiddenNameAdd: (name: string) => request<{ changed: boolean; names: number }>("POST", "/api/hidden-names", { add: name }),
  hiddenNameRemove: (name: string) => request<{ changed: boolean; names: number }>("POST", "/api/hidden-names", { remove: name }),
  piiAllowAdd: (entry: string) => request<{ changed: boolean; entries: number }>("POST", "/api/pii-allow", { add: entry }),
  piiAllowRemove: (entry: string) => request<{ changed: boolean; entries: number }>("POST", "/api/pii-allow", { remove: entry }),
  prices: () => request<{ prices: RoutePrice[]; max_prices: number; max_price: number }>("GET", "/api/usage/prices"),
  setPrice: (p: RoutePrice) => request<{ saved: boolean }>("POST", "/api/usage/prices", p),
  removePrice: (route: string) => request<{ removed: boolean }>("POST", "/api/usage/prices", { route, remove: true }),
  memoryStats: () => request<MemoryStats>("GET", "/api/memories/stats"),
  addMemory: (b: { text: string; level: string; reason?: string; project?: string }) => request<{ id: string; added: boolean }>("POST", "/api/memories", b),
  correctMemory: (id: string, b: { text: string; reason?: string; project?: string }) => request<{ id: string }>("POST", `/api/memories/${id}/correct`, b),
  exportTranscript: (name: string, text: string) => request<{ path: string }>("POST", "/api/exports/transcript", { name, text }),
  forgetMemory: (id: string, reason?: string) => request<ForgetResult & { forgotten: boolean }>("POST", `/api/memories/${id}/forget`, { reason }),
  clearTypedHistory: () => request<{ cleared?: boolean }>("POST", "/api/history/clear", {}),
  memoryHistory: (id: string) => request<{ versions: MemoryVersion[] }>("GET", `/api/memories/${id}/history`),
  fixes: (job: string) => request<{ fixes: Fix[] }>("GET", `/api/jobs/${job}/fixes`),
  applyFix: (job: string, n: number, file: string) => request<{ applied: boolean; file: string }>("POST", `/api/jobs/${job}/fixes/${n}/apply`, { file }),
  undoFix: (job: string, n: number, file: string) => request<{ undone: boolean; kept_later_edits?: boolean; file: string }>("POST", `/api/jobs/${job}/fixes/${n}/undo`, { file }),
  suggestions: (job: string) => request<{ suggestions: Suggestion[] }>("GET", `/api/jobs/${job}/memory-suggestions`),
  acceptSuggestion: (job: string, id: string) => request<{ id: string; added: boolean }>("POST", `/api/jobs/${job}/memory-suggestions/${id}/accept`, {}),
  dismissSuggestion: (job: string, id: string) => request<{ dismissed: boolean }>("POST", `/api/jobs/${job}/memory-suggestions/${id}/dismiss`, {}),
  log: () => request<LogSummary>("GET", "/api/log"),
  chat: (message: string, history: ChatTurn[], sources: string[] = [], session?: string) => request<ChatReply>("POST", "/api/chat", session ? { message, history, sources, session } : { message, history, sources }),
  passage: (folder: string, file: string, line: number) => request<Passage>("POST", "/api/ask/excerpt", { folder, file, line }),
  schedules: () => request<{ schedules: Schedule[]; runs: ScheduleRun[]; note: string }>("GET", "/api/schedules"),
  addSchedule: (repo: string, recipient: string, every_hours: number) => request<{ id: string }>("POST", "/api/schedules", { repo, recipient, every_hours }),
  removeSchedule: (id: string) => request<{ removed: boolean }>("POST", `/api/schedules/${id}/delete`, {}),
  inbox: () => request<{ items: InboxItem[]; count: number }>("GET", "/api/inbox"),
  privacy: () => request<PrivacyFacts>("GET", "/api/privacy"),
  chatSent: () => request<ChatSent>("GET", "/api/chat/sent"),
  agentAccess: () => request<{ leases: Lease[]; leasable: string[]; now: number }>("GET", "/api/agent-access"),
  agentGrant: (tool: string, uses: number, minutes: number) => request<{ tool: string; remaining: number; expires_at: number }>("POST", "/api/agent-access/grant", { tool, uses, minutes }),
  agentRevoke: (tool?: string) => request<{ revoked: number }>("POST", "/api/agent-access/revoke", tool ? { tool } : {}),
  repair: (id: string) => request<{ started: boolean }>("POST", "/api/readiness/repair", { id }),
  attestGateway: () => request<GatewayProofResult>("POST", "/api/route/gateway", {}),
  attestModel: (model: string) => request<ModelProof>("POST", "/api/route/attest", { model }),
  similarMemories: () => request<SimilarMemories>("GET", "/api/memories/similar"),
  instructions: () => request<Instructions>("GET", "/api/instructions"),
  packs: () => request<{ publishers: { key_id: string; public: string | null; label: string; added: number }[]; imported: { name: string; version: string; at: number }[]; limits: { publishers: number } }>("GET", "/api/packs"),
  packPin: (publicKey: string, label: string) => request<{ key_id: string }>("POST", "/api/packs/publishers", { public: publicKey, label }),
  packUnpin: (key_id: string) => request<{ removed: boolean }>("POST", "/api/packs/publishers/remove", { key_id }),
  packImport: (pack: string) => request<{ pack: string; version: string; imported: string[] }>("POST", "/api/packs/import", { pack }),
  methodProposals: () => request<{ proposals: MethodProposal[] }>("GET", "/api/method-proposals"),
  keepProposal: (id: number) => request<{ name: string; approved: boolean }>("POST", "/api/method-proposals/keep", { id }),
  dismissProposal: (id: number) => request<{ dismissed: boolean }>("POST", "/api/method-proposals/dismiss", { id }),
  mcp: () => request<{ servers: McpServer[]; limits: { servers: number } }>("GET", "/api/mcp"),
  mcpAdd: (b: { name: string; program: string; args: string[]; env: Record<string, string> }) => request<{ saved: boolean }>("POST", "/api/mcp/servers", b),
  mcpRemove: (name: string) => request<{ removed: boolean }>("POST", "/api/mcp/servers/remove", { name }),
  mcpDiscover: (name: string) => request<{ tools: number; changed: string[] }>("POST", "/api/mcp/discover", { name }),
  mcpTool: (server: string, tool: string, mode: "off" | "allow" | "ask", pin: string, knowing = false) => request<{ mode: string }>("POST", "/api/mcp/tools", { server, tool, mode, pin, knowing }),
  mcpPending: () => request<{ pending: { id: string; server: string; tool: string; args: string; age_secs: number }[] }>("GET", "/api/mcp/pending"),
  mcpDecide: (id: string, approve: boolean) => request<{ approved: boolean }>("POST", "/api/mcp/pending/decide", { id, approve }),
  mcpTest: (server: string, tool: string, args: unknown) => request<{ text: string; is_error: boolean; truncated: boolean }>("POST", "/api/mcp/test", { server, tool, args }),
  methods: () => request<{ methods: Method[]; bundled?: BundledMethod[]; limits: { methods: number; body_chars: number; triggers: number } }>("GET", "/api/methods"),
  addBundledMethod: (name: string) => request<{ name: string; hash: string; approved: boolean }>("POST", "/api/methods/bundled", { name }),
  saveMethod: (m: { name: string; description: string; triggers: string[]; avoid: string[]; body: string }) => request<{ name: string; hash: string; approved: boolean }>("POST", "/api/methods", m),
  approveMethod: (name: string, hash: string) => request<{ approved: boolean }>("POST", "/api/methods/approve", { name, hash }),
  revokeMethod: (name: string) => request<{ revoked: boolean }>("POST", "/api/methods/revoke", { name }),
  removeMethod: (name: string) => request<{ removed: boolean }>("POST", "/api/methods/remove", { name }),
  diagnostics: () => request<{ text: string; data: unknown }>("GET", "/api/diagnostics"),
  docIndex: () => request<{ vectors: number; models: { model: string; vectors: number; oldest: number; newest: number }[]; max: number }>("GET", "/api/documents/index"),
  forgetDocIndex: () => request<{ removed: number }>("POST", "/api/documents/index/forget", {}),
  projectSettings: () => request<{ projects: { project: string; notes_only: boolean }[]; max: number }>("GET", "/api/project-settings"),
  setNotesOnly: (project: string, notes_only: boolean) => request<{ project: string; notes_only: boolean }>("POST", "/api/project-settings", { project, notes_only }),
  setInstruction: (project: string | undefined, text: string) => request<{ saved?: boolean; removed?: boolean }>("POST", "/api/instructions", project ? { project, text } : { text }),
  memoryConflicts: () => request<MemoryConflicts>("GET", "/api/memories/conflicts"),
  keepNote: (keep: string, replaced: string) => request<{ kept: string }>("POST", "/api/memories/conflicts/keep", { keep, replaced }),
  bothRight: (a: string, b: string) => request<{ dismissed: boolean }>("POST", "/api/memories/conflicts/both", { a, b }),
  usage: (from?: number, to?: number) => request<Usage>("GET", `/api/usage${from !== undefined && to !== undefined ? `?from=${from}&to=${to}` : ""}`),
  reviewSources: () => request<{ sources: ReviewSource[] }>("GET", "/api/memories/sources"),
  forgetReview: (job: string) => request<ReviewSource & { forgotten: boolean }>("POST", `/api/memories/sources/${encodeURIComponent(job)}/forget`, {}),
  mergeMemories: (keep: string, merge: string[]) => request<{ merged: number; kept: string }>("POST", "/api/memories/merge", { keep, merge }),
  unmergeMemory: (id: string) => request<{ restored: boolean }>("POST", `/api/memories/${id}/unmerge`, {}),
  heldMemories: () => request<{ held: { id: string; text: string; kind: string; project?: string | null; created_at: number }[] }>("GET", "/api/memories/held"),
  releaseHold: (id: string) => request<{ released: boolean }>("POST", `/api/memories/${id}/release-hold`, {}),
  mergedMemories: () => request<{ merged: { id: string; text: string; merged_into: string; project?: string | null; level: string }[] }>("GET", "/api/memories/merged"),
  dormantMemories: (days?: number) => request<{ days: number; dormant: { id: string; text: string; project?: string | null; level: string; idle_days: number; uses: number }[]; note: string }>("GET", `/api/memories/dormant${days ? `?days=${days}` : ""}`),
  archivedMemories: () => request<{ archived: { id: string; text: string; project?: string | null; level: string }[] }>("GET", "/api/memories/archived"),
  pinnedMemories: () => request<{ pinned: string[] }>("GET", "/api/memories/pinned"),
  keepMemory: (id: string, action: "archive" | "restore" | "pin" | "unpin" | "keep") => request<{ done: string; id: string }>("POST", `/api/memories/${id}/${action}`, {}),
  keptChats: (archived = false) => request<{ sessions: KeptChat[] }>("GET", archived ? "/api/sessions?archived=1" : "/api/sessions"),
  pinChat: (id: string, pinned: boolean) => request<{ ok: boolean }>("POST", `/api/sessions/${id}/pin`, { pinned }),
  archiveChat: (id: string, archived: boolean) => request<{ ok: boolean }>("POST", `/api/sessions/${id}/archive`, { archived }),
  searchChats: (q: string) => request<{ results: ChatHit[] }>("GET", `/api/sessions/search?q=${encodeURIComponent(q)}`),
  renameChat: (id: string, title: string) => request<{ renamed: boolean }>("POST", `/api/sessions/${id}/rename`, { title }),
  keepChat: (title: string, project?: string) => request<{ id: string }>("POST", "/api/sessions", project ? { title, project } : { title }),
  attachFile: (file: File) => attachFile(file),
  attachment: (id: string) => request<AttachmentRead>("GET", `/api/attachments/${id}`),
  removeAttachment: (id: string) => request<{ removed: boolean }>("POST", `/api/attachments/${id}/remove`, {}),
  keptChat: (id: string) => request<{ id: string; project?: string | null; parent?: string | null; fork_at?: number | null; branches?: { id: string; title: string; fork_at: number | null; turns: number }[]; turns: ChatTurn[]; summary?: { covered: number; text: string } | null }>("GET", `/api/sessions/${id}`),
  suggestNotes: (id: string) => request<{ added: number; already_known: number; dropped: { instruction_like: number; secret: number; invalid: number; over_limit: number; assistant_voice?: number; relative_date?: number } }>("POST", `/api/sessions/${id}/suggest-notes`, {}),
  suggestedNotes: (id: string) => request<{ suggestions: { id: string; text: string; why: string; untrusted: boolean }[] }>("GET", `/api/sessions/${id}/suggested-notes`),
  acceptSuggestedNote: (id: string, sid: string) => request<{ id: string; added: boolean }>("POST", `/api/sessions/${id}/suggested-notes/${sid}/accept`, {}),
  dismissSuggestedNote: (id: string, sid: string) => request<{ dismissed: boolean }>("POST", `/api/sessions/${id}/suggested-notes/${sid}/dismiss`, {}),
  forkChat: (id: string, at: number) => request<{ id: string; parent: string; fork_at: number }>("POST", `/api/sessions/${id}/fork`, { at }),
  forgetChat: (id: string) => request<{ forgotten: boolean }>("POST", `/api/sessions/${id}/forget`, {}),
  searchStatus: () => request<{ available: boolean; provider?: string; how_to_enable?: string }>("GET", "/api/search/status"),
  searchPropose: (query: string, recency?: string) => request<SearchProposal>("POST", "/api/search/propose", recency ? { query, recency } : { query }),
  searchRun: (id: string, digest: string) => request<SearchAnswer>("POST", `/api/search/${id}/run`, { approve: true, digest }),
  exportNotesPlain: () => request<unknown>("GET", "/api/memories/export"),
  exportNotesSealed: () => request<{ file: string; recovery_key: string; notes: number; filename: string; note: string }>("POST", "/api/memories/export-sealed", {}),
  restoreNotes: (doc: unknown) => request<{ added: number; already_known: number; refused: number }>("POST", "/api/memories/import", doc),
  restoreNotesSealed: (file: string, key: string) => request<{ added: number; already_known: number; refused: number }>("POST", "/api/memories/import-sealed", { file, key }),
  importNotes: (source: string, notes: { text: string; name: string }[]) => request<{ added: number; already_known: number; refused: number; instruction_like: number; note: string }>("POST", "/api/memories/import-notes", { source, notes }),
  researchPropose: (urls: string[]) => request<ResearchProposal>("POST", "/api/research/propose", { urls }),
  researchFetch: (id: string, digest: string) => request<{ pages: ResearchPage[]; note: string }>("POST", `/api/research/${id}/fetch`, { approve: true, digest }),
  ask: (folder: string, question: string) => request<AskReply>("POST", "/api/ask", { folder, question }),
  codeStart: (folder: string, task: string) => request<{ id: string }>("POST", "/api/code", { folder, task }),
  codeList: () => request<{ tasks: { id: string; task: string; state: string; created: number }[] }>("GET", "/api/code"),
  codeGet: (id: string) => request<CodeTask>("GET", `/api/code/${id}`),
  codeApply: (id: string, digest: string) => request<{ applied: boolean; files: number }>("POST", `/api/code/${id}/apply`, { digest }),
  /** Asks a running review or coding task to stop. `at_once` is false when it can only end at its own end. */
  stopWork: (kind: "review" | "code", id: string) => request<{ stopping: boolean; at_once: boolean }>("POST", `/api/${kind === "review" ? "jobs" : "code"}/${encodeURIComponent(id)}/stop`, {}),
  codeUndo: (id: string) => request<{ undone: boolean }>("POST", `/api/code/${id}/undo`, {}),
  readiness: () => request<Readiness>("GET", "/api/readiness"),
  fs: (path?: string) => request<{ path: string; parent: string | null; dirs: { name: string; path: string }[] }>("GET", `/api/fs${path ? `?path=${encodeURIComponent(path)}` : ""}`),
  startJob: (repo: string, recipient: string) => request<{ id: string }>("POST", "/api/jobs", { repo, recipient }),
  jobs: () => request<{ jobs: Job[] }>("GET", "/api/jobs"),
  job: (id: string) => request<Job>("GET", `/api/jobs/${id}`),
  files: (id: string) => request<{ files: FileEntry[] }>("GET", `/api/jobs/${id}/files`),
  file: (id: string, path: string) => request<{ path: string; withheld: boolean; text: string | null }>("GET", `/api/jobs/${id}/file?path=${encodeURIComponent(path)}`),
  audit: (id: string) => request<{ audit: AuditEntry[] }>("GET", `/api/jobs/${id}/audit`),
  shareable: (id: string) => request<{ findings: { n: number; title: string }[]; signer_public?: string }>("GET", `/api/jobs/${id}/share`),
  shareFinding: (id: string, finding: number) => request<{ name: string; share: string; files: string[]; total_files: number; shows: string; hides: string; signer_public: string }>("POST", `/api/jobs/${id}/share`, { finding }),
  findings: (id: string) => request<{ findings: Finding[]; unparsed: number }>("GET", `/api/jobs/${id}/findings`),
  release: (id: string) => request<Release>("GET", `/api/jobs/${id}/release`),
  requestApproval: (id: string) => request<{ approval: string; digest: string; how: string }>("POST", `/api/jobs/${id}/approval`, {}),
  approval: (id: string) => request<{ state: string; approval?: string; digest?: string }>("GET", `/api/jobs/${id}/approval`),
  deliver: (id: string) => request<{ name: string; digest: string; signer: string }>("POST", `/api/jobs/${id}/deliver`, {}),
};
