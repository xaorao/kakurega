//! Two versions of a project, read from the owner's local git repository: the commit that is checked out and its merge base
//! with a branch, tag or commit the owner names. Nothing is executed. The repository is opened in isolation (no system or
//! user configuration, no environment), objects are read with a library, and no git program is started, so a hook, pager,
//! external diff or file-system monitor a repository configures never runs.
//!
//! What comes back is the list of files that differ, with both versions of each. A change larger than the limits is refused
//! with its size, never cut: a review covers exactly the change it names.

use std::collections::BTreeMap;
use std::path::Path;

/// The most files a change may touch, and the most bytes of new text it may hold, to be reviewed in one go.
pub const MAX_FILES: usize = 300;
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
/// A file larger than this on either side is named as left out rather than read.
pub const MAX_FILE_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The path inside the project, with `/` between its parts.
    pub path: String,
    pub kind: Kind,
    /// The file before the change; `None` for an added file.
    pub old: Option<Vec<u8>>,
    /// The file after the change; `None` for a deleted file.
    pub new: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    /// The commit the change is measured from: the merge base of the two, in hex.
    pub base: String,
    /// The commit that is checked out, in hex.
    pub head: String,
    pub changes: Vec<Change>,
    /// Paths that differ and were not read, each with why (a link, a submodule, a file too large).
    pub left_out: Vec<(String, &'static str)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// The folder is not the top of a git repository.
    NotARepository,
    /// The name given for the base is not one this reader takes.
    BadBase,
    /// The base does not name a commit in this repository.
    UnknownBase,
    /// The repository has no commit checked out.
    NoHead,
    /// The two commits share no history.
    NoCommonHistory,
    /// Nothing differs between the two.
    NothingChanged,
    /// The change is over a limit; reviewed whole or not at all.
    TooLarge { files: usize, bytes: usize },
    /// An object could not be read.
    Unreadable(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::NotARepository => f.write_str("that folder is not the top folder of a git repository"),
            GitError::BadBase => f.write_str("name the base as a branch, a tag or a commit (letters, digits and . _ / - ^ ~ only, at most 200)"),
            GitError::UnknownBase => f.write_str("this repository has no branch, tag or commit of that name; fetch it first"),
            GitError::NoHead => f.write_str("this repository has no commit checked out"),
            GitError::NoCommonHistory => f.write_str("the checked-out commit and the base share no history, so there is no change between them to review"),
            GitError::NothingChanged => f.write_str("the checked-out commit changes nothing relative to the base"),
            GitError::TooLarge { files, bytes } => write!(f, "the change touches {files} files and {bytes} bytes of new text; at most {MAX_FILES} files and {MAX_BYTES} bytes are reviewed in one go, and a change is reviewed whole or not at all. Review a smaller base, or the whole folder"),
            GitError::Unreadable(why) => write!(f, "the repository could not be read: {why}"),
        }
    }
}

/// Whether `spec` is a plain name for a commit: a branch, a tag, a hash, with `^` and `~` steps. Reflog and search forms
/// (`@{...}`, `:/text`) and anything that starts like an option are refused.
pub fn valid_base(spec: &str) -> bool {
    !spec.is_empty()
        && spec.len() <= 200
        && !spec.starts_with('-')
        && !spec.contains("..")
        && spec.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-' | b'^' | b'~')
        })
}

fn unreadable(e: impl std::fmt::Display) -> GitError {
    GitError::Unreadable(
        e.to_string()
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect(),
    )
}

/// Path to (object id, whether it is a plain file) for every entry of a commit's tree that is not a folder.
fn entries(tree: &gix::Tree<'_>) -> Result<BTreeMap<String, (gix::ObjectId, bool)>, GitError> {
    let mut out = BTreeMap::new();
    for e in tree.traverse().breadthfirst.files().map_err(unreadable)? {
        if e.mode.is_tree() {
            continue;
        }
        out.insert(e.filepath.to_string(), (e.oid, e.mode.is_blob()));
    }
    Ok(out)
}

/// What the checked-out commit of the repository at `dir` changes relative to its merge base with `base`.
pub fn read_change(dir: &Path, base: &str) -> Result<ChangeSet, GitError> {
    if !valid_base(base) {
        return Err(GitError::BadBase);
    }
    let repo = gix::open_opts(dir, gix::open::Options::isolated())
        .map_err(|_| GitError::NotARepository)?;
    let head = repo.head_commit().map_err(|_| GitError::NoHead)?;
    let base_commit = repo
        .rev_parse_single(base)
        .map_err(|_| GitError::UnknownBase)?
        .object()
        .map_err(unreadable)?
        .peel_to_commit()
        .map_err(|_| GitError::UnknownBase)?;
    let fork = repo
        .merge_base(head.id, base_commit.id)
        .map_err(unreadable)?
        .ok_or(GitError::NoCommonHistory)?
        .detach();
    let fork_commit = repo.find_commit(fork).map_err(unreadable)?;
    let before = entries(&fork_commit.tree().map_err(unreadable)?)?;
    let after = entries(&head.tree().map_err(unreadable)?)?;

    let mut paths: Vec<&String> = before.keys().chain(after.keys()).collect();
    paths.sort();
    paths.dedup();
    let differing: Vec<&String> = paths
        .into_iter()
        .filter(|p| before.get(*p).map(|x| x.0) != after.get(*p).map(|x| x.0))
        .collect();
    if differing.is_empty() {
        return Err(GitError::NothingChanged);
    }
    let blob = |id: gix::ObjectId| -> Result<Vec<u8>, GitError> {
        Ok(repo.find_blob(id).map_err(unreadable)?.data.clone())
    };
    let mut changes = Vec::new();
    let mut left_out = Vec::new();
    let mut bytes = 0usize;
    for path in &differing {
        let (old, new) = (before.get(*path), after.get(*path));
        if old.is_some_and(|x| !x.1) || new.is_some_and(|x| !x.1) {
            left_out.push(((*path).clone(), "a link or a nested repository, not a file"));
            continue;
        }
        let old = old.map(|x| blob(x.0)).transpose()?;
        let new = new.map(|x| blob(x.0)).transpose()?;
        if old.as_ref().is_some_and(|b| b.len() > MAX_FILE_BYTES)
            || new.as_ref().is_some_and(|b| b.len() > MAX_FILE_BYTES)
        {
            left_out.push(((*path).clone(), "larger than a file that is read"));
            continue;
        }
        bytes += new.as_ref().map_or(0, Vec::len);
        let kind = match (&old, &new) {
            (None, _) => Kind::Added,
            (_, None) => Kind::Deleted,
            _ => Kind::Modified,
        };
        changes.push(Change {
            path: (*path).clone(),
            kind,
            old,
            new,
        });
    }
    if differing.len() > MAX_FILES || bytes > MAX_BYTES {
        return Err(GitError::TooLarge {
            files: differing.len(),
            bytes,
        });
    }
    Ok(ChangeSet {
        base: fork.to_string(),
        head: head.id.to_string(),
        changes,
        left_out,
    })
}

/// The line ranges of the new version that the change wrote (added or replaced), 1-based and inclusive, merged when they
/// touch. A file that is not text, or a deleted file, has none; an added file is one range over all of it.
pub fn changed_lines(change: &Change) -> Vec<(u32, u32)> {
    let Some(new) = change
        .new
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
    else {
        return Vec::new();
    };
    let total = new.lines().count() as u32;
    let Some(old) = change.old.as_deref() else {
        return if total == 0 {
            Vec::new()
        } else {
            vec![(1, total)]
        };
    };
    let Ok(old) = std::str::from_utf8(old) else {
        return if total == 0 {
            Vec::new()
        } else {
            vec![(1, total)]
        };
    };
    let diff = similar::TextDiff::from_lines(old, new);
    let mut out: Vec<(u32, u32)> = Vec::new();
    for op in diff.ops() {
        let (tag, _, range) = op.as_tag_tuple();
        if !matches!(tag, similar::DiffTag::Insert | similar::DiffTag::Replace) || range.is_empty()
        {
            continue;
        }
        let (from, to) = (range.start as u32 + 1, range.end as u32);
        match out.last_mut() {
            Some(last) if from <= last.1 + 1 => last.1 = last.1.max(to),
            _ => out.push((from, to)),
        }
    }
    out
}

/// SHA-256 input for a change, the same on every computer: the two commits, then one line per changed file with its kind.
pub fn canonical(set: &ChangeSet) -> String {
    let mut text = format!("base {}\nhead {}\n", set.base, set.head);
    for c in &set.changes {
        let kind = match c.kind {
            Kind::Added => "added",
            Kind::Modified => "modified",
            Kind::Deleted => "deleted",
        };
        text.push_str(&format!("{kind} {}\n", c.path));
    }
    text
}

/// Whether a path from a tree is safe to write under a folder: relative, with no empty, `.`, `..` or `.git` part and no
/// backslash or control character. Git itself refuses such names, but a repository's objects can be written by anything.
pub fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path.chars().any(|c| c.is_control() || c == '\\')
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.eq_ignore_ascii_case(".git")
        })
}

/// Writes the files of the checked-out commit of the repository at `dir` into the new folder `dest`: what is committed,
/// not what is on disk beside it. Links, nested repositories, files over [`MAX_FILE_BYTES`] and unsafe names are not
/// written and are returned with why. At most `max_files` files are written; more is an error.
pub fn export_head(
    dir: &Path,
    dest: &Path,
    max_files: usize,
) -> Result<Vec<(String, &'static str)>, GitError> {
    let repo = gix::open_opts(dir, gix::open::Options::isolated())
        .map_err(|_| GitError::NotARepository)?;
    let head = repo.head_commit().map_err(|_| GitError::NoHead)?;
    let all = entries(&head.tree().map_err(unreadable)?)?;
    if all.len() > max_files {
        return Err(GitError::Unreadable(format!(
            "the commit holds {} files; at most {max_files} are reviewed",
            all.len()
        )));
    }
    std::fs::create_dir(dest).map_err(unreadable)?;
    let mut left_out = Vec::new();
    for (path, (id, plain)) in &all {
        if !plain {
            left_out.push((path.clone(), "a link or a nested repository, not a file"));
            continue;
        }
        if !safe_path(path) {
            left_out.push((
                path.chars().filter(|c| !c.is_control()).take(200).collect(),
                "a name that cannot be written safely",
            ));
            continue;
        }
        let blob = repo.find_blob(*id).map_err(unreadable)?;
        if blob.data.len() > MAX_FILE_BYTES {
            left_out.push((path.clone(), "larger than a file that is read"));
            continue;
        }
        let to = dest.join(path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(unreadable)?;
        }
        std::fs::write(&to, &blob.data).map_err(unreadable)?;
    }
    Ok(left_out)
}

/// How many committed files differ on disk from the checked-out commit (edited or removed since). Files nobody committed
/// are not counted. Counting stops at `stop_at`; a repository of more than `max_files` files is not looked through
/// and gives none.
pub fn edited_since_commit(dir: &Path, max_files: usize, stop_at: usize) -> usize {
    let Ok(repo) = gix::open_opts(dir, gix::open::Options::isolated()) else {
        return 0;
    };
    let Some(all) = repo
        .head_commit()
        .ok()
        .and_then(|c| c.tree().ok())
        .and_then(|t| entries(&t).ok())
    else {
        return 0;
    };
    if all.len() > max_files {
        return 0;
    }
    let mut n = 0;
    for (path, (id, plain)) in &all {
        if !plain || !safe_path(path) {
            continue;
        }
        let Ok(blob) = repo.find_blob(*id) else {
            continue;
        };
        let same = match std::fs::metadata(dir.join(path)) {
            Ok(m) if m.is_file() && m.len() == blob.data.len() as u64 => {
                std::fs::read(dir.join(path)).is_ok_and(|b| b == blob.data)
            }
            _ => false,
        };
        if !same {
            n += 1;
            if n >= stop_at {
                break;
            }
        }
    }
    n
}
