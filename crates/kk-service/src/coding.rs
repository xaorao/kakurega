//! Coding tasks: a model changes a copy of the owner's project in memory, the project's tests run in the sandbox, and the
//! owner reads the exact patch before anything on disk changes. Applying writes every file or none, only while each file is
//! still what the task started from, and only the patch whose digest the owner saw. Undo puts every file back while each is
//! still what the patch wrote.

use crate::fixfiles::{self, Outcome};
use kk_review::code::{Change, Kind, TestRun};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What a runner hands back for a coding task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeOutcome {
    pub ending: String,
    pub summary: String,
    pub turns: usize,
    pub steps: Vec<(String, bool, String)>,
    /// The arguments of each step, in the same order (kept from older tasks as empty).
    #[serde(default)]
    pub step_args: Vec<String>,
    /// What each step was about in plain words (kept from older tasks as empty).
    #[serde(default)]
    pub step_labels: Vec<String>,
    pub test_runs: usize,
    /// The kernel's own run of the tests on the final files (never the model's claim), and the command used.
    pub kernel_test: Option<TestRun>,
    pub test_command: Option<String>,
    pub changes: Vec<Change>,
    pub left_restricted: usize,
    pub route: String,
    /// What it used of the model, for the usage record (kept from older tasks as none).
    #[serde(default)]
    pub usage: Option<crate::questions::ModelUse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Running,
    Ready,
    /// Written down before any file is touched; found at the next start if the service stopped part way.
    Applying,
    Undoing,
    Failed(String),
    Applied,
    /// A write was interrupted or a file changed underneath: the listed files need a person to look.
    Attention(Vec<String>),
}

impl State {
    fn word(&self) -> &'static str {
        match self {
            State::Running => "running",
            State::Ready => "ready",
            State::Failed(_) => "failed",
            State::Applied => "applied",
            State::Applying => "applying",
            State::Undoing => "undoing",
            State::Attention(_) => "attention",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub repo: PathBuf,
    pub task: String,
    pub created: i64,
    pub state: State,
    pub outcome: Option<CodeOutcome>,
    pub progress: Vec<String>,
    /// When it last showed progress (seconds); 0 in records kept before this was noted, which then count from their start.
    #[serde(default)]
    pub last: i64,
}

#[derive(Default)]
pub struct Tasks {
    pub list: BTreeMap<String, Task>,
    pub next: u64,
}

impl Tasks {
    /// The tasks kept from before. One that was running when the service stopped failed; one stopped while it was being applied
    /// or undone is settled by reading the files.
    pub fn load(c: &kk_vault::rusqlite::Connection) -> Tasks {
        let mut t = Tasks::default();
        let Ok(mut stmt) = c.prepare("SELECT id, body FROM code_tasks") else {
            return t;
        };
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|r| r.flatten().collect())
            .unwrap_or_default();
        for (id, body) in rows {
            let Ok(mut task) = serde_json::from_str::<Task>(&body) else {
                continue;
            };
            if let Some(n) = id.strip_prefix("code-").and_then(|n| n.parse::<u64>().ok()) {
                t.next = t.next.max(n);
            }
            match task.state {
                State::Running => {
                    task.state =
                        State::Failed("the service stopped while the task was running".into())
                }
                State::Applying | State::Undoing => task.state = settle(&task),
                _ => {}
            }
            save(c, &task);
            t.list.insert(id, task);
        }
        t
    }
}

pub fn save(c: &kk_vault::rusqlite::Connection, t: &Task) -> bool {
    let body = serde_json::to_string(t).unwrap_or_default();
    c.execute(
        "INSERT INTO code_tasks(id, body) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET body = excluded.body",
        (&t.id, &body),
    )
    .is_ok()
}

/// After a stop part way through applying or undoing: every file as the task found it means nothing was applied; every file as
/// the patch wrote it means it was applied; anything else is named for a person to look at.
pub fn settle(t: &Task) -> State {
    let changes = t
        .outcome
        .as_ref()
        .map(|o| o.changes.as_slice())
        .unwrap_or(&[]);
    let mut before = 0;
    let mut after = 0;
    let mut odd = Vec::new();
    for c in changes {
        let now = place(&t.repo, &c.path)
            .ok()
            .and_then(|p| std::fs::read(p).ok());
        let was = (c.kind != Kind::Added).then(|| c.before.as_bytes().to_vec());
        let will = (c.kind != Kind::Deleted).then(|| c.after.as_bytes().to_vec());
        if now == was {
            before += 1;
        } else if now == will {
            after += 1;
        } else {
            odd.push(c.path.clone());
        }
    }
    if odd.is_empty() && after == 0 {
        State::Ready
    } else if odd.is_empty() && before == 0 {
        State::Applied
    } else {
        let mut mixed: Vec<String> = odd;
        if mixed.is_empty() {
            mixed = changes.iter().map(|c| c.path.clone()).collect();
        }
        State::Attention(mixed)
    }
}

/// The digest of a patch: every changed path with its kind and its text before and after, in path order. Applying requires it,
/// so what is written is exactly what the owner read.
pub fn digest(changes: &[Change]) -> String {
    let mut all = Vec::new();
    for c in changes {
        let kind = match c.kind {
            Kind::Added => "A",
            Kind::Modified => "M",
            Kind::Deleted => "D",
        };
        for part in [kind, c.path.as_str(), c.before.as_str(), c.after.as_str()] {
            all.extend_from_slice(&(part.len() as u64).to_be_bytes());
            all.extend_from_slice(part.as_bytes());
        }
    }
    kk_evidence::capsule::sha256_hex(&all)
}

pub fn view(t: &Task) -> Value {
    let o = t.outcome.as_ref();
    let changes: Vec<Value> = o
        .map(|o| {
            o.changes
                .iter()
                .map(|c| json!({"path": c.path, "kind": match c.kind { Kind::Added => "added", Kind::Modified => "modified", Kind::Deleted => "deleted" }, "diff": c.diff, "added": c.added, "removed": c.removed}))
                .collect()
        })
        .unwrap_or_default();
    let concerns = o
        .map(|o| kk_review::code::concerns(&o.changes))
        .unwrap_or_default();
    json!({
        "id": t.id,
        "task": t.task,
        "folder": t.repo.to_string_lossy(),
        "created": t.created,
        "state": t.state.word(),
        "error": match &t.state { State::Failed(m) => json!(m), _ => Value::Null },
        "attention": match &t.state { State::Attention(f) => json!(f), _ => json!([]) },
        "progress": t.progress,
        "ending": o.map(|o| o.ending.clone()),
        "summary": o.map(|o| o.summary.clone()),
        "turns": o.map(|o| o.turns),
        "steps": o.map(|o| o.steps.iter().enumerate().map(|(i, (n, refused, said))| json!({"tool": n, "label": o.step_labels.get(i), "refused": refused, "said": said})).collect::<Vec<_>>()).unwrap_or_default(),
        "ended": o.map(|o| ended(&o.ending)),
        "model_test_runs": o.map(|o| o.test_runs),
        "tests": o.and_then(|o| o.kernel_test.as_ref()).map(|t| json!({"ran": t.ran, "passed": t.passed, "output": t.output})),
        "test_command": o.and_then(|o| o.test_command.clone()),
        "withheld_files": o.map(|o| o.left_restricted),
        "route": o.map(|o| o.route.clone()),
        "changes": changes,
        "digest": o.map(|o| digest(&o.changes)),
        "concerns": concerns,
        "can_apply": t.state == State::Ready && o.is_some_and(|o| !o.changes.is_empty()) && !blocks_apply(o),
    })
}

/// How the model's work ended, in words.
fn ended(ending: &str) -> &'static str {
    match ending {
        "Finished" => "the model said it was done",
        "Answered" => "the model stopped answering with the tools before it said it was done",
        "TurnLimit" => "the model used every turn it was allowed without saying it was done",
        "Stuck" => "the model kept repeating the same call and was stopped",
        _ => "the model could not be reached or failed part way",
    }
}

/// A patch that adds a secret is never applied.
fn blocks_apply(o: Option<&CodeOutcome>) -> bool {
    o.is_some_and(|o| {
        kk_review::code::concerns(&o.changes)
            .iter()
            .any(|c| c.contains("will not be applied"))
    })
}

/// The path of a changed file on disk, refused unless it is a plain path inside the project whose folders are real folders
/// (no link leads out of the project) and, if it exists, is a regular file.
fn place(repo: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty()
        || rel.starts_with('/')
        || rel
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || rel.contains('\\')
    {
        return Err(format!("{rel}: not a plain path inside the project"));
    }
    let full = repo.join(rel);
    let mut dir = full
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repo.to_path_buf());
    // the nearest folder that exists must resolve inside the project
    while !dir.exists() {
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    let real =
        std::fs::canonicalize(&dir).map_err(|_| format!("{rel}: its folder cannot be read"))?;
    let root =
        std::fs::canonicalize(repo).map_err(|_| "the project folder cannot be read".to_string())?;
    if !real.starts_with(&root) {
        return Err(format!("{rel}: its folder leads outside the project"));
    }
    if let Ok(meta) = std::fs::symlink_metadata(&full) {
        if !meta.file_type().is_file() {
            return Err(format!("{rel}: not a regular file"));
        }
    }
    Ok(full)
}

/// Writes the patch: every modified file or none (each only while it still holds the text the task started from), then the new
/// files (only where no file is), then the deletions (only of files still as they were). A failure part way puts back what was
/// done and says so.
pub fn apply(repo: &Path, changes: &[Change]) -> Result<(), State> {
    let mut plan_mod = Vec::new();
    let mut plan_add = Vec::new();
    let mut plan_del = Vec::new();
    for c in changes {
        let full = place(repo, &c.path).map_err(State::Failed)?;
        match c.kind {
            Kind::Modified | Kind::Deleted => {
                let now = std::fs::read(&full)
                    .map_err(|_| State::Failed(format!("{}: could not be read", c.path)))?;
                if now != c.before.as_bytes() {
                    return Err(State::Failed(format!("{} has changed since the task read it, so the patch no longer applies; run the task again", c.path)));
                }
                let perms = std::fs::metadata(&full)
                    .map_err(|_| State::Failed(format!("{}: could not be read", c.path)))?
                    .permissions();
                if c.kind == Kind::Modified {
                    plan_mod.push(fixfiles::Change {
                        path: full,
                        before: now,
                        after: c.after.clone().into_bytes(),
                        perms,
                    });
                } else {
                    plan_del.push((full, now));
                }
            }
            Kind::Added => {
                if full.exists() {
                    return Err(State::Failed(format!(
                        "{} now exists, so the patch would overwrite it; run the task again",
                        c.path
                    )));
                }
                plan_add.push((full, c.after.clone()));
            }
        }
    }
    match fixfiles::write_all_or_none(&plan_mod) {
        Outcome::Done => {}
        Outcome::RolledBack => {
            return Err(State::Failed(
                "a file could not be written; nothing was changed".into(),
            ))
        }
        Outcome::Stuck(files) => {
            return Err(State::Attention(
                files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
            ))
        }
    }
    let undo_mods = |plan: &[fixfiles::Change]| {
        let back: Vec<fixfiles::Change> = plan
            .iter()
            .map(|c| fixfiles::Change {
                path: c.path.clone(),
                before: c.after.clone(),
                after: c.before.clone(),
                perms: c.perms.clone(),
            })
            .collect();
        fixfiles::write_all_or_none(&back)
    };
    let mut created: Vec<PathBuf> = Vec::new();
    for (full, text) in &plan_add {
        let ok = full
            .parent()
            .map(std::fs::create_dir_all)
            .transpose()
            .is_ok()
            && std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(full)
                .and_then(|mut f| {
                    std::io::Write::write_all(&mut f, text.as_bytes()).and_then(|_| f.sync_all())
                })
                .is_ok();
        if !ok {
            for c in &created {
                let _ = std::fs::remove_file(c);
            }
            let _ = std::fs::remove_file(full);
            // putting the changed files back can fail too; then they are named for a person to look at
            return match undo_mods(&plan_mod) {
                Outcome::Done => Err(State::Failed(
                    "a new file could not be written; everything was put back".into(),
                )),
                Outcome::RolledBack | Outcome::Stuck(_) => {
                    Err(State::Attention(f_paths(&plan_mod)))
                }
            };
        }
        created.push(full.clone());
    }
    for (full, before) in &plan_del {
        if std::fs::read(full).ok().as_deref() != Some(before.as_slice())
            || std::fs::remove_file(full).is_err()
        {
            return Err(State::Attention(vec![full.to_string_lossy().into_owned()]));
        }
    }
    Ok(())
}

fn f_paths(plan: &[fixfiles::Change]) -> Vec<String> {
    plan.iter()
        .map(|c| c.path.to_string_lossy().into_owned())
        .collect()
}

/// Puts the project back: each file only while it still holds what the patch wrote. If any file was changed since, nothing is
/// touched and the files are named.
pub fn undo(repo: &Path, changes: &[Change]) -> Result<(), State> {
    let mut changed_since = Vec::new();
    let mut back = Vec::new();
    for c in changes {
        let full = place(repo, &c.path).map_err(State::Failed)?;
        let now = std::fs::read(&full).ok();
        let expected_now = match c.kind {
            Kind::Deleted => None,
            _ => Some(c.after.as_bytes().to_vec()),
        };
        if now != expected_now {
            changed_since.push(c.path.clone());
        }
        back.push((full, c));
    }
    if !changed_since.is_empty() {
        return Err(State::Failed(format!(
            "these files changed after the patch was applied, so nothing was undone: {}",
            changed_since.join(", ")
        )));
    }
    let mods: Vec<fixfiles::Change> = back
        .iter()
        .filter(|(_, c)| c.kind == Kind::Modified)
        .map(|(full, c)| fixfiles::Change {
            path: full.clone(),
            before: c.after.clone().into_bytes(),
            after: c.before.clone().into_bytes(),
            perms: std::fs::metadata(full)
                .map(|m| m.permissions())
                .unwrap_or_else(|_| {
                    <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o644)
                }),
        })
        .collect();
    match fixfiles::write_all_or_none(&mods) {
        Outcome::Done => {}
        Outcome::RolledBack => {
            return Err(State::Failed(
                "a file could not be written back; nothing was changed".into(),
            ))
        }
        Outcome::Stuck(files) => {
            return Err(State::Attention(
                files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
            ))
        }
    }
    for (full, c) in &back {
        match c.kind {
            Kind::Added => {
                let _ = std::fs::remove_file(full);
            }
            Kind::Deleted => {
                if full
                    .parent()
                    .map(std::fs::create_dir_all)
                    .transpose()
                    .is_err()
                    || std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(full)
                        .and_then(|mut f| std::io::Write::write_all(&mut f, c.before.as_bytes()))
                        .is_err()
                {
                    return Err(State::Attention(vec![c.path.clone()]));
                }
            }
            Kind::Modified => {}
        }
    }
    Ok(())
}
