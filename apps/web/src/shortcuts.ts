// Every keyboard shortcut the workspace answers to, in one table. The help dialog is generated from this table, and the
// shortcuts that act on the whole page are found through it, so the list cannot say more or less than what works.

export interface KeyEventLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  isComposing?: boolean;
}

export type Scope = "app" | "chat";

export interface Shortcut {
  id: string;
  scope: Scope;
  /** How the keys are written on a Mac and elsewhere. */
  mac: string;
  other: string;
  /** The text that says what it does (a key of the language files). */
  label: string;
  /** True when the shortcut still works while the cursor is in a text box. */
  editable: boolean;
  /** True when the shortcut is a single printable key with no modifier: it can be switched off (WCAG 2.1.4, Character Key Shortcuts). */
  single?: boolean;
  matches(e: KeyEventLike): boolean;
  /** Events that match, used by the tests and nowhere else. */
  samples: readonly KeyEventLike[];
}

const none = { ctrlKey: false, metaKey: false, altKey: false, shiftKey: false };
const mod = (e: KeyEventLike) => e.metaKey || e.ctrlKey;
const letter = (e: KeyEventLike, k: string) => e.key.toLowerCase() === k;

export const SHORTCUTS: readonly Shortcut[] = [
  {
    id: "palette",
    scope: "app",
    mac: "⌘⇧P",
    other: "Ctrl+Shift+P",
    label: "kb.palette",
    editable: true,
    matches: (e) => mod(e) && e.shiftKey && !e.altKey && letter(e, "p"),
    samples: [{ key: "P", ctrlKey: true, metaKey: false, altKey: false, shiftKey: true }],
  },
  {
    id: "search",
    scope: "app",
    mac: "⌘K",
    other: "Ctrl+K",
    label: "kb.search",
    editable: true,
    matches: (e) => mod(e) && !e.shiftKey && !e.altKey && letter(e, "k"),
    samples: [{ key: "k", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false }],
  },
  {
    id: "help",
    scope: "app",
    mac: "?",
    other: "?",
    label: "kb.help",
    editable: false,
    single: true,
    matches: (e) => e.key === "?" && !e.ctrlKey && !e.metaKey && !e.altKey,
    samples: [{ key: "?", ctrlKey: false, metaKey: false, altKey: false, shiftKey: true }],
  },
  {
    id: "help-anywhere",
    scope: "app",
    mac: "⌘/",
    other: "Ctrl+/",
    label: "kb.helpAnywhere",
    editable: true,
    matches: (e) => mod(e) && !e.shiftKey && !e.altKey && e.key === "/",
    samples: [{ key: "/", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false }],
  },
  {
    id: "send",
    scope: "chat",
    mac: "Enter",
    other: "Enter",
    label: "kb.send",
    editable: true,
    matches: (e) => e.key === "Enter" && !e.shiftKey && !e.isComposing,
    samples: [{ key: "Enter", ...none }],
  },
  {
    id: "newline",
    scope: "chat",
    mac: "⇧Enter",
    other: "Shift+Enter",
    label: "kb.newline",
    editable: true,
    matches: (e) => e.key === "Enter" && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey,
    samples: [{ key: "Enter", ctrlKey: false, metaKey: false, altKey: false, shiftKey: true }],
  },
  {
    id: "stop",
    scope: "chat",
    mac: "Esc, ⌃G",
    other: "Esc, Ctrl+G",
    label: "kb.stop",
    editable: true,
    matches: (e) =>
      !e.metaKey && !e.altKey && !e.shiftKey && !e.isComposing && ((e.key === "Escape" && !e.ctrlKey) || (e.key.toLowerCase() === "g" && e.ctrlKey)),
    samples: [
      { key: "Escape", ...none },
      { key: "g", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false },
    ],
  },
  {
    id: "history",
    scope: "chat",
    mac: "↑ ↓",
    other: "Up, Down",
    label: "kb.history",
    editable: true,
    matches: (e) => (e.key === "ArrowUp" || e.key === "ArrowDown") && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && !e.isComposing,
    samples: [{ key: "ArrowUp", ...none }],
  },
  {
    id: "picker",
    scope: "chat",
    mac: "⇧↑",
    other: "Shift+Up",
    label: "kb.picker",
    editable: true,
    matches: (e) => e.key === "ArrowUp" && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey && !e.isComposing,
    samples: [{ key: "ArrowUp", ctrlKey: false, metaKey: false, altKey: false, shiftKey: true }],
  },
];

/** Whether the focus is somewhere text is typed, where a bare key is a letter and not a command. */
export function typingTarget(t: { tagName?: string; isContentEditable?: boolean; type?: string } | null | undefined): boolean {
  if (!t) return false;
  const tag = (t.tagName ?? "").toUpperCase();
  // a checkbox, radio button, button or slider takes no typed text, so a bare key there is still a command
  if (tag === "INPUT" && NO_TEXT_INPUT.has((t.type ?? "").toLowerCase())) return false;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable === true;
}

const NO_TEXT_INPUT = new Set(["checkbox", "radio", "button", "submit", "reset", "range", "color", "file", "image"]);

/** The page-wide shortcut this key press is, if any. A key that is only a letter in a text box is never one. */
export function appShortcut(e: KeyEventLike, target: Parameters<typeof typingTarget>[0], singleKeys = true): Shortcut | null {
  const typing = typingTarget(target);
  for (const s of SHORTCUTS) {
    if (s.scope !== "app" || !s.matches(e)) continue;
    // switched off by the person: a single printable key does nothing, the same shortcut with a modifier still works
    if (s.single && !singleKeys) return null;
    if (typing && !s.editable) return null;
    return s;
  }
  return null;
}

/** The keys of one shortcut as they are written on this kind of computer. */
export function keysFor(s: Shortcut, mac: boolean): string {
  return mac ? s.mac : s.other;
}
