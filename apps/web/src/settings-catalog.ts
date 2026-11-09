// Every setting the workspace offers, as plain data, so the Settings screen and its search read the same list and a setting
// cannot exist on one and be missing from the other. Labels and help are keys of the language files; the words a person might
// type for a setting are written next to it.

export type SettingKind = "theme" | "language" | "notify" | "link" | "info" | "shortcuts" | "piiallow" | "singlekeys" | "model";
export type SettingSection = "appearance" | "notifications" | "privacy" | "keyboard" | "data";

export interface Setting {
  id: string;
  section: SettingSection;
  kind: SettingKind;
  /** Language-file keys: the name, and what it changes or means. */
  label: string;
  help: string;
  /** Extra words that should find it, besides its label and help. */
  keywords: readonly string[];
  /** For a link, the page it opens. */
  target?: string;
}

export const SECTIONS: readonly SettingSection[] = ["appearance", "notifications", "privacy", "keyboard", "data"];

export const SETTINGS: readonly Setting[] = [
  { id: "theme", section: "appearance", kind: "theme", label: "set.theme.label", help: "set.theme.help", keywords: ["dark mode", "light mode", "colour", "color", "night", "appearance"] },
  { id: "language", section: "appearance", kind: "language", label: "set.language.label", help: "set.language.help", keywords: ["hindi", "english", "locale", "translation", "भाषा"] },
  { id: "notifications", section: "notifications", kind: "notify", label: "set.notify.label", help: "set.notify.help", keywords: ["notify", "alerts", "desktop", "popup", "waiting", "approval"] },
  { id: "model", section: "privacy", kind: "model", label: "set.model.label", help: "set.model.help", keywords: ["llm", "ollama", "qwen", "switch model", "bigger model", "faster", "smaller", "answers"] },
  { id: "where-data-goes", section: "privacy", kind: "link", label: "set.where.label", help: "set.where.help", keywords: ["privacy", "route", "model", "leaves", "network", "policy", "private"], target: "#/privacy" },
  { id: "instructions", section: "privacy", kind: "link", label: "set.instr.label", help: "set.instr.help", keywords: ["orders", "prompt", "style", "tone", "language of answers", "agents"], target: "#/instructions" },
  { id: "agent-access", section: "privacy", kind: "link", label: "set.agent.label", help: "set.agent.help", keywords: ["mcp", "lease", "tools", "agent", "permission"], target: "#/agent" },
  { id: "pii-allow", section: "privacy", kind: "piiallow", label: "set.pii.label", help: "set.pii.help", keywords: ["personal data", "pii", "email", "phone", "allowlist", "allow list", "false positive", "notice", "release", "ignore"] },
  { id: "telemetry", section: "privacy", kind: "info", label: "set.telemetry.label", help: "set.telemetry.help", keywords: ["analytics", "tracking", "usage data", "phone home", "none"] },
  { id: "single-keys", section: "keyboard", kind: "singlekeys", label: "set.single.label", help: "set.single.help", keywords: ["screen reader", "voice control", "dictation", "character key", "accessibility", "question mark", "turn off", "disable", "remap"] },
  { id: "shortcuts", section: "keyboard", kind: "shortcuts", label: "set.keys.label", help: "set.keys.help", keywords: ["keyboard", "keys", "hotkeys", "bindings", "palette"] },
  { id: "memory", section: "data", kind: "link", label: "set.memory.label", help: "set.memory.help", keywords: ["notes", "remember", "forget", "delete", "retention"], target: "#/memory" },
  { id: "usage", section: "data", kind: "link", label: "set.usage.label", help: "set.usage.help", keywords: ["tokens", "speed", "cost", "model usage", "activity"], target: "#/activity" },
  { id: "log", section: "data", kind: "link", label: "set.log.label", help: "set.log.help", keywords: ["export", "download", "audit", "signed", "history"], target: "#/activity" },
];

/**
 * The settings whose name, help or extra words hold every word typed, in any case. A name that matches comes before one that
 * only matches in its help. With nothing typed, all of them, in the order of the catalog.
 */
export function searchSettings(catalog: readonly Setting[], query: string, say: (key: string) => string): Setting[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean).slice(0, 6);
  if (words.length === 0) return [...catalog];
  const scored: { s: Setting; score: number; at: number }[] = [];
  catalog.forEach((s, at) => {
    const name = say(s.label).toLowerCase();
    const rest = `${say(s.help)} ${s.keywords.join(" ")}`.toLowerCase();
    if (!words.every((w) => name.includes(w) || rest.includes(w))) return;
    scored.push({ s, score: words.every((w) => name.includes(w)) ? 0 : 1, at });
  });
  scored.sort((a, b) => a.score - b.score || a.at - b.at);
  return scored.map((x) => x.s);
}
