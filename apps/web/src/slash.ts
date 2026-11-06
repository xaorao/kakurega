// Slash commands in the chat box: the same words as the terminal workspace for what the web can do, listed as you type "/".
// The popover's states and keys follow OpenCode's composer (MIT), packages/app/src/composer/suggestions/machine.ts (read in full):
// it opens while the whole box is "/" and letters with no space, the highlight follows the first match until a key moves it,
// Up/Down and Ctrl-N/Ctrl-P move around the ends, Tab or Enter chooses (Enter is not a choice while an input method is composing),
// Escape closes it. Written here for this page; none of its code is used.

import type { Key } from "./i18n";

export type Where = { hash: string } | { job: "open" | "findings" | "release" | "approve" | "deliver" } | { help: true } | { newChat: true };

export interface SlashCommand {
  /** The word after the slash; the same word the terminal uses. */
  name: string;
  /** The language key of its one-line description. */
  about: Key;
  /** Whether it takes a review id after the name (the newest review when none is given). */
  job?: boolean;
  go: Where;
}

export const COMMANDS: readonly SlashCommand[] = [
  { name: "help", about: "slash.help", go: { help: true } },
  { name: "review", about: "slash.review", go: { hash: "#/" } },
  { name: "reviews", about: "slash.reviews", go: { hash: "#/inbox" } },
  { name: "open", about: "slash.open", job: true, go: { job: "open" } },
  { name: "findings", about: "slash.findings", job: true, go: { job: "findings" } },
  { name: "release", about: "slash.release", job: true, go: { job: "release" } },
  { name: "approve", about: "slash.approve", job: true, go: { job: "approve" } },
  { name: "deliver", about: "slash.deliver", job: true, go: { job: "deliver" } },
  { name: "memory", about: "slash.memory", go: { hash: "#/memory" } },
  { name: "remember", about: "slash.remember", go: { hash: "#/memory" } },
  { name: "ask", about: "slash.ask", go: { hash: "#/docs" } },
  { name: "instructions", about: "slash.instructions", go: { hash: "#/instructions" } },
  { name: "usage", about: "slash.usage", go: { hash: "#/activity" } },
  { name: "log", about: "slash.log", go: { hash: "#/activity" } },
  { name: "newchat", about: "slash.newchat", go: { newChat: true } },
];

/** What follows the slash while the whole box is a slash and word characters, or null when the box is not that. */
export function slashQuery(input: string): string | null {
  const m = /^\/(\S*)$/.exec(input);
  return m ? (m[1] ?? "") : null;
}

function inOrder(name: string, q: string): boolean {
  let from = 0;
  for (const c of q) {
    const i = name.indexOf(c, from);
    if (i < 0) return false;
    from = i + 1;
  }
  return true;
}

/** The commands that fit what was typed after the slash: names that start with it, then names that contain it, then names that
 * hold its letters in order. Within each group the list's own order is kept. */
export function matches(query: string): SlashCommand[] {
  const q = query.toLowerCase();
  if (q === "") return [...COMMANDS];
  const starts = COMMANDS.filter((c) => c.name.startsWith(q));
  const inside = COMMANDS.filter((c) => !c.name.startsWith(q) && c.name.includes(q));
  const spread = COMMANDS.filter((c) => !c.name.includes(q) && inOrder(c.name, q));
  return [...starts, ...inside, ...spread];
}

/** A typed line that is a command of ours: its name must be one of the list, written exactly. Anything else is a message. */
export function parseLine(input: string): { cmd: SlashCommand; arg: string } | null {
  const m = /^\/([a-z]+)(?:\s+(\S[^\n]*))?\s*$/.exec(input);
  if (!m) return null;
  const cmd = COMMANDS.find((c) => c.name === m[1]);
  if (!cmd) return null;
  const arg = (m[2] ?? "").trim();
  if (arg !== "" && !cmd.job) return null;
  return { cmd, arg };
}

/** A review id as the service writes them, so nothing else can reach the address bar through a command. */
export function validJob(arg: string): boolean {
  return /^job-\d{1,9}$/.test(arg);
}

export interface PopoverState {
  /** The highlighted row, or -1 for none. */
  active: number;
  /** Whether a key has moved the highlight; until then it follows the first row as the list changes. */
  chosen: boolean;
  /** Closed by Escape until the text changes. */
  dismissed: boolean;
}

export const closedAtStart: PopoverState = { active: 0, chosen: false, dismissed: false };

export interface KeyLike {
  key: string;
  ctrlKey?: boolean;
  isComposing?: boolean;
}

export type Step =
  | { kind: "none" }
  | { kind: "move"; state: PopoverState }
  | { kind: "choose"; index: number }
  | { kind: "close"; state: PopoverState };

/** What a key does while the popover is open with `n` rows. Keys that mean nothing here are left to the box. */
export function onKey(s: PopoverState, n: number, e: KeyLike): Step {
  if (e.key === "Escape") return { kind: "close", state: { ...s, dismissed: true } };
  if (e.key === "Tab" || (e.key === "Enter" && !e.isComposing)) {
    return s.active >= 0 && s.active < n ? { kind: "choose", index: s.active } : { kind: "none" };
  }
  const down = e.key === "ArrowDown" || (!!e.ctrlKey && e.key.toLowerCase() === "n");
  const up = e.key === "ArrowUp" || (!!e.ctrlKey && e.key.toLowerCase() === "p");
  if ((!down && !up) || n === 0) return { kind: "none" };
  const at = s.active < 0 || s.active >= n ? -1 : s.active;
  const next = at < 0 ? (down ? 0 : n - 1) : (at + (down ? 1 : -1) + n) % n;
  return { kind: "move", state: { ...s, active: next, chosen: true } };
}

/** The state after the text changed: a new list, the highlight back on the first row unless the owner has moved it, reopened. */
export function afterInput(s: PopoverState, n: number): PopoverState {
  const active = s.chosen && s.active < n ? s.active : n > 0 ? 0 : -1;
  return { active, chosen: s.chosen && active === s.active, dismissed: false };
}
