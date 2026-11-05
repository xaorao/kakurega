//! The terminal lists the freshness windows locally (it does not link the fetcher); this test, in a crate that sees both,
//! keeps that list identical to the one the fetcher validates.

#[test]
fn sw01_the_terminal_s_freshness_windows_are_the_fetcher_s() {
    assert_eq!(kk_tui::cmd::RECENCY_WINDOWS, kk_fetch::RECENCY_WINDOWS);
}
