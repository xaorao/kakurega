//! The backup file. One file holds a set of named byte blobs, sealed under a 256-bit recovery key that is shown to
//! the owner once and never stored beside the backup.
//!
//! Design (see docs/engineering-decisions/backup.md): a fresh random salt per backup is expanded with HKDF-SHA256
//! from the recovery key into a one-use encryption key, so no key is ever used for two backups; the whole header is
//! authenticated as associated data, so changing the version, the salt or the creation time makes opening fail; there
//! is exactly one format and no fallback to older ones. The envelope shape (versioned header, random nonce, context
//! bound as associated data) follows the structure in Ghast's companion-checkpoint (MIT); see NOTICE.md.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAGIC: &[u8; 8] = b"KKBACKUP";
pub const VERSION: u8 = 1;
pub const MAX_ENTRIES: usize = 64;
pub const MAX_NAME: usize = 64;
pub const MAX_TOTAL: usize = 1 << 30;
const INFO: &[u8] = b"kakurega-backup-v1";
const HEADER_LEN: usize = 8 + 1 + 32 + 12 + 8;

/// Named byte blobs, in order.
pub type Entries = Vec<(String, Vec<u8>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupError {
    BadKeyText,
    NotABackup,
    UnsupportedVersion(u8),
    /// The key is wrong or the file was changed. These cannot be told apart, on purpose.
    CannotOpen,
    BadArchive(&'static str),
    TooBig,
    BadName,
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupError::BadKeyText => write!(f, "the recovery key is not written correctly"),
            BackupError::NotABackup => write!(f, "this is not a Kakurega backup file"),
            BackupError::UnsupportedVersion(v) => {
                write!(f, "this backup was made by a newer version (format {v})")
            }
            BackupError::CannotOpen => {
                write!(f, "the recovery key is wrong or the file has been changed")
            }
            BackupError::BadArchive(m) => write!(f, "the backup contents are damaged: {m}"),
            BackupError::TooBig => write!(f, "the backup is too large"),
            BackupError::BadName => write!(f, "a name inside the backup is not allowed"),
        }
    }
}

impl std::error::Error for BackupError {}

// ---- recovery key ----

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn checksum(key: &[u8; 32]) -> [u8; 4] {
    let h = Sha256::digest([b"kakurega-recovery-key-check".as_slice(), key].concat());
    [h[0], h[1], h[2], h[3]]
}

fn encode32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buf = (buf << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn decode32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in text.chars() {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let v = ALPHABET.iter().position(|a| *a as char == c)? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// A new random recovery key.
pub fn new_key() -> Zeroizing<[u8; 32]> {
    let mut k = Zeroizing::new([0u8; 32]);
    getrandom::fill(&mut *k).expect("operating system randomness");
    k
}

/// The key as written for the owner: `KKRK-` then groups of five characters, with a check so a mistyped character is
/// caught before any restore is tried.
pub fn key_to_text(key: &[u8; 32]) -> String {
    let mut raw = key.to_vec();
    raw.extend_from_slice(&checksum(key));
    let body = encode32(&raw);
    let groups: Vec<&str> = body
        .as_bytes()
        .chunks(5)
        .map(|c| std::str::from_utf8(c).expect("ascii"))
        .collect();
    format!("KKRK-{}", groups.join("-"))
}

pub fn key_from_text(text: &str) -> Result<Zeroizing<[u8; 32]>, BackupError> {
    let t = text.trim();
    let body = t
        .strip_prefix("KKRK-")
        .or_else(|| t.strip_prefix("kkrk-"))
        .ok_or(BackupError::BadKeyText)?;
    let compact: String = body
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .collect();
    let raw = decode32(&compact).ok_or(BackupError::BadKeyText)?;
    if raw.len() < 36 {
        return Err(BackupError::BadKeyText);
    }
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&raw[..32]);
    if raw[32..36] != checksum(&key) {
        return Err(BackupError::BadKeyText);
    }
    Ok(key)
}

// ---- names and archive ----

pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.starts_with('.')
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
        })
}

fn pack(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, BackupError> {
    if entries.is_empty() || entries.len() > MAX_ENTRIES {
        return Err(BackupError::BadArchive("wrong number of entries"));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0usize;
    let mut out = Vec::new();
    out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (name, bytes) in entries {
        if !name_ok(name) || !seen.insert(name.clone()) {
            return Err(BackupError::BadName);
        }
        total = total.saturating_add(bytes.len());
        if total > MAX_TOTAL {
            return Err(BackupError::TooBig);
        }
        out.extend_from_slice(&(name.len() as u16).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        out.extend_from_slice(&Sha256::digest(bytes));
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

fn unpack(mut b: &[u8]) -> Result<Entries, BackupError> {
    fn take<'a>(b: &mut &'a [u8], n: usize) -> Result<&'a [u8], BackupError> {
        if b.len() < n {
            return Err(BackupError::BadArchive("cut short"));
        }
        let (head, tail) = b.split_at(n);
        *b = tail;
        Ok(head)
    }
    let count = u32::from_be_bytes(take(&mut b, 4)?.try_into().expect("4 bytes")) as usize;
    if count == 0 || count > MAX_ENTRIES {
        return Err(BackupError::BadArchive("wrong number of entries"));
    }
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for _ in 0..count {
        let nlen = u16::from_be_bytes(take(&mut b, 2)?.try_into().expect("2 bytes")) as usize;
        let name = std::str::from_utf8(take(&mut b, nlen)?)
            .map_err(|_| BackupError::BadName)?
            .to_string();
        if !name_ok(&name) || !seen.insert(name.clone()) {
            return Err(BackupError::BadName);
        }
        let len = u64::from_be_bytes(take(&mut b, 8)?.try_into().expect("8 bytes"));
        let len = usize::try_from(len).map_err(|_| BackupError::TooBig)?;
        total = total.saturating_add(len);
        if total > MAX_TOTAL {
            return Err(BackupError::TooBig);
        }
        let digest = take(&mut b, 32)?.to_vec();
        let data = take(&mut b, len)?;
        if Sha256::digest(data).as_slice() != digest.as_slice() {
            return Err(BackupError::BadArchive(
                "an entry does not match its checksum",
            ));
        }
        out.push((name, data.to_vec()));
    }
    if !b.is_empty() {
        return Err(BackupError::BadArchive("extra bytes after the last entry"));
    }
    Ok(out)
}

// ---- envelope ----

fn one_use_key(recovery: &[u8; 32], salt: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    let mut k = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(salt), recovery)
        .expand(INFO, &mut *k)
        .expect("32 bytes is a valid length");
    k
}

/// Seals the entries. `now` is recorded in the authenticated header.
pub fn seal(
    entries: &[(String, Vec<u8>)],
    recovery: &[u8; 32],
    now: u64,
) -> Result<Vec<u8>, BackupError> {
    let plain = Zeroizing::new(pack(entries)?);
    let mut salt = [0u8; 32];
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut salt).expect("operating system randomness");
    getrandom::fill(&mut nonce).expect("operating system randomness");
    seal_with(&plain, recovery, now, salt, nonce)
}

fn seal_with(
    plain: &[u8],
    recovery: &[u8; 32],
    now: u64,
    salt: [u8; 32],
    nonce: [u8; 12],
) -> Result<Vec<u8>, BackupError> {
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.push(VERSION);
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);
    header.extend_from_slice(&now.to_be_bytes());
    let key = one_use_key(recovery, &salt);
    let cipher = ChaCha20Poly1305::new(key.as_slice().into());
    let sealed = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: &header,
            },
        )
        .map_err(|_| BackupError::CannotOpen)?;
    header.extend_from_slice(&sealed);
    Ok(header)
}

/// Facts readable without the key. They are not authenticated until `open` succeeds.
pub fn peek(file: &[u8]) -> Result<(u8, u64), BackupError> {
    if file.len() < HEADER_LEN || &file[..8] != MAGIC {
        return Err(BackupError::NotABackup);
    }
    let created = u64::from_be_bytes(
        file[HEADER_LEN - 8..HEADER_LEN]
            .try_into()
            .expect("8 bytes"),
    );
    Ok((file[8], created))
}

/// Opens a backup. Returns the entries and the creation time that was authenticated.
pub fn open(file: &[u8], recovery: &[u8; 32]) -> Result<(Entries, u64), BackupError> {
    let (version, created) = peek(file)?;
    if version != VERSION {
        return Err(BackupError::UnsupportedVersion(version));
    }
    if file.len() < HEADER_LEN + 16 {
        return Err(BackupError::NotABackup);
    }
    let (header, sealed) = file.split_at(HEADER_LEN);
    let salt: [u8; 32] = header[9..41].try_into().expect("32 bytes");
    let nonce = &header[41..53];
    let key = one_use_key(recovery, &salt);
    let cipher = ChaCha20Poly1305::new(key.as_slice().into());
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: header,
                },
            )
            .map_err(|_| BackupError::CannotOpen)?,
    );
    Ok((unpack(&plain)?, created))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<(String, Vec<u8>)> {
        vec![
            ("memory.db".into(), b"alpha".to_vec()),
            ("vault.key".into(), vec![7u8; 64]),
            ("empty.bin".into(), vec![]),
        ]
    }

    #[test]
    fn bk01_a_backup_opens_with_its_key_and_gives_back_every_byte() {
        let key = new_key();
        let file = seal(&entries(), &key, 1234).unwrap();
        assert_eq!(peek(&file).unwrap(), (VERSION, 1234));
        let (got, created) = open(&file, &key).unwrap();
        assert_eq!((got, created), (entries(), 1234));
        assert!(
            !file.windows(5).any(|w| w == b"alpha"),
            "the contents are not readable in the file"
        );
    }

    #[test]
    fn bk02_the_wrong_key_and_every_single_changed_byte_are_refused() {
        let key = new_key();
        let file = seal(&entries(), &key, 5).unwrap();
        assert_eq!(open(&file, &new_key()).err(), Some(BackupError::CannotOpen));
        for i in 0..file.len() {
            let mut bad = file.clone();
            bad[i] ^= 0x01;
            assert!(
                open(&bad, &key).is_err(),
                "flipping byte {i} must be detected"
            );
        }
        assert!(open(&file[..file.len() - 1], &key).is_err(), "truncation");
        let mut longer = file.clone();
        longer.push(0);
        assert!(open(&longer, &key).is_err(), "extension");
    }

    #[test]
    fn bk03_two_backups_of_the_same_data_share_nothing() {
        let key = new_key();
        let (a, b) = (
            seal(&entries(), &key, 1).unwrap(),
            seal(&entries(), &key, 1).unwrap(),
        );
        assert_ne!(a[9..53], b[9..53], "fresh salt and nonce each time");
        assert_ne!(a[HEADER_LEN..], b[HEADER_LEN..]);
    }

    #[test]
    fn bk04_other_formats_and_files_are_not_guessed_at() {
        let key = new_key();
        assert_eq!(open(b"", &key).err(), Some(BackupError::NotABackup));
        assert_eq!(
            open(
                b"not a backup at all, just some text that is long enough",
                &key
            )
            .err(),
            Some(BackupError::NotABackup)
        );
        let mut file = seal(&entries(), &key, 1).unwrap();
        file[8] = 2;
        assert_eq!(
            open(&file, &key).err(),
            Some(BackupError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn bk05_names_and_counts_are_strict_on_the_way_in_and_out() {
        let key = new_key();
        for bad in [
            "../x",
            "a/b",
            ".hidden",
            "UPPER",
            "",
            "with space",
            &"a".repeat(65),
            "nul\0",
        ] {
            assert_eq!(
                seal(&[(bad.to_string(), vec![1])], &key, 1).err(),
                Some(BackupError::BadName),
                "{bad:?}"
            );
        }
        assert!(seal(&[], &key, 1).is_err());
        assert!(
            seal(&[("a".into(), vec![]), ("a".into(), vec![])], &key, 1).is_err(),
            "duplicate names"
        );
        // a hostile archive sealed with the real key is still refused when opened
        for (label, plain) in [
            ("traversal", {
                let mut v = 1u32.to_be_bytes().to_vec();
                v.extend_from_slice(&4u16.to_be_bytes());
                v.extend_from_slice(b"../x");
                v.extend_from_slice(&0u64.to_be_bytes());
                v.extend_from_slice(&Sha256::digest(b""));
                v
            }),
            ("huge count", u32::MAX.to_be_bytes().to_vec()),
            ("huge length", {
                let mut v = 1u32.to_be_bytes().to_vec();
                v.extend_from_slice(&1u16.to_be_bytes());
                v.extend_from_slice(b"a");
                v.extend_from_slice(&u64::MAX.to_be_bytes());
                v
            }),
            ("duplicate names", {
                let one = pack(&[("a".into(), vec![1])]).unwrap();
                let mut v = 2u32.to_be_bytes().to_vec();
                v.extend_from_slice(&one[4..]);
                v.extend_from_slice(&one[4..]);
                v
            }),
            ("wrong entry checksum", {
                let mut v = pack(&[("a".into(), vec![1, 2, 3])]).unwrap();
                let n = v.len();
                v[n - 4] ^= 0xff;
                v
            }),
            ("trailing bytes", {
                let mut v = pack(&[("a".into(), vec![1])]).unwrap();
                v.push(0);
                v
            }),
        ] {
            let file = seal_with(&plain, &key, 1, [1; 32], [2; 12]).unwrap();
            assert!(open(&file, &key).is_err(), "{label}");
        }
    }

    #[test]
    fn bk06_the_recovery_key_text_round_trips_and_catches_typing_mistakes() {
        let key = new_key();
        let text = key_to_text(&key);
        assert!(
            text.starts_with("KKRK-") && text.matches('-').count() >= 8,
            "{text}"
        );
        assert_eq!(*key_from_text(&text).unwrap(), *key);
        assert_eq!(
            *key_from_text(&text.to_lowercase()).unwrap(),
            *key,
            "case does not matter"
        );
        assert_eq!(
            *key_from_text(&format!(
                "  {}  ",
                text.replace('-', " ").replacen("KKRK ", "KKRK-", 1)
            ))
            .unwrap(),
            *key,
            "spaces for dashes"
        );
        let mut chars: Vec<char> = text.chars().collect();
        let i = chars.iter().position(|c| *c == '-').unwrap() + 1;
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        assert_eq!(
            key_from_text(&chars.into_iter().collect::<String>()).err(),
            Some(BackupError::BadKeyText),
            "one wrong character"
        );
        for bad in ["", "KKRK-", "KKRK-AAAA", "ABCD-EFGH", "KKRK-!!!!-!!!!"] {
            assert!(key_from_text(bad).is_err(), "{bad}");
        }
        assert_ne!(key_to_text(&key), key_to_text(&new_key()));
    }

    #[test]
    fn bk07_a_key_derived_for_one_backup_does_not_open_another() {
        let key = new_key();
        let one = seal_with(&pack(&entries()).unwrap(), &key, 1, [1; 32], [3; 12]).unwrap();
        let mut two = seal_with(&pack(&entries()).unwrap(), &key, 1, [2; 32], [3; 12]).unwrap();
        // swapping the sealed part of one under the header of the other fails
        two.splice(HEADER_LEN.., one[HEADER_LEN..].iter().copied());
        assert!(open(&two, &key).is_err());
    }
}
