//! Secrets that have no shape of their own: a password a person made up, a door code, a random token dropped into a
//! sentence. The main detector works from prefixes, checksums and assignments, and has nothing to hold on to here. This
//! reader works from how people write and say such things: the word for the thing ("password", "passcode", "PIN"), then
//! the value a few words later.
//!
//! It is used where a single line can be left out and the reader is told so (attached files, transcripts, passages of a
//! folder). It is not part of `scan`, so it never causes a whole file to be withheld from a review.
//!
//! What it cannot see: a value said with no word for what it is, a password that is an ordinary word, a value split over
//! two lines.

/// Words for a secret that is letters and digits or symbols.
const NAMES: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "credential",
    "credentials",
    "secret",
    "token",
    "apikey",
    "api key",
    "access key",
    "secret key",
    "private key",
    "auth key",
    "login",
];
/// Words for a secret that may be digits alone.
const CODES: &[&str] = &[
    "pin",
    "passcode",
    "otp",
    "verification code",
    "security code",
    "access code",
    "door code",
    "gate code",
    "alarm code",
    "login code",
    "recovery code",
    "one-time code",
    "2fa code",
    "cvv",
    "cvc",
];
/// How many words after the name the value may come.
const REACH: usize = 8;

fn trim_token(t: &str) -> &str {
    let wrap = |c: char| {
        matches!(
            c,
            '"' | '\''
                | '`'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | ','
                | ';'
                | '“'
                | '”'
                | '‘'
                | '’'
        )
    };
    // "value". and (value), lose what is around them, in whatever order it was put
    t.trim_matches(wrap)
        .trim_end_matches(['.', '?', '!', ':'])
        .trim_matches(wrap)
}

/// A value that could be a made-up password or a token: at least six characters with a letter and a digit, or a letter and
/// a symbol inside it. A date, a version, a size, an address or a path is not one.
fn password_like(t: &str) -> bool {
    let n = t.chars().count();
    if !(6..=200).contains(&n) || t.contains("://") || t.starts_with('/') || t.starts_with("www.") {
        return false;
    }
    let letters = t.chars().filter(|c| c.is_alphabetic()).count();
    let digits = t.chars().filter(|c| c.is_ascii_digit()).count();
    if letters == 0 {
        return false;
    }
    // v1.2.3, 2026-01-05, 10:30am, 16gb, 3rd-party style words
    let versionish =
        t.starts_with(['v', 'V']) && t[1..].chars().all(|c| c.is_ascii_digit() || c == '.');
    let unit = digits > 0
        && letters <= 3
        && t.chars()
            .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ':' | ','))
            .count()
            + letters
            == n;
    if versionish || unit {
        return false;
    }
    // an email address or a domain is a personal detail or a place, not a password
    if t.contains('@') && t.rsplit('@').next().is_some_and(|d| d.contains('.')) {
        return false;
    }
    if digits > 0 {
        return true;
    }
    // no digit: a symbol inside the word, not just a hyphenated or dotted phrase of ordinary words
    let inner: String = t.chars().skip(1).take(n.saturating_sub(2)).collect();
    inner.chars().any(|c| "!@#$%^&*+=~|\\".contains(c))
}

fn code_like(t: &str) -> bool {
    let d: String = t.chars().filter(|c| !matches!(c, '-' | ' ')).collect();
    (4..=10).contains(&d.len()) && d.chars().all(|c| c.is_ascii_digit())
}

/// A long run of letters and digits with all three of capitals, small letters and digits well mixed: a random token,
/// wherever it stands. A lower-case hash or an id of one case is not one.
fn random_token(t: &str) -> bool {
    let n = t.len();
    if !(28..=200).contains(&n) || !t.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let up = t.chars().filter(char::is_ascii_uppercase).count();
    let low = t.chars().filter(char::is_ascii_lowercase).count();
    let dig = t.chars().filter(char::is_ascii_digit).count();
    up >= 3 && low >= 3 && dig >= 2
}

/// The byte ranges of the values in this text that read like a secret someone wrote or said.
pub fn find(text: &str) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    // inside a block that is public by its own heading (a public key, a certificate) a long run of letters and digits is
    // not a secret
    let mut public_block = false;
    for (line_start, line) in lines_with_offsets(text) {
        if line.contains("-----BEGIN") {
            public_block = line.contains("PUBLIC KEY") || line.contains("CERTIFICATE");
        }
        let in_public = public_block;
        if line.contains("-----END") {
            public_block = false;
        }
        let lower = line.to_lowercase();
        // tokens with their place in the line
        let tokens: Vec<(usize, &str)> = line
            .split_whitespace()
            .map(|t| (t.as_ptr() as usize - line.as_ptr() as usize, t))
            .collect();
        for (at, raw) in &tokens {
            let t = trim_token(raw);
            if !in_public && random_token(t) {
                let s = line_start + at + raw.find(t).unwrap_or(0);
                out.push((s, s + t.len()));
            }
        }
        for (names, digits_ok) in [(NAMES, false), (CODES, true)] {
            for name in names {
                let mut from = 0;
                while let Some(found) = lower[from..].find(name) {
                    let (s, e) = (from + found, from + found + name.len());
                    from = e;
                    let before_ok = lower[..s]
                        .chars()
                        .next_back()
                        .is_none_or(|c| !c.is_alphanumeric());
                    // "passwords", "tokens", "PINs" are still the word
                    let after = lower[e..].trim_start_matches('s');
                    let after_ok = after.chars().next().is_none_or(|c| !c.is_alphanumeric());
                    if !before_ok || !after_ok || lower.len() != line.len() {
                        continue;
                    }
                    for (at, raw) in tokens
                        .iter()
                        .filter(|(at, raw)| at + raw.len() > e)
                        .take(REACH)
                    {
                        // a value glued to its name (passwd=abc123): only what follows the name is looked at
                        let (at, raw) = if *at < e {
                            (&e, &raw[e - at..])
                        } else {
                            (at, *raw)
                        };
                        let t = trim_token(raw);
                        // the value may be glued to a sign: password=abc123, pin:4821
                        let t = t.rsplit(['=', ':']).next().unwrap_or(t);
                        let hit = password_like(t) || (digits_ok && code_like(t));
                        if hit
                            && !names
                                .iter()
                                .chain(NAMES)
                                .chain(CODES)
                                .any(|w| t.eq_ignore_ascii_case(w))
                        {
                            let s = line_start + at + raw.rfind(t).unwrap_or(0);
                            out.push((s, s + t.len()));
                            break;
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn lines_with_offsets(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0usize;
    text.split_inclusive('\n').map(move |raw| {
        let start = at;
        at += raw.len();
        (start, raw.trim_end_matches(['\n', '\r']))
    })
}

/// Whether the text holds such a value.
pub fn holds(text: &str) -> bool {
    !find(text).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(text: &str) -> Vec<&str> {
        find(text).into_iter().map(|(s, e)| &text[s..e]).collect()
    }

    #[test]
    fn sp01_a_password_or_code_named_and_then_said_is_found() {
        for (text, value) in [
            ("Can you update the staging server password? It's still hunter2 from the demo.", "hunter2"),
            ("the password is Tr0ub4dor&3", "Tr0ub4dor&3"),
            ("Password: correct-horse&481", "correct-horse&481"),
            ("admin passwd=Blue42Harbor", "Blue42Harbor"),
            ("The wifi passphrase was \"winter&229\".", "winter&229"),
            ("our login for the portal is ops / Summer2026!", "Summer2026"),
            ("For the record, the credential we use is aB3dE5fG7hJ9kL1mN3pQ5rS7tU9vW1xY and it must not be shared.", "aB3dE5fG7hJ9kL1mN3pQ5rS7tU9vW1xY"),
            ("The door code is 482913.", "482913"),
            ("PIN 4821", "4821"),
            ("Your verification code: 993-120", "993-120"),
            ("the api key they gave us was zx81-qq42-lmno", "zx81-qq42-lmno"),
            ("Passwords: the new one is r3dPanda", "r3dPanda"),
            ("secret=p@ssw0rd", "p@ssw0rd"),
        ] {
            assert_eq!(values(text), vec![value], "{text}");
        }
    }

    #[test]
    fn sp02_a_random_token_is_found_with_no_word_for_it_and_a_hash_or_id_is_not() {
        let token = "q7Rk2ZpL9xWv4TnB6cJmY8sD3fGh5KaE";
        assert_eq!(
            values(&format!("Use {token} when you call the export job.")),
            vec![token]
        );
        // the body of a public key or a certificate is public; the same run of characters outside such a block is not
        let public = format!(
            "-----BEGIN PUBLIC KEY-----\n{token}{token}\n-----END PUBLIC KEY-----\nthen {token}"
        );
        assert_eq!(find(&public).len(), 1, "{:?}", values(&public));
        assert!(values(&format!(
            "-----BEGIN CERTIFICATE-----\n{token}\n-----END CERTIFICATE-----"
        ))
        .is_empty());
        for plain in [
            "commit 3f2a9c1e7b8d4f6a0c5e2b9d7f1a3c8e6b4d2f0a",
            "sha256 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            "id 550e8400-e29b-41d4-a716-446655440000",
            "SCREAMINGSNAKECASECONSTANTNAME1234567",
            "thisisalonglowercasewordwithdigits12345",
            "see ThePricingPageRedesignProposalDocument for details",
            "Order ABcd12EFgh34 shipped on Monday.",
        ] {
            assert!(values(plain).is_empty(), "{plain}: {:?}", values(plain));
        }
    }

    #[test]
    fn sp03_ordinary_talk_about_passwords_tokens_and_codes_is_left_alone() {
        for plain in [
            "The password is required for every account.",
            "Please reset your password before Friday.",
            "Who owns the password policy? We still have no owner.",
            "The token limit is 8k and the window is 200000 tokens.",
            "We count 4096 tokens per request on average.",
            "The secret to good pricing is listening to customers.",
            "The status code is 200 and the error code was 404.",
            "Promo code SUMMER gives ten percent off.",
            "The login page is at https://portal.example.com/login2",
            "Rotate the credentials by 2026-01-05 at 10:30am.",
            "Our login flow moved to v2.3.1 last week.",
            "Send the password to asha.rao@meridian.example",
            "The private key lives in the vault, never in the repo.",
            "Pinned the dependency to 16gb machines.",
            "The opinion of the team is that spinning up 12345 servers is too much.",
            "Passwords go in column x9 of the sheet.",
            "Enter access code 12 on floor 3.",
            "The detokenizer is gpt2bpe9 and the subsecretary is b4dger7.",
        ] {
            assert!(values(plain).is_empty(), "{plain}: {:?}", values(plain));
        }
    }

    #[test]
    fn sp04_the_value_must_be_near_its_name_and_only_the_value_is_marked() {
        let far = "The password policy changed, and after a long discussion about many other unrelated things we chose plan B7x9Q2.";
        assert!(values(far).is_empty(), "{:?}", values(far));
        let text =
            "Line one is fine.\nBen: the passcode is 771204, do not share.\nLine three is fine.";
        let spans = find(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].0..spans[0].1], "771204");
        assert!(holds(text) && !holds("Line one is fine."));
        // other scripts around it do not move the place
        let hindi = "पासवर्ड यह है: password is नमस्ते42x";
        assert_eq!(values(hindi), vec!["नमस्ते42x"]);
    }
}
