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
/// order, the pages that did not fit, so one long page never crowds out a short one. The text keeps the owner's order.
pub fn fit_pages(pages: &[(String, String)], budget: u64) -> Fitted {
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
        match cut(&pages[i].0, &pages[i].1, left) {
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
        let f = fit_pages(&pages, 10_000);
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
        let f = fit_pages(&pages, budget);
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
        let f = fit_pages(&pages, 150);
        assert_eq!(summary(&f), (0, 0, 1, 0));
        assert!(f.text.is_empty());
        assert_eq!(fit_pages(&pages, 0).left_out, 1);
    }

    #[test]
    fn bg05_a_later_small_page_still_fits_after_a_large_one_is_cut() {
        let pages = vec![
            page(1, "a long page of text ", 20_000),
            page(2, "short ", 60),
        ];
        let f = fit_pages(&pages, 2_000);
        let s = summary(&f);
        assert!(s.3 <= 2_000, "{s:?}");
        assert_eq!(
            (s.0, s.1, s.2),
            (2, 1, 0),
            "the cut page leaves room for the short one: {s:?}"
        );
    }
}
