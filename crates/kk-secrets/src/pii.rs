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
    let mut out: BTreeMap<Kind, usize> = BTreeMap::new();
    let mut add = |k: Kind| *out.entry(k).or_insert(0) += 1;
    for _ in emails(text) {
        add(Kind::Email);
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
                add(k);
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
                    add(Kind::IndianPan);
                } else if is_iban(word) {
                    add(Kind::Iban);
                }
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    out
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
    let sizes: Vec<usize> = raw.split([' ', '-', '.']).map(str::len).collect();
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
