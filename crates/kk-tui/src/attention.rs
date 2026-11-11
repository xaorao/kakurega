//! Telling the owner, through the terminal, that something waits for them: one bell and a short title.
//!
//! The terminal's bell and title are shown by other software (the terminal's tab bar, a tmux status line, a screen share, the
//! desktop's notification history), so what is sent is a fixed phrase and never anything from the job: no recipient, folder,
//! file or finding. Nothing is sent when the owner turned it off, and a bell is sent once per event.

/// What the owner may want to know about while looking at something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// A review finished and can be released, which needs the owner's approval.
    ReviewReady,
    /// A review stopped without a result to release.
    ReviewStopped,
    /// The service the workspace talks to stopped answering.
    ServiceDown,
    /// An expert on a team run stopped to ask the owner something.
    TeamAsks,
}

impl Attention {
    /// The words for the title. Fixed; the same for every job.
    pub fn phrase(self) -> &'static str {
        match self {
            Attention::ReviewReady => "a review is ready",
            Attention::ReviewStopped => "a review stopped",
            Attention::ServiceDown => "the service is not answering",
            Attention::TeamAsks => "an expert has a question for you",
        }
    }
}

/// Whether `KAKU_NOTIFY` leaves the bell and the title on: only `off`, `0`, `false` and `no` (any case) turn them off.
pub fn enabled(setting: Option<&str>) -> bool {
    !matches!(
        setting.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("off" | "0" | "false" | "no")
    )
}

/// The bytes that ring the bell, or nothing when the owner is looking at the terminal or turned this off. A terminal that does
/// not report focus leaves `focused` unknown, and then the bell rings: it is harmless and it is the only signal that terminal
/// can give.
pub fn bell(setting: Option<&str>, focused: Option<bool>) -> Option<&'static str> {
    (enabled(setting) && focused != Some(true)).then_some("\x07")
}

/// What is known about the terminal this runs in, taken from the environment it was started with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TermInfo {
    /// `TERM_PROGRAM`, when set.
    pub program: Option<String>,
    /// `TERM`, when set.
    pub term: Option<String>,
    /// Whether a `KITTY_WINDOW_ID` is set.
    pub kitty_window: bool,
    /// Whether the terminal is inside tmux (`TMUX` is set), which only passes a sequence on when it is wrapped.
    pub tmux: bool,
}

impl TermInfo {
    pub fn from_env() -> TermInfo {
        TermInfo {
            program: std::env::var("TERM_PROGRAM").ok(),
            term: std::env::var("TERM").ok(),
            kitty_window: std::env::var_os("KITTY_WINDOW_ID").is_some(),
            tmux: std::env::var_os("TMUX").is_some_and(|v| !v.is_empty()),
        }
    }

    /// Terminals known to show an OSC 9 sequence as a desktop notification: iTerm2, WezTerm, Ghostty, Warp and kitty. Any other
    /// terminal would print nothing or, worse, something, so it gets the bell alone.
    pub fn shows_osc9(&self) -> bool {
        let program = self.program.as_deref().unwrap_or("");
        self.kitty_window
            || self.term.as_deref() == Some("xterm-kitty")
            || matches!(
                program,
                "iTerm.app" | "WezTerm" | "ghostty" | "WarpTerminal"
            )
    }
}

/// How the owner is told: a desktop notification on a terminal that shows one, else the bell. `KAKU_NOTIFY=bell` keeps the bell
/// even where a notification would show. One method per event, never both.
pub fn alert(
    setting: Option<&str>,
    focused: Option<bool>,
    term: &TermInfo,
    what: Attention,
) -> Option<String> {
    if !enabled(setting) || focused == Some(true) {
        return None;
    }
    let bell_only = setting.is_some_and(|s| s.trim().eq_ignore_ascii_case("bell"));
    if bell_only || !term.shows_osc9() {
        return Some("\x07".to_string());
    }
    // the same fixed phrase as the title, and no character from anywhere else, so nothing can end the sequence early
    let sequence = format!("\x1b]9;Kakurega: {}\x07", what.phrase());
    Some(if term.tmux {
        // tmux hands a sequence on only inside its own passthrough wrapper, with every escape doubled
        format!("\x1bPtmux;{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"))
    } else {
        sequence
    })
}

/// The sequence that sets the window title to `Kakurega`, with the fixed phrase after it when something waits. No character that
/// could end the sequence early can be in it: the phrases are constants.
pub fn title(setting: Option<&str>, waiting: Option<Attention>) -> Option<String> {
    enabled(setting).then(|| match waiting {
        Some(a) => format!("\x1b]2;Kakurega: {}\x07", a.phrase()),
        None => "\x1b]2;Kakurega\x07".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at01_only_the_words_off_turn_it_off() {
        for on in [
            None,
            Some(""),
            Some("on"),
            Some("1"),
            Some("bell"),
            Some("offf"),
        ] {
            assert!(enabled(on), "{on:?}");
        }
        for off in ["off", "OFF", " Off ", "0", "false", "No"] {
            assert!(!enabled(Some(off)), "{off:?}");
        }
    }

    #[test]
    fn at02_the_bell_rings_unless_the_owner_is_known_to_be_looking_or_it_is_off() {
        assert_eq!(bell(None, Some(false)), Some("\x07"));
        assert_eq!(bell(None, None), Some("\x07"));
        assert_eq!(bell(None, Some(true)), None);
        assert_eq!(bell(Some("off"), Some(false)), None);
    }

    #[test]
    fn at03_the_title_is_a_fixed_phrase_in_a_well_formed_sequence() {
        for a in [Attention::ReviewReady, Attention::ReviewStopped] {
            let t = title(None, Some(a)).unwrap();
            assert!(t.starts_with("\x1b]2;Kakurega: ") && t.ends_with('\x07'));
            assert_eq!(
                t.matches('\x07').count(),
                1,
                "the sequence ends once, at the end"
            );
            assert_eq!(t.matches('\x1b').count(), 1);
        }
        assert_ne!(
            Attention::ReviewReady.phrase(),
            Attention::ReviewStopped.phrase()
        );
        assert_eq!(Attention::ReviewStopped.phrase(), "a review stopped");
        assert_eq!(title(None, None).unwrap(), "\x1b]2;Kakurega\x07");
        assert_eq!(title(Some("off"), Some(Attention::ReviewReady)), None);
    }

    fn term(program: Option<&str>) -> TermInfo {
        TermInfo {
            program: program.map(str::to_string),
            ..TermInfo::default()
        }
    }

    #[test]
    fn at04_a_terminal_that_shows_notifications_gets_one_fixed_phrase_and_no_bell() {
        for p in ["iTerm.app", "WezTerm", "ghostty", "WarpTerminal"] {
            let got = alert(None, Some(false), &term(Some(p)), Attention::ReviewReady).unwrap();
            assert_eq!(got, "\x1b]9;Kakurega: a review is ready\x07", "{p}");
            assert_eq!(
                got.matches('\x07').count(),
                1,
                "the terminator is the only bell byte: no separate bell"
            );
        }
        let kitty = TermInfo {
            kitty_window: true,
            ..TermInfo::default()
        };
        assert!(alert(None, None, &kitty, Attention::ReviewStopped)
            .unwrap()
            .starts_with("\x1b]9;Kakurega: a review stopped"));
        let kitty_term = TermInfo {
            term: Some("xterm-kitty".into()),
            ..TermInfo::default()
        };
        assert!(kitty_term.shows_osc9());
    }

    #[test]
    fn at05_other_terminals_get_the_bell_alone() {
        for p in [
            None,
            Some("Apple_Terminal"),
            Some("vscode"),
            Some("Alacritty"),
            Some("iTerm"),
            Some("ghostty2"),
        ] {
            assert_eq!(
                alert(None, Some(false), &term(p), Attention::ReviewReady).as_deref(),
                Some("\x07"),
                "{p:?}"
            );
        }
    }

    #[test]
    fn at06_bell_setting_keeps_the_bell_even_where_a_notification_would_show() {
        for s in ["bell", " BELL "] {
            assert_eq!(
                alert(
                    Some(s),
                    None,
                    &term(Some("iTerm.app")),
                    Attention::ReviewReady
                )
                .as_deref(),
                Some("\x07")
            );
        }
    }

    #[test]
    fn at07_off_and_a_focused_terminal_send_nothing() {
        assert_eq!(
            alert(
                Some("off"),
                None,
                &term(Some("iTerm.app")),
                Attention::ReviewReady
            ),
            None
        );
        assert_eq!(
            alert(
                None,
                Some(true),
                &term(Some("iTerm.app")),
                Attention::ReviewReady
            ),
            None
        );
        assert_eq!(
            alert(None, Some(true), &term(None), Attention::ReviewReady),
            None
        );
    }

    #[test]
    fn at08_inside_tmux_the_notification_is_wrapped_with_every_escape_doubled() {
        let t = TermInfo {
            program: Some("iTerm.app".into()),
            tmux: true,
            ..TermInfo::default()
        };
        assert_eq!(
            alert(None, None, &t, Attention::ReviewReady).unwrap(),
            "\x1bPtmux;\x1b\x1b]9;Kakurega: a review is ready\x07\x1b\\"
        );
        // the bell is not wrapped: tmux handles it itself
        let plain = TermInfo {
            tmux: true,
            ..TermInfo::default()
        };
        assert_eq!(
            alert(None, None, &plain, Attention::ReviewReady).as_deref(),
            Some("\x07")
        );
    }

    #[test]
    fn at09_the_notification_body_is_only_the_fixed_phrase() {
        for a in [Attention::ReviewReady, Attention::ReviewStopped] {
            let got = alert(None, None, &term(Some("WezTerm")), a).unwrap();
            let body = got
                .strip_prefix("\x1b]9;")
                .unwrap()
                .strip_suffix('\x07')
                .unwrap();
            assert_eq!(body, format!("Kakurega: {}", a.phrase()));
            assert!(body.chars().all(|c| !c.is_control()));
            assert!(body.len() < 255, "fits Ghostty's notification body");
        }
    }
}
