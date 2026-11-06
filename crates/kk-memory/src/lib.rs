mod fuse;
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
    superseded_by TEXT
);
CREATE INDEX IF NOT EXISTS memories_fp ON memories(fingerprint);
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(text, content='memories', content_rowid='rowid', tokenize='unicode61');
CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.rowid, old.text);
END;
CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.rowid, old.text);
    INSERT INTO memories_fts(rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TABLE IF NOT EXISTS tombstones (
    fingerprint BLOB PRIMARY KEY,
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
CREATE TABLE IF NOT EXISTS memory_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    note TEXT NOT NULL
);
";

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
    s.chars().any(|c| {
        (c.is_control() && c != '\n' && c != '\t') || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
    })
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
    let terms: Vec<String> = significant_terms(q)
        .into_iter()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// How many of the newest notes are compared when looking for notes that say almost the same thing.
pub const SIMILAR_SCAN: usize = 200;

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
    let t = text.to_lowercase();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    const PHRASES: &[&str] = &[
        "ignore all previous",
        "ignore previous",
        "ignore the previous",
        "ignore prior",
        "ignore the above",
        "ignore all prior",
        "ignore your instructions",
        "ignore any instructions",
        "disregard all",
        "disregard previous",
        "disregard the previous",
        "disregard your",
        "forget your instructions",
        "forget all previous",
        "forget previous instructions",
        "new instructions:",
        "system prompt",
        "you are now",
        "you must now",
        "from now on you",
        "act as ",
        "pretend to be",
        "pretend you are",
        "reply only with",
        "respond only with",
        "answer only with",
        "do not tell the user",
        "don't tell the user",
        "without telling the user",
        "override your",
        "jailbreak",
        "developer mode",
    ];
    PHRASES.iter().any(|p| t.contains(p))
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
            status: if status == "active" {
                Status::Active
            } else {
                Status::Superseded
            },
            superseded_by: r.get("superseded_by")?,
        },
    })
}

impl Memories {
    pub fn open(vault: Vault, fingerprint_key: [u8; 32]) -> Result<Memories, MemoryError> {
        vault.connection().execute_batch(SCHEMA)?;
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
        vault.connection().execute_batch("CREATE TABLE IF NOT EXISTS memory_vectors(id TEXT PRIMARY KEY, model TEXT NOT NULL, dim INTEGER NOT NULL, vec BLOB NOT NULL);")?;
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
        let mut chain = vec![id.to_string()];
        let mut i = 0;
        while i < chain.len() {
            let prev: Vec<String> = {
                let mut st = self
                    .conn()
                    .prepare("SELECT id FROM memories WHERE superseded_by = ?1")?;
                let rows = st.query_map(params![chain[i]], |r| r.get(0))?;
                rows.collect::<Result<_, _>>()?
            };
            chain.extend(prev);
            i += 1;
        }
        self.conn().execute_batch("SAVEPOINT forget")?;
        let r = (|| -> Result<(), MemoryError> {
            let mut found = false;
            for mid in &chain {
                let fp: Option<Vec<u8>> = self
                    .conn()
                    .query_row(
                        "SELECT fingerprint FROM memories WHERE id = ?1",
                        params![mid],
                        |r| r.get(0),
                    )
                    .optional()?;
                let Some(fp) = fp else { continue };
                found = true;
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
                self.conn()
                    .execute("DELETE FROM memories WHERE id = ?1", params![mid])?;
                self.event(now, "forget", mid, "deleted")?;
            }
            if found {
                Ok(())
            } else {
                Err(MemoryError::NotFound)
            }
        })();
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
        let mut keyword: Vec<String> = Vec::new();
        let mut by_id: std::collections::HashMap<String, Row> = std::collections::HashMap::new();
        if let Some(q) = fts_query(query) {
            let mut st = self.conn().prepare(
                "SELECT m.* FROM memories_fts f JOIN memories m ON m.rowid = f.rowid
                 WHERE memories_fts MATCH ?1 AND m.status = 'active'
                   AND (m.valid_from IS NULL OR m.valid_from <= ?2) AND (m.valid_to IS NULL OR m.valid_to > ?2)
                   AND m.level <= ?4 AND (?5 = 1 OR m.project IS NULL OR m.project = ?6)
                 ORDER BY bm25(memories_fts) LIMIT ?3",
            )?;
            let rows = st.query_map(
                params![q, as_of, pool as i64, level, all, proj],
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
                 ORDER BY m.created_at DESC, m.id LIMIT 2000",
            )?;
            let rows: Vec<Row> = st
                .query_map(params![as_of, level, all, proj], row_to_memory)?
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
        let by_meaning = self.rank_by_meaning(query, as_of, level, pool, scope, &mut by_id);
        let mut out = Vec::new();
        for f in fuse::fuse_weighted(
            &[keyword, similar, by_meaning],
            &[1.0, 1.0, self.meaning_weight],
        ) {
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
            let how = format!("matched by {}", parts.join(" and by "));
            out.push(Recalled {
                why: format!("{}; {how}", explain(&r.m)),
                untrusted: r.untrusted,
                memory: r.m,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Ranks readable notes by closeness of meaning to the query. Missing vectors are made on the way (a few at a time, so a
    /// recall stays quick); any problem with the embedder just means this ranking is empty and keywords still work.
    fn rank_by_meaning(
        &self,
        query: &str,
        as_of: i64,
        level: i64,
        pool: usize,
        scope: Scope,
        by_id: &mut std::collections::HashMap<String, Row>,
    ) -> Vec<String> {
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
             ORDER BY m.created_at DESC, m.id LIMIT 1000",
        ) else { return Vec::new() };
        let Ok(rows) = st.query_map(params![as_of, level, all, proj], |r| {
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
                    memory: r.m,
                });
                if out.len() >= limit.max(1) {
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
}

impl Recalled {
    pub fn instruction_like(&self) -> bool {
        instruction_like(&self.memory.text)
    }
}

fn explain(m: &Memory) -> String {
    let src = match &m.source {
        Source::User => "you asked to remember it".to_string(),
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
