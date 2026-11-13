//! Questions about a project: kept in the encrypted store with their answer, the places it rests on as the files showed
//! them, and the scripts run. Nothing here leaves this computer.

use kk_review::explore::{Depth, Ending, ExploreReport};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What a runner hands back for a question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOutcome {
    pub report: ExploreReport,
    pub route: String,
    pub elapsed_ms: u64,
    /// What it used of the model, for the usage record; none from a runner that does not count.
    #[serde(default)]
    pub usage: Option<ModelUse>,
}

/// What a project question or a coding task used of the model: which route, how many calls and tokens, how long. For the
/// usage record (and so the cost a model elsewhere charges); never any words.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUse {
    pub route: String,
    pub class: String,
    pub simulated: bool,
    pub calls: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Running,
    Answered,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub folder: PathBuf,
    pub question: String,
    pub depth: Depth,
    pub created: i64,
    pub state: State,
    pub outcome: Option<QuestionOutcome>,
    pub progress: Vec<String>,
}

#[derive(Default)]
pub struct Questions {
    pub list: BTreeMap<String, Question>,
    pub next: u64,
}

/// The most questions kept; the oldest answered ones make room for new ones.
pub const MAX_KEPT: usize = 100;

impl Questions {
    /// The questions kept from before. One that was being answered when the service stopped failed.
    pub fn load(c: &kk_vault::rusqlite::Connection) -> Questions {
        // the last number given out is kept apart from the questions, so forgetting every question never gives an old
        // number to a new one
        let mut q = Questions {
            next: c
                .query_row(
                    "SELECT last FROM id_counters WHERE kind = 'question'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .map(|n| u64::try_from(n).unwrap_or(0))
                .unwrap_or(0),
            ..Questions::default()
        };
        let Ok(mut stmt) = c.prepare("SELECT id, body FROM questions") else {
            return q;
        };
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|r| r.flatten().collect())
            .unwrap_or_default();
        for (id, body) in rows {
            let Ok(mut item) = serde_json::from_str::<Question>(&body) else {
                continue;
            };
            if let Some(n) = id
                .strip_prefix("question-")
                .and_then(|n| n.parse::<u64>().ok())
            {
                q.next = q.next.max(n);
            }
            if item.state == State::Running {
                item.state = State::Failed(
                    "the service stopped while the question was being answered".into(),
                );
                save(c, &item);
            }
            q.list.insert(id, item);
        }
        q
    }
}

pub fn save(c: &kk_vault::rusqlite::Connection, q: &Question) -> bool {
    let body = serde_json::to_string(q).unwrap_or_default();
    c.execute(
        "INSERT INTO questions(id, body) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET body = excluded.body",
        (&q.id, &body),
    )
    .is_ok()
}

/// Records that numbers up to `n` have been given out.
pub fn note_number(c: &kk_vault::rusqlite::Connection, n: u64) -> bool {
    c.execute(
        "INSERT INTO id_counters(kind, last) VALUES ('question', ?1) ON CONFLICT(kind) DO UPDATE SET last = max(last, excluded.last)",
        [i64::try_from(n).unwrap_or(i64::MAX)],
    )
    .is_ok()
}

pub fn remove(c: &kk_vault::rusqlite::Connection, id: &str) -> bool {
    c.execute("DELETE FROM questions WHERE id = ?1", [id])
        .is_ok_and(|n| n == 1)
}

fn ending(e: &Ending) -> (&'static str, String) {
    match e {
        Ending::Answered => ("answered", String::new()),
        Ending::AnsweredWithoutTool => ("unchecked", "The model answered without naming the places its answer rests on, so nothing in it was checked.".into()),
        Ending::TurnLimit => ("turn_limit", "The model used all its steps without answering. Ask again, more narrowly, or with more depth.".into()),
        Ending::Stuck => ("stuck", "The model repeated the same step and was stopped.".into()),
        Ending::ModelFailed(m) => ("model_failed", format!("The model stopped answering: {m}")),
        Ending::Stopped => ("stopped", "Stopped by you.".into()),
    }
}

/// The question as the owner's screens show it.
pub fn view(q: &Question) -> Value {
    let (state, failure) = match &q.state {
        State::Running => ("running", String::new()),
        State::Answered => ("answered", String::new()),
        State::Failed(m) => ("failed", m.clone()),
    };
    let mut v = json!({
        "id": q.id,
        "folder": q.folder.to_string_lossy(),
        "project": q.folder.file_name().map(|n| n.to_string_lossy().into_owned()),
        "question": q.question,
        "depth": q.depth.as_str(),
        "created": q.created,
        "state": state,
        "failure": failure,
        "progress": q.progress,
    });
    if let Some(o) = &q.outcome {
        let r = &o.report;
        let (end, note) = ending(&r.ending);
        let checked = r.citations.iter().filter(|c| c.found.checked()).count();
        v["answer"] = json!({
            "ending": end,
            "note": note,
            "text": r.answer,
            "not_found": r.not_found,
            "places": r.citations,
            "checked": checked,
            "unchecked": r.citations.len() - checked,
            "checks": r.checks,
            "files_read": r.files_read,
            "steps": r.steps,
            "turns": r.turns,
            "withheld": r.withheld,
            "withheld_files": r.withheld_files,
            "corrected": r.corrected,
            "route": o.route,
            "elapsed_ms": o.elapsed_ms,
        });
    }
    v
}
