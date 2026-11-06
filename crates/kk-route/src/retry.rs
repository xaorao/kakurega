//! Trying a model call again when it failed for a passing reason.
//!
//! A model that is starting up, restarting or overloaded refuses or drops a connection for a moment. The owner should not
//! see that as a failure, so such a call is tried again after a short, growing wait. Only failures that say "not now" are
//! retried; a refusal, a bad request, a conversation larger than the window or a call that simply took too long is final,
//! because trying the same thing again cannot change the answer (or would make the owner wait twice as long for it).

use crate::RouteError;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Retry {
    /// Tries after the first one.
    pub retries: usize,
    /// The wait before the first retry; each next one doubles, up to `max_delay`.
    pub first_delay: Duration,
    pub max_delay: Duration,
}

impl Default for Retry {
    fn default() -> Self {
        Retry {
            retries: 2,
            first_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(4),
        }
    }
}

impl Retry {
    /// No retries at all.
    pub fn none() -> Self {
        Retry {
            retries: 0,
            first_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        }
    }

    /// The wait before retry number `n` (from 1), with up to a fifth added or taken away so callers do not retry together.
    pub fn delay(&self, n: usize, jitter: f64) -> Duration {
        let base = self
            .first_delay
            .saturating_mul(1u32 << (n.saturating_sub(1)).min(16))
            .min(self.max_delay);
        base.mul_f64(0.8 + 0.4 * jitter.clamp(0.0, 1.0))
    }
}

/// Whether trying the same call again could end differently.
pub fn is_transient(e: &RouteError) -> bool {
    matches!(e, RouteError::Unreachable(_) | RouteError::Busy(_))
}

fn jitter() -> f64 {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    f64::from(n % 1000) / 1000.0
}

/// Runs `op`, trying again after a wait while it fails with a passing reason, at most `policy.retries` more times. `sleep`
/// is a parameter so the waits can be skipped in tests. The error returned says how many tries were made.
pub fn run<T>(
    policy: &Retry,
    sleep: &dyn Fn(Duration),
    mut op: impl FnMut() -> Result<T, RouteError>,
) -> Result<T, RouteError> {
    let mut tried = 0;
    loop {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if is_transient(&e) && tried < policy.retries => {
                tried += 1;
                sleep(policy.delay(tried, jitter()));
            }
            Err(e) if tried > 0 && is_transient(&e) => {
                return Err(match e {
                    RouteError::Unreachable(m) => {
                        RouteError::Unreachable(format!("{m} (tried {} times)", tried + 1))
                    }
                    RouteError::Busy(m) => {
                        RouteError::Busy(format!("{m} (tried {} times)", tried + 1))
                    }
                    other => other,
                })
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn unreachable() -> RouteError {
        RouteError::Unreachable("refused".into())
    }

    #[test]
    fn rt01_a_call_that_fails_twice_then_works_completes_and_waits_are_growing() {
        let calls = Cell::new(0);
        let waits = RefCell::new(Vec::new());
        let r = run(&Retry::default(), &|d| waits.borrow_mut().push(d), || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(unreachable())
            } else {
                Ok(7)
            }
        });
        assert_eq!(r.unwrap(), 7);
        assert_eq!(calls.get(), 3);
        let w = waits.borrow();
        assert_eq!(w.len(), 2);
        assert!(
            w[0] >= Duration::from_millis(400) && w[0] <= Duration::from_millis(600),
            "{:?}",
            w[0]
        );
        assert!(
            w[1] >= Duration::from_millis(800) && w[1] <= Duration::from_millis(1200),
            "{:?}",
            w[1]
        );
    }

    #[test]
    fn rt02_a_call_that_keeps_failing_stops_after_the_limit_and_says_how_often_it_tried() {
        let calls = Cell::new(0);
        let r: Result<(), _> = run(&Retry::default(), &|_| {}, || {
            calls.set(calls.get() + 1);
            Err(RouteError::Busy("status 503".into()))
        });
        assert_eq!(calls.get(), 3);
        assert!(
            matches!(r, Err(RouteError::Busy(m)) if m.contains("status 503") && m.contains("tried 3 times"))
        );
    }

    #[test]
    fn rt03_final_failures_are_never_retried() {
        for e in [
            RouteError::BadResponse("status 400".into()),
            RouteError::ContextTooLong {
                requested: 9,
                window: 4,
            },
            RouteError::EndpointNotLoopback("x".into()),
            RouteError::NoEligibleRoute,
            RouteError::ScriptExhausted,
            RouteError::TimedOut("slow".into()),
        ] {
            let calls = Cell::new(0);
            let shown = e.to_string();
            let mut once = Some(e);
            let r: Result<(), _> = run(&Retry::default(), &|_| panic!("waited"), || {
                calls.set(calls.get() + 1);
                Err(once.take().unwrap_or(RouteError::NoEligibleRoute))
            });
            assert_eq!(calls.get(), 1, "{shown}");
            assert_eq!(
                r.unwrap_err().to_string(),
                shown,
                "the error is passed on untouched"
            );
        }
    }

    #[test]
    fn rt04_no_retries_means_one_try_and_the_plain_error() {
        let calls = Cell::new(0);
        let r: Result<(), _> = run(&Retry::none(), &|_| panic!("waited"), || {
            calls.set(calls.get() + 1);
            Err(unreachable())
        });
        assert_eq!(calls.get(), 1);
        assert_eq!(r.unwrap_err().to_string(), "model unreachable: refused");
    }

    #[test]
    fn rt05_waits_double_are_capped_and_jittered_within_a_fifth() {
        let p = Retry {
            retries: 9,
            first_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(4),
        };
        assert_eq!(p.delay(1, 0.5), Duration::from_millis(500));
        assert_eq!(p.delay(2, 0.5), Duration::from_millis(1000));
        assert_eq!(p.delay(4, 0.5), Duration::from_secs(4));
        assert_eq!(p.delay(9, 0.5), Duration::from_secs(4), "capped");
        assert_eq!(p.delay(1, 0.0), Duration::from_millis(400));
        assert_eq!(p.delay(1, 1.0), Duration::from_millis(600));
        assert_eq!(
            p.delay(1, 7.0),
            Duration::from_millis(600),
            "jitter outside 0..1 is clamped"
        );
        assert_eq!(p.delay(100, 0.5), Duration::from_secs(4));
    }
}
