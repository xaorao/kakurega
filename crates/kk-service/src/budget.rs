//! How much of the model's window the chosen web pages may use. Without this, a few long pages in a script that costs
//! many tokens (Hindi, Gujarati) are larger than the window and the model refuses the whole question.

use kk_route::tokens::estimate_text;

/// Room kept for the reply: 15% of the window, or 16k tokens for a window of at least 32k (OpenCode `session/compaction.ts`
/// `calculateCeiling`, RESERVE_SHARE and RESERVE_MIN).
pub fn reply_reserve(window: u64) -> u64 {
    let share = window * 15 / 100;
    let min = if window >= 32_000 { 16_000 } else { 0 };
    share.max(min)
}

/// A page is only worth trimming if at least this many tokens of its text still fit.
const MIN_PAGE_TOKENS: u64 = 200;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Fitted {
    pub text: String,
    pub used: usize,
    /// Pages whose end was left out so the rest fits.
    pub trimmed: usize,
    /// Pages that did not fit at all.
    pub left_out: usize,
}

const MARKER: &str = "\n[the rest of this page was left out to fit the model's window]\n\n";
/// Where a part of a page was left out between two parts that were kept.
const GAP: &str = "\n[part of this page was left out here to fit the model's window]\n";
/// The size of the parts a page is cut into when the parts that match the question are kept, in tokens.
const PART_TOKENS: u64 = 300;

/// The words of a question worth looking for: lower case, at least three letters (or any word in a script without
/// spaces-between-short-words, such as Devanagari), without the commonest English words.
fn terms(text: &str) -> Vec<String> {
    const COMMON: &[&str] = &[
        "the",
        "and",
        "for",
        "are",
        "was",
        "what",
        "which",
        "who",
        "whom",
        "this",
        "that",
        "with",
        "from",
        "how",
        "why",
        "when",
        "where",
        "does",
        "did",
        "has",
        "have",
        "had",
        "not",
        "but",
        "you",
        "your",
        "our",
        "its",
        "into",
        "about",
        "according",
        "file",
        "attached",
        "page",
        "answer",
        "only",
        "say",
        "says",
        "tell",
        "please",
        "there",
        "their",
        "they",
        "them",
        "then",
        "than",
        "can",
        "could",
        "would",
        "should",
        "will",
        "any",
        "all",
        "one",
    ];
    let mut out: Vec<String> = Vec::new();
    for w in text
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .map(|w| w.trim_matches(|c| c == '-' || c == '_').to_lowercase())
    {
        let long = w.chars().count() >= 3 || !w.is_ascii();
        if long && !COMMON.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// A page cut into parts of about [`PART_TOKENS`], at line ends where it can be.
fn parts(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in body.split_inclusive('\n') {
        if !cur.is_empty() && estimate_text(&cur) + estimate_text(line) > PART_TOKENS {
            out.push(std::mem::take(&mut cur));
        }
        cur.push_str(line);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// What a page keeps when it does not fit and the question names something in it: its first part, then the parts that
/// match the question best (more of its words, rarer words counting more), in the page's own order, with each gap marked.
/// None when the question matches nothing beyond the first part, so the start of the page is kept instead.
fn cut_by_question(header: &str, body: &str, room: u64, question: &str) -> Option<String> {
    let words = terms(question);
    let ps = parts(body);
    if words.is_empty() || ps.len() < 2 {
        return None;
    }
    let lower: Vec<String> = ps.iter().map(|p| p.to_lowercase()).collect();
    // a word in few parts tells more than one in many
    let weight: Vec<f64> = words
        .iter()
        .map(|w| {
            let n = lower.iter().filter(|p| p.contains(w.as_str())).count();
            if n == 0 {
                0.0
            } else {
                // a word in every part points at none of them
                (ps.len() as f64 / n as f64).ln()
            }
        })
        .collect();
    let score = |i: usize| -> f64 {
        words
            .iter()
            .zip(&weight)
            .filter(|(w, _)| lower[i].contains(w.as_str()))
            .map(|(_, wt)| *wt)
            .sum()
    };
    let mut ranked: Vec<(usize, f64)> = (1..ps.len())
        .map(|i| (i, score(i)))
        .filter(|(_, s)| *s > 0.0)
        .collect();
    if ranked.is_empty() {
        return None;
    }
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let fixed = estimate_text(&format!("{header}\n{MARKER}"));
    let gap = estimate_text(GAP);
    let mut keep = vec![false; ps.len()];
    let mut used = fixed;
    let first = estimate_text(&ps[0]);
    if used + first > room {
        return None;
    }
    keep[0] = true;
    used += first;
    for (i, _) in ranked {
        let cost = estimate_text(&ps[i]) + gap;
        if used + cost <= room {
            keep[i] = true;
            used += cost;
        }
    }
    if !keep[1..].iter().any(|k| *k) {
        return None;
    }
    // the room that is left goes to the rest of the page in its order, so as much of it is given as fits
    for i in 1..ps.len() {
        if !keep[i] {
            let cost = estimate_text(&ps[i]) + gap;
            if used + cost <= room {
                keep[i] = true;
                used += cost;
            }
        }
    }
    let mut text = format!("{header}\n");
    for (i, p) in ps.iter().enumerate() {
        if keep[i] {
            if i > 0 && !keep[i - 1] {
                text.push_str(GAP);
            }
            text.push_str(p);
        }
    }
    text.push_str(if keep[ps.len() - 1] { "\n\n" } else { MARKER });
    Some(text)
}

/// The longest start of a page that fits in `room` tokens, marked as cut, or None if too little of it would fit to be useful.
fn cut(header: &str, body: &str, room: u64) -> Option<String> {
    let fixed = estimate_text(&format!("{header}\n{MARKER}"));
    if room < fixed + MIN_PAGE_TOKENS {
        return None;
    }
    let chars: Vec<char> = body.chars().collect();
    let fits = |n: usize| estimate_text(&chars[..n].iter().collect::<String>()) + fixed <= room;
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let kept: String = chars[..lo].iter().collect();
    (estimate_text(&kept) >= MIN_PAGE_TOKENS).then(|| format!("{header}\n{kept}{MARKER}"))
}

/// Fits the pages into `budget` tokens. Every page that fits whole is given whole; the room left is then used to cut, in
/// order, the pages that did not fit, so one long page never crowds out a short one. A page that is cut keeps its start and
/// the parts that match `question` best (a fact at the end of a long file is not lost to the cut), or only its start when
/// the question matches nothing in it. The text keeps the owner's order.
pub fn fit_pages(pages: &[(String, String)], budget: u64, question: &str) -> Fitted {
    let mut pieces: Vec<Option<String>> = vec![None; pages.len()];
    let mut left = budget;
    let mut too_long = Vec::new();
    for (i, (header, body)) in pages.iter().enumerate() {
        let whole = format!("{header}\n{body}\n\n");
        let cost = estimate_text(&whole);
        if cost <= left {
            left -= cost;
            pieces[i] = Some(whole);
        } else {
            too_long.push(i);
        }
    }
    let mut out = Fitted::default();
    for i in too_long {
        let room = left;
        match cut_by_question(&pages[i].0, &pages[i].1, room, question)
            .or_else(|| cut(&pages[i].0, &pages[i].1, room))
        {
            Some(piece) => {
                left = left.saturating_sub(estimate_text(&piece));
                pieces[i] = Some(piece);
                out.trimmed += 1;
            }
            None => out.left_out += 1,
        }
    }
    for piece in pieces.into_iter().flatten() {
        out.text.push_str(&piece);
        out.used += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(n: usize, text: &str, chars: usize) -> (String, String) {
        (
            format!("[source: https://example.org/{n} | Page {n}]"),
            text.chars().cycle().take(chars).collect(),
        )
    }

    #[test]
    fn bg01_the_reserve_follows_the_reference() {
        assert_eq!(reply_reserve(16_384), 2_457);
        assert_eq!(reply_reserve(32_000), 16_000);
        assert_eq!(reply_reserve(200_000), 30_000);
        assert_eq!(reply_reserve(0), 0);
    }

    #[test]
    fn bg02_pages_that_fit_are_given_whole_and_in_order() {
        let pages = vec![page(1, "alpha beta ", 400), page(2, "gamma delta ", 400)];
        let f = fit_pages(&pages, 10_000, "");
        assert_eq!((f.used, f.trimmed, f.left_out), (2, 0, 0));
        assert!(f.text.find("example.org/1").unwrap() < f.text.find("example.org/2").unwrap());
        assert!(!f.text.contains("left out"));
    }

    /// Counts only, so a failure never prints the page text.
    fn summary(f: &Fitted) -> (usize, usize, usize, u64) {
        (f.used, f.trimmed, f.left_out, estimate_text(&f.text))
    }

    #[test]
    fn bg03_five_long_hindi_pages_never_exceed_the_budget() {
        let hindi = "तिमाही रिपोर्ट से पता चलता है कि क्षेत्रीय बिक्री बढ़ी जबकि शिपिंग में देरी हुई। ";
        let pages: Vec<_> = (1..=5).map(|n| page(n, hindi, 8000)).collect();
        let one = estimate_text(&format!("{}\n{}\n\n", pages[0].0, pages[0].1));
        // room for one and a half pages: the first fits whole, the second is cut, the rest cannot fit
        let budget = one + one / 2;
        let f = fit_pages(&pages, budget, "");
        let s = summary(&f);
        assert!(s.3 <= budget, "{s:?} over budget {budget}");
        assert_eq!(
            (s.0, s.1, s.2),
            (2, 1, 3),
            "{s:?} with budget {budget}, one page {one}"
        );
        assert!(
            f.text.contains("[the rest of this page was left out"),
            "a cut page says so: {s:?}"
        );
        assert!(
            f.text.contains("example.org/1")
                && f.text.contains("example.org/2")
                && !f.text.contains("example.org/3")
        );
    }

    #[test]
    fn bg04_with_almost_no_room_a_page_is_left_out_instead_of_sent_as_a_scrap() {
        let pages = vec![page(1, "words and more words ", 8000)];
        let f = fit_pages(&pages, 150, "");
        assert_eq!(summary(&f), (0, 0, 1, 0));
        assert!(f.text.is_empty());
        assert_eq!(fit_pages(&pages, 0, "").left_out, 1);
    }

    #[test]
    fn bg05_a_later_small_page_still_fits_after_a_large_one_is_cut() {
        let pages = vec![
            page(1, "a long page of text ", 20_000),
            page(2, "short ", 60),
        ];
        let f = fit_pages(&pages, 2_000, "");
        let s = summary(&f);
        assert!(s.3 <= 2_000, "{s:?}");
        assert_eq!(
            (s.0, s.1, s.2),
            (2, 1, 0),
            "the cut page leaves room for the short one: {s:?}"
        );
    }

    fn ledger(fact_at: usize, lines: usize) -> String {
        (0..lines)
            .map(|i| {
                if i == fact_at {
                    "Final note: the archive passphrase hint for the auditors is HARBOUR-LANTERN-42.\n".to_string()
                } else {
                    format!("Entry {i}: ledger invoice refund approval export finance billing quarter vendor.\n")
                }
            })
            .collect()
    }

    #[test]
    fn bu10_a_cut_page_keeps_its_start_and_the_part_the_question_is_about_in_order_with_the_gaps_marked(
    ) {
        let body = ledger(2_399, 2_400);
        let pages = vec![("[file: long.txt]".to_string(), body)];
        let budget = 3_000;
        let q =
            "What is the archive passphrase hint for the auditors, according to the attached file?";
        let f = fit_pages(&pages, budget, q);
        assert_eq!((f.used, f.trimmed, f.left_out), (1, 1, 0));
        assert!(
            f.text.contains("HARBOUR-LANTERN-42"),
            "the fact at the end is kept"
        );
        assert!(f.text.contains("Entry 0:"), "the start is kept");
        assert!(f.text.contains(GAP.trim()), "a gap is marked");
        assert!(
            estimate_text(&f.text) <= budget,
            "{}",
            estimate_text(&f.text)
        );
        assert!(
            estimate_text(&f.text) > budget * 9 / 10,
            "the room is used, not left empty: {} of {budget}",
            estimate_text(&f.text)
        );
        // the kept parts are in the page's order: the start before the note
        assert!(f.text.find("Entry 0:").unwrap() < f.text.find("HARBOUR").unwrap());
        // the same page with no question keeps only its start, as before
        let plain = fit_pages(&pages, budget, "");
        assert!(!plain.text.contains("HARBOUR") && plain.text.ends_with(MARKER));
        // a question that matches nothing in the page keeps its start too
        let off = fit_pages(&pages, budget, "Was the weather pleasant?");
        assert_eq!(off.text, plain.text);
    }

    #[test]
    fn bu11_a_word_in_every_part_counts_for_nothing_and_a_page_that_fits_is_given_whole() {
        let body = ledger(1_200, 2_400);
        let pages = vec![("[file: long.txt]".to_string(), body.clone())];
        // "ledger" is in every part, so it chooses nothing; "auditors" points at the one part that has it
        let f = fit_pages(&pages, 2_000, "ledger auditors");
        assert!(
            f.text.contains("HARBOUR-LANTERN-42"),
            "the rare word decides"
        );
        let only_common = fit_pages(&pages, 2_000, "ledger invoice");
        assert_eq!(
            only_common.text,
            fit_pages(&pages, 2_000, "").text,
            "words in every part: the start, as with no question"
        );
        // a page that fits is never cut, whatever the question
        let short = vec![("[file: s.txt]".to_string(), ledger(3, 5))];
        let whole = fit_pages(&short, 10_000, "auditors");
        assert_eq!((whole.trimmed, whole.text.contains(GAP.trim())), (0, false));
        assert!(terms("What is the archive hint? औडिट").contains(&"औडिट".to_string()));
        assert!(!terms("What is the hint").contains(&"what".to_string()));
    }
}
