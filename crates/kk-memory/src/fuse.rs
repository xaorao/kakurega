//! Ranking for recall: BM25 keyword order and a typo-tolerant wording similarity, merged by reciprocal rank fusion.
//!
//! The fusion rule (a note's score is the sum of 1 / (60 + rank) over every list it appears in, ranks counted from 1)
//! and the sizing of the candidate pool (eight times the limit, between 64 and 512) follow the behaviour of
//! Anamnesis's hybrid search. This module is written for this project; ties are broken deterministically, which the
//! source does not do.

use std::collections::{BTreeMap, BTreeSet};

pub const RRF_K: f64 = 60.0;
pub const POOL_FLOOR: usize = 64;
pub const POOL_CEIL: usize = 512;

pub fn pool_for_limit(limit: usize) -> usize {
    if limit == 0 {
        return 0;
    }
    limit
        .saturating_mul(8)
        .clamp(POOL_FLOOR, POOL_CEIL)
        .max(limit)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fused {
    pub id: String,
    pub score: f64,
    /// 1-based rank in each input list, `None` where the note was absent.
    pub ranks: Vec<Option<usize>>,
}

/// Merges ranked lists of note ids. Equal scores are ordered by id, so the result never depends on hashing or arrival order.
pub fn fuse(lists: &[Vec<String>]) -> Vec<Fused> {
    let mut acc: BTreeMap<&str, Fused> = BTreeMap::new();
    for (li, list) in lists.iter().enumerate() {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut rank = 0usize;
        for id in list {
            if !seen.insert(id.as_str()) {
                continue;
            }
            rank += 1;
            let e = acc.entry(id.as_str()).or_insert_with(|| Fused {
                id: id.clone(),
                score: 0.0,
                ranks: vec![None; lists.len()],
            });
            e.score += 1.0 / (RRF_K + rank as f64);
            e.ranks[li] = Some(rank);
        }
    }
    let mut out: Vec<Fused> = acc.into_values().collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    out
}

fn words(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
}

/// The set of three-character pieces of every word, each word padded with a space at both ends.
pub fn trigrams(s: &str) -> BTreeSet<[char; 3]> {
    let mut out = BTreeSet::new();
    for w in words(s) {
        let chars: Vec<char> = std::iter::once(' ')
            .chain(w.chars())
            .chain(std::iter::once(' '))
            .collect();
        for t in chars.windows(3) {
            out.insert([t[0], t[1], t[2]]);
        }
    }
    out
}

/// The share of the query's pieces that the text also has: 1.0 when every piece is present. Short queries against long
/// notes work because only the query's side is counted.
pub fn coverage(query: &BTreeSet<[char; 3]>, text: &BTreeSet<[char; 3]>) -> f64 {
    if query.is_empty() {
        return 0.0;
    }
    query.intersection(text).count() as f64 / query.len() as f64
}

/// Notes whose wording is close enough to the query to be worth ranking, best first. Ties go to the lower index so the
/// order never depends on hashing.
pub fn rank_similar(query: &str, notes: &[(String, &str)], min: f64) -> Vec<String> {
    let q = trigrams(query);
    if q.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(f64, usize)> = notes
        .iter()
        .enumerate()
        .filter_map(|(i, (_, text))| {
            let c = coverage(&q, &trigrams(text));
            (c >= min).then_some((c, i))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    scored
        .into_iter()
        .map(|(_, i)| notes[i].0.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fu01_pool_size_has_a_floor_a_ceiling_and_never_goes_below_the_limit() {
        assert_eq!(pool_for_limit(0), 0);
        assert_eq!(pool_for_limit(1), 64);
        assert_eq!(pool_for_limit(20), 160);
        assert_eq!(pool_for_limit(200), 512);
        assert_eq!(pool_for_limit(1000), 1000);
    }

    #[test]
    fn fu02_a_note_in_both_lists_beats_a_note_first_in_one() {
        let f = fuse(&[ids(&["a", "b", "c"]), ids(&["c", "b"])]);
        assert_eq!(
            f.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["c", "b", "a"],
            "{f:?}"
        );
        assert!((f[0].score - (1.0 / 63.0 + 1.0 / 61.0)).abs() < 1e-12);
        assert_eq!(f[0].ranks, vec![Some(3), Some(1)]);
        assert_eq!(f[1].ranks, vec![Some(2), Some(2)]);
        assert_eq!(f[2].ranks, vec![Some(1), None], "present in one list only");
        assert!(
            f[2].score < f[1].score,
            "first in one list is worth less than middling in both"
        );
    }

    #[test]
    fn fu03_ties_resolve_the_same_way_every_time() {
        let want = fuse(&[ids(&["x", "y"]), ids(&["y", "x"])]);
        assert_eq!(
            want[0].id, "x",
            "equal score, equal best rank, so the id decides"
        );
        for _ in 0..50 {
            assert_eq!(fuse(&[ids(&["x", "y"]), ids(&["y", "x"])]), want);
        }
        let f = fuse(&[ids(&["m", "n"]), ids(&["o"])]);
        assert_eq!(
            f.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["m", "o", "n"],
            "equal scores: better best rank first, then id"
        );
    }

    #[test]
    fn fu04_duplicates_inside_one_list_count_once_and_empty_input_is_empty() {
        let f = fuse(&[ids(&["a", "a", "b"])]);
        assert_eq!(f[0].ranks, vec![Some(1)]);
        assert_eq!(
            f[1].ranks,
            vec![Some(2)],
            "the repeat does not use up a rank"
        );
        assert!(fuse(&[]).is_empty() && fuse(&[vec![]]).is_empty());
    }

    #[test]
    fn fu05_similar_wording_survives_plurals_and_small_typos_but_not_unrelated_text() {
        let notes = vec![
            (
                "n1".to_string(),
                "Acme pays invoices on the 20th of each month",
            ),
            ("n2".to_string(), "Tomatoes need staking in June"),
            ("n3".to_string(), "Invoice numbers start at 1000"),
        ];
        let r = rank_similar("acme payd invoice", &notes, 0.4);
        assert_eq!(r.first().map(String::as_str), Some("n1"), "{r:?}");
        assert!(!r.contains(&"n2".to_string()), "{r:?}");
        assert!(rank_similar("zzzz qqqq", &notes, 0.4).is_empty());
        assert!(rank_similar("", &notes, 0.0).is_empty());
        assert_eq!(coverage(&trigrams(""), &trigrams("abc")), 0.0);
        let hindi = trigrams("चालान भुगतान");
        assert!(coverage(&hindi, &trigrams("हर महीने चालान भुगतान 20 तारीख को")) > 0.9);
    }
}
