//! Random compositions of markdown pieces, markup, control characters, wide and combining characters and very long words,
//! rendered at widths from 0 to 200. A panic in the terminal reader would close the whole workspace.

use kk_tui::markdown::render;
use kk_tui::ui::{wrap, Theme};

fn rng(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

const PIECES: &[&str] = &[
    "# ",
    "## ",
    "- ",
    "* ",
    "1. ",
    "> ",
    "```",
    "```rust\n",
    "\n",
    "\n\n",
    "|",
    "| a | b |\n|---|---|\n",
    "**",
    "*",
    "`",
    "[x](https://e.example/",
    ")",
    "](",
    "![i](",
    "<b>",
    "</b>",
    "<script>",
    "&amp;",
    "\t",
    "\r\n",
    "\u{200b}",
    "\u{200e}",
    "\u{200f}",
    "\u{202a}",
    "\u{202d}",
    "\u{2066}",
    "\u{2067}",
    "\u{2069}",
    "\u{202e}",
    "\u{0301}",
    "e\u{0301}\u{0301}\u{0301}",
    "😀",
    "👨‍👩‍👧‍👦",
    "日本語",
    "ｗｉｄｅ",
    "\u{0}",
    "\u{1b}[2J",
    "\u{7f}",
    "word ",
    "longwordlongwordlongwordlongwordlongwordlongword",
    "    ",
    "  - nested",
    "\u{feff}",
    "\u{2028}",
    "\u{9f}",
    "~~",
    "---",
    "***",
    "<!--",
    "-->",
    "[ref]: http://x",
    "$$",
    "\\",
];

#[test]
fn fz01_random_hostile_markdown_never_panics_at_any_width_and_never_puts_a_control_character_on_the_screen(
) {
    // fixed, so a failure can be reproduced; change the number to look at another run
    let mut seed = 0x1234_5678_9abc_def1u64;
    let th = Theme {
        color: true,
        palette: kk_tui::ui::Palette::Auto,
    };
    let th_plain = Theme {
        color: false,
        palette: kk_tui::ui::Palette::Auto,
    };
    let mut worst = 0usize;
    for round in 0..4000 {
        let n = 1 + (rng(&mut seed) % 40) as usize;
        let mut text = String::new();
        for _ in 0..n {
            text.push_str(PIECES[(rng(&mut seed) % PIECES.len() as u64) as usize]);
        }
        for width in [0usize, 1, 2, 3, 5, 10, 40, 200] {
            let r = std::panic::catch_unwind(|| render(&text, width, &th));
            match r {
                Ok(lines) => {
                    worst = worst.max(lines.len());
                    // no control character reaches the screen text
                    for l in &lines {
                        let s: String = l.spans.iter().map(|sp| sp.content.as_ref()).collect();
                        assert!(
                            !s.chars().any(|c| c.is_control() && c != '\t'),
                            "round {round} width {width}: control char in {s:?} from {text:?}"
                        );
                        assert!(
                            !s.chars().any(|c| matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}' | '\u{2028}' | '\u{2029}')),
                            "round {round} width {width}: a direction or invisible character reached the screen in {s:?}"
                        );
                    }
                }
                Err(_) => panic!("render panicked: width {width} text {text:?}"),
            }
            let w = std::panic::catch_unwind(|| wrap(&text, width));
            assert!(w.is_ok(), "wrap panicked: width {width} text {text:?}");
        }
        let _ = render(&text, 30, &th_plain);
    }
    assert!(worst > 5, "the fuzz made no long documents: {worst}");
}
