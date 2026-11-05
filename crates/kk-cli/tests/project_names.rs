//! The terminal checks a project name locally (it is a client of the service and does not link the memory crate); this
//! test, in a crate that sees both, keeps that check identical to the service's rule.

#[test]
fn pj04_the_terminal_s_name_check_agrees_with_the_service_s_rule_on_every_odd_case() {
    let names = [
        "",
        " ",
        "a",
        "papermark",
        "my project",
        " lead",
        "trail ",
        "a/b",
        "a\\b",
        "tab\there",
        "line\nbreak",
        "cr\rx",
        "nul\u{0}x",
        "esc\u{1b}x",
        "bidi\u{202e}x",
        "zw\u{200b}x",
        "bom\u{feff}x",
        "ünï",
        "日本語",
        &"x".repeat(80),
        &"x".repeat(81),
        &"é".repeat(80),
        &"é".repeat(81),
        "dot.",
        ".hidden",
        "-dash",
        "a:b",
        "a*b",
    ];
    for n in names {
        assert_eq!(
            kk_tui::project::valid_name(n),
            kk_memory::valid_project(n),
            "{n:?}"
        );
    }
}
