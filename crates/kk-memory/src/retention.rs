//! How fresh and how used a note is, as a number between 0 and 1, used only to order notes that already match a question.
//!
//! A note's freshness halves every `half_life_days` since it was last used (or, if it never was, since it was written), and
//! each use adds a little, on a log scale so a note cannot make itself permanent by being used. It never removes, hides or
//! changes a note, and a note that is clearly the best match still comes first: the number is a bounded nudge to the
//! match score (see `FRESHNESS_SPAN`), not a ranking of its own.

pub const DAY_SECS: f64 = 86_400.0;

/// The most a stale, unused note can lose from its match score: a twentieth. Scores in the merge fall by about 1.6% from one
/// place to the next near the top, so this moves a note about three places near the top and further down the list, where scores are closer together.
pub const FRESHNESS_SPAN: f64 = 0.05;

/// A note kept for a project loses relevance faster than one that applies everywhere.
pub const PROJECT_HALF_LIFE_DAYS: f64 = 90.0;
pub const GLOBAL_HALF_LIFE_DAYS: f64 = 180.0;

/// Uses at which the usage part is full; beyond it more uses add nothing.
const FULL_USE: f64 = 20.0;

/// Freshness 6 parts, use 1 part.
const FRESH_PARTS: f64 = 6.0;
const USE_PARTS: f64 = 1.0;

pub fn half_life_days(has_project: bool) -> f64 {
    if has_project {
        PROJECT_HALF_LIFE_DAYS
    } else {
        GLOBAL_HALF_LIFE_DAYS
    }
}

/// `now` and the times are seconds. A time in the future counts as now; a half-life that is not a positive number counts as
/// the global one.
pub fn retention(
    now: i64,
    created_at: i64,
    last_used: Option<i64>,
    uses: u32,
    half_life_days: f64,
) -> f64 {
    let half = if half_life_days.is_finite() && half_life_days > 0.0 {
        half_life_days
    } else {
        GLOBAL_HALF_LIFE_DAYS
    };
    let anchor = last_used.unwrap_or(created_at).max(created_at.min(now));
    let age_days = (now.saturating_sub(anchor)).max(0) as f64 / DAY_SECS;
    let fresh = 0.5f64.powf(age_days / half);
    let used = ((uses as f64).ln_1p() / FULL_USE.ln_1p()).clamp(0.0, 1.0);
    ((FRESH_PARTS * fresh + USE_PARTS * used) / (FRESH_PARTS + USE_PARTS)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: i64 = 86_400;

    #[test]
    fn rt01_a_new_note_is_fresh_and_freshness_halves_every_half_life() {
        let now = 1_000 * D;
        assert!((retention(now, now, None, 0, 180.0) - 6.0 / 7.0).abs() < 1e-12);
        let half = retention(now, now - 180 * D, None, 0, 180.0);
        assert!((half - 3.0 / 7.0).abs() < 1e-9, "{half}");
        let two = retention(now, now - 360 * D, None, 0, 180.0);
        assert!((two - 1.5 / 7.0).abs() < 1e-9, "{two}");
    }

    #[test]
    fn rt02_use_counts_on_a_log_scale_and_is_capped_so_a_note_cannot_make_itself_permanent() {
        let now = 1_000 * D;
        let old = now - 3650 * D;
        let none = retention(now, old, None, 0, 180.0);
        let some = retention(now, old, None, 3, 180.0);
        let many = retention(now, old, None, 20, 180.0);
        let absurd = retention(now, old, None, u32::MAX, 180.0);
        assert!(none < some && some < many);
        assert_eq!(many, absurd, "past twenty uses more use adds nothing");
        assert!(
            many < 0.15,
            "a ten year old note stays low however often it was used: {many}"
        );
        // a recent use refreshes
        let reused = retention(now, old, Some(now - D), 1, 180.0);
        assert!(reused > 0.8, "{reused}");
    }

    #[test]
    fn rt03_projects_age_faster_and_odd_inputs_are_safe() {
        let now = 1_000 * D;
        let t = now - 90 * D;
        assert!(
            retention(now, t, None, 0, half_life_days(true))
                < retention(now, t, None, 0, half_life_days(false))
        );
        for h in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                retention(now, t, None, 0, h),
                retention(now, t, None, 0, GLOBAL_HALF_LIFE_DAYS)
            );
        }
        // a note dated in the future, a use older than the note, a clock before the epoch
        assert!((retention(now, now + 5 * D, None, 0, 180.0) - 6.0 / 7.0).abs() < 1e-12);
        assert_eq!(
            retention(now, now - 10 * D, Some(now - 500 * D), 0, 180.0),
            retention(now, now - 10 * D, None, 0, 180.0)
        );
        assert!(retention(-5, -9, None, 0, 180.0).is_finite());
        assert!(retention(i64::MAX, i64::MIN, None, 0, 180.0) >= 0.0);
        assert!(retention(i64::MIN, i64::MAX, Some(i64::MAX), u32::MAX, 1.0) <= 1.0);
    }
}
