/** Sentences the service writes in English into a release and its disclosure check. They are part of what is signed, so
 *  the service never changes them; the page shows each in the page's language when it knows the sentence, and as it came
 *  otherwise. */
/** A key of the page's text and the values to put into it. */
type Tr = (key: string, params?: Record<string, string | number>) => string;

const EXACT: Record<string, string> = {
  "That the private source repository is the one that was reviewed: only a keyed commitment to its file list is signed, and the key stays with the owner.": "srv.notEst.repo",
  "That the defect exists in the full private code: the check ran on the released minimal sample only.": "srv.notEst.full",
  "That no other defects exist: the review lists what it found, not what is absent.": "srv.notEst.others",
  "labelled secret or restricted; never read by the agent and never released": "srv.withheld.why",
  "Distinct source files already shown to this recipient for this project, with every spelling of the recipient name that reduces to the same letters and digits.": "srv.disc.checked.files",
  "Number of releases to this recipient in the last 24 hours.": "srv.disc.checked.releases",
  "Share of the project's bytes already shown, so a few large files cannot hide behind a low file count.": "srv.disc.checked.bytes",
  "Whether combined excerpts reveal something that no single file shows.": "srv.disc.not.combined",
  "Other names or organisations that the same people may use.": "srv.disc.not.names",
  "Information the recipient obtained from sources other than Kakurega.": "srv.disc.not.elsewhere",
};

const PATTERNS: [RegExp, (m: RegExpMatchArray, t: Tr) => string][] = [
  [/^(\d+) withheld files?$/, (m, t) => t(m[1] === "1" ? "srv.withheld.what.one" : "srv.withheld.what", { n: Number(m[1]) })],
  [
    /^Together with earlier releases, (\d+) of this project's (\d+) files \((\d+)% by (size|file count)\) would have been shown to this recipient, above the (\d+)% (limit|review threshold)\.$/,
    (m, t) => t(m[6] === "limit" ? "srv.disc.overLimit" : "srv.disc.overReview", { files: Number(m[1]), total: Number(m[2]), pct: Number(m[3]), measure: t(m[4] === "size" ? "srv.disc.bySize" : "srv.disc.byCount"), limit: Number(m[5]) }),
  ],
  [/^(\d+) releases went to this recipient in the last 24 hours; the limit is (\d+)\.$/, (m, t) => t("srv.disc.perDay", { n: Number(m[1]), limit: Number(m[2]) })],
];

/** The sentence in the page's language through `t` when it is one the page knows, else exactly as the service wrote it. */
export function serverText(s: string, english: boolean, t: Tr): string {
  if (english) return s;
  const key = EXACT[s];
  if (key) return t(key);
  for (const [re, f] of PATTERNS) {
    const m = s.match(re);
    if (m) return f(m, t);
  }
  return s;
}
