mod cjk;
mod fuse;
mod retention;
mod secret;

use hmac::{Hmac, Mac};
use kk_types::label::{Integrity, Label, Level};
use kk_vault::rusqlite::{params, Connection, OptionalExtension};
use kk_vault::Vault;
use sha2::Sha256;
use std::collections::BTreeSet;
use std::fmt;

pub const MAX_TEXT: usize = 4096;
pub const MAX_REASON: usize = 512;
pub const MAX_SUBJECT: usize = 128;
pub const MAX_PROPOSALS_PER_JOB: i64 = 5;

#[derive(Debug)]
pub enum MemoryError {
    Refused(String),
    NotFound,
    Storage(String),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryError::Refused(m) => write!(f, "{m}"),
            MemoryError::NotFound => write!(f, "no such memory"),
            MemoryError::Storage(m) => write!(f, "memory storage error: {m}"),
        }
    }
}

impl std::error::Error for MemoryError {}

impl From<kk_vault::rusqlite::Error> for MemoryError {
    fn from(e: kk_vault::rusqlite::Error) -> Self {
        MemoryError::Storage(e.to_string())
    }
}

fn refuse<T>(m: &str) -> Result<T, MemoryError> {
    Err(MemoryError::Refused(m.to_string()))
}

/// A project name is a folder name: 1 to 80 plain characters, no surrounding spaces, no path separators.
pub fn valid_project(p: &str) -> bool {
    !(p.trim() != p
        || p.is_empty()
        || p.chars().count() > 80
        // unlike note text, a name may hold no control character at all (not even a tab or a line break), because it is
        // shown inline next to the note
        || p.chars().any(char::is_control)
        || has_hidden_chars(p)
        || p.contains('/')
        || p.contains('\\'))
}

/// Which notes a lookup may see by project. `Global` is what applies everywhere; `Project` adds that project's notes;
/// `All` is for the owner's own Memory screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope<'a> {
    Global,
    Project(&'a str),
    All,
}

impl Scope<'_> {
    fn params(self) -> (i64, Option<String>) {
        match self {
            Scope::Global => (0, None),
            Scope::Project(p) => (0, Some(p.to_string())),
            Scope::All => (1, None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    User,
    Review(String),
}

#[derive(Debug, Clone)]
pub struct NewMemory {
    pub text: String,
    pub label: Label,
    pub subject: Option<String>,
    /// The project folder this note is about. A project note is offered only when that project is in play; no project means
    /// the note applies everywhere.
    pub project: Option<String>,
    pub reason: String,
    pub source: Source,
    pub valid_from: Option<i64>,
    pub valid_to: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    pub id: String,
    pub text: String,
    pub label: Label,
    pub subject: Option<String>,
    pub project: Option<String>,
    pub reason: String,
    pub source: Source,
    pub created_at: i64,
    pub valid_from: Option<i64>,
    pub valid_to: Option<i64>,
    pub status: Status,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Active,
    Superseded,
    /// Hidden because the owner merged it into another note; it can be brought back.
    Merged,
    /// Held back: a newer secret detector matched this note after it was saved. Not offered to any model, review or list until the
    /// owner decides: forget it, or say it is not a secret.
    Held,
    /// Put away by the owner because it stopped being useful. Kept whole and encrypted, out of recall, lists and every prompt,
    /// until the owner restores it or forgets it.
    Archived,
}

pub const MIN_DORMANT_DAYS: i64 = 30;
pub const MAX_DORMANT_DAYS: i64 = 3650;
pub const DEFAULT_DORMANT_DAYS: i64 = 180;

/// A current note proposed for putting away, with why: how long it has been quiet and how often a model has been given it.
#[derive(Debug, Clone)]
pub struct Dormant {
    pub memory: Memory,
    pub idle_days: i64,
    pub uses: u32,
    pub last_sign: i64,
}

/// What forgetting a review removes (or would remove).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Purge {
    pub notes: usize,
    pub suggestions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remembered {
    Added(String),
    AlreadyKnown(String),
}

/// Turns text into vectors that are close when the meaning is close. The one implementation Kakurega ships talks to a
/// model on this computer; vectors are as private as the notes they came from.
pub trait Embedder: Send + Sync {
    fn model(&self) -> String;
    /// Vectors for stored notes.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
    /// The vector for a question. Some models want questions and documents prepared differently.
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed(&[text.to_string()])?
            .into_iter()
            .next()
            .ok_or_else(|| "no vector came back".to_string())
    }
    /// How close a note must be to count as a match. Models differ a lot in how spread out their scores are.
    fn min_similarity(&self) -> f32 {
        0.35
    }
}

/// At most this many notes can enter a recall through meaning alone, so a weak model cannot flood the results.
/// Chosen on development questions of the LongMemEval retrieval benchmark and confirmed on questions held out from the
/// choice (docs/engineering-decisions/memory.md).
const MEANING_ONLY_CAP: usize = 10;

/// How much the meaning ranking counts in the merge with the keyword and similar-wording rankings. Same source as the cap.
const MEANING_WEIGHT: f64 = 4.0;

/// Makes the function the index triggers call available on a connection. `Memories::open` does this itself; a tool that writes
/// to the notes table by hand on its own connection must do it first or the triggers fail.
pub fn register_index_function(
    conn: &kk_vault::rusqlite::Connection,
) -> kk_vault::rusqlite::Result<()> {
    conn.create_scalar_function(
        "kk_fts",
        1,
        kk_vault::rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
            | kk_vault::rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        |ctx| Ok(cjk::expand(&ctx.get::<String>(0).unwrap_or_default())),
    )?;
    // true when every compartment a note carries is among the ones allowed; the same rule as `Label::confidentiality_within`,
    // applied while candidates are chosen so notes the caller may not see never take a place in the pool
    conn.create_scalar_function(
        "kk_comp_ok",
        2,
        kk_vault::rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
            | kk_vault::rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        |ctx| {
            let note = ctx.get::<String>(0).unwrap_or_default();
            let allowed = ctx.get::<String>(1).unwrap_or_default();
            let ok = note
                .split('\u{1f}')
                .filter(|c| !c.is_empty())
                .all(|c| allowed.split('\u{1f}').any(|a| a == c));
            Ok(ok)
        },
    )
}

fn allowed_compartments(clearance: &Label) -> String {
    clearance
        .compartments
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\u{1f}")
}

pub struct Memories {
    vault: Vault,
    fp_key: [u8; 32],
    embedder: Option<std::sync::Arc<dyn Embedder>>,
    /// How many notes the meaning ranking keeps; see `MEANING_ONLY_CAP`.
    meaning_cap: usize,
    /// How much the meaning ranking counts in the merge; see `fuse::fuse_weighted`.
    meaning_weight: f64,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    text TEXT NOT NULL,
    fingerprint BLOB NOT NULL,
    level INTEGER NOT NULL,
    compartments TEXT NOT NULL,
    untrusted INTEGER NOT NULL,
    unknown_sensitivity INTEGER NOT NULL,
    subject TEXT,
    project TEXT,
    reason TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    source_ref TEXT,
    created_at INTEGER NOT NULL,
    valid_from INTEGER,
    valid_to INTEGER,
    status TEXT NOT NULL,
    superseded_by TEXT,
    merged_into TEXT
);
CREATE INDEX IF NOT EXISTS memories_fp ON memories(fingerprint);
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(text, content='memories', content_rowid='rowid', tokenize='unicode61');
CREATE TABLE IF NOT EXISTS tombstones (
    fingerprint BLOB PRIMARY KEY,
    reason TEXT NOT NULL,
    forgotten_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS conflict_dismissed (
    a TEXT NOT NULL,
    b TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (a, b)
);
CREATE TABLE IF NOT EXISTS forgotten_sources (
    source_ref TEXT PRIMARY KEY,
    reason TEXT NOT NULL,
    forgotten_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS proposals (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    text TEXT NOT NULL,
    fingerprint BLOB NOT NULL,
    why TEXT NOT NULL,
    level INTEGER NOT NULL,
    compartments TEXT NOT NULL,
    untrusted INTEGER NOT NULL,
    unknown_sensitivity INTEGER NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS memory_use (
    id TEXT PRIMARY KEY,
    last_at INTEGER NOT NULL,
    uses INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS memory_keep (
    id TEXT PRIMARY KEY,
    pinned INTEGER NOT NULL,
    kept_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS memory_hold (
    id TEXT PRIMARY KEY,
    rule TEXT NOT NULL,
    at INTEGER NOT NULL,
    cleared_version INTEGER
);
CREATE TABLE IF NOT EXISTS memory_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    note TEXT NOT NULL
);
";

/// The triggers that keep the index current; notes go in through `kk_fts` so pairs of characters are indexed for scripts written without spaces.
const FTS_TRIGGERS: &str = "
CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, text) VALUES (new.rowid, kk_fts(new.text));
END;
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.rowid, kk_fts(old.text));
END;
CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.rowid, kk_fts(old.text));
    INSERT INTO memories_fts(rowid, text) VALUES (new.rowid, kk_fts(new.text));
END;
";

/// Bumped whenever how notes are indexed changes; an older index is rebuilt once at open.
const FTS_VERSION: &str = "2";

fn level_to_int(l: Level) -> i64 {
    match l {
        Level::Public => 0,
        Level::Internal => 1,
        Level::Confidential => 2,
        Level::Restricted => 3,
    }
}

fn int_to_level(i: i64) -> Level {
    match i {
        0 => Level::Public,
        1 => Level::Internal,
        2 => Level::Confidential,
        _ => Level::Restricted,
    }
}

fn normalise(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn has_hidden_chars(s: &str) -> bool {
    s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
        || kk_types::injection::hidden_chars(s)
}

fn new_id() -> String {
    let mut b = [0u8; 12];
    getrandom::fill(&mut b).expect("system randomness");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "any", "can", "had", "her", "was",
    "one", "our", "out", "has", "have", "his", "how", "its", "may", "who", "did", "get", "now",
    "see", "she", "too", "use", "what", "when", "where", "which", "while", "with", "that", "this",
    "these", "those", "there", "their", "they", "them", "then", "than", "from", "into", "about",
    "would", "could", "should", "does", "doing", "each", "other", "some", "such", "only", "own",
    "same", "very", "just", "also", "been", "being", "were", "will", "your", "say", "says", "tell",
    "give", "show", "know",
];

fn significant_terms(q: &str) -> Vec<String> {
    q.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .filter(|t| {
            let l = t.to_lowercase();
            if t.is_ascii() {
                l.len() >= 3 && !STOPWORDS.contains(&l.as_str())
            } else {
                t.chars().count() >= 2
            }
        })
        .take(12)
        .map(str::to_string)
        .collect()
}

fn fts_query(q: &str) -> Option<String> {
    // a term in a script written without spaces is searched as the same pairs of characters the note was indexed as, in order
    let terms: Vec<String> = significant_terms(q)
        .into_iter()
        .map(|t| format!("\"{}\"", cjk::expand(&t).replace('"', "")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// How many of the newest notes are compared when looking for notes that say almost the same thing.
pub const SIMILAR_SCAN: usize = 200;

/// How many of the newest notes are compared for disagreements.
pub const CONFLICT_SCAN: usize = 300;
/// The most disagreeing pairs listed at once.
pub const MAX_CONFLICTS: usize = 50;
/// Two notes that share this much of their wording, and still do not say the same thing, disagree.
const CONFLICT_SIMILARITY: f64 = 0.45;

/// Why two notes are shown as disagreeing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictWhy {
    /// Both are about this subject (the owner's own word for it) and say different things.
    SameSubject(String),
    /// Nearly the same words, but a changed number, a negation or a swapped word makes them say different things.
    OtherClaim,
}

#[derive(Debug, Clone)]
pub struct Conflict {
    pub a: Recalled,
    pub b: Recalled,
    pub why: ConflictWhy,
}

#[derive(Debug, Clone)]
pub struct Conflicts {
    pub pairs: Vec<Conflict>,
    pub scanned: usize,
    /// False when there were more notes than were compared.
    pub complete: bool,
}

/// Groups of look-alike notes, and how much of the store was compared.
#[derive(Debug, Clone)]
pub struct SimilarNotes {
    /// Newest note first inside each group.
    pub groups: Vec<Vec<Recalled>>,
    pub scanned: usize,
    /// False when there were more notes than were compared.
    pub complete: bool,
}

/// Wording overlap at or above which a suggestion counts as already known.
const NEAR_DUPLICATE: f64 = 0.8;

/// Two close wordings can still disagree: a different number, or a flipped yes/no, is a changed fact and must be shown.
fn same_claim(a: &str, b: &str) -> bool {
    fn marks(s: &str) -> (std::collections::BTreeSet<String>, bool) {
        let lower = s.to_lowercase();
        let mut numbers = std::collections::BTreeSet::new();
        let mut run = String::new();
        for c in lower.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_digit() {
                run.push(c);
            } else if !run.is_empty() {
                numbers.insert(std::mem::take(&mut run));
            }
        }
        let negated = lower
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .any(|w| {
                matches!(
                    w,
                    "not" | "no" | "never" | "without" | "cannot" | "disabled" | "off"
                ) || w.ends_with("n't")
            });
        (numbers, negated)
    }
    marks(a) == marks(b) && same_order(a, b)
}

/// The words of a text, lowercased, in order.
fn word_sequence(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Wording overlap that ignores order says "prefer tabs over spaces" and "prefer spaces over tabs" are the same note. They are
/// opposites. Two texts count as the same claim only when, after lining their words up in order, every word that is left over
/// is a small word or a near-identical spelling of a word left over on the other side (a plural, a typo). A word that merely
/// moved is never "near identical": the same word turning up in a different place is a changed meaning. When unsure the
/// owner is shown the suggestion: a duplicate asked twice costs a click, a changed fact that was hidden costs a wrong note.
fn same_order(a: &str, b: &str) -> bool {
    let (x, y) = (word_sequence(a), word_sequence(b));
    let (n, m) = (x.len(), y.len());
    // longest common subsequence of words, kept whole so the words left over can be named
    let mut table = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..n {
        for j in 0..m {
            table[i + 1][j + 1] = if x[i] == y[j] {
                table[i][j] + 1
            } else {
                table[i][j + 1].max(table[i + 1][j])
            };
        }
    }
    let (mut in_x, mut in_y) = (vec![false; n], vec![false; m]);
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        if x[i - 1] == y[j - 1] {
            in_x[i - 1] = true;
            in_y[j - 1] = true;
            i -= 1;
            j -= 1;
        } else if table[i - 1][j] >= table[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    let small = |w: &str| w.chars().count() < 3 || STOPWORDS.contains(&w);
    let left_x: Vec<&String> = x
        .iter()
        .zip(&in_x)
        .filter(|(w, k)| !**k && !small(w))
        .map(|(w, _)| w)
        .collect();
    let left_y: Vec<&String> = y
        .iter()
        .zip(&in_y)
        .filter(|(w, k)| !**k && !small(w))
        .map(|(w, _)| w)
        .collect();
    if left_x.len() != left_y.len() {
        return false;
    }
    let mut taken = vec![false; left_y.len()];
    for w in left_x {
        let tw = fuse::trigrams(w);
        let partner = left_y.iter().enumerate().find(|(k, v)| {
            !taken[*k] && ***v != *w && fuse::jaccard(&tw, &fuse::trigrams(v)) >= NEAR_SPELLING
        });
        match partner {
            Some((k, _)) => taken[k] = true,
            None => return false,
        }
    }
    true
}

/// How alike two single words must be, by their letter trigrams, to count as the same word spelled differently.
const NEAR_SPELLING: f64 = 0.5;

/// True when saved text reads like an instruction to an AI system. Such notes stay in memory but are never handed to a model as context.
pub fn instruction_like(text: &str) -> bool {
    kk_types::injection::scan(text).instruction_like()
}

struct Row {
    m: Memory,
    untrusted: bool,
}

fn row_to_memory(r: &kk_vault::rusqlite::Row<'_>) -> kk_vault::rusqlite::Result<Row> {
    let comps: String = r.get("compartments")?;
    let untrusted = r.get::<_, i64>("untrusted")? != 0;
    let mut label = Label::new(int_to_level(r.get("level")?));
    for c in comps.split('\u{1f}').filter(|c| !c.is_empty()) {
        label = label.with_compartment(c);
    }
    if untrusted {
        label = label.untrusted();
    }
    if r.get::<_, i64>("unknown_sensitivity")? != 0 {
        label = label.unknown_sensitivity();
    }
    let kind: String = r.get("source_kind")?;
    let sref: Option<String> = r.get("source_ref")?;
    let status: String = r.get("status")?;
    Ok(Row {
        untrusted,
        m: Memory {
            id: r.get("id")?,
            text: r.get("text")?,
            label,
            subject: r.get("subject")?,
            project: r.get("project")?,
            reason: r.get("reason")?,
            source: if kind == "review" {
                Source::Review(sref.unwrap_or_default())
            } else {
                Source::User
            },
            created_at: r.get("created_at")?,
            valid_from: r.get("valid_from")?,
            valid_to: r.get("valid_to")?,
            status: match status.as_str() {
                "active" => Status::Active,
                "merged" => Status::Merged,
                "held" => Status::Held,
                "archived" => Status::Archived,
                _ => Status::Superseded,
            },
            superseded_by: r.get("superseded_by")?,
        },
    })
}

impl Memories {
    /// When the secret detector is a newer generation than the one that last looked at this store, every note is checked again with
    /// it, and a note it now matches is held back (status `held`): no recall, list, review or model sees it until the owner decides.
    /// A note the owner already cleared under this generation is not held again. Returns how many notes were newly held.
    fn rescan_for_secrets(vault: &Vault) -> Result<usize, MemoryError> {
        let c = vault.connection();
        let now_version = kk_secrets::DETECTOR_VERSION.to_string();
        let have: Option<String> = c
            .query_row(
                "SELECT value FROM memory_meta WHERE key = 'detector_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if have.as_deref() == Some(now_version.as_str()) {
            return Ok(0);
        }
        // a store rescanned by a newer detector than this program has is left alone
        if let Some(h) = have.as_deref().and_then(|h| h.parse::<u32>().ok()) {
            if h > kk_secrets::DETECTOR_VERSION {
                return Ok(0);
            }
        }
        let rows: Vec<(String, String)> = {
            let mut st = c.prepare(
                "SELECT id, text FROM memories WHERE status IN ('active', 'merged', 'archived') AND text <> ''",
            )?;
            let found = st
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .flatten()
                .collect::<Vec<_>>();
            found
        };
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut held = 0usize;
        c.execute_batch("SAVEPOINT secret_rescan")?;
        let r = (|| -> Result<(), MemoryError> {
            for (id, text) in &rows {
                let Some(rule) = secret::looks_secret(text) else {
                    continue;
                };
                let cleared: Option<i64> = c
                    .query_row(
                        "SELECT cleared_version FROM memory_hold WHERE id = ?1",
                        params![id],
                        |r| r.get::<_, Option<i64>>(0),
                    )
                    .optional()?
                    .flatten();
                if cleared == Some(i64::from(kk_secrets::DETECTOR_VERSION)) {
                    continue;
                }
                c.execute(
                    "UPDATE memories SET status = 'held' WHERE id = ?1",
                    params![id],
                )?;
                c.execute(
                    "INSERT OR REPLACE INTO memory_hold(id, rule, at, cleared_version) VALUES (?1, ?2, ?3, NULL)",
                    params![id, rule, at],
                )?;
                c.execute(
                    "INSERT INTO memory_events(at, kind, memory_id, note) VALUES (?1, 'held', ?2, ?3)",
                    params![at, id, rule],
                )?;
                held += 1;
            }
            c.execute(
                "INSERT OR REPLACE INTO memory_meta(key, value) VALUES ('detector_version', ?1)",
                params![now_version],
            )?;
            Ok(())
        })();
        match r {
            Ok(()) => c.execute_batch("RELEASE secret_rescan")?,
            Err(e) => {
                c.execute_batch("ROLLBACK TO secret_rescan; RELEASE secret_rescan")?;
                return Err(e);
            }
        }
        Ok(held)
    }

    /// The text of a note and its earlier versions, for finding copies of it elsewhere before it is forgotten. Only a note the caller
    /// may read.
    pub fn texts_of(&self, id: &str, clearance: &Label) -> Result<Vec<String>, MemoryError> {
        let chain = self.lineage_of(&[id.to_string()], false)?;
        let mut out = Vec::new();
        for cid in &chain {
            let row = self
                .conn()
                .query_row(
                    "SELECT * FROM memories WHERE id = ?1",
                    params![cid],
                    row_to_memory,
                )
                .optional()?;
            if let Some(r) = row {
                if r.m.label.confidentiality_within(clearance)
                    && !r.m.text.is_empty()
                    && !out.contains(&r.m.text)
                {
                    out.push(r.m.text);
                }
            }
        }
        Ok(out)
    }

    /// The conversations and reviews a note and its earlier versions were accepted from (`Source::Review(ref)`), for finding where
    /// a paraphrased note came from before it is forgotten. Only versions the caller may read; a note the owner typed has none.
    pub fn source_refs_of(&self, id: &str, clearance: &Label) -> Result<Vec<String>, MemoryError> {
        let chain = self.lineage_of(&[id.to_string()], false)?;
        let mut out: Vec<String> = Vec::new();
        for cid in &chain {
            let row = self
                .conn()
                .query_row(
                    "SELECT * FROM memories WHERE id = ?1",
                    params![cid],
                    row_to_memory,
                )
                .optional()?;
            if let Some(r) = row {
                if let Source::Review(reference) = &r.m.source {
                    if r.m.label.confidentiality_within(clearance)
                        && !reference.is_empty()
                        && !out.contains(reference)
                    {
                        out.push(reference.clone());
                    }
                }
            }
        }
        Ok(out)
    }

    /// Notes held back by a newer secret detector, with the kind of secret it saw, newest first. Only notes the caller may read.
    pub fn held(&self, clearance: &Label) -> Result<Vec<(Memory, String)>, MemoryError> {
        let mut st = self.conn().prepare(
            "SELECT m.*, h.rule AS hold_rule FROM memories m JOIN memory_hold h ON h.id = m.id
             WHERE m.status = 'held' ORDER BY h.at DESC, m.rowid DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok((row_to_memory(r)?, r.get::<_, String>("hold_rule")?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (row, rule) = r?;
            if row.m.label.confidentiality_within(clearance) {
                out.push((row.m, rule));
            }
        }
        Ok(out)
    }

    /// The owner says a held note is not a secret: it is offered again, and this detector generation will not hold it a second time.
    pub fn release_hold(&self, now: i64, id: &str, clearance: &Label) -> Result<(), MemoryError> {
        let held = self.held(clearance)?;
        if !held.iter().any(|(m, _)| m.id == id) {
            return Err(MemoryError::NotFound);
        }
        self.conn().execute(
            "UPDATE memories SET status = 'active' WHERE id = ?1 AND status = 'held'",
            params![id],
        )?;
        self.conn().execute(
            "UPDATE memory_hold SET cleared_version = ?2 WHERE id = ?1",
            params![id, i64::from(kk_secrets::DETECTOR_VERSION)],
        )?;
        self.event(now, "hold_released", id, "owner said it is not a secret")?;
        Ok(())
    }

    /// An index made before notes in Chinese, Japanese and Korean were indexed as pairs is rebuilt once, from the notes, with the
    /// triggers that keep it current. A new store has no notes and the rebuild costs nothing.
    fn ensure_index_version(vault: &Vault) -> Result<(), MemoryError> {
        let c = vault.connection();
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        let have: Option<String> = c
            .query_row(
                "SELECT value FROM memory_meta WHERE key = 'fts_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if have.as_deref() == Some(FTS_VERSION) {
            return Ok(());
        }
        // a store made by a newer version is left alone: an older program rebuilding its index would undo what the newer one did
        if let (Some(found), Ok(mine)) = (
            have.as_deref().and_then(|h| h.parse::<u32>().ok()),
            FTS_VERSION.parse::<u32>(),
        ) {
            if found > mine {
                return refuse("this notes store was made by a newer version of Kakurega; open it with that version or a newer one");
            }
        }
        c.execute_batch("SAVEPOINT fts_rebuild")?;
        let r = (|| -> Result<(), MemoryError> {
            c.execute_batch(
                "DROP TRIGGER IF EXISTS memories_ai; DROP TRIGGER IF EXISTS memories_ad; DROP TRIGGER IF EXISTS memories_au;",
            )?;
            c.execute_batch("INSERT INTO memories_fts(memories_fts) VALUES('delete-all');")?;
            c.execute_batch(
                "INSERT INTO memories_fts(rowid, text) SELECT rowid, kk_fts(text) FROM memories;",
            )?;
            c.execute_batch(FTS_TRIGGERS)?;
            c.execute(
                "INSERT OR REPLACE INTO memory_meta(key, value) VALUES ('fts_version', ?1)",
                params![FTS_VERSION],
            )?;
            Ok(())
        })();
        match r {
            Ok(()) => c.execute_batch("RELEASE fts_rebuild")?,
            Err(e) => {
                c.execute_batch("ROLLBACK TO fts_rebuild; RELEASE fts_rebuild")?;
                return Err(e);
            }
        }
        Ok(())
    }

    pub fn open(vault: Vault, fingerprint_key: [u8; 32]) -> Result<Memories, MemoryError> {
        // the index sees notes through this function, so it must exist on this connection before any trigger can fire
        register_index_function(vault.connection())?;
        vault.connection().execute_batch(SCHEMA)?;
        vault.connection().execute_batch(FTS_TRIGGERS)?;
        Self::ensure_index_version(&vault)?;
        Self::rescan_for_secrets(&vault)?;
        let has_project: bool = vault
            .connection()
            .prepare("PRAGMA table_info(memories)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .flatten()
            .any(|c| c == "project");
        if !has_project {
            vault
                .connection()
                .execute_batch("ALTER TABLE memories ADD COLUMN project TEXT;")?;
        }
        let has_merged: bool = vault
            .connection()
            .prepare("PRAGMA table_info(memories)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .flatten()
            .any(|c| c == "merged_into");
        if !has_merged {
            vault
                .connection()
                .execute_batch("ALTER TABLE memories ADD COLUMN merged_into TEXT;")?;
        }
        vault.connection().execute_batch("CREATE TABLE IF NOT EXISTS memory_vectors(id TEXT PRIMARY KEY, model TEXT NOT NULL, dim INTEGER NOT NULL, vec BLOB NOT NULL);")?;
        let opened = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        vault.connection().execute(
            "INSERT OR IGNORE INTO memory_meta(key, value) VALUES ('use_records_since', ?1)",
            params![opened.to_string()],
        )?;
        Ok(Memories {
            vault,
            fp_key: fingerprint_key,
            embedder: None,
            meaning_cap: MEANING_ONLY_CAP,
            meaning_weight: MEANING_WEIGHT,
        })
    }

    pub fn set_embedder(&mut self, e: std::sync::Arc<dyn Embedder>) {
        self.embedder = Some(e);
    }

    /// Number of notes that currently have a stored vector.
    /// How many notes the meaning ranking may keep before it is merged with the keyword rankings (default 5). A weak model
    /// should keep this small so it cannot flood the results; 0 turns meaning off. Measured, not guessed: see the recall
    /// benchmark in `docs/engineering-decisions/memory.md`.
    pub fn set_meaning_cap(&mut self, cap: usize) {
        self.meaning_cap = cap;
    }

    /// How much the meaning ranking counts when it is merged with the keyword rankings (default 1, the same as each of them;
    /// limited to 0 to 10). Measured, not guessed: see the recall benchmark in `docs/engineering-decisions/memory.md`.
    pub fn set_meaning_weight(&mut self, weight: f64) {
        self.meaning_weight = weight;
    }

    pub fn vector_count(&self) -> usize {
        self.conn()
            .query_row("SELECT count(*) FROM memory_vectors", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap_or(0) as usize
    }

    fn conn(&self) -> &Connection {
        self.vault.connection()
    }

    fn fingerprint(&self, text: &str) -> Vec<u8> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.fp_key)
            .expect("hmac accepts any key length");
        mac.update(normalise(text).as_bytes());
        mac.finalize().into_bytes().to_vec()
    }

    fn check_project(project: &Option<String>) -> Result<(), MemoryError> {
        if let Some(p) = project {
            if !valid_project(p) {
                return refuse("a project name is a folder name of 1 to 80 plain characters");
            }
        }
        Ok(())
    }

    fn check(&self, text: &str, reason: &str, subject: &Option<String>) -> Result<(), MemoryError> {
        if text.trim().is_empty() {
            return refuse("a memory cannot be empty");
        }
        if text.len() > MAX_TEXT
            || reason.len() > MAX_REASON
            || subject.as_ref().is_some_and(|s| s.len() > MAX_SUBJECT)
        {
            return refuse("the memory, its reason or its subject is too long");
        }
        if reason.trim().is_empty() {
            return refuse("say why this is remembered: a memory needs a reason");
        }
        if has_hidden_chars(text)
            || has_hidden_chars(reason)
            || subject.as_ref().is_some_and(|s| has_hidden_chars(s))
        {
            return refuse("the text contains control or direction-changing characters");
        }
        if let Some(what) = secret::looks_secret(text) {
            return Err(MemoryError::Refused(format!(
                "this looks like {what}; secrets are never stored as memory"
            )));
        }
        Ok(())
    }

    fn event(&self, now: i64, kind: &str, id: &str, note: &str) -> Result<(), MemoryError> {
        self.conn().execute(
            "INSERT INTO memory_events(at, kind, memory_id, note) VALUES (?1, ?2, ?3, ?4)",
            params![now, kind, id, note],
        )?;
        Ok(())
    }

    fn insert(&self, now: i64, id: &str, m: &NewMemory, fp: &[u8]) -> Result<(), MemoryError> {
        let comps = m
            .label
            .compartments
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\u{1f}");
        let (kind, sref) = match &m.source {
            Source::User => ("user", None),
            Source::Review(r) => ("review", Some(r.clone())),
        };
        self.conn().execute(
            "INSERT INTO memories(id, text, fingerprint, level, compartments, untrusted, unknown_sensitivity, subject, reason, source_kind, source_ref, created_at, valid_from, valid_to, status, project)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'active', ?15)",
            params![id, m.text, fp, level_to_int(m.label.level), comps, (m.label.integrity == Integrity::Untrusted) as i64, m.label.sensitivity_unknown as i64, m.subject, m.reason, kind, sref, now, m.valid_from, m.valid_to, m.project],
        )?;
        Ok(())
    }

    pub fn remember(&self, now: i64, m: NewMemory) -> Result<Remembered, MemoryError> {
        self.check(&m.text, &m.reason, &m.subject)?;
        if let Source::Review(job) = &m.source {
            if self.source_is_forgotten(job)? {
                return refuse("that review was forgotten, so nothing more from it is kept");
            }
        }
        Self::check_project(&m.project)?;
        if let (Some(a), Some(b)) = (m.valid_from, m.valid_to) {
            if b <= a {
                return refuse("the end of validity must be after its start");
            }
        }
        let fp = self.fingerprint(&m.text);
        if self
            .conn()
            .query_row(
                "SELECT 1 FROM tombstones WHERE fingerprint = ?1",
                params![fp],
                |_| Ok(()),
            )
            .optional()?
            .is_some()
        {
            return refuse("you asked to forget this; it will not be stored again unless you lift that decision");
        }
        let existing: Option<String> = self
            .conn()
            .query_row(
                "SELECT id FROM memories WHERE fingerprint = ?1 AND status = 'active' AND project IS ?2",
                params![fp, m.project],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(Remembered::AlreadyKnown(id));
        }
        let archived: Option<String> = self
            .conn()
            .query_row(
                "SELECT id FROM memories WHERE fingerprint = ?1 AND status = 'archived' AND project IS ?2",
                params![fp, m.project],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = archived {
            self.bring_back(now, &id)?;
            return Ok(Remembered::AlreadyKnown(id));
        }
        let id = new_id();
        self.insert(now, &id, &m, &fp)?;
        self.event(now, "remember", &id, "added")?;
        Ok(Remembered::Added(id))
    }

    pub fn correct(
        &self,
        now: i64,
        id: &str,
        replacement: NewMemory,
    ) -> Result<String, MemoryError> {
        self.check(&replacement.text, &replacement.reason, &replacement.subject)?;
        Self::check_project(&replacement.project)?;
        let old: Option<String> = self
            .conn()
            .query_row(
                "SELECT status FROM memories WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        match old.as_deref() {
            None => return Err(MemoryError::NotFound),
            Some("active") => {}
            Some("merged") => {
                return refuse("that note was merged into another one; undo the merge first")
            }
            Some(_) => return refuse("that memory was already replaced; correct the current one"),
        }
        let fp = self.fingerprint(&replacement.text);
        if self
            .conn()
            .query_row(
                "SELECT 1 FROM tombstones WHERE fingerprint = ?1",
                params![fp],
                |_| Ok(()),
            )
            .optional()?
            .is_some()
        {
            return refuse("you asked to forget this text; it will not be stored again");
        }
        let new = new_id();
        self.conn().execute_batch("SAVEPOINT correct")?;
        let r = (|| -> Result<(), MemoryError> {
            self.insert(now, &new, &replacement, &fp)?;
            self.conn().execute(
                "UPDATE memories SET status = 'superseded', superseded_by = ?1 WHERE id = ?2",
                params![new, id],
            )?;
            self.conn()
                .execute("DELETE FROM memory_vectors WHERE id = ?1", params![id])?;
            self.event(now, "correct", id, &format!("replaced by {new}"))
        })();
        match r {
            Ok(()) => self.conn().execute_batch("RELEASE correct")?,
            Err(e) => {
                self.conn()
                    .execute_batch("ROLLBACK TO correct; RELEASE correct")?;
                return Err(e);
            }
        }
        Ok(new)
    }

    pub fn forget(&self, now: i64, id: &str, reason: &str) -> Result<(), MemoryError> {
        if reason.len() > MAX_REASON || has_hidden_chars(reason) {
            return refuse("the reason is too long or contains control characters");
        }
        self.conn().execute_batch("SAVEPOINT forget")?;
        let r = (|| -> Result<(), MemoryError> {
            let chain = self.lineage_of(&[id.to_string()], false)?;
            if self.erase_notes(now, &chain, reason)? == 0 {
                return Err(MemoryError::NotFound);
            }
            Ok(())
        })();
        self.finish_forget(r)
    }

    fn finish_forget(&self, r: Result<(), MemoryError>) -> Result<(), MemoryError> {
        match r {
            Ok(()) => self.conn().execute_batch("RELEASE forget")?,
            Err(e) => {
                self.conn()
                    .execute_batch("ROLLBACK TO forget; RELEASE forget")?;
                return Err(e);
            }
        }
        self.conn().execute_batch("INSERT INTO memories_fts(memories_fts) VALUES('optimize'); PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// The notes with these ids and every note that is one of their earlier versions or was merged under them. With `forward`,
    /// also every later version that replaced them, and the earlier versions and merged notes of those.
    fn lineage_of(&self, roots: &[String], forward: bool) -> Result<Vec<String>, MemoryError> {
        let mut chain: Vec<String> = Vec::new();
        for r in roots {
            if !chain.contains(r) {
                chain.push(r.clone());
            }
        }
        let mut i = 0;
        while i < chain.len() {
            let prev: Vec<String> = {
                let mut st = self.conn().prepare(
                    "SELECT id FROM memories WHERE superseded_by = ?1 OR merged_into = ?1",
                )?;
                let rows = st.query_map(params![chain[i]], |r| r.get(0))?;
                rows.collect::<Result<_, _>>()?
            };
            for p in prev {
                if !chain.contains(&p) {
                    chain.push(p);
                }
            }
            if forward {
                let next: Option<String> = self
                    .conn()
                    .query_row(
                        "SELECT superseded_by FROM memories WHERE id = ?1",
                        params![chain[i]],
                        |r| r.get(0),
                    )
                    .optional()?
                    .flatten();
                if let Some(n) = next {
                    if !chain.contains(&n) {
                        chain.push(n);
                    }
                }
            }
            i += 1;
        }
        Ok(chain)
    }

    /// Erases these notes for good: their words are blanked, then the rows and their meaning vectors are deleted, the same
    /// words are blocked from being saved again and any suggestion holding them is dismissed. Returns how many existed.
    fn erase_notes(&self, now: i64, ids: &[String], reason: &str) -> Result<usize, MemoryError> {
        let mut found = 0;
        for mid in ids {
            let fp: Option<Vec<u8>> = self
                .conn()
                .query_row(
                    "SELECT fingerprint FROM memories WHERE id = ?1",
                    params![mid],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(fp) = fp else { continue };
            found += 1;
            self.conn().execute("INSERT OR REPLACE INTO tombstones(fingerprint, reason, forgotten_at) VALUES (?1, ?2, ?3)", params![fp, reason, now])?;
            // Any review suggestion holding the same text is part of the same forget decision.
            self.conn().execute(
                "UPDATE proposals SET text = '', why = '', status = CASE WHEN status = 'pending' THEN 'dismissed' ELSE status END WHERE fingerprint = ?1",
                params![fp],
            )?;
            self.conn().execute(
                "UPDATE memories SET text = '', subject = NULL, project = NULL, reason = '' WHERE id = ?1",
                params![mid],
            )?;
            self.conn()
                .execute("DELETE FROM memory_vectors WHERE id = ?1", params![mid])?;
            self.conn().execute(
                "DELETE FROM conflict_dismissed WHERE a = ?1 OR b = ?1",
                params![mid],
            )?;
            self.conn()
                .execute("DELETE FROM memory_use WHERE id = ?1", params![mid])?;
            self.conn()
                .execute("DELETE FROM memory_hold WHERE id = ?1", params![mid])?;
            self.conn()
                .execute("DELETE FROM memory_keep WHERE id = ?1", params![mid])?;
            self.conn()
                .execute("DELETE FROM memories WHERE id = ?1", params![mid])?;
            self.event(now, "forget", mid, "deleted")?;
        }
        Ok(found)
    }

    fn check_source_name(job_id: &str) -> Result<(), MemoryError> {
        let ok = !job_id.is_empty()
            && job_id.len() <= 64
            && job_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if ok {
            Ok(())
        } else {
            refuse("a review is named by letters, digits, dashes and underscores, at most 64")
        }
    }

    /// What forgetting this review would remove: the notes it produced with their earlier versions and anything merged under
    /// them, and the suggestions still waiting. Changes nothing.
    pub fn source_footprint(&self, job_id: &str) -> Result<Purge, MemoryError> {
        Self::check_source_name(job_id)?;
        let roots: Vec<String> = {
            let mut st = self.conn().prepare(
                "SELECT id FROM memories WHERE source_kind = 'review' AND source_ref = ?1",
            )?;
            let rows = st.query_map(params![job_id], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let notes = self.lineage_of(&roots, true)?.len();
        let suggestions: i64 = self.conn().query_row(
            "SELECT count(*) FROM proposals WHERE job_id = ?1 AND status = 'pending'",
            params![job_id],
            |r| r.get(0),
        )?;
        Ok(Purge {
            notes,
            suggestions: suggestions as usize,
        })
    }

    /// Forgets everything one review produced: the notes it wrote (with earlier versions and anything merged under them,
    /// as forgetting one note does), and the suggestions still waiting. The review is written down as forgotten first, so
    /// it can neither suggest nor save anything again, and the exact words of what was removed stay blocked from being
    /// saved again. All or nothing. A review that produced nothing is recorded too.
    pub fn forget_source(
        &self,
        now: i64,
        job_id: &str,
        reason: &str,
    ) -> Result<Purge, MemoryError> {
        Self::check_source_name(job_id)?;
        if reason.len() > MAX_REASON || has_hidden_chars(reason) {
            return refuse("the reason is too long or contains control characters");
        }
        self.conn().execute_batch("SAVEPOINT forget")?;
        let r = (|| -> Result<Purge, MemoryError> {
            self.conn().execute(
                "INSERT OR REPLACE INTO forgotten_sources(source_ref, reason, forgotten_at) VALUES (?1, ?2, ?3)",
                params![job_id, reason, now],
            )?;
            let roots: Vec<String> = {
                let mut st = self.conn().prepare(
                    "SELECT id FROM memories WHERE source_kind = 'review' AND source_ref = ?1",
                )?;
                let rows = st.query_map(params![job_id], |r| r.get(0))?;
                rows.collect::<Result<_, _>>()?
            };
            let chain = self.lineage_of(&roots, true)?;
            let notes = self.erase_notes(now, &chain, reason)?;
            // suggestions that were never accepted: their words are blocked and then blanked
            self.conn().execute(
                "INSERT OR REPLACE INTO tombstones(fingerprint, reason, forgotten_at) SELECT fingerprint, ?2, ?3 FROM proposals WHERE job_id = ?1 AND status = 'pending'",
                params![job_id, reason, now],
            )?;
            let suggestions = self.conn().execute(
                "UPDATE proposals SET text = '', why = '', status = 'dismissed' WHERE job_id = ?1 AND status = 'pending'",
                params![job_id],
            )?;
            self.event(now, "forget_source", job_id, "forgotten")?;
            Ok(Purge { notes, suggestions })
        })();
        match r {
            Ok(p) => {
                self.finish_forget(Ok(()))?;
                Ok(p)
            }
            Err(e) => {
                self.finish_forget(Err(e))?;
                unreachable!("finish_forget returns the error it is given")
            }
        }
    }

    /// The reviews that have notes or waiting suggestions here, newest first, with what forgetting each would remove. At most 200.
    pub fn review_sources(&self) -> Result<Vec<(String, Purge)>, MemoryError> {
        let jobs: Vec<String> = {
            let mut st = self.conn().prepare(
                "SELECT source_ref FROM memories WHERE source_kind = 'review' AND source_ref IS NOT NULL
                 UNION SELECT job_id FROM proposals WHERE status = 'pending'
                 ORDER BY 1 DESC LIMIT 200",
            )?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut out = Vec::new();
        for job in jobs {
            if let Ok(p) = self.source_footprint(&job) {
                if p.notes + p.suggestions > 0 {
                    out.push((job, p));
                }
            }
        }
        Ok(out)
    }

    fn source_is_forgotten(&self, job_id: &str) -> Result<bool, MemoryError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT 1 FROM forgotten_sources WHERE source_ref = ?1",
                params![job_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Most notes merged in one step.
    pub const MAX_MERGE: usize = 8;

    /// Merges look-alike notes the owner has chosen: `others` stop appearing in lists, recall and reports and point at
    /// `survivor`, whose words are not changed. Nothing is deleted, so `unmerge` can bring each note back. All the notes
    /// must be active, readable at `clearance` and in the same project, and either every note is merged or none.
    /// Returns how many notes were merged.
    pub fn merge(
        &self,
        now: i64,
        survivor: &str,
        others: &[String],
        clearance: &Label,
    ) -> Result<usize, MemoryError> {
        let mut ids: Vec<&String> = Vec::new();
        for o in others {
            if !ids.contains(&o) {
                ids.push(o);
            }
        }
        if ids.is_empty() {
            return refuse("choose at least one other note to merge into it");
        }
        if ids.iter().any(|o| o.as_str() == survivor) {
            return refuse("a note is not merged into itself");
        }
        if ids.len() > Self::MAX_MERGE {
            return refuse("merge at most eight notes at a time");
        }
        let load = |id: &str| -> Result<Row, MemoryError> {
            let row = self
                .conn()
                .query_row(
                    "SELECT * FROM memories WHERE id = ?1",
                    params![id],
                    row_to_memory,
                )
                .optional()?
                .ok_or(MemoryError::NotFound)?;
            if !row.m.label.confidentiality_within(clearance) {
                return Err(MemoryError::NotFound);
            }
            Ok(row)
        };
        let keep = load(survivor)?;
        if keep.m.status != Status::Active {
            return refuse("the note to keep is not an active note");
        }
        let mut rows = Vec::new();
        for o in &ids {
            let row = load(o)?;
            if row.m.status != Status::Active {
                return refuse("one of the notes is already merged or replaced");
            }
            if row.m.project != keep.m.project {
                return refuse("notes of different projects are not merged");
            }
            rows.push(row);
        }
        self.conn().execute_batch("SAVEPOINT merge_notes")?;
        let r = (|| -> Result<(), MemoryError> {
            for row in &rows {
                self.conn().execute(
                    "UPDATE memories SET status = 'merged', merged_into = ?1 WHERE id = ?2 AND status = 'active'",
                    params![survivor, row.m.id],
                )?;
                self.conn().execute(
                    "DELETE FROM memory_vectors WHERE id = ?1",
                    params![row.m.id],
                )?;
                self.event(now, "merge", &row.m.id, &format!("merged into {survivor}"))?;
            }
            Ok(())
        })();
        match r {
            Ok(()) => self.conn().execute_batch("RELEASE merge_notes")?,
            Err(e) => {
                self.conn()
                    .execute_batch("ROLLBACK TO merge_notes; RELEASE merge_notes")?;
                return Err(e);
            }
        }
        Ok(rows.len())
    }

    /// Brings a merged note back as an active note. Its words were never changed.
    pub fn unmerge(&self, now: i64, id: &str, clearance: &Label) -> Result<(), MemoryError> {
        let row = self
            .conn()
            .query_row(
                "SELECT * FROM memories WHERE id = ?1",
                params![id],
                row_to_memory,
            )
            .optional()?
            .ok_or(MemoryError::NotFound)?;
        if !row.m.label.confidentiality_within(clearance) {
            return Err(MemoryError::NotFound);
        }
        let n = self.conn().execute(
            "UPDATE memories SET status = 'active', merged_into = NULL WHERE id = ?1 AND status = 'merged'",
            params![id],
        )?;
        if n == 0 {
            return refuse("that note is not merged");
        }
        self.event(now, "unmerge", id, "brought back")
    }

    /// Every merged note `clearance` may read with the note it was merged into, newest first. At most 500.
    pub fn merged_notes(&self, clearance: &Label) -> Result<Vec<(Memory, String)>, MemoryError> {
        let mut st = self.conn().prepare(
            "SELECT * FROM memories WHERE status = 'merged' ORDER BY created_at DESC, rowid DESC LIMIT 500",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                row_to_memory(r)?,
                r.get::<_, Option<String>>("merged_into")?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (row, into) = row?;
            let into = into.unwrap_or_default();
            // a note merged into one the caller may not read would give away that the other note exists
            if row.m.label.confidentiality_within(clearance) && self.readable(&into, clearance)? {
                out.push((row.m, into));
            }
        }
        Ok(out)
    }

    /// The earliest moment this store began recording which notes reach a model (unix seconds). A note older than this has no
    /// use record because none was kept, not because it was never used, so this is the oldest "last sign of life" any note can have.
    pub fn use_records_since(&self) -> i64 {
        self.conn()
            .query_row(
                "SELECT value FROM memory_meta WHERE key = 'use_records_since'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    /// For tests: moves the point where use records began, so a note can be made to look old.
    #[doc(hidden)]
    pub fn set_use_records_since_for_test(&self, at: i64) {
        let _ = self.conn().execute(
            "INSERT OR REPLACE INTO memory_meta(key, value) VALUES ('use_records_since', ?1)",
            params![at.to_string()],
        );
    }

    /// For tests: makes a note, its use record and its keep record look `secs` older, and says use records began long ago.
    #[doc(hidden)]
    pub fn backdate_for_test(&self, id: &str, secs: i64) {
        let c = self.conn();
        let _ = c.execute(
            "UPDATE memories SET created_at = created_at - ?1 WHERE id = ?2",
            params![secs, id],
        );
        let _ = c.execute(
            "UPDATE memory_use SET last_at = last_at - ?1 WHERE id = ?2",
            params![secs, id],
        );
        let _ = c.execute(
            "UPDATE memory_keep SET kept_at = kept_at - ?1 WHERE id = ?2",
            params![secs, id],
        );
        self.set_use_records_since_for_test(0);
    }

    /// Active, unpinned notes that have shown no sign of life for at least `days` (30 to 3650), oldest sign first, with how many
    /// whole days it has been and how often the note has reached a model. A sign of life is the latest of: when the note was
    /// written, when it last reached a model, when the owner last kept, restored or pinned it, and when use records began.
    /// A note is only ever proposed here; nothing is archived until the owner says so. At most 200.
    pub fn dormant(
        &self,
        clearance: &Label,
        now: i64,
        days: i64,
        limit: usize,
    ) -> Result<Vec<Dormant>, MemoryError> {
        if !(MIN_DORMANT_DAYS..=MAX_DORMANT_DAYS).contains(&days) {
            return refuse("days must be a whole number from 30 to 3650");
        }
        let cutoff = now.saturating_sub(days.saturating_mul(86_400));
        let since = self.use_records_since();
        let mut st = self.conn().prepare(
            "SELECT m.*, COALESCE(u.last_at, 0) AS used_at, COALESCE(u.uses, 0) AS uses, COALESCE(k.kept_at, 0) AS kept_at
               FROM memories m
               LEFT JOIN memory_use u ON u.id = m.id
               LEFT JOIN memory_keep k ON k.id = m.id
              WHERE m.status = 'active' AND m.text <> '' AND COALESCE(k.pinned, 0) = 0
              ORDER BY m.rowid",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                row_to_memory(r)?,
                r.get::<_, i64>("used_at")?,
                r.get::<_, i64>("uses")?,
                r.get::<_, i64>("kept_at")?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (row, used_at, uses, kept_at) = row?;
            let last = row.m.created_at.max(used_at).max(kept_at).max(since);
            if last > cutoff || !row.m.label.confidentiality_within(clearance) {
                continue;
            }
            out.push(Dormant {
                idle_days: (now.saturating_sub(last) / 86_400).max(0),
                uses: uses.clamp(0, u32::MAX as i64) as u32,
                memory: row.m,
                last_sign: last,
            });
        }
        out.sort_by(|a, b| {
            a.last_sign
                .cmp(&b.last_sign)
                .then(a.memory.id.cmp(&b.memory.id))
        });
        out.truncate(limit.clamp(1, 200));
        Ok(out)
    }

    fn readable_in(&self, id: &str, clearance: &Label) -> Result<Memory, MemoryError> {
        let row = self
            .conn()
            .query_row(
                "SELECT * FROM memories WHERE id = ?1",
                params![id],
                row_to_memory,
            )
            .optional()?
            .ok_or(MemoryError::NotFound)?;
        if !row.m.label.confidentiality_within(clearance) {
            return Err(MemoryError::NotFound);
        }
        Ok(row.m)
    }

    /// Puts a current note away. Out of recall, lists and prompts; kept whole; `restore` brings it back.
    pub fn archive(&self, now: i64, id: &str, clearance: &Label) -> Result<(), MemoryError> {
        self.readable_in(id, clearance)?;
        let n = self.conn().execute(
            "UPDATE memories SET status = 'archived' WHERE id = ?1 AND status = 'active'",
            params![id],
        )?;
        if n == 0 {
            return refuse("only a current note can be archived");
        }
        self.conn()
            .execute("DELETE FROM memory_keep WHERE id = ?1", params![id])?;
        self.event(now, "archive", id, "put away")
    }

    fn bring_back(&self, now: i64, id: &str) -> Result<(), MemoryError> {
        let n = self.conn().execute(
            "UPDATE memories SET status = 'active' WHERE id = ?1 AND status = 'archived'",
            params![id],
        )?;
        if n == 0 {
            return refuse("that note is not archived");
        }
        self.conn().execute(
            "INSERT OR REPLACE INTO memory_keep(id, pinned, kept_at) VALUES (?1, 0, ?2)",
            params![id, now],
        )?;
        self.event(now, "restore", id, "brought back")
    }

    /// Brings an archived note back. Its clock starts again from now.
    pub fn restore(&self, now: i64, id: &str, clearance: &Label) -> Result<(), MemoryError> {
        self.readable_in(id, clearance)?;
        self.bring_back(now, id)
    }

    /// Every archived note `clearance` may read, newest first. At most 500.
    pub fn archived_notes(&self, clearance: &Label) -> Result<Vec<Memory>, MemoryError> {
        let mut st = self.conn().prepare(
            "SELECT * FROM memories WHERE status = 'archived' ORDER BY created_at DESC, rowid DESC LIMIT 500",
        )?;
        let rows = st.query_map([], row_to_memory)?;
        let mut out = Vec::new();
        for row in rows {
            let row = row?;
            if row.m.label.confidentiality_within(clearance) {
                out.push(row.m);
            }
        }
        Ok(out)
    }

    /// Says a current note is still true: its clock starts again from now. Changes nothing else.
    pub fn keep(&self, now: i64, id: &str, clearance: &Label) -> Result<(), MemoryError> {
        self.set_keep(now, id, clearance, None, "keep", "still true")
    }

    /// Exempts a current note from being proposed as dormant, for as long as it is pinned.
    pub fn pin(&self, now: i64, id: &str, clearance: &Label) -> Result<(), MemoryError> {
        self.set_keep(now, id, clearance, Some(true), "pin", "pinned")
    }

    pub fn unpin(&self, now: i64, id: &str, clearance: &Label) -> Result<(), MemoryError> {
        self.set_keep(now, id, clearance, Some(false), "unpin", "unpinned")
    }

    fn set_keep(
        &self,
        now: i64,
        id: &str,
        clearance: &Label,
        pinned: Option<bool>,
        kind: &str,
        note: &str,
    ) -> Result<(), MemoryError> {
        self.readable_in(id, clearance)?;
        let active: bool = self.conn().query_row(
            "SELECT status = 'active' FROM memories WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        if !active {
            return refuse("only a current note can be kept or pinned");
        }
        let was = self.is_pinned(id);
        let pin = pinned.unwrap_or(was);
        self.conn().execute(
            "INSERT OR REPLACE INTO memory_keep(id, pinned, kept_at) VALUES (?1, ?2, ?3)",
            params![id, pin as i64, now],
        )?;
        self.event(now, kind, id, note)
    }

    /// Whether the owner pinned this note.
    pub fn is_pinned(&self, id: &str) -> bool {
        self.conn()
            .query_row(
                "SELECT pinned FROM memory_keep WHERE id = ?1",
                params![id],
                |r| r.get::<_, i64>(0),
            )
            .map(|v| v != 0)
            .unwrap_or(false)
    }

    /// Whether a note exists and `clearance` may read it.
    fn readable(&self, id: &str, clearance: &Label) -> Result<bool, MemoryError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT * FROM memories WHERE id = ?1",
                params![id],
                row_to_memory,
            )
            .optional()?
            .is_some_and(|r| r.m.label.confidentiality_within(clearance)))
    }

    /// The notes merged into `survivor` that `clearance` may read, newest first.
    pub fn merged_under(
        &self,
        survivor: &str,
        clearance: &Label,
    ) -> Result<Vec<Memory>, MemoryError> {
        if !self.readable(survivor, clearance)? {
            return Ok(Vec::new());
        }
        let mut st = self.conn().prepare(
            "SELECT * FROM memories WHERE merged_into = ?1 AND status = 'merged' ORDER BY created_at DESC, rowid DESC",
        )?;
        let rows = st.query_map(params![survivor], row_to_memory)?;
        let mut out = Vec::new();
        for row in rows {
            let row = row?;
            if row.m.label.confidentiality_within(clearance) {
                out.push(row.m);
            }
        }
        Ok(out)
    }

    pub fn lift_forget(&self, now: i64, text: &str) -> Result<bool, MemoryError> {
        let n = self.conn().execute(
            "DELETE FROM tombstones WHERE fingerprint = ?1",
            params![self.fingerprint(text)],
        )?;
        if n > 0 {
            self.event(now, "lift_forget", "-", "a forget decision was lifted")?;
        }
        Ok(n > 0)
    }

    /// Notes relevant to `query` that `clearance` may read, best first. Two rankings are merged: keyword relevance
    /// (BM25) and closeness of wording (tolerates plurals and small typos). Both are limited by the same date and
    /// label rules inside the query, so a note the caller may not see can never take a place.
    pub fn recall(
        &self,
        query: &str,
        clearance: &Label,
        as_of: i64,
        limit: usize,
    ) -> Result<Vec<Recalled>, MemoryError> {
        self.recall_scoped(query, clearance, as_of, limit, Scope::Global)
    }

    /// Like `recall`, for a given project scope. The plain `recall` sees only notes that apply everywhere, so a project's
    /// notes can never reach a conversation or a review about something else by accident.
    pub fn recall_scoped(
        &self,
        query: &str,
        clearance: &Label,
        as_of: i64,
        limit: usize,
        scope: Scope,
    ) -> Result<Vec<Recalled>, MemoryError> {
        let (all, proj) = scope.params();
        let limit = limit.clamp(1, 50);
        let pool = fuse::pool_for_limit(limit).max(512);
        let level = level_to_int(clearance.level);
        let allowed = allowed_compartments(clearance);
        let mut keyword: Vec<String> = Vec::new();
        let mut by_id: std::collections::HashMap<String, Row> = std::collections::HashMap::new();
        if let Some(q) = fts_query(query) {
            let mut st = self.conn().prepare(
                "SELECT m.* FROM memories_fts f JOIN memories m ON m.rowid = f.rowid
                 WHERE memories_fts MATCH ?1 AND m.status = 'active'
                   AND (m.valid_from IS NULL OR m.valid_from <= ?2) AND (m.valid_to IS NULL OR m.valid_to > ?2)
                   AND m.level <= ?4 AND (?5 = 1 OR m.project IS NULL OR m.project = ?6)
                   AND kk_comp_ok(m.compartments, ?7)
                 ORDER BY bm25(memories_fts) LIMIT ?3",
            )?;
            let rows = st.query_map(
                params![q, as_of, pool as i64, level, all, proj, allowed],
                row_to_memory,
            )?;
            for r in rows {
                let r = r?;
                keyword.push(r.m.id.clone());
                by_id.insert(r.m.id.clone(), r);
            }
        }
        let similar: Vec<String> = {
            let mut st = self.conn().prepare(
                "SELECT m.* FROM memories m
                 WHERE m.status = 'active'
                   AND (m.valid_from IS NULL OR m.valid_from <= ?1) AND (m.valid_to IS NULL OR m.valid_to > ?1)
                   AND m.level <= ?2 AND (?3 = 1 OR m.project IS NULL OR m.project = ?4)
                   AND kk_comp_ok(m.compartments, ?5)
                 ORDER BY m.created_at DESC, m.id LIMIT 2000",
            )?;
            let rows: Vec<Row> = st
                .query_map(params![as_of, level, all, proj, allowed], row_to_memory)?
                .collect::<Result<_, _>>()?;
            let texts: Vec<(String, &str)> = rows
                .iter()
                .map(|r| (r.m.id.clone(), r.m.text.as_str()))
                .collect();
            let ranked = fuse::rank_similar(&significant_terms(query).join(" "), &texts, 0.5);
            let ranked: Vec<String> = ranked.into_iter().take(pool).collect();
            for r in rows {
                if ranked.contains(&r.m.id) && !by_id.contains_key(&r.m.id) {
                    by_id.insert(r.m.id.clone(), r);
                }
            }
            ranked
        };
        let by_meaning = self.rank_by_meaning(query, as_of, clearance, pool, scope, &mut by_id);
        let mut fused = fuse::fuse_weighted(
            &[keyword, similar, by_meaning],
            &[1.0, 1.0, self.meaning_weight],
        );
        self.apply_freshness(as_of, &by_id, &mut fused);
        let mut out = Vec::new();
        for f in fused {
            let Some(r) = by_id.remove(&f.id) else {
                continue;
            };
            if !r.m.label.confidentiality_within(clearance) {
                continue;
            }
            let mut parts = Vec::new();
            if f.ranks[0].is_some() {
                parts.push("keywords");
            }
            if f.ranks[1].is_some() {
                parts.push("similar wording");
            }
            if f.ranks[2].is_some() {
                parts.push("meaning");
            }
            let lower = r.m.text.to_lowercase();
            let matched_terms: Vec<String> = significant_terms(query)
                .into_iter()
                .filter(|t| lower.contains(&t.to_lowercase()))
                .take(6)
                .collect();
            let how = if matched_terms.is_empty() {
                format!("matched by {}", parts.join(" and by "))
            } else {
                format!(
                    "matched by {} on {}",
                    parts.join(" and by "),
                    matched_terms.join(", ")
                )
            };
            out.push(Recalled {
                why: format!("{}; {how}", explain(&r.m)),
                untrusted: r.untrusted,
                matched_terms,
                rank: out.len() + 1,
                memory: r.m,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Lowers the score of stale, unused notes by at most `retention::FRESHNESS_SPAN` and puts the list back in order. It
    /// only reorders notes that the rankings already found, and the span is small enough that a note a few places better
    /// still comes first: it settles between notes that match about equally.
    fn apply_freshness(
        &self,
        as_of: i64,
        found: &std::collections::HashMap<String, Row>,
        fused: &mut [fuse::Fused],
    ) {
        let mut used: std::collections::HashMap<String, (i64, u32)> =
            std::collections::HashMap::new();
        if let Ok(mut st) = self
            .conn()
            .prepare("SELECT id, last_at, uses FROM memory_use")
        {
            if let Ok(rows) = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            }) {
                for (id, at, n) in rows.flatten() {
                    used.insert(id, (at, n.clamp(0, u32::MAX as i64) as u32));
                }
            }
        }
        for f in fused.iter_mut() {
            let Some(r) = found.get(&f.id) else { continue };
            let (last, n) = used.get(&f.id).map_or((None, 0), |(a, n)| (Some(*a), *n));
            let half = retention::half_life_days(r.m.project.is_some());
            let keep = retention::retention(as_of, r.m.created_at, last, n, half);
            f.score *= 1.0 - retention::FRESHNESS_SPAN * (1.0 - keep);
        }
        // a stable sort keeps the merge's own tie order (better best rank, then id) for notes that end up equal
        fused.sort_by(|a, b| b.score.total_cmp(&a.score));
    }

    /// Records that these notes were given to a model or shown as an answer, which makes them rank a little higher next
    /// time. It changes nothing about the notes themselves. Unknown ids and ids the owner forgot are ignored.
    pub fn record_use(&self, now: i64, ids: &[String]) -> Result<(), MemoryError> {
        for id in ids.iter().take(50) {
            self.conn().execute(
                "INSERT INTO memory_use(id, last_at, uses)
                 SELECT ?1, ?2, 1 WHERE EXISTS (SELECT 1 FROM memories WHERE id = ?1)
                 ON CONFLICT(id) DO UPDATE SET last_at = excluded.last_at, uses = uses + 1",
                params![id, now],
            )?;
        }
        Ok(())
    }

    /// How many times each of these notes reached the model and when it last did (unix seconds); a note never used has no entry.
    pub fn usage(&self, ids: &[String]) -> std::collections::HashMap<String, (u32, i64)> {
        let mut out = std::collections::HashMap::new();
        for id in ids.iter().take(500) {
            if let Ok((uses, last)) = self.conn().query_row(
                "SELECT uses, last_at FROM memory_use WHERE id = ?1",
                params![id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            ) {
                out.insert(id.clone(), (uses.clamp(0, u32::MAX as i64) as u32, last));
            }
        }
        out
    }

    /// For tests: puts text into a stored note as an older detector would have let it in, and makes the store say that only that
    /// older detector has looked at it, so the next open checks it again.
    #[doc(hidden)]
    pub fn plant_secret_for_test(&self, id: &str, text: &str) {
        let _ = self.conn().execute(
            "UPDATE memories SET text = ?1 WHERE id = ?2",
            params![text, id],
        );
        let _ = self.conn().execute(
            "UPDATE memory_meta SET value = '0' WHERE key = 'detector_version'",
            [],
        );
    }

    /// How many entries the full-text index itself holds for a match expression, whether or not a note still stands behind them.
    #[doc(hidden)]
    pub fn index_entries_for_test(&self, matching: &str) -> i64 {
        self.conn()
            .query_row(
                "SELECT COUNT(*) FROM memories_fts WHERE memories_fts MATCH ?1",
                params![matching],
                |r| r.get(0),
            )
            .unwrap_or(-1)
    }

    #[doc(hidden)]
    pub fn use_count_for_test(&self, id: &str) -> i64 {
        self.conn()
            .query_row(
                "SELECT uses FROM memory_use WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    #[doc(hidden)]
    pub fn use_rows_for_test(&self) -> i64 {
        self.conn()
            .query_row("SELECT COUNT(*) FROM memory_use", [], |r| r.get(0))
            .unwrap_or(-1)
    }

    /// Ranks readable notes by closeness of meaning to the query. Missing vectors are made on the way (a few at a time, so a
    /// recall stays quick); any problem with the embedder just means this ranking is empty and keywords still work.
    fn rank_by_meaning(
        &self,
        query: &str,
        as_of: i64,
        clearance: &Label,
        pool: usize,
        scope: Scope,
        by_id: &mut std::collections::HashMap<String, Row>,
    ) -> Vec<String> {
        let level = level_to_int(clearance.level);
        let allowed = allowed_compartments(clearance);
        let (all, proj) = scope.params();
        const MAKE_PER_RECALL: usize = 24;
        let Some(e) = &self.embedder else {
            return Vec::new();
        };
        let model = e.model();
        let Ok(mut st) = self.conn().prepare(
            "SELECT m.*, v.model AS vmodel, v.vec AS vvec FROM memories m LEFT JOIN memory_vectors v ON v.id = m.id
             WHERE m.status = 'active' AND m.text <> ''
               AND (m.valid_from IS NULL OR m.valid_from <= ?1) AND (m.valid_to IS NULL OR m.valid_to > ?1)
               AND m.level <= ?2 AND (?3 = 1 OR m.project IS NULL OR m.project = ?4)
               AND kk_comp_ok(m.compartments, ?5)
             ORDER BY m.created_at DESC, m.id LIMIT 1000",
        ) else { return Vec::new() };
        let Ok(rows) = st.query_map(params![as_of, level, all, proj, allowed], |r| {
            let row = row_to_memory(r)?;
            let vm: Option<String> = r.get("vmodel")?;
            let vv: Option<Vec<u8>> = r.get("vvec")?;
            Ok((row, vm, vv))
        }) else {
            return Vec::new();
        };
        let all: Vec<(Row, Option<String>, Option<Vec<u8>>)> = rows.flatten().collect();
        drop(st);
        let mut have: Vec<(String, Vec<f32>)> = Vec::new();
        let mut missing: Vec<(String, String)> = Vec::new();
        for (row, vm, vv) in &all {
            match (vm, vv.as_deref().and_then(fuse::from_blob)) {
                (Some(m), Some(v)) if *m == model => have.push((row.m.id.clone(), v)),
                _ if missing.len() < MAKE_PER_RECALL => {
                    missing.push((row.m.id.clone(), row.m.text.clone()))
                }
                _ => {}
            }
        }
        if !missing.is_empty() {
            let texts: Vec<String> = missing.iter().map(|(_, t)| t.clone()).collect();
            if let Ok(vecs) = e.embed(&texts) {
                if vecs.len() == missing.len() {
                    for ((id, _), v) in missing.iter().zip(vecs) {
                        if v.is_empty() || v.iter().any(|x| !x.is_finite()) {
                            continue;
                        }
                        let _ = self.conn().execute(
                            "INSERT OR REPLACE INTO memory_vectors(id, model, dim, vec) VALUES (?1, ?2, ?3, ?4)",
                            params![id, model, v.len() as i64, fuse::to_blob(&v)],
                        );
                        have.push((id.clone(), v));
                    }
                }
            }
        }
        let Ok(q) = e.embed_query(query) else {
            return Vec::new();
        };
        let min = e.min_similarity();
        let mut scored: Vec<(f32, String)> = have
            .into_iter()
            .map(|(id, v)| (fuse::cosine(&q, &v), id))
            .filter(|(c, _)| *c >= min)
            .collect();
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.cmp(&b.1))
        });
        scored.truncate(pool.min(self.meaning_cap));
        let ids: Vec<String> = scored.into_iter().map(|(_, id)| id).collect();
        for id in &ids {
            if !by_id.contains_key(id) {
                if let Ok(r) = self.conn().query_row(
                    "SELECT * FROM memories WHERE id = ?1",
                    params![id],
                    row_to_memory,
                ) {
                    by_id.insert(id.clone(), r);
                }
            }
        }
        ids
    }

    pub fn propose(
        &self,
        now: i64,
        job_id: &str,
        text: &str,
        why: &str,
        label: &Label,
    ) -> Result<Option<String>, MemoryError> {
        self.propose_with_cap(now, job_id, text, why, label, MAX_PROPOSALS_PER_JOB)
    }

    pub fn propose_with_cap(
        &self,
        now: i64,
        job_id: &str,
        text: &str,
        why: &str,
        label: &Label,
        cap: i64,
    ) -> Result<Option<String>, MemoryError> {
        let why = if why.trim().is_empty() {
            "the review agent suggested it"
        } else {
            why
        };
        if text.trim().is_empty()
            || text.len() > MAX_TEXT
            || why.len() > MAX_REASON
            || has_hidden_chars(text)
            || has_hidden_chars(why)
        {
            return Ok(None);
        }
        if self.source_is_forgotten(job_id)? {
            return Ok(None);
        }
        let fp = self.fingerprint(text);
        let blocked: bool = self
            .conn()
            .query_row(
                "SELECT 1 FROM tombstones WHERE fingerprint = ?1",
                params![fp],
                |_| Ok(()),
            )
            .optional()?
            .is_some()
            || self
                .conn()
                .query_row(
                    "SELECT 1 FROM memories WHERE fingerprint = ?1 AND status = 'active'",
                    params![fp],
                    |_| Ok(()),
                )
                .optional()?
                .is_some()
            || self
                .conn()
                .query_row(
                    "SELECT 1 FROM proposals WHERE job_id = ?1 AND fingerprint = ?2",
                    params![job_id, fp],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
        let count: i64 = self.conn().query_row(
            "SELECT count(*) FROM proposals WHERE job_id = ?1",
            params![job_id],
            |r| r.get(0),
        )?;
        if blocked
            || count >= cap
            || secret::looks_secret(text).is_some()
            || self.near_duplicate(text, job_id)?
        {
            return Ok(None);
        }
        let id = new_id();
        let comps = label
            .compartments
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\u{1f}");
        self.conn().execute(
            "INSERT INTO proposals(id, job_id, text, fingerprint, why, level, compartments, untrusted, unknown_sensitivity, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10)",
            params![id, job_id, text, fp, why, level_to_int(label.level), comps, (label.integrity == Integrity::Untrusted) as i64, label.sensitivity_unknown as i64, now],
        )?;
        self.event(now, "propose", &id, "suggested by a review")?;
        Ok(Some(id))
    }

    /// True when a suggestion says almost what a saved note, or another suggestion of the same review, already says. The
    /// owner would only be asked to decide the same thing twice. Looks at the newest notes only, so the cost stays bounded.
    fn near_duplicate(&self, text: &str, job_id: &str) -> Result<bool, MemoryError> {
        let mine = fuse::trigrams(text);
        let mut seen = Vec::new();
        {
            let mut st = self.conn().prepare("SELECT text FROM memories WHERE status = 'active' ORDER BY created_at DESC, rowid DESC LIMIT 2000")?;
            for t in st.query_map([], |r| r.get::<_, String>(0))? {
                seen.push(t?);
            }
            let mut st = self
                .conn()
                .prepare("SELECT text FROM proposals WHERE job_id = ?1 AND status = 'pending'")?;
            for t in st.query_map(params![job_id], |r| r.get::<_, String>(0))? {
                seen.push(t?);
            }
        }
        Ok(seen.iter().any(|t| {
            fuse::jaccard(&mine, &fuse::trigrams(t)) >= NEAR_DUPLICATE && same_claim(text, t)
        }))
    }

    pub fn pending_proposals(&self, job_id: &str) -> Result<Vec<Proposal>, MemoryError> {
        let mut st = self.conn().prepare("SELECT id, text, why, level, compartments, untrusted, unknown_sensitivity FROM proposals WHERE job_id = ?1 AND status = 'pending' ORDER BY created_at, rowid")?;
        let rows = st.query_map(params![job_id], |r| {
            let comps: String = r.get(4)?;
            let mut label = Label::new(int_to_level(r.get(3)?));
            for c in comps.split('\u{1f}').filter(|c| !c.is_empty()) {
                label = label.with_compartment(c);
            }
            if r.get::<_, i64>(5)? != 0 {
                label = label.untrusted();
            }
            if r.get::<_, i64>(6)? != 0 {
                label = label.unknown_sensitivity();
            }
            Ok(Proposal {
                id: r.get(0)?,
                text: r.get(1)?,
                why: r.get(2)?,
                label,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn accept_proposal(
        &self,
        now: i64,
        id: &str,
        job_id: &str,
        project: Option<String>,
    ) -> Result<Remembered, MemoryError> {
        let p = self
            .pending_proposals(job_id)?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(MemoryError::NotFound)?;
        let r = self.remember(
            now,
            NewMemory {
                text: p.text.clone(),
                label: p.label.clone(),
                subject: None,
                project,
                reason: format!("suggested by review {job_id}: {}", p.why),
                source: Source::Review(job_id.to_string()),
                valid_from: None,
                valid_to: None,
            },
        )?;
        // The memory now holds the text; the suggestion row keeps no second copy.
        self.conn().execute(
            "UPDATE proposals SET status = 'accepted', text = '', why = '' WHERE id = ?1",
            params![id],
        )?;
        self.event(now, "accept_proposal", id, "accepted by the owner")?;
        Ok(r)
    }

    /// Dismisses every suggestion still waiting for the owner from one source, clearing its text. Returns how many.
    pub fn dismiss_pending(&self, now: i64, job_id: &str) -> Result<usize, MemoryError> {
        let n = self.conn().execute(
            "UPDATE proposals SET status = 'dismissed', text = '', why = '' WHERE job_id = ?1 AND status = 'pending'",
            params![job_id],
        )?;
        if n > 0 {
            self.event(now, "dismiss_pending", job_id, "the source was forgotten")?;
        }
        Ok(n)
    }

    pub fn dismiss_proposal(&self, now: i64, id: &str, job_id: &str) -> Result<(), MemoryError> {
        let n = self.conn().execute("UPDATE proposals SET status = 'dismissed', text = '', why = '' WHERE id = ?1 AND job_id = ?2 AND status = 'pending'", params![id, job_id])?;
        if n == 0 {
            return Err(MemoryError::NotFound);
        }
        self.event(now, "dismiss_proposal", id, "dismissed by the owner")?;
        Ok(())
    }

    pub fn counts_by_level(&self) -> Result<Vec<(String, i64)>, MemoryError> {
        let mut st = self.conn().prepare("SELECT level, count(*) FROM memories WHERE status = 'active' GROUP BY level ORDER BY level")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            let (l, n) = r?;
            out.push((format!("{:?}", int_to_level(l)).to_lowercase(), n));
        }
        Ok(out)
    }

    pub fn owner_clearance(&self) -> Result<Label, MemoryError> {
        let mut label = Label::new(Level::Restricted);
        let mut st = self
            .conn()
            .prepare("SELECT DISTINCT compartments FROM memories WHERE compartments != ''")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        for r in rows {
            for c in r?.split('\u{1f}').filter(|c| !c.is_empty()) {
                label = label.with_compartment(c);
            }
        }
        Ok(label)
    }

    pub fn list(
        &self,
        clearance: &Label,
        as_of: i64,
        limit: usize,
    ) -> Result<Vec<Recalled>, MemoryError> {
        self.list_scoped(clearance, as_of, limit, Scope::Global)
    }

    pub fn list_scoped(
        &self,
        clearance: &Label,
        as_of: i64,
        limit: usize,
        scope: Scope,
    ) -> Result<Vec<Recalled>, MemoryError> {
        let (all, proj) = scope.params();
        let mut st = self.conn().prepare(
            "SELECT * FROM memories WHERE status = 'active'
               AND (valid_from IS NULL OR valid_from <= ?1) AND (valid_to IS NULL OR valid_to > ?1)
               AND (?2 = 1 OR project IS NULL OR project = ?3)
             ORDER BY created_at DESC, rowid DESC",
        )?;
        let rows = st.query_map(params![as_of, all, proj], row_to_memory)?;
        let mut out = Vec::new();
        for r in rows {
            let r = r?;
            if r.m.label.confidentiality_within(clearance) {
                out.push(Recalled {
                    why: explain(&r.m),
                    untrusted: r.untrusted,
                    matched_terms: Vec::new(),
                    rank: out.len() + 1,
                    memory: r.m,
                });
                if out.len() >= limit.max(1) {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Notes that stop applying within `within` seconds after `as_of`, soonest first, so the owner can renew or let them go before
    /// they quietly stop being recalled. Notes with no end date, notes already over and notes that are not active are never listed.
    pub fn expiring(
        &self,
        clearance: &Label,
        as_of: i64,
        within: i64,
        limit: usize,
        scope: Scope,
    ) -> Result<Vec<Recalled>, MemoryError> {
        let (all, proj) = scope.params();
        let until = as_of.saturating_add(within.max(0));
        let mut st = self.conn().prepare(
            "SELECT * FROM memories WHERE status = 'active'
               AND (valid_from IS NULL OR valid_from <= ?1)
               AND valid_to IS NOT NULL AND valid_to > ?1 AND valid_to <= ?2
               AND (?3 = 1 OR project IS NULL OR project = ?4)
             ORDER BY valid_to ASC, rowid ASC",
        )?;
        let rows = st.query_map(params![as_of, until, all, proj], row_to_memory)?;
        let mut out = Vec::new();
        for r in rows {
            let r = r?;
            if r.m.label.confidentiality_within(clearance) {
                out.push(Recalled {
                    why: explain(&r.m),
                    untrusted: r.untrusted,
                    matched_terms: Vec::new(),
                    rank: out.len() + 1,
                    memory: r.m,
                });
                if out.len() >= limit.clamp(1, 200) {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Saved notes that say almost the same thing, for the owner to look at. Read-only: nothing is merged, forgotten or
    /// changed. The newest 200 notes the caller may see are compared. Each group is anchored on its newest note and holds
    /// only notes that match that anchor directly (the same overlap and the same claim as the suggestion check), so a chain
    /// of look-alikes never pulls two unrelated notes into one group. Notes for different projects are not look-alikes of
    /// each other, and a note of fewer than four words is too short to compare.
    pub fn similar_notes(
        &self,
        clearance: &Label,
        as_of: i64,
        scope: Scope,
    ) -> Result<SimilarNotes, MemoryError> {
        let notes = self.list_scoped(clearance, as_of, SIMILAR_SCAN, scope)?;
        let scanned = notes.len();
        struct Anchor {
            grams: std::collections::BTreeSet<[char; 3]>,
            first: usize,
        }
        let mut anchors: Vec<Anchor> = Vec::new();
        let mut members: Vec<Vec<usize>> = Vec::new();
        for (i, n) in notes.iter().enumerate() {
            let words = word_sequence(&n.memory.text).len();
            if words < 4 {
                continue;
            }
            let grams = fuse::trigrams(&n.memory.text);
            let home = anchors.iter().position(|a| {
                let first = &notes[a.first].memory;
                first.project == n.memory.project
                    && fuse::jaccard(&a.grams, &grams) >= NEAR_DUPLICATE
                    && same_claim(&first.text, &n.memory.text)
            });
            match home {
                Some(g) => members[g].push(i),
                None => {
                    anchors.push(Anchor { grams, first: i });
                    members.push(vec![i]);
                }
            }
        }
        let mut groups: Vec<Vec<Recalled>> = members
            .into_iter()
            .filter(|g| g.len() >= 2)
            .map(|g| g.into_iter().map(|i| notes[i].clone()).collect())
            .collect();
        // the largest groups first; the newest note decides ties, as the list is already newest first
        groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
        Ok(SimilarNotes {
            groups,
            scanned,
            complete: scanned < SIMILAR_SCAN,
        })
    }

    /// Pairs of notes, valid now and of the same project, that disagree: the owner gave them the same subject but they say
    /// different things, or they share most of their words and still do not say the same thing. Pairs the owner already
    /// called both right are left out. Only a reading: nothing is changed.
    pub fn conflicts(
        &self,
        clearance: &Label,
        as_of: i64,
        scope: Scope,
    ) -> Result<Conflicts, MemoryError> {
        let notes = self.list_scoped(clearance, as_of, CONFLICT_SCAN, scope)?;
        let scanned = notes.len();
        let dismissed: BTreeSet<(String, String)> = {
            let mut st = self.conn().prepare("SELECT a, b FROM conflict_dismissed")?;
            let rows =
                st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let grams: Vec<_> = notes
            .iter()
            .map(|n| fuse::trigrams(&n.memory.text))
            .collect();
        let words: Vec<usize> = notes
            .iter()
            .map(|n| word_sequence(&n.memory.text).len())
            .collect();
        let subject = |n: &Recalled| {
            n.memory
                .subject
                .as_deref()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
        };
        let mut found: Vec<(f64, usize, usize, ConflictWhy)> = Vec::new();
        for i in 0..notes.len() {
            for j in i + 1..notes.len() {
                let (x, y) = (&notes[i].memory, &notes[j].memory);
                if x.project != y.project || x.id == y.id {
                    continue;
                }
                let key = if x.id < y.id {
                    (x.id.clone(), y.id.clone())
                } else {
                    (y.id.clone(), x.id.clone())
                };
                if dismissed.contains(&key) {
                    continue;
                }
                let same_subject = matches!((subject(&notes[i]), subject(&notes[j])), (Some(a), Some(b)) if a == b);
                let sim = if words[i] >= 4 && words[j] >= 4 {
                    fuse::jaccard(&grams[i], &grams[j])
                } else {
                    0.0
                };
                if same_subject && !same_claim(&x.text, &y.text) {
                    found.push((
                        2.0 + sim,
                        i,
                        j,
                        ConflictWhy::SameSubject(subject(&notes[i]).unwrap_or_default()),
                    ));
                } else if sim >= CONFLICT_SIMILARITY && !same_claim(&x.text, &y.text) {
                    found.push((sim, i, j, ConflictWhy::OtherClaim));
                }
            }
        }
        found.sort_by(|p, q| q.0.total_cmp(&p.0));
        found.truncate(MAX_CONFLICTS);
        Ok(Conflicts {
            pairs: found
                .into_iter()
                .map(|(_, i, j, why)| Conflict {
                    a: notes[i].clone(),
                    b: notes[j].clone(),
                    why,
                })
                .collect(),
            scanned,
            complete: scanned < CONFLICT_SCAN,
        })
    }

    fn load_readable(&self, id: &str, clearance: &Label) -> Result<Row, MemoryError> {
        let row = self
            .conn()
            .query_row(
                "SELECT * FROM memories WHERE id = ?1",
                params![id],
                row_to_memory,
            )
            .optional()?
            .ok_or(MemoryError::NotFound)?;
        if !row.m.label.confidentiality_within(clearance) {
            return Err(MemoryError::NotFound);
        }
        Ok(row)
    }

    /// Keeps one of two disagreeing notes and replaces the other with it: the replaced note stops being recalled and stays
    /// as the earlier version in the kept note's history. Both must be active notes of the same project the owner may read,
    /// and no note may have been merged under the replaced one (unmerge those first).
    pub fn supersede(
        &self,
        now: i64,
        keep: &str,
        replaced: &str,
        clearance: &Label,
    ) -> Result<(), MemoryError> {
        if keep == replaced {
            return refuse("a note is not replaced by itself");
        }
        let (k, r) = (
            self.load_readable(keep, clearance)?,
            self.load_readable(replaced, clearance)?,
        );
        if k.m.status != Status::Active || r.m.status != Status::Active {
            return refuse("one of the notes is already replaced or merged");
        }
        if k.m.project != r.m.project {
            return refuse("notes of different projects do not replace each other");
        }
        let under: i64 = self.conn().query_row(
            "SELECT count(*) FROM memories WHERE merged_into = ?1",
            params![replaced],
            |row| row.get(0),
        )?;
        if under > 0 {
            return refuse("notes were merged under the one to replace: bring them back first");
        }
        self.conn().execute_batch("SAVEPOINT supersede_note")?;
        let done = (|| -> Result<(), MemoryError> {
            self.conn().execute(
                "UPDATE memories SET status = 'superseded', superseded_by = ?1 WHERE id = ?2 AND status = 'active'",
                params![keep, replaced],
            )?;
            self.conn().execute(
                "DELETE FROM memory_vectors WHERE id = ?1",
                params![replaced],
            )?;
            self.event(now, "supersede", replaced, &format!("replaced by {keep}"))?;
            Ok(())
        })();
        match done {
            Ok(()) => self.conn().execute_batch("RELEASE supersede_note")?,
            Err(e) => {
                self.conn()
                    .execute_batch("ROLLBACK TO supersede_note; RELEASE supersede_note")?;
                return Err(e);
            }
        }
        Ok(())
    }

    /// The owner says both notes are right: that pair is not shown again, until one of them is corrected into a new note.
    pub fn dismiss_conflict(
        &self,
        now: i64,
        a: &str,
        b: &str,
        clearance: &Label,
    ) -> Result<(), MemoryError> {
        if a == b {
            return refuse("a note does not disagree with itself");
        }
        let (x, y) = (
            self.load_readable(a, clearance)?,
            self.load_readable(b, clearance)?,
        );
        if x.m.status != Status::Active || y.m.status != Status::Active {
            return refuse("one of the notes is already replaced or merged");
        }
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        self.conn().execute(
            "INSERT OR IGNORE INTO conflict_dismissed(a, b, at) VALUES (?1, ?2, ?3)",
            params![lo, hi, now],
        )?;
        self.event(now, "dismiss_conflict", lo, "both notes are right")?;
        Ok(())
    }

    pub fn history(&self, id: &str, clearance: &Label) -> Result<Vec<Memory>, MemoryError> {
        let mut start = id.to_string();
        let mut seen = BTreeSet::new();
        while let Some(prev) = self
            .conn()
            .query_row(
                "SELECT id FROM memories WHERE superseded_by = ?1",
                params![start],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            if !seen.insert(prev.clone()) {
                break;
            }
            start = prev;
        }
        let mut out = Vec::new();
        let mut cur = Some(start);
        while let Some(c) = cur {
            let Some(row) = self
                .conn()
                .query_row(
                    "SELECT * FROM memories WHERE id = ?1",
                    params![c],
                    row_to_memory,
                )
                .optional()?
            else {
                break;
            };
            if !row.m.label.confidentiality_within(clearance) {
                return Ok(Vec::new());
            }
            cur = row.m.superseded_by.clone();
            out.push(row.m);
        }
        Ok(out)
    }

    #[doc(hidden)]
    pub fn connection_for_tests(&self) -> &Connection {
        self.conn()
    }

    pub fn events(&self) -> Result<Vec<(i64, String, String, String)>, MemoryError> {
        let mut st = self
            .conn()
            .prepare("SELECT at, kind, memory_id, note FROM memory_events ORDER BY seq")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub text: String,
    pub why: String,
    pub label: Label,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recalled {
    pub memory: Memory,
    pub why: String,
    pub untrusted: bool,
    /// Words of the question that appear in the note (at most 6), so the owner can see what it was matched on.
    pub matched_terms: Vec<String>,
    /// Position in this answer, starting at 1.
    pub rank: usize,
}

/// Where a note came from, as a closed set an owner and a program can both rely on.
pub const ORIGINS: [&str; 4] = ["owner", "imported", "conversation", "review"];

impl Recalled {
    /// `owner`: written by the owner. `imported`: added from a file or another assistant (kept as untrusted text).
    /// `conversation`: suggested from a kept chat and accepted. `review`: suggested by a code review and accepted.
    pub fn origin(&self) -> &'static str {
        match &self.memory.source {
            Source::User if self.untrusted => "imported",
            Source::User => "owner",
            Source::Review(id) if id.starts_with("chat-") => "conversation",
            Source::Review(_) => "review",
        }
    }

    pub fn instruction_like(&self) -> bool {
        instruction_like(&self.memory.text)
    }
}

fn explain(m: &Memory) -> String {
    let src = match &m.source {
        Source::User => "you asked to remember it".to_string(),
        Source::Review(id) if id.starts_with("chat-") => format!("it came from conversation {id}"),
        Source::Review(id) => format!("it came from review {id}"),
    };
    let window = match (m.valid_from, m.valid_to) {
        (None, None) => String::new(),
        (a, b) => format!(
            "; valid {}{}",
            a.map(|x| format!("from {x} ")).unwrap_or_default(),
            b.map(|x| format!("until {x}")).unwrap_or_default()
        ),
    };
    format!("{}: {}{}", src, m.reason, window)
}
