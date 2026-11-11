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

impl Service {
    /// The seat for an id: a built-in expert, or one of the owner's methods approved for exactly its current text.
    fn team_seat(&self, id: &str) -> Option<(Seat, String)> {
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
                "SELECT id, goal, project, status, final, final_note, created, updated FROM team_runs WHERE id = ?1",
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
        let _ = store.connection().execute(
            "UPDATE team_tasks SET state = 'running', attempts = attempts + 1 WHERE run = ?1 AND n = ?2",
            kk_vault::rusqlite::params![id, n as i64],
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

    fn team_stop_asked(&self, id: &str) -> bool {
        self.team.lock().expect("lock").stop.contains(id)
    }

    fn team_json(&self, run: &RunRow) -> Value {
        let tasks = self.team_tasks(&run.id);
        let active = self.team.lock().expect("lock").active.contains(&run.id);
        json!({
            "id": run.id,
            "goal": run.goal,
            "project": run.project,
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
        let body = ChatBody {
            message: run.goal.clone(),
            history: Vec::new(),
            sources: Vec::new(),
            attachments: Vec::new(),
            session: None,
            project: run.project.clone(),
        };
        let Ok(chat) = chat_validate(self, body) else {
            self.team_set_final(id, None, "the goal could not be prepared for the model");
            return "stopped";
        };
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
                    self.team_finish(id, &run.goal, &tasks, window);
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
            let result = self
                .runner
                .chat_stream(messages, &mut |_| !self.team_stop_asked(id));
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
    fn team_finish(&self, id: &str, goal: &str, tasks: &[TaskRow], window: Option<u64>) {
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
        match self.runner.chat(messages) {
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
        attachments: Vec::new(),
        session: None,
        project: b.project.clone(),
    };
    let project = match chat_validate(&svc, probe) {
        Ok(run) => run.body.project,
        Err(resp) => return *resp,
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
                svc2.runner
                    .chat(vec![
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
            "INSERT INTO team_runs(id, goal, project, status, final, final_note, created, updated) VALUES(?1, ?2, ?3, 'planned', NULL, '', ?4, ?4)",
            kk_vault::rusqlite::params![id, goal, project, at],
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
