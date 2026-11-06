//! The owner's own list of what must never leave: `.kakuignore` at the top of a project.
//!
//! The syntax is a deliberately small part of what `.gitignore` readers know, so it means what they expect:
//!
//! - a blank line, or one starting with `#`, is nothing;
//! - `name` matches a file or a folder of that name at any depth, and everything inside a folder that matches;
//! - a `/` at the start or in the middle ties the pattern to the project's top folder; a `/` at the end means a folder;
//! - `*` is any run of characters inside one name, `?` is one character, `**` as a whole part is any number of folders;
//! - letters are compared without regard to case, so a list written on a case-insensitive disk still holds.
//!
//! What it never does: a line starting with `!` (negation) is ignored and counted, because a list that can un-withhold a
//! file is a list that a later line can quietly undo. Escapes and `[a-z]` classes are not understood and are taken
//! literally. At most 500 patterns of at most 200 characters count; the rest are ignored and counted.

pub const MAX_PATTERNS: usize = 500;
pub const MAX_PATTERN_CHARS: usize = 200;
/// A path deeper than this is only compared as far as this many folders.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone)]
enum Part {
    AnyFolders,
    Name(Vec<char>),
}

#[derive(Debug, Clone)]
struct Pattern {
    parts: Vec<Part>,
    folder_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Withhold {
    patterns: Vec<Pattern>,
    ignored: usize,
}

impl Withhold {
    /// Reads a list. Never fails: what cannot be understood is counted in `ignored_lines`.
    pub fn parse(text: &str) -> Withhold {
        let mut out = Withhold::default();
        for raw in text.lines() {
            let line = raw
                .trim_end_matches([' ', '\t', '\r'])
                .trim_start_matches([' ', '\t']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('!')
                || line.chars().count() > MAX_PATTERN_CHARS
                || out.patterns.len() >= MAX_PATTERNS
            {
                out.ignored += 1;
                continue;
            }
            let folder_only = line.ends_with('/');
            let body = line.trim_end_matches('/');
            if body.trim_start_matches('/').is_empty() {
                out.ignored += 1;
                continue;
            }
            let anchored = body.starts_with('/') || body.trim_start_matches('/').contains('/');
            let mut parts: Vec<Part> = Vec::new();
            if !anchored {
                parts.push(Part::AnyFolders);
            }
            for seg in body.trim_start_matches('/').split('/') {
                if seg == "**" {
                    if !matches!(parts.last(), Some(Part::AnyFolders)) {
                        parts.push(Part::AnyFolders);
                    }
                } else if !seg.is_empty() {
                    // `a**b` is taken as `a*b`
                    parts.push(Part::Name(seg.to_lowercase().chars().collect()));
                }
            }
            out.patterns.push(Pattern { parts, folder_only });
        }
        out
    }

    /// How many non-empty, non-comment lines were not taken as patterns (negations, too many, too long).
    pub fn ignored_lines(&self) -> usize {
        self.ignored
    }

    /// Whether a file, given by its path from the project's top folder with `/` between parts, is on the list: it matches a
    /// pattern itself, or sits inside a folder that does.
    pub fn matches(&self, path: &str) -> bool {
        let lower = path.to_lowercase();
        let comps: Vec<Vec<char>> = lower
            .split('/')
            .filter(|c| !c.is_empty())
            .take(MAX_DEPTH)
            .map(|c| c.chars().collect())
            .collect();
        if comps.is_empty() {
            return false;
        }
        self.patterns.iter().any(|p| matches_prefix(p, &comps))
    }
}

/// Whether the pattern matches the first `k` parts of the path for some `k` (a folder that matches covers what is inside it;
/// a folder-only pattern needs at least one more part after the match, so a file called like the folder does not count).
fn matches_prefix(p: &Pattern, comps: &[Vec<char>]) -> bool {
    let (n, m) = (p.parts.len(), comps.len());
    // reach[i][j]: the first i parts of the pattern match the first j parts of the path
    let mut reach = vec![vec![false; m + 1]; n + 1];
    reach[0][0] = true;
    for i in 0..n {
        for j in 0..=m {
            if !reach[i][j] {
                continue;
            }
            match &p.parts[i] {
                Part::AnyFolders => {
                    for cell in reach[i + 1].iter_mut().skip(j) {
                        *cell = true;
                    }
                }
                Part::Name(pat) => {
                    if j < m && glob(pat, &comps[j]) {
                        reach[i + 1][j + 1] = true;
                    }
                }
            }
        }
    }
    let longest = if p.folder_only {
        m.saturating_sub(1)
    } else {
        m
    };
    (1..=longest).any(|j| reach[n][j])
}

/// `*` and `?` against one name, in time proportional to the product of the two lengths.
fn glob(pat: &[char], name: &[char]) -> bool {
    let (mut p, mut s) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while s < name.len() {
        if p < pat.len() && (pat[p] == '?' || pat[p] == name[s]) && pat[p] != '*' {
            p += 1;
            s += 1;
        } else if p < pat.len() && pat[p] == '*' {
            star = p;
            mark = s;
            p += 1;
        } else if star != usize::MAX {
            p = star + 1;
            mark += 1;
            s = mark;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == '*' {
        p += 1;
    }
    p == pat.len()
}
