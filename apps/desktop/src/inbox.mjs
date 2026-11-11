// What waits for the owner, as the desktop shell learns it, and the system notifications it shows about it.
//
// The rules are the web app's (apps/web/src/notify.ts): only four kinds are announced, each item once, at most three at a
// time, nothing that was already waiting when the shell started, and never while the window is in front. The text says that
// something waits and never what, for whom or in which project: it goes through the operating system, which shows it on a
// locked screen and keeps it in a history this program cannot clear. Items are cut down to their kind and ids as soon as
// they are read, so nothing that describes the work can reach a notification by mistake.
//
// The shell reads the inbox through its admin token. `GET /admin/inbox` is the inbox itself; a service without that route
// answers 404, and the shell then works the reviews' part of the inbox out of `GET /admin/jobs` (team runs are not visible
// there, so a team's question is not announced by such a service).

export const ANNOUNCED = new Set(["approval_waiting", "ready_to_review", "failed", "team_question"]);
export const KINDS = new Set(["approval_waiting", "ready_to_review", "ready_to_deliver", "failed", "suggestions", "team_planned", "team_question", "team_interrupted"]);
export const MAX_PER_UPDATE = 3;
/** The service's wording for a review the owner stopped (kk-review `STOPPED_BY_OWNER`): that needs nothing from the owner. */
export const STOPPED_BY_OWNER = "stopped by you";

const ID = /^[A-Za-z0-9_-]{1,64}$/;

export const keyOf = (i) => `${i.kind}:${i.job ?? `${i.team ?? ""}#${i.task ?? 0}`}:${i.since}`;

/** One inbox item reduced to what the shell may use: its kind, the ids it points at and when it began. Anything else is dropped. */
export function clean(raw) {
  if (!raw || typeof raw !== "object" || !KINDS.has(raw.kind)) return null;
  const out = { kind: raw.kind, since: Number.isFinite(raw.since) ? raw.since : 0 };
  if (typeof raw.job === "string" && ID.test(raw.job)) out.job = raw.job;
  if (typeof raw.team === "string" && ID.test(raw.team)) out.team = raw.team;
  if (Number.isInteger(raw.task) && raw.task >= 0) out.task = raw.task;
  if (!out.job && !out.team) return null;
  return out;
}

/** The reviews' part of the inbox, from the admin view of the reviews, by the same rules as the service's inbox. */
export function fromJobs(jobs) {
  const items = [];
  for (const j of Array.isArray(jobs) ? jobs : []) {
    if (!j || typeof j.id !== "string" || !ID.test(j.id)) continue;
    if (j.state === "ready" && !j.delivered) {
      const kind = j.approval === "waiting" ? "approval_waiting" : typeof j.approval === "string" && j.approval !== "denied" ? "ready_to_deliver" : "ready_to_review";
      items.push({ kind, job: j.id, since: 0 });
    }
    if (j.state === "failed" && j.note !== STOPPED_BY_OWNER) items.push({ kind: "failed", job: j.id, since: 0 });
  }
  return items;
}

/** The inbox now: `{ items, complete }`, where `complete` is false when team runs could not be seen. Throws when the service does not answer. */
export async function readInbox(client) {
  const r = await client.get("/admin/inbox");
  if (r.status === 200 && Array.isArray(r.json?.items)) return { items: r.json.items.map(clean).filter(Boolean), complete: true };
  if (r.status !== 404) throw new Error(`the inbox could not be read (${r.status})`);
  const jobs = await client.get("/admin/jobs");
  if (jobs.status !== 200) throw new Error(`the reviews could not be read (${jobs.status})`);
  return { items: fromJobs(jobs.json?.jobs), complete: false };
}

/** The screen of the app a notification about this item opens. Only well-formed ids are put in an address. */
export function targetFor(item) {
  if (item?.team && ID.test(item.team)) return `#/team/${item.team}`;
  if (item?.job && ID.test(item.job)) return `#/job/${item.job}`;
  return "#/inbox";
}

/**
 * `show(title, body, target, kind)` puts one notification on screen; `hasFocus()` says whether the owner is looking at the
 * window; `enabled()` whether the owner wants notifications; `text(kind)` the words for a kind.
 */
export function createNotifier({ show, hasFocus, enabled, text }) {
  const seen = new Set();
  let baseline = false;
  return {
    /** The inbox as it is now. Returns how many notifications were shown. Never throws. */
    observe(items) {
      let shown = 0;
      try {
        const fresh = (Array.isArray(items) ? items : []).filter((i) => i && !seen.has(keyOf(i)));
        for (const i of fresh) seen.add(keyOf(i));
        if (!baseline) {
          baseline = true;
          return 0;
        }
        if (!enabled()) return 0;
        for (const i of fresh) {
          if (shown >= MAX_PER_UPDATE) break;
          if (!ANNOUNCED.has(i.kind) || hasFocus()) continue;
          try {
            show("Kakurega", text(`notify.${i.kind}`), targetFor(i), i.kind);
            shown++;
          } catch {
            /* a notification that cannot be shown is not retried */
          }
        }
      } catch {
        /* a notification must never break the shell */
      }
      return shown;
    },
  };
}
