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
}

impl Attention {
    /// The words for the title. Fixed; the same for every job.
    pub fn phrase(self) -> &'static str {
        match self {
            Attention::ReviewReady => "a review is ready",
            Attention::ReviewStopped => "a review stopped",
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
}
