export interface CommentFile { name: string; text: string | null }

/**
 * Text from a finding, inside a fenced block: GitHub renders nothing in one, so it cannot notify a person, link an issue
 * or a page, or become markup. The fence is longer than any run of backticks in the text, and characters that cannot be
 * seen (and could hide or reorder what is shown) are dropped.
 */
export function fenced(text: string): string {
  const clean = text.replace(/[\u0000-\u0008\u000b-\u001f\u007f\u200b-\u200f\u202a-\u202e\u2066-\u2069\ufeff]/g, "").replace(/\r/g, "");
  const longest = Math.max(0, ...(clean.match(/`+/g) ?? []).map((r) => r.length));
  const fence = "`".repeat(Math.max(3, longest + 1));
  return `${fence}text\n${clean}\n${fence}`;
}

function code(text: string): string {
  return "`" + text.replace(/[`\r\n]/g, " ").slice(0, 300) + "`";
}

/**
 * A pull-request comment made only from the files of a release that was approved and delivered: each confirmed
 * finding with where it is and why, the commits when the review was of a change, and how to check the signed capsule.
 * Nothing that was not in the release is in it. Null when the release holds no finding that can be read.
 */
export function prComment(files: CommentFile[], digest: string): string | null {
  const byName = new Map(files.map((f) => [f.name, f.text ?? ""]));
  const findings: { title: string; where: string; why: string }[] = [];
  for (let n = 1; byName.has(`finding-${n}.txt`); n++) {
    const text = byName.get(`finding-${n}.txt`) ?? "";
    const m = /^Finding \d+: (.*)\nLocation: (.*)\n\n([\s\S]*?)\n\nHow to check: /.exec(text);
    if (!m) return null;
    findings.push({ title: m[1] ?? "", where: m[2] ?? "", why: m[3] ?? "" });
  }
  if (findings.length === 0) return null;
  const out: string[] = [];
  out.push(`### Private security review: ${findings.length === 1 ? "1 confirmed finding" : `${findings.length} confirmed findings`}`, "");
  const change = byName.get("change.txt") ?? "";
  const head = /^reviewed commit: ([0-9a-f]{40,64})$/m.exec(change);
  const base = /^base commit: ([0-9a-f]{40,64})$/m.exec(change);
  if (head && base) out.push(`Reviewed commit ${code(head[1] ?? "")}, measured from ${code(base[1] ?? "")}. Only findings that touch this change are listed; code the change did not touch is not covered.`, "");
  findings.forEach((f, i) => {
    out.push(`**Finding ${i + 1}** at ${code(f.where)}`, "", fenced(`${f.title}\n\n${f.why}`), "");
  });
  out.push(
    "How this was checked: for each finding a script was run in a sealed sandbox. It first showed the normal behaviour working, then reproduced the defect twice with the same output, and stopped reproducing it when the cited file was emptied.",
    "",
    `The scripts, the minimal samples and the expected output are in the signed capsule \`review.kkg\` (capsule ${code(digest.slice(0, 32))}). Anyone holding the capsule can check it offline with \`kaku verify review.kkg\`, which prints that capsule name and each signed claim, and can re-run each script with the \`run-check.py\` inside it. The review ran privately; the project's code was not posted here.`,
  );
  return out.join("\n") + "\n";
}
