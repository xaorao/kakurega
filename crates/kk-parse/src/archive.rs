//! ZIP containers: Office files and plain archives. Nothing is ever unpacked to disk; every part is read into memory under
//! a budget, and a name inside the archive is only a label.

use crate::{detect, read as read_any, Ctx, Kind, ParseError};
use std::io::{Cursor, Read};
use zip::ZipArchive;

pub(crate) type Pack<'a> = ZipArchive<Cursor<&'a [u8]>>;

pub(crate) fn open(bytes: &[u8]) -> Result<Pack<'_>, ParseError> {
    ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ParseError::Damaged(format!("not a readable zip: {e}")))
}

/// Word, Excel or a plain archive, from the parts a file holds. Falls back to an archive when the structure is unreadable,
/// so the later error names the real problem.
pub(crate) fn office_or_archive(bytes: &[u8]) -> Kind {
    let Ok(mut z) = open(bytes) else {
        return Kind::Archive;
    };
    let has = |z: &mut Pack<'_>, n: &str| find(z, n).is_some();
    if has(&mut z, "word/document.xml") {
        Kind::Word
    } else if has(&mut z, "xl/workbook.xml") {
        Kind::Excel
    } else {
        Kind::Archive
    }
}

/// The position of a part by name, found without decrypting anything.
fn find(z: &mut Pack<'_>, name: &str) -> Option<usize> {
    (0..z.len()).find(|&i| {
        z.by_index_raw(i)
            .ok()
            .is_some_and(|f| f.name().is_ok_and(|n| n == name))
    })
}

/// One part of a zip by name, read within the member and unpacked budgets, or `None` when it is not there.
pub(crate) fn part(
    ctx: &mut Ctx,
    z: &mut Pack<'_>,
    name: &str,
) -> Result<Option<Vec<u8>>, ParseError> {
    let Some(i) = find(z, name) else {
        return Ok(None);
    };
    let (size, packed, encrypted) = {
        let f = z
            .by_index_raw(i)
            .map_err(|e| ParseError::Damaged(e.to_string()))?;
        (f.size(), f.compressed_size(), f.encrypted())
    };
    if encrypted {
        return Err(ParseError::Protected);
    }
    check_sizes(
        name,
        size,
        packed,
        ctx.limits.max_member,
        ctx.limits.max_ratio,
    )?;
    ctx.unpack(size)?;
    Ok(Some(unpack(z, i, name, ctx.limits.max_member)?))
}

/// The bytes of one member, read with a hard cap: the declared size is a claim, so the read itself is bounded and a member
/// that lies about its size cannot grow past the limit.
fn unpack(z: &mut Pack<'_>, i: usize, shown: &str, max_member: u64) -> Result<Vec<u8>, ParseError> {
    let mut f = z
        .by_index(i)
        .map_err(|e| ParseError::Damaged(format!("{shown}: {e}")))?;
    let mut out = Vec::with_capacity(f.size().min(1 << 20) as usize);
    f.by_ref()
        .take(max_member + 1)
        .read_to_end(&mut out)
        .map_err(|e| ParseError::Damaged(format!("{shown}: {e}")))?;
    if out.len() as u64 > max_member {
        return Err(ParseError::Bomb(format!(
            "{shown} unpacks to more than {} MiB",
            max_member / (1024 * 1024)
        )));
    }
    Ok(out)
}

fn check_sizes(
    name: &str,
    size: u64,
    packed: u64,
    max_member: u64,
    max_ratio: u64,
) -> Result<(), ParseError> {
    if size > max_member {
        return Err(ParseError::Bomb(format!(
            "{name} unpacks to more than {} MiB",
            max_member / (1024 * 1024)
        )));
    }
    if size > 1 << 20 && size > packed.max(1).saturating_mul(max_ratio) {
        return Err(ParseError::Bomb(format!(
            "{name} unpacks to over {max_ratio} times its packed size"
        )));
    }
    Ok(())
}

/// A member name safe to show: no path climbing, no absolute path, no control or direction characters.
fn shown_name(raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw.starts_with('/')
        || raw.starts_with('\\')
        || raw.contains('\0')
        || raw.split(['/', '\\']).any(|p| p == "..")
        || raw.chars().any(|c| c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
    {
        return None;
    }
    Some(raw.chars().take(120).collect())
}

pub(crate) fn read(
    ctx: &mut Ctx,
    prefix: &str,
    name: &str,
    bytes: &[u8],
    depth: usize,
) -> Result<(), ParseError> {
    if depth >= ctx.limits.max_depth {
        return Err(ParseError::Bomb(format!(
            "archives inside archives go deeper than {}",
            ctx.limits.max_depth
        )));
    }
    let mut z = open(bytes)?;
    let total = z.len();
    if total > ctx.limits.max_entries {
        return Err(ParseError::Bomb(format!(
            "{total} files in one archive, at most {} are read",
            ctx.limits.max_entries
        )));
    }
    // `prefix` already ends with this archive's own name when it is a member of another archive
    let here = if prefix.is_empty() {
        name.to_string()
    } else {
        prefix.to_string()
    };
    let mut listed = Vec::new();
    let mut skipped = Vec::new();
    for i in 0..total {
        ctx.entries += 1;
        let (raw, is_dir, size, packed, encrypted) = {
            let f = z
                .by_index_raw(i)
                .map_err(|e| ParseError::Damaged(e.to_string()))?;
            (
                f.name().map(|n| n.into_owned()).unwrap_or_default(),
                f.is_dir(),
                f.size(),
                f.compressed_size(),
                f.encrypted(),
            )
        };
        if is_dir {
            continue;
        }
        let Some(shown) = shown_name(&raw) else {
            skipped.push("a file with an unsafe name".to_string());
            continue;
        };
        listed.push(shown.clone());
        if encrypted {
            skipped.push(format!("{shown} (password-protected)"));
            continue;
        }
        check_sizes(
            &shown,
            size,
            packed,
            ctx.limits.max_member,
            ctx.limits.max_ratio,
        )?;
        ctx.unpack(size)?;
        let data = unpack(&mut z, i, &shown, ctx.limits.max_member)?;
        match detect(&shown, &data) {
            Some(kind) => {
                if read_any(
                    ctx,
                    &format!("{here} > {shown}"),
                    &shown,
                    kind,
                    &data,
                    depth + 1,
                )
                .is_err()
                {
                    skipped.push(format!("{shown} (could not be read)"));
                }
            }
            None => skipped.push(format!("{shown} (not a kind of file read here)")),
        }
        if ctx.truncated {
            break;
        }
    }
    ctx.note(format!(
        "{here}: an archive of {} file(s): {}.",
        listed.len(),
        listed
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    ));
    for s in skipped.iter().take(30) {
        ctx.note(format!("{here}: left out {s}."));
    }
    Ok(())
}

/// The names of all parts of a zip, in order, for finding the numbered ones (headers, footers, sheets).
pub(crate) fn names(z: &mut Pack<'_>) -> Vec<String> {
    (0..z.len())
        .filter_map(|i| {
            z.by_index_raw(i)
                .ok()
                .and_then(|f| f.name().ok().map(|n| n.into_owned()))
        })
        .collect()
}
