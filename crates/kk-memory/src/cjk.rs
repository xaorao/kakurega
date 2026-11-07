//! Finding notes written in Chinese, Japanese or Korean by their words.
//!
//! The full-text index cuts text at spaces and punctuation. Those languages are written without spaces, so a whole sentence is
//! one "word" and a search for a word inside it finds nothing. The usual fix is a dictionary-based segmenter; that is a large
//! dependency and a data file to ship and keep honest. This is the dictionary-free alternative that search engines use for
//! these scripts: every run of such characters is indexed as overlapping pairs of characters, and a search is the same pairs in
//! the same order, so a search matches exactly where the typed characters appear next to each other in the note.
//!
//! Anamnesis (Apache-2.0) solves the same problem with jieba's `cut_for_search` on both sides (`crates/store/src/cjk.rs`);
//! only the idea of treating the indexed text and the query text symmetrically is taken from it.

/// Whether this character belongs to a script that is written without spaces between words.
pub fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}'      // Han, extension A
        | '\u{4E00}'..='\u{9FFF}'    // Han
        | '\u{F900}'..='\u{FAFF}'    // Han compatibility
        | '\u{3040}'..='\u{309F}'    // Hiragana
        | '\u{30A0}'..='\u{30FF}'    // Katakana
        | '\u{AC00}'..='\u{D7AF}'    // Hangul syllables
        | '\u{20000}'..='\u{2A6DF}'  // Han, extension B
    )
}

/// The text with every run of such characters replaced by overlapping pairs separated by spaces ("发票并" becomes "发票 票并");
/// a run of one character stays as it is; everything else is untouched. Deterministic and cheap: it runs on every note written
/// and every note removed.
pub fn expand(text: &str) -> String {
    if !text.chars().any(is_cjk) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut i = 0;
    while i < chars.len() {
        if !is_cjk(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && is_cjk(chars[j]) {
            j += 1;
        }
        if i > 0 && !chars[i - 1].is_whitespace() {
            out.push(' ');
        }
        if j - i == 1 {
            out.push(chars[i]);
        } else {
            for k in i..j - 1 {
                if k > i {
                    out.push(' ');
                }
                out.push(chars[k]);
                out.push(chars[k + 1]);
            }
        }
        if j < chars.len() && !chars[j].is_whitespace() {
            out.push(' ');
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cj01_runs_become_overlapping_pairs_and_other_text_is_untouched() {
        assert_eq!(expand("发票并且"), "发票 票并 并且");
        assert_eq!(expand("invoice"), "invoice");
        assert_eq!(expand(""), "");
        assert_eq!(expand("税"), "税", "one character stays");
        assert_eq!(expand("客户 prefers 欧元"), "客户 prefers 欧元");
        assert_eq!(expand("pay客户喜欢end"), "pay 客户 户喜 喜欢 end");
        assert_eq!(
            expand("請求書、ユーロ"),
            "請求 求書 、ユーロ".replace("、ユーロ", "、 ユー ーロ")
        );
    }

    #[test]
    fn cj02_kana_hangul_and_text_with_marks_are_handled_and_nothing_is_lost() {
        assert_eq!(expand("ユーロ"), "ユー ーロ");
        assert_eq!(expand("한국어"), "한국 국어");
        assert_eq!(
            expand("ग्राहक चालान"),
            "ग्राहक चालान",
            "Hindi is already split by spaces"
        );
        // every character of the input is still in the output, in order
        for s in ["発票を開く", "a客b户c", "  客户  ", "🙂客户🙂", "客\u{0}户"] {
            let kept: String = expand(s).chars().filter(|c| !c.is_whitespace()).collect();
            let want: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
            // pairs repeat inner characters, so check by walking: each wanted char appears in order
            let mut it = kept.chars();
            assert!(
                want.iter().all(|w| it.any(|k| k == *w)),
                "{s:?} -> {:?}",
                expand(s)
            );
        }
    }

    #[test]
    fn cj03_a_search_for_adjacent_characters_is_a_phrase_of_the_same_pairs() {
        let note = expand("客户喜欢用欧元开发票并且在十五号付款");
        for q in ["发票", "开发票", "欧元", "付款", "票并且"] {
            let phrase = expand(q);
            assert!(note.contains(&phrase), "{q}: {phrase:?} not in {note:?}");
        }
        for q in ["票发", "客欧", "款付"] {
            assert!(!note.contains(&expand(q)), "{q}");
        }
    }
}
