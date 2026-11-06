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
  signer: string;
  signer_public: string;
}
export interface ReadinessCheck { id: string; label: string; ok: boolean; required?: boolean; detail: string; fix: string; repairable?: boolean; repair?: { state: "running" | "done" | "failed"; detail: string } }
export interface Readiness { ready: boolean; checks: ReadinessCheck[] }
export interface MemoryItem { id: string; text: string; why: string; untrusted: boolean; level: string; compartments: string[]; created_at: number; valid_to: number | null; source: string; project?: string | null }
export interface MemoryVersion { id: string; text: string; reason: string; created_at: number; current: boolean }
export interface Suggestion { id: string; text: string; why: string; level: string; untrusted: boolean }
export interface LogEntry { seq: number; at: number; kind: string; subject: string; data: Record<string, string | number> }
export interface LogSummary { count: number; head: string | null; public_key: string; recent: LogEntry[] }
export interface ChatTurn { role: "user" | "assistant"; content: string }
export interface ChatReply { instructions_used?: boolean; project?: string | null; reply: string; route: { id: string; class: string; simulated: boolean; stays_here?: boolean; host?: string | null }; notes_used: number; notes?: { id: string; text: string; why: string; untrusted: boolean; project?: string | null }[]; notes_withheld: number; notes_left_out?: number; web_sources_used: number; web_sources_trimmed?: number; web_sources_left_out?: number; saved?: boolean; not_saved?: "full" | "forgotten" | "error" | null; usage?: { context_tokens?: number; context_window?: number | null; estimate?: boolean; context_tokens_estimate: number; reply_tokens_estimate: number; history_turns_sent: number; history_turns_dropped: number; history_turns_summarized?: number; summary_text?: string | null; compacted_now?: boolean; compaction_failed?: string | null } }
export interface Instructions {
  global: { text: string; updated_at: number } | null;
  projects: { project: string; text: string; updated_at: number }[];
  max_chars: number;
}

export interface ReviewSource {
  job: string;
  notes: number;
  suggestions: number;
}

export interface SimilarMemories { groups: MemoryItem[][]; scanned: number; complete: boolean; note: string }
export interface KeptChat { id: string; title: string; created: number; updated: number; turns: number; project?: string | null }
export interface ModelProof { verified: boolean; model: string; checked_at?: number; means?: string[]; does_not_mean?: string[]; deployments?: { signer: string; tcb: string; advisories: string[]; gpu_verified: boolean; compose_hash: string | null; os_image_hash: string | null; nonce: string }[] }
export interface GatewayProofResult { verified: boolean; gateway?: { signer: string; tcb: string; advisories: string[]; compose_hash: string | null; tls_bound: boolean; tls_fingerprint: string | null; nonce: string }; means?: string[]; does_not_mean?: string[] }
export interface Passage { file: string; start: number; focus: number; lines: string[] }
export interface PrivacyFacts { model: { class: string; id: string; name?: string | null; simulated: boolean; host: string | null; stays_here: boolean }; search: { on: boolean; host: string | null }; extension: { on: boolean }; telegram: { on: boolean }; agents: { tools_open: string[]; calls_are_logged: boolean }; notes: { count: number; encrypted_at_rest: boolean }; approvals: { signing_key_enrolled: boolean }; updates: { checks_in_background: boolean } }
export interface Lease { tool: string; remaining: number; expires_at: number; issued_at: number }
export interface InboxItem { kind: "approval_waiting" | "ready_to_review" | "ready_to_deliver" | "failed" | "suggestions"; job: string; recipient: string; since: number; what: string; count?: number }
export interface Schedule { id: string; repo: string; recipient: string; every_hours: number; next_at: number }
export interface ScheduleRun { schedule: string; at: number; outcome: string; job: string | null }
export interface ResearchItem { url: string; host?: string; allowed: boolean; reason?: string }
export interface ResearchProposal { id: string; digest: string; items: ResearchItem[]; will_request: string[]; stays_here: string }
export interface SearchProposal { id: string; digest: string; query: string; recency: string | null; will_send_to: string; stays_here: string }
export interface SearchAnswer { results: SearchHit[]; note: string; engines_failed: string[]; empty_because: string | null }
export interface SearchHit { title: string; snippet: string; url: string; host: string | null; withheld: boolean }
export interface ResearchPage { page_id?: string; url: string; final_url?: string; title?: string; sha256?: string; truncated?: boolean; looks_like_instructions?: boolean; error?: string }
export interface AskCitation { file: string; quote: string; verdict: "verified" | "not_found" | "unknown_file" | "bad_length"; line: number | null }
export interface AskReply { checkable: boolean; answer?: string; citations?: AskCitation[]; verified?: number; unchecked_text?: string; files_used: string[]; files_withheld: string[]; files_skipped: string[]; injection_suspects: string[]; route: { simulated: boolean } }
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
  files?: ReleaseFile[];
  removed?: { what: string; why: string }[];
  not_confirmed: { title: string; why: string }[];
  not_established?: string[];
  personal_data?: { file: string; found: string }[];
}

export const api = {
  health: () => request<{ ok: boolean }>("GET", "/api/health"),
  pair: (code: string) => request<{ csrf: string }>("POST", "/api/pair", { code }),
  session: () => request<{ paired: boolean; csrf: string }>("GET", "/api/session"),
  logout: () => request<void>("POST", "/api/logout"),
  config: () => request<Config>("GET", "/api/config"),
  memories: (q?: string) => request<{ memories: MemoryItem[] }>("GET", `/api/memories${q ? `?q=${encodeURIComponent(q)}` : ""}`),
  addMemory: (b: { text: string; level: string; reason?: string; project?: string }) => request<{ id: string; added: boolean }>("POST", "/api/memories", b),
  correctMemory: (id: string, b: { text: string; reason?: string; project?: string }) => request<{ id: string }>("POST", `/api/memories/${id}/correct`, b),
  forgetMemory: (id: string, reason?: string) => request<{ forgotten: boolean }>("POST", `/api/memories/${id}/forget`, { reason }),
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
  agentAccess: () => request<{ leases: Lease[]; leasable: string[]; now: number }>("GET", "/api/agent-access"),
  agentGrant: (tool: string, uses: number, minutes: number) => request<{ tool: string; remaining: number; expires_at: number }>("POST", "/api/agent-access/grant", { tool, uses, minutes }),
  agentRevoke: (tool?: string) => request<{ revoked: number }>("POST", "/api/agent-access/revoke", tool ? { tool } : {}),
  repair: (id: string) => request<{ started: boolean }>("POST", "/api/readiness/repair", { id }),
  attestGateway: () => request<GatewayProofResult>("POST", "/api/route/gateway", {}),
  attestModel: (model: string) => request<ModelProof>("POST", "/api/route/attest", { model }),
  similarMemories: () => request<SimilarMemories>("GET", "/api/memories/similar"),
  instructions: () => request<Instructions>("GET", "/api/instructions"),
  setInstruction: (project: string | undefined, text: string) => request<{ saved?: boolean; removed?: boolean }>("POST", "/api/instructions", project ? { project, text } : { text }),
  reviewSources: () => request<{ sources: ReviewSource[] }>("GET", "/api/memories/sources"),
  forgetReview: (job: string) => request<ReviewSource & { forgotten: boolean }>("POST", `/api/memories/sources/${encodeURIComponent(job)}/forget`, {}),
  mergeMemories: (keep: string, merge: string[]) => request<{ merged: number; kept: string }>("POST", "/api/memories/merge", { keep, merge }),
  unmergeMemory: (id: string) => request<{ restored: boolean }>("POST", `/api/memories/${id}/unmerge`, {}),
  mergedMemories: () => request<{ merged: { id: string; text: string; merged_into: string; project?: string | null; level: string }[] }>("GET", "/api/memories/merged"),
  keptChats: () => request<{ sessions: KeptChat[] }>("GET", "/api/sessions"),
  keepChat: (title: string, project?: string) => request<{ id: string }>("POST", "/api/sessions", project ? { title, project } : { title }),
  keptChat: (id: string) => request<{ id: string; project?: string | null; turns: ChatTurn[]; summary?: { covered: number; text: string } | null }>("GET", `/api/sessions/${id}`),
  forgetChat: (id: string) => request<{ forgotten: boolean }>("POST", `/api/sessions/${id}/forget`, {}),
  searchStatus: () => request<{ available: boolean; provider?: string; how_to_enable?: string }>("GET", "/api/search/status"),
  searchPropose: (query: string, recency?: string) => request<SearchProposal>("POST", "/api/search/propose", recency ? { query, recency } : { query }),
  searchRun: (id: string, digest: string) => request<SearchAnswer>("POST", `/api/search/${id}/run`, { approve: true, digest }),
  importNotes: (source: string, notes: { text: string; name: string }[]) => request<{ added: number; already_known: number; refused: number; instruction_like: number; note: string }>("POST", "/api/memories/import-notes", { source, notes }),
  researchPropose: (urls: string[]) => request<ResearchProposal>("POST", "/api/research/propose", { urls }),
  researchFetch: (id: string, digest: string) => request<{ pages: ResearchPage[]; note: string }>("POST", `/api/research/${id}/fetch`, { approve: true, digest }),
  ask: (folder: string, question: string) => request<AskReply>("POST", "/api/ask", { folder, question }),
  readiness: () => request<Readiness>("GET", "/api/readiness"),
  fs: (path?: string) => request<{ path: string; parent: string | null; dirs: { name: string; path: string }[] }>("GET", `/api/fs${path ? `?path=${encodeURIComponent(path)}` : ""}`),
  startJob: (repo: string, recipient: string) => request<{ id: string }>("POST", "/api/jobs", { repo, recipient }),
  jobs: () => request<{ jobs: Job[] }>("GET", "/api/jobs"),
  job: (id: string) => request<Job>("GET", `/api/jobs/${id}`),
  files: (id: string) => request<{ files: FileEntry[] }>("GET", `/api/jobs/${id}/files`),
  file: (id: string, path: string) => request<{ path: string; withheld: boolean; text: string | null }>("GET", `/api/jobs/${id}/file?path=${encodeURIComponent(path)}`),
  audit: (id: string) => request<{ audit: AuditEntry[] }>("GET", `/api/jobs/${id}/audit`),
  findings: (id: string) => request<{ findings: Finding[]; unparsed: number }>("GET", `/api/jobs/${id}/findings`),
  release: (id: string) => request<Release>("GET", `/api/jobs/${id}/release`),
  requestApproval: (id: string) => request<{ approval: string; digest: string; how: string }>("POST", `/api/jobs/${id}/approval`, {}),
  approval: (id: string) => request<{ state: string; approval?: string; digest?: string }>("GET", `/api/jobs/${id}/approval`),
  deliver: (id: string) => request<{ name: string; digest: string; signer: string }>("POST", `/api/jobs/${id}/deliver`, {}),
};
