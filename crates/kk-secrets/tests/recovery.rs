//! A wallet's recovery phrase: found as exactly its words, however it is introduced, and ordinary writing is left alone.

const TWELVE: &str = "legal winner thank year wave sausage worth useful legal winner thank yellow";
const TWENTY_FOUR: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual";

fn found(text: &str) -> Vec<&str> {
    kk_secrets::scan(text)
        .iter()
        .filter(|h| h.rule == "recovery phrase")
        .map(|h| &text[h.start..h.end])
        .collect()
}

#[test]
fn rp01_the_range_is_exactly_the_phrase_however_it_is_introduced() {
    for text in [
        format!("recovery phrase - - - - - {TWELVE}"),
        format!("my seed phrase is {TWELVE} and keep it safe"),
        format!("Seed phrase:\n{TWELVE}\n"),
        format!("mnemonic = \"{TWELVE}\""),
        format!("write this down: {TWELVE}"),
        format!("{TWELVE} is what the wallet showed me"),
    ] {
        assert_eq!(found(&text), [TWELVE], "{text}");
    }
    assert_eq!(
        found(&format!("backup phrase: {TWENTY_FOUR}")),
        [TWENTY_FOUR]
    );
    // a capital at the start of a sentence, and commas between the words
    let spoken =
        "Legal, winner, thank, year, wave, sausage, worth, useful, legal, winner, thank, yellow";
    assert_eq!(found(&format!("The words are {spoken}.")).len(), 1);
    // words that are not on the standard list are still taken for a phrase after its name, at a standard length
    let other = "apple river stone cloud night angle brave candy dwarf eagle fable giant";
    assert_eq!(found(&format!("my seed phrase: {other}")), [other]);
}

#[test]
fn rp02_ordinary_writing_is_not_a_recovery_phrase() {
    for text in [
        "Keep your seed phrase somewhere safe and never share it with anyone at all, ever.",
        "The recovery phrase screen shows twelve words that you should write down on paper today.",
        "We walked along the river until the bridge, then turned north toward the old market square and home.",
        "abandon ability able about above absent absorb abstract absurd abuse access",
        "A mnemonic is a memory aid such as a rhyme, a short song or an easy sentence.",
        // list words in a row that fail the list's own checksum, and one word many times over
        "legal winner thank year wave sausage worth useful legal winner thank abandon",
        &"word ".repeat(200),
    ] {
        assert!(found(text).is_empty(), "{text}: {:?}", found(text));
    }
}

#[test]
fn rp03_a_long_standard_phrase_is_found_with_no_name_and_words_after_it() {
    let phrase = "letter advice cage absurd amount doctor acoustic avoid letter advice cage absurd amount doctor acoustic avoid letter advice cage absurd amount doctor acoustic bless";
    assert_eq!(found(&format!("{phrase}\n")), [phrase]);
    // more list words after it on the line: the whole run is withheld, never less than the phrase
    let text = format!("{phrase} able about");
    let got = found(&text);
    assert!(got.len() == 1 && got[0].starts_with(phrase), "{got:?}");
}
