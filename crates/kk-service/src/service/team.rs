//! Team runs: a lead's plan of tasks for experts, started by the owner, run one task at a time, kept in the vault so a
//! restart loses no finished work. The rules of a plan and of what may run next live in `crate::team`; this file stores
//! runs, asks the model, and answers the owner's requests.

use super::*;
use crate::experts::{self, Seat, METHOD_PREFIX};
use crate::team::{self, Answer, Next, Slot, State};

const MAX_RUNS: i64 = 200;
/// A result is kept whole up to this size; a longer one is cut at the end and says so.
const MAX_RESULT_CHARS: usize = 60_000;
const MAX_ROSTER: usize = 24;

/// Runs that have a driver thread, and runs the owner asked to stop.
#[derive(Default)]
pub(super) struct Live {
    active: std::collections::HashSet<String>,
    stop: std::collections::HashSet<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlanBody {
    goal: String,
    /// Who the lead may hand work to. Left out: every built-in expert and every approved method.
    #[serde(default)]
    experts: Vec<String>,
    #[serde(default)]
    project: Option<String>,
    /// Files the owner attached for this run (ids from `POST /api/attachments`). Every expert and the lead get them as
    /// labelled data, through the same checks a chat message's files pass.
    #[serde(default)]
    attachments: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AnswerBody {
    task: usize,
    answer: String,
}

struct TaskRow {
    n: usize,
    expert: String,
    expert_name: String,
    after: Vec<usize>,
    task: String,
    state: State,
    result: String,
    note: String,
    question: String,
    asked: Vec<(String, String)>,
    attempts: u32,
}

struct RunRow {
    id: String,
    goal: String,
    project: Option<String>,
    attachments: Vec<String>,
    /// The projects of the files attached to the run, kept when it was planned: the files go after two hours, what the
    /// experts made of them stays.
    sources: Vec<String>,
    /// The most private route level this run has worked at; what it holds is not handed to a lesser one.
    held: i64,
    status: String,
    final_text: Option<String>,
    final_note: String,
    created: i64,
    updated: i64,
}

fn refuse(status: StatusCode, code: &str, why: &str) -> Response {
    error(status, code, why)
}

fn run_id_ok(id: &str) -> bool {
    id.len() == 17
        && id.starts_with("team-")
        && id[5..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn not_found() -> Response {
    refuse(
        StatusCode::NOT_FOUND,
        "NO_SUCH_RUN",
        "there is no team run with that id",
    )
}

fn slots(tasks: &[TaskRow]) -> Vec<Slot> {
    tasks
        .iter()
        .map(|t| Slot {
            n: t.n,
            after: t.after.clone(),
            state: t.state,
        })
        .collect()
}

/// Takes a run off the list of working runs when its driver ends, however it ends. A driver that ended without saying how
/// leaves the run interrupted, with the task it was asking put back to waiting. Nothing here may panic.
struct DriverGuard {
    svc: Arc<Service>,
    id: String,
    ended: Option<&'static str>,
}

impl Drop for DriverGuard {
    fn drop(&mut self) {
        if let Ok(store) = self.svc.store.lock() {
            let c = store.connection();
            if self.ended.is_none() {
                let _ = c.execute(
                    "UPDATE team_tasks SET state = 'waiting', attempts = MAX(attempts - 1, 0) WHERE run = ?1 AND state = 'running'",
                    [&self.id],
                );
            }
            // the status is written before the driver is taken off the list, so a reader never sees "running" with no driver
            let _ = c.execute(
                "UPDATE team_runs SET status = ?2, updated = ?3 WHERE id = ?1",
                kk_vault::rusqlite::params![self.id, self.ended.unwrap_or("interrupted"), now()],
            );
        }
        if let Ok(mut live) = self.svc.team.lock() {
            live.active.remove(&self.id);
            live.stop.remove(&self.id);
        }
    }
}

/// "1 defect", "2 defects".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The seat of the coding agent installed on this computer. It is never in a plan unless the owner named it.
/// Said when a run that worked with a more private model would go on with a lesser one.
const MORE_PRIVATE: &str = "this run worked with a more private model than the one in use now, and what it holds is not sent to this one. Switch the model back in Settings to carry on, or plan a new run";
pub(super) const OUTSIDE_SEAT: &str = "claude-code";
/// The seat whose task is a sandboxed review of the run's project folder. It reads the project's files through the same
/// worker, sandbox and broker every review uses; no expert reads files any other way.
pub(super) const REVIEW_SEAT: &str = "project-review";
pub(super) const REVIEW_NAME: &str = "Project reviewer";
const REVIEW_NOT_HANDED: &str = "(this answer holds words about the project's files (a review's findings or a reading of the code), or was written after reading them; it is not handed to an agent outside this program. Open the run to read it)";
/// The seat whose task is answered by reading the run's project folder on this computer: the same question agent as
/// "Ask a project", every line its answer rests on checked against the files.
pub(super) const READER_SEAT: &str = "project-reader";
pub(super) const READER_NAME: &str = "Code reader";
pub(super) const READER_SUMMARY: &str = "Reads the files of the run's project folder on this computer and answers its task from them. Every line its answer rests on is checked against the file, and it may run a short script in the sealed sandbox. Its answer is handed to the other experts only while the model is on this computer.";
pub(super) const REVIEW_SUMMARY: &str = "Reviews the files of the run's project folder in the sealed sandbox and reports only the defects it could reproduce. It is the same review as New review, whatever its task says, so give it the task of reviewing the project. It takes minutes, and it needs a project that has a folder.";
const OUTSIDE_SUMMARY: &str = "The coding agent installed on this computer. Not private: each task for it is shown to the owner first and sent only on their yes. Give it only a task that needs none of the owner's files.";
/// How a question to the owner about a hand-over begins, so it is told apart from an expert's own question.
const HANDOVER_MARK: &str = "[hand-over]";
/// What the owner's yes is replaced with, in the store, the moment the text is about to be sent.
const HANDOVER_SENT: &str = "yes (sent)";

impl Service {
    /// The seat for an id: a built-in expert, or one of the owner's methods approved for exactly its current text.
    /// The folder of a project, when the project has one this program may still read.
    fn team_project_folder(&self, project: Option<&str>) -> Option<std::path::PathBuf> {
        let name = project?;
        let folder: Option<String> = {
            let store = self.store.lock().expect("lock");
            store
                .connection()
                .query_row("SELECT folder FROM projects WHERE name = ?1", [name], |r| {
                    r.get(0)
                })
                .ok()
                .flatten()
        };
        self.allowed_dir(&folder?)
    }

    /// A task whose expert is the project reviewer: an ordinary review of the run's project folder is started (it is
    /// listed with the other reviews, runs in the sandbox, and can be stopped), and what it could reproduce becomes the
    /// task's result. True when the owner stopped the run while it worked.
    ///
    /// The findings are words about the owner's files, so they are handed on only while the model the other experts use
    /// is on this computer; a review's findings otherwise leave only through a release the owner approves.
    fn team_review_task(self: &Arc<Self>, id: &str, n: usize, run: &RunRow) -> bool {
        let fail = |why: &str| self.team_set_task(id, n, State::Failed, "", why, "");
        if self.runner.route_info()["class"] != "device_only" {
            fail("the model in use is not on this computer, and a review's findings are handed to other experts only when it is; choose the model on this computer in Settings, or start a review yourself");
            return false;
        }
        let Some(folder) = self.team_project_folder(run.project.as_deref()) else {
            fail("the run's project has no folder this program may read; give the project a folder under Projects and plan again");
            return false;
        };
        // from here the run holds words about the owner's files: it is marked as held on this computer, so neither a later
        // task nor the closing answer goes to a model elsewhere, however the model is changed in between
        if !self.team_hold_here(id) {
            fail(
                "the run could not be marked as held on this computer, so the project was not read",
            );
            return false;
        }
        let job = match self.start_job(&folder.to_string_lossy(), &format!("team run {id}")) {
            Ok(j) => j,
            Err(why) => {
                fail(&format!("the review could not start: {why}"));
                return false;
            }
        };
        let mut asked_to_stop = false;
        let state = loop {
            let state = {
                let g = self.inner.lock().expect("lock");
                g.jobs.get(&job).map(|j| j.state.clone())
            };
            match state {
                Some(JobState::Running) => {
                    if !asked_to_stop && self.team_stop_asked(id) {
                        asked_to_stop = true;
                        self.stop_ask(&job);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                other => break other,
            }
        };
        if asked_to_stop {
            return true;
        }
        match state {
            Some(JobState::Failed(why)) => fail(&format!(
                "the review ({job}) failed: {}",
                why.chars()
                    .filter(|c| !c.is_control())
                    .take(200)
                    .collect::<String>()
            )),
            None => fail("the review was removed before it finished"),
            Some(_) => {
                let snap = self.inner.lock().expect("lock").snaps.get(&job).cloned();
                let findings: Vec<&crate::snapshot::SnapFinding> = snap
                    .as_ref()
                    .map(|s| s.findings.iter().filter(|f| f.reproduced).collect())
                    .unwrap_or_default();
                let not_confirmed = snap
                    .as_ref()
                    .map_or(0, |s| s.findings.len() - findings.len() + s.unparsed);
                let mut text = format!(
                    "Review {job} of the project folder: {} the sandbox reproduced{}.\n",
                    count(findings.len(), "defect", "defects"),
                    if not_confirmed > 0 {
                        format!(
                            ", {} it could not reproduce (left out)",
                            count(not_confirmed, "suspicion", "suspicions")
                        )
                    } else {
                        String::new()
                    }
                );
                for (i, f) in findings.iter().enumerate() {
                    // a finding's words come from a model that read the project: a line that holds a key is not handed on
                    let why = if kk_secrets::holds_secret(&f.explanation)
                        || kk_secrets::spoken::holds(&f.explanation)
                    {
                        "(its explanation holds what looks like a key and is left out)"
                    } else {
                        f.explanation.trim()
                    };
                    let title = if kk_secrets::holds_secret(&f.title) {
                        "(title left out)"
                    } else {
                        f.title.trim()
                    };
                    // one line per finding, so the list keeps its numbers wherever it is shown
                    let why_line: String = why.split_whitespace().collect::<Vec<_>>().join(" ");
                    text.push_str(&format!(
                        "{}. {} ({}:{}): {}\n",
                        i + 1,
                        title,
                        f.file,
                        f.line,
                        why_line
                    ));
                }
                if findings.is_empty() {
                    text.push_str("Nothing was confirmed. That is not proof that the project has no defect.\n");
                }
                let kept: String = text.chars().take(MAX_RESULT_CHARS).collect();
                self.team_set_task(id, n, State::Done, &kept, &format!("from the sandboxed review {job}; open it under Reviews for the files and the proof"), "");
            }
        }
        false
    }

    /// A task whose expert is the code reader: the question agent reads the run's project folder on this computer and
    /// answers the task, each line it rests on checked against the files. Like a review's findings, the answer is words
    /// about the owner's files: it is made only with the model on this computer, and the run is held on this computer from
    /// then on. True when the owner stopped the run while it worked.
    /// Marks a run as holding material of this computer; false when the mark could not be written.
    fn team_hold_here(&self, id: &str) -> bool {
        let store = self.store.lock().expect("lock");
        store
            .connection()
            .execute("UPDATE team_runs SET held = 3 WHERE id = ?1", [id])
            .is_ok_and(|n| n == 1)
    }

    fn team_reader_task(
        self: &Arc<Self>,
        id: &str,
        n: usize,
        run: &RunRow,
        question: &str,
    ) -> bool {
        let fail = |why: &str| self.team_set_task(id, n, State::Failed, "", why, "");
        if self.route_for_owner()["stays_here"] != true {
            fail("the model in use is not on this computer, and the code reader reads the project's files only with a model on this computer; choose it in Settings");
            return false;
        }
        let Some(folder) = self.team_project_folder(run.project.as_deref()) else {
            fail("the run's project has no folder this program may read; give the project a folder under Projects and plan again");
            return false;
        };
        if !self.team_hold_here(id) {
            fail(
                "the run could not be marked as held on this computer, so the project was not read",
            );
            return false;
        }
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            // the owner's stop of the run reaches the reader at its next step
            let (svc, run_id, stop, done) =
                (self.clone(), id.to_string(), stop.clone(), done.clone());
            std::thread::spawn(move || {
                while !done.load(std::sync::atomic::Ordering::SeqCst) {
                    if svc.team_stop_asked(&run_id) {
                        stop.store(true, std::sync::atomic::Ordering::SeqCst);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            });
        }
        let imported = kk_review::import::scan_tree(&folder, kk_types::label::Level::Internal);
        let result = self.runner.project_question(
            &imported.files,
            question,
            kk_review::explore::Depth::Normal,
            &|_| {},
            &stop,
        );
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            return true;
        }
        let o = match result {
            Ok(o) => o,
            Err(why) => {
                fail(&format!(
                    "the code reader could not answer: {}",
                    why.chars()
                        .filter(|c| !c.is_control())
                        .take(200)
                        .collect::<String>()
                ));
                return false;
            }
        };
        let r = &o.report;
        if r.answer.trim().is_empty() {
            fail("the code reader gave no answer");
            return false;
        }
        let found = r.citations.iter().filter(|c| c.found.checked()).count();
        let mut text = format!("{}\n", r.answer.trim());
        if r.citations.is_empty() {
            text.push_str(
                "(It names no line of the project it rests on, so nothing in it was checked.)\n",
            );
        } else {
            text.push_str(&format!(
                "Lines it rests on, checked against the project's files ({found} of {} found):\n",
                r.citations.len()
            ));
            for c in &r.citations {
                match &c.found {
                    kk_review::explore::Found::AtLine | kk_review::explore::Found::Moved { .. } => {
                        let at = match c.found {
                            kk_review::explore::Found::Moved { line } => line,
                            _ => c.line,
                        };
                        let shown: String = c
                            .text
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .chars()
                            .take(200)
                            .collect();
                        text.push_str(&format!("- {}:{at}: {shown}\n", c.file));
                    }
                    _ => text.push_str(&format!(
                        "- {}:{}: NOT in the file (the reader's words; do not rely on it)\n",
                        c.file, c.line
                    )),
                }
            }
        }
        for k in &r.checks {
            let out: String = k.run.output.lines().take(6).collect::<Vec<_>>().join(" | ");
            text.push_str(&format!(
                "A script it ran in the sandbox printed: {}\n",
                out.chars().take(300).collect::<String>()
            ));
        }
        // a line of the owner's files that holds a key is not handed on, whatever the detector at import missed
        if kk_secrets::holds_secret(&text) || kk_secrets::spoken::holds(&text) {
            fail("the code reader's answer holds what looks like a key or a password, so it is not handed on; ask the question under Ask a project to read it");
            return false;
        }
        let kept: String = text.chars().take(MAX_RESULT_CHARS).collect();
        self.team_set_task(
            id,
            n,
            State::Done,
            &kept,
            &format!(
                "from reading the project folder; {found} of {} lines found",
                r.citations.len()
            ),
            "",
        );
        false
    }

    /// A task whose expert is the coding agent installed on this computer. Nothing is sent on the lead's word: the task
    /// stops and asks the owner, showing the exact text and its fingerprint, and only the owner's "yes" to that very text
    /// sends it, once. Any other answer skips the hand-over. The text is the task and the earlier results it builds on,
    /// without the owner's notes or instructions, and it is refused outright when it holds a key or a password.
    fn team_outside_task(
        &self,
        id: &str,
        n: usize,
        goal: &str,
        total: usize,
        t: &TaskRow,
        earlier: &[(&str, &str)],
    ) {
        // what the owner already answered about a hand-over is not part of what is handed over
        let said: Vec<(String, String)> = t
            .asked
            .iter()
            .filter(|(q, _)| !q.starts_with(HANDOVER_MARK))
            .cloned()
            .collect();
        let text = team::task_question(goal, n, total, &t.task, earlier, &said);
        let digest = kk_evidence::capsule::sha256_hex(text.as_bytes());
        let stamp = format!("fingerprint {digest}");
        let decided = t
            .asked
            .iter()
            .rev()
            .find(|(q, _)| q.starts_with(HANDOVER_MARK));
        match decided {
            Some((q, answer))
                if q.contains(&stamp) && answer.trim().eq_ignore_ascii_case("yes") =>
            {
                // the yes is used up in the store before anything is sent: if the service stops while the agent works,
                // the text is not sent a second time when the run is carried on
                let mut spent = t.asked.clone();
                if let Some(last) = spent
                    .iter_mut()
                    .rev()
                    .find(|(q, _)| q.starts_with(HANDOVER_MARK))
                {
                    last.1 = HANDOVER_SENT.to_string();
                }
                let saved = {
                    let store = self.store.lock().expect("lock");
                    store
                        .connection()
                        .execute(
                            "UPDATE team_tasks SET asked = ?3 WHERE run = ?1 AND n = ?2",
                            kk_vault::rusqlite::params![id, n as i64, json!(spent).to_string()],
                        )
                        .is_ok_and(|rows| rows == 1)
                };
                if !saved {
                    self.team_set_task(
                        id,
                        n,
                        State::Failed,
                        "",
                        "the hand-over could not be recorded, so nothing was sent",
                        "",
                    );
                    return;
                }
                // nothing leaves unless the log says it did; the yes stays used, so nothing is sent later either
                if !self.log_event(
                    "outbound.model",
                    "Claude Code on this computer, then its maker",
                    &[
                        ("chars", text.chars().count().into()),
                        ("swapped", 0usize.into()),
                    ],
                ) {
                    self.team_set_task(
                        id,
                        n,
                        State::Failed,
                        "",
                        "the signed log could not be written (is the disk full?), so nothing was sent to the coding agent",
                        "",
                    );
                    return;
                }
                match self
                    .runner
                    .handoff_run_until(&text, &|| self.team_stop_asked(id))
                {
                    Ok(reply) => {
                        let kept: String = reply.chars().take(MAX_RESULT_CHARS).collect();
                        self.team_set_task(id, n, State::Done, &kept, "answered by the coding agent on this computer after your yes; not private", "");
                    }
                    Err(why) => {
                        let why: String =
                            why.chars().filter(|c| !c.is_control()).take(200).collect();
                        self.team_set_task(
                            id,
                            n,
                            State::Failed,
                            "",
                            &format!("the coding agent gave no answer: {why}"),
                            "",
                        );
                    }
                }
            }
            // a hand-over that was started and never finished (the service stopped while the agent worked)
            Some((q, answer)) if q.contains(&stamp) && answer == HANDOVER_SENT => {
                self.team_set_task(id, n, State::Failed, "", "this text was handed to the coding agent and the service stopped before its answer was saved; it is not sent a second time. Plan the run again to send it again", "");
            }
            // the owner answered a hand-over question for this very text with something other than yes
            Some((q, _)) if q.contains(&stamp) => {
                self.team_set_task(
                    id,
                    n,
                    State::Failed,
                    "",
                    "you did not approve handing this task to the coding agent",
                    "",
                );
            }
            // not asked yet, or the text has changed since it was asked: ask (again), within the usual limit
            _ if t.asked.len() >= team::MAX_ASKS => {
                self.team_set_task(
                    id,
                    n,
                    State::Failed,
                    "",
                    "it asked the owner more than twice",
                    "",
                );
            }
            _ if kk_secrets::holds_secret(&text) || kk_secrets::spoken::holds(&text) => {
                self.team_set_task(id, n, State::Failed, "", "the task holds what looks like a key or a password, so it was not offered for hand-over", "");
            }
            _ => {
                let question = format!(
                    "{HANDOVER_MARK} The lead gave this task to the coding agent installed on this computer. That is not private: the text below would go to its maker under your own account. Answer yes to send exactly this text, once. Any other answer skips it.\n\n{text}\n\n{stamp}"
                );
                self.team_set_task(id, n, State::NeedsInput, "", "", &question);
            }
        }
    }

    fn team_seat(&self, id: &str) -> Option<(Seat, String)> {
        // the coding agent installed on this computer can be given a task, when there is one and the owner named it
        if id == REVIEW_SEAT {
            return Some((
                Seat {
                    id: id.to_string(),
                    name: REVIEW_NAME.to_string(),
                    instruction: String::new(),
                },
                REVIEW_SUMMARY.to_string(),
            ));
        }
        if id == READER_SEAT {
            return Some((
                Seat {
                    id: id.to_string(),
                    name: READER_NAME.to_string(),
                    instruction: String::new(),
                },
                READER_SUMMARY.to_string(),
            ));
        }
        if id == OUTSIDE_SEAT {
            return self.runner.handoff_tool().map(|tool| {
                (
                    Seat {
                        id: id.to_string(),
                        name: tool,
                        instruction: String::new(),
                    },
                    OUTSIDE_SUMMARY.to_string(),
                )
            });
        }
        match id.strip_prefix(METHOD_PREFIX) {
            Some(name) => self
                .methods_active()
                .into_iter()
                .find(|a| a.method.name == name)
                .map(|a| {
                    (
                        Seat {
                            id: id.to_string(),
                            name: a.method.name.clone(),
                            instruction: kk_methods::render(std::slice::from_ref(&a)),
                        },
                        a.method.description.clone(),
                    )
                }),
            None => experts::find(id).map(|e| (Seat::from(e), e.summary.to_string())),
        }
    }

    fn team_run(&self, id: &str) -> Option<RunRow> {
        let store = self.store.lock().expect("lock");
        store
            .connection()
            .query_row(
                "SELECT id, goal, project, status, final, final_note, created, updated, attachments, coalesce(held, (SELECT CASE WHEN count(*) > 0 THEN 3 ELSE 0 END FROM team_tasks t WHERE t.run = team_runs.id AND t.result <> '')), sources FROM team_runs WHERE id = ?1",
                [id],
                |r| {
                    Ok(RunRow {
                        id: r.get(0)?,
                        goal: r.get(1)?,
                        project: r.get(2)?,
                        status: r.get(3)?,
                        final_text: r.get(4)?,
                        final_note: r.get(5)?,
                        created: r.get(6)?,
                        updated: r.get(7)?,
                        attachments: r
                            .get::<_, Option<String>>(8)?
                            .and_then(|t| serde_json::from_str(&t).ok())
                            .unwrap_or_default(),
                        held: r.get(9)?,
                        sources: r
                            .get::<_, Option<String>>(10)?
                            .and_then(|t| serde_json::from_str(&t).ok())
                            .unwrap_or_default(),
                    })
                },
            )
            .ok()
    }

    fn team_tasks(&self, id: &str) -> Vec<TaskRow> {
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        let Ok(mut st) = c.prepare(
            "SELECT n, expert, expert_name, after, task, state, result, note, question, asked, attempts FROM team_tasks WHERE run = ?1 ORDER BY n",
        ) else {
            return vec![];
        };
        st.query_map([id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, i64>(10)?,
            ))
        })
        .map(|rows| {
            rows.flatten()
                .filter_map(
                    |(
                        n,
                        expert,
                        expert_name,
                        after,
                        task,
                        state,
                        result,
                        note,
                        question,
                        asked,
                        attempts,
                    )| {
                        Some(TaskRow {
                            n: n as usize,
                            expert,
                            expert_name,
                            after: serde_json::from_str(&after).ok()?,
                            task,
                            // a state this version does not know is not guessed at: the task cannot run
                            state: State::from_word(&state).unwrap_or(State::Failed),
                            result,
                            note,
                            question,
                            asked: serde_json::from_str(&asked).unwrap_or_default(),
                            attempts: attempts as u32,
                        })
                    },
                )
                .collect()
        })
        .unwrap_or_default()
    }

    fn team_set_status(&self, id: &str, status: &str) {
        let store = self.store.lock().expect("lock");
        let _ = store.connection().execute(
            "UPDATE team_runs SET status = ?2, updated = ?3 WHERE id = ?1",
            kk_vault::rusqlite::params![id, status, now()],
        );
    }

    fn team_set_final(&self, id: &str, text: Option<&str>, note: &str) {
        let store = self.store.lock().expect("lock");
        let _ = store.connection().execute(
            "UPDATE team_runs SET final = ?2, final_note = ?3, updated = ?4 WHERE id = ?1",
            kk_vault::rusqlite::params![id, text, note, now()],
        );
    }

    fn team_set_task(
        &self,
        id: &str,
        n: usize,
        state: State,
        result: &str,
        note: &str,
        question: &str,
    ) {
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        let _ = c.execute(
            "UPDATE team_tasks SET state = ?3, result = ?4, note = ?5, question = ?6 WHERE run = ?1 AND n = ?2",
            kk_vault::rusqlite::params![id, n as i64, state.word(), result, note, question],
        );
        let _ = c.execute(
            "UPDATE team_runs SET updated = ?2 WHERE id = ?1",
            kk_vault::rusqlite::params![id, now()],
        );
    }

    fn team_begin_task(&self, id: &str, n: usize) {
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        let _ = c.execute(
            "UPDATE team_tasks SET state = 'running', attempts = attempts + 1 WHERE run = ?1 AND n = ?2",
            kk_vault::rusqlite::params![id, n as i64],
        );
        // a task beginning is a sign of life
        let _ = c.execute(
            "UPDATE team_runs SET updated = ?2 WHERE id = ?1",
            kk_vault::rusqlite::params![id, now()],
        );
    }

    /// A stopped call is not an attempt: the task goes back to waiting as it was.
    fn team_unbegin_task(&self, id: &str, n: usize) {
        let store = self.store.lock().expect("lock");
        let _ = store.connection().execute(
            "UPDATE team_tasks SET state = 'waiting', attempts = MAX(attempts - 1, 0) WHERE run = ?1 AND n = ?2 AND state = 'running'",
            kk_vault::rusqlite::params![id, n as i64],
        );
    }

    /// Called once when the service starts: a run that was running has no driver any more. Its finished tasks are kept,
    /// the task that was being asked goes back to waiting, and the owner can start it again.
    pub(super) fn team_recover(&self) {
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        let _ = c.execute(
            "UPDATE team_tasks SET state = 'waiting', attempts = MAX(attempts - 1, 0) WHERE state = 'running'",
            [],
        );
        let _ = c.execute(
            "UPDATE team_runs SET status = 'interrupted', updated = ?1 WHERE status = 'running'",
            [now()],
        );
    }

    /// What team runs need from the owner: a plan not yet started, an expert's question, a run the service's stop cut off.
    pub(super) fn team_inbox(&self) -> Vec<Value> {
        let runs: Vec<(String, String, String, i64)> = {
            let store = self.store.lock().expect("lock");
            let c = store.connection();
            c.prepare("SELECT id, goal, status, updated FROM team_runs WHERE status IN ('planned', 'waiting', 'interrupted') ORDER BY updated DESC")
                .and_then(|mut st| {
                    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                        .map(|rows| rows.flatten().collect())
                })
                .unwrap_or_default()
        };
        let mut items = Vec::new();
        for (id, goal, status, updated) in runs {
            let title: String = goal.chars().take(80).collect();
            let item = |kind: &str, what: String| json!({"kind": kind, "team": id, "title": title, "since": updated, "what": what});
            match status.as_str() {
                "planned" => items.push(item("team_planned", "A plan is waiting for you. Read it, then start it or delete it.".into())),
                "interrupted" => items.push(item(
                    "team_interrupted",
                    "The service stopped while this was running. Finished tasks are kept; carry on to ask the rest.".into(),
                )),
                _ => {
                    for t in self.team_tasks(&id).iter().filter(|t| t.state == State::NeedsInput) {
                        let mut v = item("team_question", format!("{} asks: {}", t.expert_name, t.question));
                        v["task"] = json!(t.n);
                        items.push(v);
                    }
                }
            }
        }
        items
    }

    /// The runs that are working now: id, goal, when planned, when a task last began or ended.
    pub(super) fn team_working(&self) -> Vec<(String, String, i64, i64)> {
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        c.prepare("SELECT id, goal, created, updated FROM team_runs WHERE status = 'running' ORDER BY updated DESC")
            .and_then(|mut st| {
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                    .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
    }

    fn team_stop_asked(&self, id: &str) -> bool {
        self.team.lock().expect("lock").stop.contains(id)
    }

    /// The names of a run's attached files that are still held; one that is gone is listed as null.
    fn team_file_names(&self, ids: &[String]) -> Vec<Option<String>> {
        let mut store = self.attachments.lock().expect("lock");
        ids.iter()
            .map(|id| store.get(now(), id).map(|s| s.parsed.name.clone()))
            .collect()
    }

    fn team_json(&self, run: &RunRow) -> Value {
        let tasks = self.team_tasks(&run.id);
        let active = self.team.lock().expect("lock").active.contains(&run.id);
        json!({
            "id": run.id,
            "goal": run.goal,
            "project": run.project,
            "files": self.team_file_names(&run.attachments),
            "status": run.status,
            "working": active,
            "final": run.final_text,
            "final_note": run.final_note,
            "created": run.created,
            "updated": run.updated,
            "tasks": tasks.iter().map(|t| json!({
                "n": t.n,
                "expert": t.expert,
                "expert_name": t.expert_name,
                "after": t.after,
                "task": t.task,
                "state": t.state.word(),
                "result": t.result,
                "note": t.note,
                "question": t.question,
                "asked": t.asked.iter().map(|(q, a)| json!({"question": q, "answer": a})).collect::<Vec<_>>(),
                "attempts": t.attempts,
            })).collect::<Vec<_>>(),
        })
    }

    /// Starts the one driver of a run, unless it already has one.
    fn team_spawn(self: &Arc<Self>, id: &str) -> bool {
        {
            let mut live = self.team.lock().expect("lock");
            if !live.active.insert(id.to_string()) {
                return false;
            }
            live.stop.remove(id);
        }
        self.team_set_status(id, "running");
        let svc = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            // if the driver dies (a fault in a model client, say), the run must not stay "running" with nobody driving it
            let mut guard = DriverGuard {
                svc: svc.clone(),
                id: id.clone(),
                ended: None,
            };
            let ended = svc.team_drive(&id);
            guard.ended = Some(ended);
            drop(guard);
            // an answer that arrived while this driver was deciding to wait found it still listed and started nothing:
            // look once more now that it is off the list
            if ended == "waiting"
                && matches!(team::next(&slots(&svc.team_tasks(&id))), Next::Run(_))
            {
                svc.team_spawn(&id);
            }
        });
        true
    }

    /// Runs tasks until nothing more can run. Returns the status the run ends this drive with.
    fn team_drive(self: &Arc<Self>, id: &str) -> &'static str {
        let Some(run) = self.team_run(id) else {
            return "stopped";
        };
        // every task is asked with what an ordinary chat about the goal would have been given: the same notes, rules and route
        // the run's project may allow only a more private model than the one in use now: said in its own words
        if let Some((name, _)) = self.floor_above_route(run.project.as_deref(), None) {
            self.team_set_final(id, None, &super::projects::floor_words(&name));
            return "stopped";
        }
        // and so may the projects its attached files belong to
        if let Some(name) = self.projects_above_route(&run.sources) {
            self.team_set_final(id, None, &super::projects::floor_words(&name));
            return "stopped";
        }
        let body = ChatBody {
            message: run.goal.clone(),
            history: Vec::new(),
            sources: Vec::new(),
            attachments: run.attachments.clone(),
            session: None,
            project: run.project.clone(),
            held: None,
        };
        let Ok(chat) = chat_validate(self, body) else {
            self.team_set_final(
                id,
                None,
                if run.attachments.is_empty() {
                    "the goal could not be prepared for the model"
                } else {
                    "an attached file is no longer held (files are kept in memory for two hours): attach it again and plan a new run"
                },
            );
            return "stopped";
        };
        // a run that worked with a more private model holds notes, files and results that are not for a lesser one
        let mut need = self.route_level();
        // the run's project may also allow only a more private model than the one in use now
        if let Some((name, _)) = self.floor_above_route(run.project.as_deref(), None) {
            self.team_set_final(id, None, &super::projects::floor_words(&name));
            return "stopped";
        }
        need = need.max(self.project_floor(run.project.as_deref()));
        if let Some(name) = self.projects_above_route(&run.sources) {
            self.team_set_final(id, None, &super::projects::floor_words(&name));
            return "stopped";
        }
        need = need.max(self.projects_floor(&run.sources));
        if run.held > need {
            self.team_set_final(id, None, MORE_PRIVATE);
            return "stopped";
        }
        {
            let store = self.store.lock().expect("lock");
            let _ = store.connection().execute(
                "UPDATE team_runs SET held = max(coalesce(held, 0), ?2) WHERE id = ?1",
                kk_vault::rusqlite::params![id, need],
            );
        }
        let ctx = chat_context(self, &chat.body, None);
        if ctx.nothing_to_read() {
            self.team_set_final(id, None, &no_model_reply().text);
            return "stopped";
        }
        self.remember_sent(&ctx.messages, &ctx.excluded_notes);
        let window = self.runner.route_info()["context_tokens"].as_u64();
        loop {
            if self.team_stop_asked(id) {
                return "stopped";
            }
            // the project's floor is read again before every task: raised while the run worked, it holds from the next one
            if let Some((name, _)) = self.floor_above_route(run.project.as_deref(), None) {
                self.team_set_final(id, None, &super::projects::floor_words(&name));
                return "stopped";
            }
            if let Some(name) = self.projects_above_route(&run.sources) {
                self.team_set_final(id, None, &super::projects::floor_words(&name));
                return "stopped";
            }
            need = need
                .max(self.project_floor(run.project.as_deref()))
                .max(self.projects_floor(&run.sources));
            let mut tasks = self.team_tasks(id);
            let mut s = slots(&tasks);
            for n in team::settle_skips(&mut s) {
                let t = tasks
                    .iter_mut()
                    .find(|t| t.n == n)
                    .expect("a slot per task");
                let cause = t
                    .after
                    .iter()
                    .find(|d| {
                        s.iter().any(|x| {
                            x.n == **d && matches!(x.state, State::Failed | State::Skipped)
                        })
                    })
                    .copied()
                    .unwrap_or(0);
                t.state = State::Skipped;
                self.team_set_task(
                    id,
                    n,
                    State::Skipped,
                    "",
                    &format!("it waited on task {cause}, which was not done"),
                    "",
                );
            }
            let n = match team::next(&s) {
                Next::Run(n) => n,
                Next::WaitForOwner => return "waiting",
                Next::Finished => {
                    self.team_finish(id, &run.goal, &tasks, window, need);
                    return "done";
                }
            };
            let total = tasks.len();
            let t = tasks
                .iter()
                .find(|t| t.n == n)
                .expect("the task the scheduler named");
            // the seat is looked up now, not when the plan was written: a method edited or revoked since has no seat
            let Some((seat, _)) = self.team_seat(&t.expert) else {
                self.team_set_task(id, n, State::Failed, "", "its expert is no longer available (a method must be approved for its current text)", "");
                continue;
            };
            self.team_begin_task(id, n);
            let cap = experts::answer_cap(window, t.after.len().max(1));
            let earlier_owned: Vec<(String, String)> = t
                .after
                .iter()
                .filter_map(|d| tasks.iter().find(|x| x.n == *d))
                .map(|x| (x.expert_name.clone(), experts::fit(&x.result, cap).0))
                .collect();
            let earlier: Vec<(&str, &str)> = earlier_owned
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect();
            let question = team::task_question(&run.goal, n, total, &t.task, &earlier, &t.asked);
            // for the coding agent outside: a review's findings are words about the owner's files and leave this computer
            // only through a release, so they are not offered for hand-over at all
            // that holds for an expert that read the findings too: its answer may quote them, so the answer of any task
            // that follows a review, directly or through other tasks, is not offered either
            let mut from_review: std::collections::BTreeSet<usize> = tasks
                .iter()
                .filter(|x| x.expert == REVIEW_SEAT || x.expert == READER_SEAT)
                .map(|x| x.n)
                .collect();
            loop {
                let more: Vec<usize> = tasks
                    .iter()
                    .filter(|x| {
                        !from_review.contains(&x.n)
                            && x.after.iter().any(|d| from_review.contains(d))
                    })
                    .map(|x| x.n)
                    .collect();
                if more.is_empty() {
                    break;
                }
                from_review.extend(more);
            }
            let outside_owned: Vec<(String, String)> = t
                .after
                .iter()
                .filter_map(|d| tasks.iter().find(|x| x.n == *d))
                .map(|x| {
                    if from_review.contains(&x.n) {
                        (x.expert_name.clone(), REVIEW_NOT_HANDED.to_string())
                    } else {
                        (x.expert_name.clone(), experts::fit(&x.result, cap).0)
                    }
                })
                .collect();
            let earlier_outside: Vec<(&str, &str)> = outside_owned
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect();
            if t.expert == REVIEW_SEAT {
                let stopped = self.team_review_task(id, n, &run);
                need = need.max(self.team_run(id).map_or(0, |r| r.held));
                self.log_event(
                    "team.task",
                    id,
                    &[
                        ("task", n.into()),
                        ("attempt", ((t.attempts + 1) as usize).into()),
                    ],
                );
                if stopped {
                    self.team_unbegin_task(id, n);
                    return "stopped";
                }
                continue;
            }
            if t.expert == READER_SEAT {
                let stopped = self.team_reader_task(id, n, &run, &question);
                need = need.max(self.team_run(id).map_or(0, |r| r.held));
                self.log_event(
                    "team.task",
                    id,
                    &[
                        ("task", n.into()),
                        ("attempt", ((t.attempts + 1) as usize).into()),
                    ],
                );
                if stopped {
                    self.team_unbegin_task(id, n);
                    return "stopped";
                }
                continue;
            }
            if t.expert == OUTSIDE_SEAT {
                // the coding agent outside is not private: a project kept on this computer or on confidential hardware is
                // never handed to it, whatever the owner would answer
                if self
                    .project_floor(run.project.as_deref())
                    .max(self.projects_floor(&run.sources))
                    > 1
                {
                    let name = run.project.clone().unwrap_or_default();
                    self.team_set_task(id, n, State::Failed, "", &format!("the project {name} is kept more privately than an agent outside this program allows, so nothing of it is handed over"), "");
                } else {
                    self.team_outside_task(id, n, &run.goal, total, t, &earlier_outside);
                }
                self.log_event(
                    "team.task",
                    id,
                    &[
                        ("task", n.into()),
                        ("attempt", ((t.attempts + 1) as usize).into()),
                    ],
                );
                continue;
            }
            let mut messages = ctx.messages.clone();
            let at = usize::from(messages.first().is_some_and(|m| m.role == "system"));
            messages.insert(
                at,
                kk_types::chat::ChatMessage::new("system", team::TASK_INSTRUCTION),
            );
            messages.insert(
                at,
                kk_types::chat::ChatMessage::new("system", &seat.instruction),
            );
            if let Some(last) = messages.iter_mut().rev().find(|m| m.role == "user") {
                *last = kk_types::chat::ChatMessage::new("user", &question);
            } else {
                messages.push(kk_types::chat::ChatMessage::new("user", &question));
            }
            let started = std::time::Instant::now();
            let estimated = kk_route::tokens::estimate_messages(&messages);
            let result = self.ask_stream_at(messages, &mut |_| !self.team_stop_asked(id), need);
            // the model was switched to a less private one while this run worked: the run stops where it is
            if matches!(&result, Err(e) if e == swap::ROUTE_CHANGED) {
                self.team_unbegin_task(id, n);
                self.team_set_final(id, None, MORE_PRIVATE);
                return "stopped";
            }
            let attempts = t.attempts + 1;
            let mut cut_short = false;
            let text = match result {
                Ok(s) if s.stopped || self.team_stop_asked(id) => {
                    self.team_unbegin_task(id, n);
                    return "stopped";
                }
                Ok(s) => {
                    let measured = s.reply.prompt_tokens.filter(|n| *n > 0);
                    self.record_usage(
                        &s.reply.route_id,
                        &s.reply.class,
                        s.reply.simulated,
                        measured.unwrap_or(estimated),
                        measured.is_some(),
                        kk_route::tokens::estimate_text(&s.reply.text),
                        started.elapsed().as_millis() as i64,
                    );
                    cut_short = s.reply.cut_short;
                    Ok(s.reply.text)
                }
                Err(e) => Err(e),
            };
            let answer = match &text {
                Ok(t) => team::read_answer(t),
                Err(_) => Answer::Empty,
            };
            match answer {
                Answer::Result(r) => {
                    let mut kept: String = r.chars().take(MAX_RESULT_CHARS).collect();
                    let note = if kept.chars().count() < r.chars().count() {
                        kept.push_str("\n[the result was longer and was cut here]");
                        "the result was cut to fit"
                    } else if cut_short {
                        "the answer reached the model's length limit and stops there"
                    } else {
                        ""
                    };
                    self.team_set_task(id, n, State::Done, &kept, note, "");
                }
                Answer::NeedsInput(q) if t.asked.len() < team::MAX_ASKS => {
                    self.team_set_task(id, n, State::NeedsInput, "", "", &q);
                }
                Answer::NeedsInput(_) => {
                    self.team_set_task(
                        id,
                        n,
                        State::Failed,
                        "",
                        "it asked the owner more than twice",
                        "",
                    );
                }
                Answer::Empty if attempts < team::MAX_ATTEMPTS => {
                    self.team_set_task(id, n, State::Waiting, "", "", "");
                }
                Answer::Empty => {
                    let why = match &text {
                        Err(e) => format!(
                            "its expert could not be asked: {}",
                            e.chars()
                                .filter(|c| !c.is_control())
                                .take(200)
                                .collect::<String>()
                        ),
                        Ok(_) => "its expert gave no answer".to_string(),
                    };
                    self.team_set_task(id, n, State::Failed, "", &why, "");
                }
            }
            self.log_event(
                "team.task",
                id,
                &[("task", n.into()), ("attempt", (attempts as usize).into())],
            );
        }
    }

    /// The lead's closing answer. If the lead cannot be asked, the tasks' results stand and the run says why there is no summary.
    fn team_finish(&self, id: &str, goal: &str, tasks: &[TaskRow], window: Option<u64>, need: i64) {
        let done: Vec<&TaskRow> = tasks.iter().filter(|t| t.state == State::Done).collect();
        if done.is_empty() {
            self.team_set_final(id, None, "no task was done, so there is nothing to sum up");
            return;
        }
        let cap = experts::answer_cap(window, done.len());
        let cut: Vec<String> = done
            .iter()
            .map(|t| experts::fit(&t.result, cap).0)
            .collect();
        let results: Vec<(usize, &str, &str, &str)> = done
            .iter()
            .zip(&cut)
            .map(|(t, r)| (t.n, t.expert_name.as_str(), t.task.as_str(), r.as_str()))
            .collect();
        let missing: Vec<(usize, String)> = tasks
            .iter()
            .filter(|t| t.state != State::Done)
            .map(|t| (t.n, t.note.clone()))
            .collect();
        let messages = vec![
            kk_types::chat::ChatMessage::new("system", team::FINAL_INSTRUCTION),
            kk_types::chat::ChatMessage::new(
                "user",
                &team::final_question(goal, &results, &missing),
            ),
        ];
        match self.ask_counted_at(messages, need).map(|(r, _)| r) {
            Err(e) if e == swap::ROUTE_CHANGED => self.team_set_final(id, None, MORE_PRIVATE),
            Ok(r) if !r.text.trim().is_empty() => self.team_set_final(id, Some(r.text.trim()), ""),
            Ok(_) => self.team_set_final(
                id,
                None,
                "the lead wrote nothing; the tasks' results are below",
            ),
            Err(_) => self.team_set_final(
                id,
                None,
                "the lead could not be asked; the tasks' results are below",
            ),
        }
    }
}

/// Asks the lead for a plan and keeps it, not started. Nothing runs until the owner starts it.
pub(super) async fn team_plan(axum::Extension(svc): Svc, Json(b): Json<PlanBody>) -> Response {
    let goal = b.goal.trim().to_string();
    if goal.is_empty() || goal.chars().count() > team::MAX_GOAL_CHARS || !chat_clean(&goal) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "BAD_GOAL",
            "write a goal of 1 to 4000 characters without control characters",
        );
    }
    // the same checks an ordinary chat makes of a project name
    let probe = ChatBody {
        message: goal.clone(),
        history: Vec::new(),
        sources: Vec::new(),
        attachments: b.attachments.clone(),
        session: None,
        project: b.project.clone(),
        held: None,
    };
    let project = match chat_validate(&svc, probe) {
        Ok(run) => run.body.project,
        Err(resp) => return *resp,
    };
    // the projects the attached files belong to, kept with the run
    let sources: Vec<String> = {
        let mut store = svc.attachments.lock().expect("lock");
        let mut out: Vec<String> = Vec::new();
        for a in &b.attachments {
            if let Some(s) = store.get(now(), a) {
                for p in &s.projects {
                    if !out.contains(p) {
                        out.push(p.clone());
                    }
                }
            }
        }
        out
    };
    let mut ids: Vec<String> = if b.experts.is_empty() {
        let mut all: Vec<String> = experts::EXPERTS.iter().map(|e| e.id.to_string()).collect();
        all.extend(
            svc.methods_active()
                .iter()
                .map(|a| format!("{METHOD_PREFIX}{}", a.method.name)),
        );
        all
    } else {
        b.experts.clone()
    };
    ids.truncate(MAX_ROSTER + 1);
    if ids.len() > MAX_ROSTER {
        return refuse(
            StatusCode::BAD_REQUEST,
            "EXPERT_COUNT",
            "a lead can hand work to at most 24 experts",
        );
    }
    if ids.iter().any(|i| i == REVIEW_SEAT || i == READER_SEAT)
        && svc.team_project_folder(project.as_deref()).is_none()
    {
        return refuse(
            StatusCode::BAD_REQUEST,
            "NO_PROJECT_FOLDER",
            "the project reviewer and the code reader need a project that has a folder: choose one for this run, or give the project a folder under Projects",
        );
    }
    let mut roster: Vec<(String, String)> = Vec::new();
    for id in &ids {
        match svc.team_seat(id) {
            Some(_) if roster.iter().any(|(r, _)| r == id) => {
                return refuse(
                    StatusCode::BAD_REQUEST,
                    "EXPERT_TWICE",
                    "each expert is named once",
                )
            }
            Some((_, summary)) => roster.push((id.clone(), summary)),
            None => {
                return refuse(
                    StatusCode::BAD_REQUEST,
                    "NO_SUCH_EXPERT",
                    "there is no expert with that name",
                )
            }
        }
    }
    {
        let store = svc.store.lock().expect("lock");
        let n: i64 = store
            .connection()
            .query_row("SELECT COUNT(*) FROM team_runs", [], |r| r.get(0))
            .unwrap_or(0);
        if n >= MAX_RUNS {
            return refuse(
                StatusCode::CONFLICT,
                "TOO_MANY_RUNS",
                "there are 200 team runs; delete one you no longer need first",
            );
        }
    }
    let svc2 = svc.clone();
    let goal2 = goal.clone();
    let roster2 = roster.clone();
    let planned =
        tokio::task::spawn_blocking(move || -> Result<Vec<team::PlanTask>, (bool, String)> {
            let valid: Vec<String> = roster2.iter().map(|(id, _)| id.clone()).collect();
            let question = team::plan_question(&goal2, &roster2);
            let ask = |q: &str| {
                svc2.ask(vec![
                    kk_types::chat::ChatMessage::new("system", team::PLAN_INSTRUCTION),
                    kk_types::chat::ChatMessage::new("user", q),
                ])
                .map_err(|e| (true, e))
            };
            let first = ask(&question)?;
            match team::parse_plan(&first.text, &valid) {
                Ok(p) => Ok(p),
                Err(why) => {
                    // one more try, with the reason; a second unusable plan is the answer
                    let second = ask(&team::repair_question(&question, &why))?;
                    team::parse_plan(&second.text, &valid).map_err(|why| (false, why))
                }
            }
        })
        .await;
    let plan = match planned {
        Ok(Ok(p)) => p,
        Ok(Err((true, e))) => return error(StatusCode::BAD_GATEWAY, "NO_MODEL", &format!("the lead could not be asked: {e}")),
        Ok(Err((false, why))) => {
            return refuse(
                StatusCode::UNPROCESSABLE_ENTITY,
                "PLAN_REFUSED",
                &format!("the lead did not write a usable plan ({why}). Nothing was started; try again or say the goal more simply"),
            )
        }
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", "the plan could not be made"),
    };
    let mut raw = [0u8; 6];
    getrandom::fill(&mut raw).expect("operating system randomness");
    let id = format!(
        "team-{}",
        raw.iter().map(|x| format!("{x:02x}")).collect::<String>()
    );
    {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        let at = now();
        let _ = c.execute(
            "INSERT INTO team_runs(id, goal, project, status, final, final_note, created, updated, attachments, sources) VALUES(?1, ?2, ?3, 'planned', NULL, '', ?4, ?4, ?5, ?6)",
            kk_vault::rusqlite::params![id, goal, project, at, serde_json::to_string(&b.attachments).unwrap_or_else(|_| "[]".into()), serde_json::to_string(&sources).unwrap_or_else(|_| "[]".into())],
        );
        for t in &plan {
            let name = seat_name(&t.expert);
            let _ = c.execute(
                "INSERT INTO team_tasks(run, n, expert, expert_name, after, task, state) VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'waiting')",
                kk_vault::rusqlite::params![id, t.n as i64, t.expert, name, json!(t.after).to_string(), t.task],
            );
        }
    }
    svc.log_event("team.planned", &id, &[("tasks", plan.len().into())]);
    match svc.team_run(&id) {
        Some(run) => (StatusCode::CREATED, Json(svc.team_json(&run))).into_response(),
        None => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            "the plan could not be saved",
        ),
    }
}

/// The name shown for an expert id.
fn seat_name(id: &str) -> String {
    if id == REVIEW_SEAT {
        return REVIEW_NAME.to_string();
    }
    if id == READER_SEAT {
        return READER_NAME.to_string();
    }
    match id.strip_prefix(METHOD_PREFIX) {
        Some(name) => name.to_string(),
        None => experts::find(id)
            .map(|e| e.name.to_string())
            .unwrap_or_else(|| id.to_string()),
    }
}

pub(super) async fn team_list(axum::Extension(svc): Svc) -> Json<Value> {
    let rows: Vec<(String, String, String, i64, i64)> = {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        c.prepare(
            "SELECT id, goal, status, created, updated FROM team_runs ORDER BY created DESC, id",
        )
        .and_then(|mut st| {
            st.query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default()
    };
    let runs: Vec<Value> = rows
        .into_iter()
        .map(|(id, goal, status, created, updated)| {
            let tasks = svc.team_tasks(&id);
            let count = |s: State| tasks.iter().filter(|t| t.state == s).count();
            json!({
                "id": id,
                "goal": goal.chars().take(160).collect::<String>(),
                "status": status,
                "created": created,
                "updated": updated,
                "tasks": tasks.len(),
                "done": count(State::Done),
                "needs_input": count(State::NeedsInput),
                "not_done": count(State::Failed) + count(State::Skipped),
            })
        })
        .collect();
    Json(json!({"runs": runs}))
}

pub(super) async fn team_get(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    if !run_id_ok(&id) {
        return not_found();
    }
    match svc.team_run(&id) {
        Some(run) => Json(svc.team_json(&run)).into_response(),
        None => not_found(),
    }
}

/// Starts a planned run, or carries on one that was stopped or interrupted. Finished tasks are never asked again.
pub(super) async fn team_start(
    axum::Extension(svc): Svc,
    UrlPath(id): UrlPath<String>,
) -> Response {
    if !run_id_ok(&id) {
        return not_found();
    }
    let Some(run) = svc.team_run(&id) else {
        return not_found();
    };
    match run.status.as_str() {
        "planned" | "stopped" | "interrupted" | "waiting" => {}
        "running" => {
            return refuse(
                StatusCode::CONFLICT,
                "ALREADY_RUNNING",
                "this run is already working",
            )
        }
        _ => {
            return refuse(
                StatusCode::CONFLICT,
                "RUN_ENDED",
                "this run has ended; plan a new one",
            )
        }
    }
    if !svc.team_spawn(&id) {
        return refuse(
            StatusCode::CONFLICT,
            "ALREADY_RUNNING",
            "this run is already working",
        );
    }
    svc.log_event("team.started", &id, &[]);
    match svc.team_run(&id) {
        Some(run) => Json(svc.team_json(&run)).into_response(),
        None => not_found(),
    }
}

/// Stops a working run: the answer being written is cut off and no further task is asked. Finished tasks stay.
pub(super) async fn team_stop(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    if !run_id_ok(&id) {
        return not_found();
    }
    let Some(run) = svc.team_run(&id) else {
        return not_found();
    };
    let working = {
        let mut live = svc.team.lock().expect("lock");
        let working = live.active.contains(&id);
        if working {
            live.stop.insert(id.clone());
        }
        working
    };
    if !working {
        if matches!(run.status.as_str(), "planned" | "waiting" | "interrupted") {
            svc.team_set_status(&id, "stopped");
        } else if run.status != "stopped" {
            return refuse(StatusCode::CONFLICT, "RUN_ENDED", "this run has ended");
        }
    }
    svc.log_event("team.stop_asked", &id, &[]);
    Json(json!({"id": id, "stopping": working})).into_response()
}

/// The owner's answer to a task's question. The task is asked again with the answer, and what waited on it can follow.
pub(super) async fn team_answer(
    axum::Extension(svc): Svc,
    UrlPath(id): UrlPath<String>,
    Json(b): Json<AnswerBody>,
) -> Response {
    if !run_id_ok(&id) {
        return not_found();
    }
    if svc.team_run(&id).is_none() {
        return not_found();
    }
    let answer = b.answer.trim().to_string();
    if answer.is_empty() || answer.chars().count() > team::MAX_ANSWER_CHARS || !chat_clean(&answer)
    {
        return refuse(
            StatusCode::BAD_REQUEST,
            "BAD_ANSWER",
            "write an answer of 1 to 2000 characters without control characters",
        );
    }
    let tasks = svc.team_tasks(&id);
    let Some(t) = tasks.iter().find(|t| t.n == b.task) else {
        return refuse(
            StatusCode::NOT_FOUND,
            "NO_SUCH_TASK",
            "this run has no task with that number",
        );
    };
    if t.state != State::NeedsInput {
        return refuse(
            StatusCode::CONFLICT,
            "NOT_ASKING",
            "that task is not waiting for an answer",
        );
    }
    let mut asked = t.asked.clone();
    asked.push((t.question.clone(), answer));
    let changed = {
        let store = svc.store.lock().expect("lock");
        store
            .connection()
            .execute(
                "UPDATE team_tasks SET state = 'waiting', question = '', asked = ?3, attempts = 0 WHERE run = ?1 AND n = ?2 AND state = 'needs_input'",
                kk_vault::rusqlite::params![id, b.task as i64, json!(asked).to_string()],
            )
            .unwrap_or(0)
    };
    if changed != 1 {
        return refuse(
            StatusCode::CONFLICT,
            "NOT_ASKING",
            "that task is not waiting for an answer",
        );
    }
    svc.log_event("team.answered", &id, &[("task", b.task.into())]);
    // if the driver is still busy with another task it will reach this one; otherwise carry on now
    svc.team_spawn(&id);
    match svc.team_run(&id) {
        Some(run) => Json(svc.team_json(&run)).into_response(),
        None => not_found(),
    }
}

pub(super) async fn team_delete(
    axum::Extension(svc): Svc,
    UrlPath(id): UrlPath<String>,
) -> Response {
    if !run_id_ok(&id) {
        return not_found();
    }
    if svc.team_run(&id).is_none() {
        return not_found();
    }
    if svc.team.lock().expect("lock").active.contains(&id) {
        return refuse(
            StatusCode::CONFLICT,
            "STILL_WORKING",
            "stop the run before deleting it",
        );
    }
    {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        let _ = c.execute("DELETE FROM team_tasks WHERE run = ?1", [&id]);
        let _ = c.execute("DELETE FROM team_runs WHERE id = ?1", [&id]);
    }
    svc.log_event("team.deleted", &id, &[]);
    Json(json!({"deleted": id})).into_response()
}
