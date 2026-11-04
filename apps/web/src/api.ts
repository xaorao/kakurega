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

export interface Config {
  version: string;
  route: { id: string; class: string; model?: string; simulated: boolean; transport?: string };
  roots: string[];
  test_mode: boolean;
  signer: string;
  signer_public: string;
}
export interface ReadinessCheck { id: string; label: string; ok: boolean; detail: string; fix: string }
export interface Readiness { ready: boolean; checks: ReadinessCheck[] }
export interface MemoryItem { id: string; text: string; why: string; untrusted: boolean; level: string; compartments: string[]; created_at: number; valid_to: number | null; source: string }
export interface MemoryVersion { id: string; text: string; reason: string; created_at: number; current: boolean }
export interface Suggestion { id: string; text: string; why: string; level: string; untrusted: boolean }
export interface LogEntry { seq: number; at: number; kind: string; subject: string; data: Record<string, string | number> }
export interface LogSummary { count: number; head: string | null; public_key: string; recent: LogEntry[] }
export interface ChatTurn { role: "user" | "assistant"; content: string }
export interface ChatReply { reply: string; route: { id: string; class: string; simulated: boolean }; notes_used: number; notes_withheld: number }
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
export interface Finding { title: string; file: string; line: number; explanation: string; reproducer: string; status: string; output: string; sample: string[] }
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
}

export const api = {
  health: () => request<{ ok: boolean }>("GET", "/api/health"),
  pair: (code: string) => request<{ csrf: string }>("POST", "/api/pair", { code }),
  session: () => request<{ paired: boolean; csrf: string }>("GET", "/api/session"),
  logout: () => request<void>("POST", "/api/logout"),
  config: () => request<Config>("GET", "/api/config"),
  memories: (q?: string) => request<{ memories: MemoryItem[] }>("GET", `/api/memories${q ? `?q=${encodeURIComponent(q)}` : ""}`),
  addMemory: (b: { text: string; level: string; reason?: string }) => request<{ id: string; added: boolean }>("POST", "/api/memories", b),
  correctMemory: (id: string, b: { text: string; reason?: string }) => request<{ id: string }>("POST", `/api/memories/${id}/correct`, b),
  forgetMemory: (id: string, reason?: string) => request<{ forgotten: boolean }>("POST", `/api/memories/${id}/forget`, { reason }),
  memoryHistory: (id: string) => request<{ versions: MemoryVersion[] }>("GET", `/api/memories/${id}/history`),
  suggestions: (job: string) => request<{ suggestions: Suggestion[] }>("GET", `/api/jobs/${job}/memory-suggestions`),
  acceptSuggestion: (job: string, id: string) => request<{ id: string; added: boolean }>("POST", `/api/jobs/${job}/memory-suggestions/${id}/accept`, {}),
  dismissSuggestion: (job: string, id: string) => request<{ dismissed: boolean }>("POST", `/api/jobs/${job}/memory-suggestions/${id}/dismiss`, {}),
  log: () => request<LogSummary>("GET", "/api/log"),
  chat: (message: string, history: ChatTurn[]) => request<ChatReply>("POST", "/api/chat", { message, history }),
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
