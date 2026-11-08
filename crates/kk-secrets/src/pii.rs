//! Personal data that a person would want to know about before a file leaves the computer. This is a notice, not a block:
//! code and test data are full of example addresses, so nothing here refuses a release. Only counts per kind are reported,
//! never the values, so a notice itself leaks nothing.
//!
//! Every numeric kind is checked with its own checksum (Luhn for payment cards, mod 97 for IBAN, Verhoeff for Aadhaar) so
//! that ordinary long numbers are not reported.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Email,
    PaymentCard,
    Iban,
    IndianPan,
    Aadhaar,
    UsSsn,
    Phone,
}

impl Kind {
    pub fn label(self, n: usize) -> &'static str {
        let one = n == 1;
        match self {
            Kind::Email => {
                if one {
                    "email address"
                } else {
                    "email addresses"
                }
            }
            Kind::PaymentCard => {
                if one {
                    "payment card number"
                } else {
                    "payment card numbers"
                }
            }
            Kind::Iban => {
                if one {
                    "bank account number (IBAN)"
                } else {
                    "bank account numbers (IBAN)"
                }
            }
            Kind::IndianPan => {
                if one {
                    "Indian PAN number"
                } else {
                    "Indian PAN numbers"
                }
            }
            Kind::Aadhaar => {
                if one {
                    "Aadhaar number"
                } else {
                    "Aadhaar numbers"
                }
            }
            Kind::UsSsn => {
                if one {
                    "US social security number"
                } else {
                    "US social security numbers"
                }
            }
            Kind::Phone => {
                if one {
                    "phone number"
                } else {
                    "phone numbers"
                }
            }
        }
    }
}

/// How many of each kind of personal data the text seems to contain.
pub fn scan(text: &str) -> BTreeMap<Kind, usize> {
    scan_with(text, &Allow::none()).found
}

/// What a scan found, and how many matches the owner's own list kept out of the count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub found: BTreeMap<Kind, usize>,
    pub allowed: usize,
}

/// `scan`, leaving out the values the owner said are not personal data. The number left out is returned, so a notice can say
/// that the list was used instead of silently reporting less.
pub fn scan_with(text: &str, allow: &Allow) -> Scan {
    let mut out: BTreeMap<Kind, usize> = BTreeMap::new();
    let mut allowed = 0usize;
    let mut add = |k: Kind, value: &str| {
        if allow.permits(k, value) {
            allowed += 1;
        } else {
            *out.entry(k).or_insert(0) += 1;
        }
    };
    for (s, e) in emails(text) {
        add(Kind::Email, &text[s..e]);
    }
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        let c = bytes[i];
        let boundary_before = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if c.is_ascii_digit() && boundary_before {
            let (candidates, _) = digit_groups(bytes, i);
            let mut first_end = i;
            while first_end < n && bytes[first_end].is_ascii_digit() {
                first_end += 1;
            }
            let plus = i > 0 && bytes[i - 1] == b'+';
            // the longest run of grouped digits first, then shorter ones, so two numbers next to each other are both seen
            let found = candidates.iter().rev().find_map(|(digits, end)| {
                let after_ok = *end >= n || !bytes[*end].is_ascii_alphanumeric();
                if after_ok {
                    classify_digits(digits, &text[i..*end], plus).map(|k| (k, *end))
                } else {
                    None
                }
            });
            if let Some((k, end)) = found {
                add(k, &text[i..end]);
                i = end;
            } else {
                // nothing fit: move past the first group only, so a number that follows it is still looked at
                i = first_end.max(i + 1);
            }
            continue;
        }
        if c.is_ascii_uppercase() && boundary_before {
            let mut j = i;
            while j < n && (bytes[j].is_ascii_uppercase() || bytes[j].is_ascii_digit()) {
                j += 1;
            }
            let after_ok = j >= n || !bytes[j].is_ascii_alphanumeric();
            if after_ok {
                let word = &text[i..j];
                if is_pan(word) {
                    add(Kind::IndianPan, word);
                } else if is_iban(word) {
                    add(Kind::Iban, word);
                } else if let Some(end) = spaced_iban(text, i) {
                    // printed in groups of four: GB82 WEST 1234 5698 7654 32
                    add(Kind::Iban, &text[i..end]);
                    i = end;
                    continue;
                }
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    Scan {
        found: out,
        allowed,
    }
}

/// The most entries an allowlist may hold, and the longest entry.
pub const MAX_ENTRIES: usize = 200;
pub const MAX_ENTRY_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pattern {
    /// `@acme.com`: an email address at that domain or any subdomain of it.
    Domain(String),
    /// Anything with a `*` in it, which stands for any run of characters.
    Glob(Vec<char>),
    /// The value in lower case, and the same with spaces and dashes taken out (for numbers).
    Exact(String, String),
}

/// The values the owner has said are not personal data (a fixture mailbox, a company domain, a test card number). Each entry
/// is `me@acme.com` (that value), `*@fixtures.test` (a pattern, `*` stands for any text) or `@acme.com` (that domain and its
/// subdomains, for email addresses). Comparison ignores case, and for numbers ignores spaces and dashes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allow {
    patterns: Vec<Pattern>,
}

/// Checks one entry and returns it as it will be kept (trimmed). A list that allowed everything would switch the notice off
/// without saying so, so an entry must carry at least four letters or digits of its own, and a domain must be a real one.
pub fn check_entry(raw: &str) -> Result<String, &'static str> {
    let e = raw.trim();
    if e.is_empty() {
        return Err("an empty entry matches nothing");
    }
    if e.chars().count() > MAX_ENTRY_CHARS {
        return Err("an entry is at most 200 characters");
    }
    if e.chars()
        .any(|c| c.is_control() || c.is_whitespace() && c != ' ')
    {
        return Err("an entry has no control characters or line breaks");
    }
    if e.chars().filter(|c| c.is_alphanumeric()).count() < 4 {
        return Err("an entry needs at least four letters or digits of its own, or it would allow almost everything");
    }
    if let Some(domain) = e.strip_prefix('@') {
        let labels: Vec<&str> = domain.split('.').collect();
        let ok = labels.len() >= 2
            && labels.iter().all(|l| {
                !l.is_empty()
                    && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    && !l.starts_with('-')
                    && !l.ends_with('-')
            })
            && labels
                .last()
                .is_some_and(|t| t.len() >= 2 && t.bytes().all(|b| b.is_ascii_alphabetic()));
        if !ok {
            return Err("a domain entry is @ followed by a full domain such as @acme.com");
        }
    }
    Ok(e.to_string())
}

fn fold(value: &str, digits_only_separators: bool) -> String {
    value
        .chars()
        .filter(|c| !(digits_only_separators && (*c == ' ' || *c == '-')))
        .flat_map(char::to_lowercase)
        .collect()
}

fn glob_match(pat: &[char], text: &[char]) -> bool {
    // iterative wildcard match: no recursion, no backtracking blow-up (at most pat * text steps)
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while t < text.len() {
        if p < pat.len() && pat[p] == '*' {
            star = Some(p);
            mark = t;
            p += 1;
        } else if p < pat.len() && pat[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some(sp) = star {
            p = sp + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == '*' {
        p += 1;
    }
    p == pat.len()
}

impl Allow {
    pub fn none() -> Allow {
        Allow::default()
    }

    /// Builds the list from stored entries. An entry that fails `check_entry` is left out, so a damaged list can only make
    /// the notice say more, never less.
    pub fn new(entries: &[String]) -> Allow {
        let patterns = entries
            .iter()
            .take(MAX_ENTRIES)
            .filter_map(|raw| check_entry(raw).ok())
            .map(|e| {
                if let Some(d) = e.strip_prefix('@') {
                    Pattern::Domain(d.to_ascii_lowercase())
                } else if e.contains('*') {
                    Pattern::Glob(fold(&e, false).chars().collect())
                } else {
                    Pattern::Exact(fold(&e, false), fold(&e, true))
                }
            })
            .collect();
        Allow { patterns }
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Whether the owner said this value is not personal data.
    pub fn permits(&self, kind: Kind, value: &str) -> bool {
        if self.patterns.is_empty() || value.len() > 512 {
            return false;
        }
        let is_email = kind == Kind::Email;
        let plain = fold(value, !is_email);
        let chars: Vec<char> = fold(value, false).chars().collect();
        self.patterns.iter().any(|p| match p {
            Pattern::Domain(d) => {
                is_email
                    && plain
                        .rsplit_once('@')
                        .is_some_and(|(_, host)| host == d || host.ends_with(&format!(".{d}")))
            }
            Pattern::Glob(g) => glob_match(g, &chars),
            Pattern::Exact(lower, stripped) => {
                if is_email {
                    *lower == plain
                } else {
                    *stripped == plain
                }
            }
        })
    }
}

/// "2 email addresses, 1 payment card number" for a notice line; empty when there is nothing to say.
pub fn describe(found: &BTreeMap<Kind, usize>) -> String {
    found
        .iter()
        .map(|(k, n)| format!("{n} {}", k.label(*n)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every way to read the digits that start at `start` as one number written with single spaces, dashes or dots between
/// groups: the digits and where each reading ends, shortest first, plus where the longest one ends.
fn digit_groups(b: &[u8], start: usize) -> (Vec<(Vec<u8>, usize)>, usize) {
    let mut out = Vec::new();
    let mut digits = Vec::new();
    let mut groups = 0;
    let mut i = start;
    while i < b.len() && digits.len() <= 19 && groups <= 7 {
        if b[i].is_ascii_digit() {
            digits.push(b[i] - b'0');
            i += 1;
            // only lengths a kind of personal number can have are worth keeping (9 for SSN, 10-15 phone, 12 Aadhaar, 13-19 card)
            if (i >= b.len() || !b[i].is_ascii_digit()) && (9..=19).contains(&digits.len()) {
                out.push((digits.clone(), i));
            }
        } else if matches!(b[i], b' ' | b'-' | b'.')
            && i + 1 < b.len()
            && b[i + 1].is_ascii_digit()
            && !digits.is_empty()
        {
            i += 1;
            groups += 1;
        } else if b[i] == b')'
            && start > 0
            && b[start - 1] == b'('
            && digits.len() == 3
            && i + 2 < b.len()
            && b[i + 1] == b' '
            && b[i + 2].is_ascii_digit()
        {
            // an area code in brackets: (415) 555-2671
            i += 2;
            groups += 1;
        } else {
            break;
        }
    }
    let longest = out.last().map_or(start + 1, |(_, e)| *e);
    (out, longest)
}

fn classify_digits(d: &[u8], raw: &str, plus: bool) -> Option<Kind> {
    let len = d.len();
    let separators = raw
        .bytes()
        .filter(|b| matches!(b, b' ' | b'-' | b'.'))
        .count();
    if (13..=19).contains(&len) && luhn(d) && !all_same(d) && separators <= 5 && card_issuer(d) {
        return Some(Kind::PaymentCard);
    }
    if len == 12 && d[0] >= 2 && verhoeff(d) && !all_same(d) {
        return Some(Kind::Aadhaar);
    }
    if len == 9 && raw.len() == 11 && raw.as_bytes()[3] == b'-' && raw.as_bytes()[6] == b'-' {
        let area = d[0] as u32 * 100 + d[1] as u32 * 10 + d[2] as u32;
        let group = d[3] * 10 + d[4];
        let serial = d[5] as u32 * 1000 + d[6] as u32 * 100 + d[7] as u32 * 10 + d[8] as u32;
        if area != 0 && area != 666 && area < 900 && group != 0 && serial != 0 {
            return Some(Kind::UsSsn);
        }
    }
    // a phone number is only claimed with an international "+" or clear grouping, so ids and timestamps are left alone
    let sizes: Vec<usize> = raw
        .split([' ', '-', '.'])
        .map(|g| g.trim_end_matches(')').len())
        .collect();
    let national = matches!(sizes.as_slice(), [3, 3, 4] | [4, 3, 3] | [5, 5]);
    if (10..=15).contains(&len)
        && !all_same(d)
        && ((plus && sizes.len() <= 6) || national)
        && !luhn_looks_card(d)
    {
        return Some(Kind::Phone);
    }
    None
}

fn luhn_looks_card(d: &[u8]) -> bool {
    (13..=19).contains(&d.len()) && luhn(d) && card_issuer(d)
}

/// The number starts like a real card network's and has a length that network issues. Checksums alone would flag one long
/// number in ten (timestamps, ids).
fn card_issuer(d: &[u8]) -> bool {
    let n = d.len();
    let p2 = d[0] as u32 * 10 + d[1] as u32;
    let p4 = if n >= 4 {
        p2 * 100 + d[2] as u32 * 10 + d[3] as u32
    } else {
        0
    };
    let visa = d[0] == 4 && matches!(n, 13 | 16 | 19);
    let mastercard = n == 16 && ((51..=55).contains(&p2) || (2221..=2720).contains(&p4));
    let amex = n == 15 && matches!(p2, 34 | 37);
    let discover =
        (16..=19).contains(&n) && (p4 == 6011 || p2 == 65 || (644..=649).contains(&(p4 / 10)));
    let jcb = (16..=19).contains(&n) && (3528..=3589).contains(&p4);
    let diners =
        (14..=19).contains(&n) && (matches!(p2, 36 | 38 | 39) || (300..=305).contains(&(p4 / 10)));
    let rupay = n == 16
        && (p4 == 6521 || p4 == 6522 || p2 == 60 || p2 == 81 || p2 == 82 || (p4 / 10) == 508);
    visa || mastercard || amex || discover || jcb || diners || rupay
}

fn all_same(d: &[u8]) -> bool {
    d.iter().all(|x| *x == d[0])
}

pub fn luhn(d: &[u8]) -> bool {
    let mut sum = 0u32;
    for (i, x) in d.iter().rev().enumerate() {
        let mut v = *x as u32;
        if i % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum.is_multiple_of(10)
}

pub fn verhoeff(d: &[u8]) -> bool {
    const MUL: [[u8; 10]; 10] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 2, 3, 4, 0, 6, 7, 8, 9, 5],
        [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
        [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
        [4, 0, 1, 2, 3, 9, 5, 6, 7, 8],
        [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
        [6, 5, 9, 8, 7, 1, 0, 4, 3, 2],
        [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
        [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
        [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
    ];
    const PERM: [[u8; 10]; 8] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 5, 7, 6, 2, 8, 3, 0, 9, 4],
        [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
        [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
        [9, 4, 5, 3, 1, 2, 6, 8, 7, 0],
        [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
        [2, 7, 9, 3, 8, 0, 6, 4, 1, 5],
        [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
    ];
    let mut c = 0u8;
    for (i, x) in d.iter().rev().enumerate() {
        c = MUL[c as usize][PERM[i % 8][*x as usize] as usize];
    }
    c == 0
}

fn is_pan(w: &str) -> bool {
    let b = w.as_bytes();
    b.len() == 10
        && b[..5].iter().all(u8::is_ascii_uppercase)
        && b[5..9].iter().all(u8::is_ascii_digit)
        && b[9].is_ascii_uppercase()
        && b"PCHABGJLFT".contains(&b[3])
}

/// An IBAN written in groups of up to four letters and digits separated by single spaces, starting at `start` with the two
/// letters and two check digits: where it ends, if the groups together are a valid IBAN.
fn spaced_iban(text: &str, start: usize) -> Option<usize> {
    let b = text.as_bytes();
    let first = &b[start..];
    if first.len() < 4
        || !(first[0].is_ascii_uppercase()
            && first[1].is_ascii_uppercase()
            && first[2].is_ascii_digit()
            && first[3].is_ascii_digit())
    {
        return None;
    }
    let mut joined = String::new();
    let mut i = start;
    let mut best = None;
    let mut groups = 0;
    while groups < 10 {
        let g_start = i;
        while i < b.len() && (b[i].is_ascii_uppercase() || b[i].is_ascii_digit()) {
            i += 1;
        }
        let len = i - g_start;
        if len == 0 || len > 4 || (groups == 0 && len != 4) {
            break;
        }
        joined.push_str(&text[g_start..i]);
        groups += 1;
        // a group must end the token: a lower-case letter right after it means this is a word, not an IBAN
        if i < b.len() && b[i].is_ascii_alphanumeric() {
            break;
        }
        if groups >= 3 && joined.len() <= 34 && is_iban(&joined) {
            best = Some(i);
        }
        if joined.len() >= 34 || i + 1 >= b.len() || b[i] != b' ' {
            break;
        }
        i += 1;
    }
    best
}

fn is_iban(w: &str) -> bool {
    let b = w.as_bytes();
    if !(15..=34).contains(&b.len())
        || !b[0].is_ascii_uppercase()
        || !b[1].is_ascii_uppercase()
        || !b[2].is_ascii_digit()
        || !b[3].is_ascii_digit()
    {
        return false;
    }
    // move the first four characters to the end, turn letters into numbers (A=10), and the remainder mod 97 must be 1
    let rearranged = b[4..].iter().chain(b[..4].iter());
    let mut rem = 0u32;
    for ch in rearranged {
        if ch.is_ascii_digit() {
            rem = (rem * 10 + (ch - b'0') as u32) % 97;
        } else if ch.is_ascii_uppercase() {
            rem = (rem * 100 + (ch - b'A') as u32 + 10) % 97;
        } else {
            return false;
        }
    }
    rem == 1
}

/// Email addresses: a local part, "@", and a domain with a dot, bounded by characters that cannot be part of an address.
fn emails(text: &str) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let local = |c: u8| c.is_ascii_alphanumeric() || b"._%+-".contains(&c);
    let domain = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'.';
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'@' {
            let mut s = i;
            while s > 0 && local(b[s - 1]) {
                s -= 1;
            }
            let mut e = i + 1;
            while e < b.len() && domain(b[e]) {
                e += 1;
            }
            // a trailing dot belongs to the sentence
            while e > i + 1 && b[e - 1] == b'.' {
                e -= 1;
            }
            let dom = &text[i + 1..e];
            let tld_ok = dom
                .rsplit('.')
                .next()
                .is_some_and(|t| t.len() >= 2 && t.bytes().all(|c| c.is_ascii_alphabetic()));
            if s < i && dom.contains('.') && tld_ok && !dom.starts_with('.') && !dom.contains("..")
            {
                out.push((s, e));
            }
            i = e.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}
