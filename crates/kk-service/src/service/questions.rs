//! Questions about a project, from the owner's screens: one is answered at a time, only about a folder the service may read,
//! only by a model on this computer or in confidential hardware whose proof is checked (the agent reads the whole project,
//! as a review's does), and the answer is kept with the places it rests on as the files showed them.

use super::*;
use crate::questions::{self as q, Question, State};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StartBody {
    folder: String,
    question: String,
    #[serde(default)]
    depth: Option<String>,
}

const MAX_QUESTION_CHARS: usize = 2000;

impl Service {
    fn keep_question(&self, item: &Question) -> bool {
        match self.store.lock() {
            Ok(v) => q::save(v.connection(), item),
            Err(_) => false,
        }
    }
}

pub(super) async fn start(axum::Extension(svc): Svc, Json(b): Json<StartBody>) -> Response {
    let question = b.question.trim().to_string();
    if question.is_empty()
        || question.chars().count() > MAX_QUESTION_CHARS
        || !chat_clean(&question)
    {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_QUESTION",
            "ask the question in 1 to 2000 characters without control characters",
        );
    }
    if !kk_secrets::scan(&question).is_empty() || kk_secrets::spoken::holds(&question) {
        return error(
            StatusCode::BAD_REQUEST,
            "SECRET_IN_QUESTION",
            "the question holds what looks like a secret; leave it out",
        );
    }
    let Some(depth) = kk_review::explore::Depth::parse(b.depth.as_deref().unwrap_or("")) else {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_DEPTH",
            "depth is quick, normal or thorough",
        );
    };
    let Some(folder) = svc.allowed_dir(&b.folder) else {
        return error(
            StatusCode::FORBIDDEN,
            "NOT_ALLOWED",
            "that folder is not available",
        );
    };
    if let Some(refusal) = svc.agent_refusal("question", &folder) {
        return refusal;
    }
    let (id, forgotten) = {
        let mut g = svc.questions.lock().expect("lock");
        if g.list.values().any(|x| x.state == State::Running) {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "BUSY",
                "a question is already being answered; wait for it to finish",
            );
        }
        // the oldest answered questions make room, and the owner is told which; one the store would not let go of stays,
        // and nothing new is started
        let mut forgotten: Vec<String> = Vec::new();
        while g.list.len() >= q::MAX_KEPT {
            let Some(oldest) = g
                .list
                .values()
                .filter(|x| x.state != State::Running)
                .min_by_key(|x| {
                    (
                        x.created,
                        x.id.strip_prefix("question-")
                            .and_then(|n| n.parse::<u64>().ok())
                            .unwrap_or(0),
                    )
                })
                .map(|x| x.id.clone())
            else {
                break;
            };
            let gone = svc
                .store
                .lock()
                .is_ok_and(|store| q::remove(store.connection(), &oldest));
            if !gone {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "STORE_FAILED",
                    "an old question could not be forgotten to make room, so the new one was not started",
                );
            }
            g.list.remove(&oldest);
            forgotten.push(oldest);
        }
        let number = g.next + 1;
        if !svc
            .store
            .lock()
            .is_ok_and(|store| q::note_number(store.connection(), number))
        {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORE_FAILED",
                "the question could not be kept, so it was not started",
            );
        }
        g.next = number;
        let id = format!("question-{}", g.next);
        let item = Question {
            id: id.clone(),
            folder: folder.clone(),
            question: question.clone(),
            depth,
            created: now(),
            state: State::Running,
            outcome: None,
            progress: Vec::new(),
        };
        if !svc.keep_question(&item) {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORE_FAILED",
                "the question could not be kept, so it was not started",
            );
        }
        g.list.insert(id.clone(), item);
        (id, forgotten)
    };
    svc.log_event("question.started", &id, &[("depth", depth.as_str().into())]);
    let svc2 = svc.clone();
    let id2 = id.clone();
    let stop = svc.stop_switch(&id);
    std::thread::spawn(move || {
        let imported = kk_review::import::scan_tree(&folder, kk_types::label::Level::Internal);
        let progress = |line: &str| {
            if let Some(item) = svc2.questions.lock().expect("lock").list.get_mut(&id2) {
                if item.progress.len() < 200 {
                    item.progress.push(line.chars().take(200).collect());
                }
            }
        };
        let mut result =
            svc2.runner
                .project_question(&imported.files, &question, depth, &progress, &stop);
        if let Ok(o) = &result {
            svc2.record_model_use(o.usage.as_ref());
        }
        if svc2.stop_switch_done(&id2) {
            result = Err(kk_review::job::STOPPED_BY_OWNER.into());
        }
        // which files the agent was not given, by name: an answer that says "I could not find it" may be about one of them
        if let Ok(o) = &mut result {
            o.report.withheld_files = imported
                .files
                .iter()
                .filter(|f| f.label.level >= kk_types::label::Level::Restricted)
                .map(|f| f.path.clone())
                .take(50)
                .collect();
        }
        let mut g = svc2.questions.lock().expect("lock");
        let mut checked = 0usize;
        if let Some(item) = g.list.get_mut(&id2) {
            match result {
                Ok(o) => {
                    checked = o
                        .report
                        .citations
                        .iter()
                        .filter(|c| c.found.checked())
                        .count();
                    item.state = match &o.report.ending {
                        kk_review::explore::Ending::ModelFailed(why)
                            if o.report.answer.trim().is_empty() =>
                        {
                            State::Failed(format!(
                                "The model stopped answering: {}",
                                why.chars().take(400).collect::<String>()
                            ))
                        }
                        _ => State::Answered,
                    };
                    item.outcome = Some(o);
                }
                Err(m) => item.state = State::Failed(m.chars().take(500).collect()),
            }
            svc2.keep_question(item);
        }
        drop(g);
        svc2.log_event(
            "question.finished",
            &id2,
            &[("checked_places", checked.into())],
        );
    });
    // 100 questions are kept: the oldest answered ones that made room for this one are named
    (
        StatusCode::ACCEPTED,
        Json(json!({"id": id, "forgotten": forgotten, "model": where_the_model_is(&svc)})),
    )
        .into_response()
}

pub(super) async fn list(axum::Extension(svc): Svc) -> Json<Value> {
    let g = svc.questions.lock().expect("lock");
    let mut all: Vec<&Question> = g.list.values().collect();
    // newest first; questions of one second in the order of their numbers
    all.sort_by_key(|x| {
        std::cmp::Reverse((
            x.created,
            x.id.strip_prefix("question-")
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0),
        ))
    });
    let items: Vec<Value> = all
        .into_iter()
        .take(q::MAX_KEPT)
        .map(|x| {
            let v = q::view(x);
            json!({"id": v["id"], "question": v["question"], "project": v["project"], "folder": v["folder"], "state": v["state"], "created": v["created"], "checked": v["answer"]["checked"], "unchecked": v["answer"]["unchecked"]})
        })
        .collect();
    Json(json!({"questions": items, "max": q::MAX_KEPT}))
}

pub(super) async fn get(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let g = svc.questions.lock().expect("lock");
    match g.list.get(&id) {
        Some(x) => Json(q::view(x)).into_response(),
        None => error(
            StatusCode::NOT_FOUND,
            "NO_QUESTION",
            "there is no such question",
        ),
    }
}

/// Forgets a question and its answer. One being answered is stopped first, by its own switch.
pub(super) async fn forget(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let mut g = svc.questions.lock().expect("lock");
    match g.list.get(&id).map(|x| x.state.clone()) {
        None => {
            return error(
                StatusCode::NOT_FOUND,
                "NO_QUESTION",
                "there is no such question",
            )
        }
        Some(State::Running) => {
            return error(
                StatusCode::CONFLICT,
                "RUNNING",
                "the question is still being answered; stop it first",
            )
        }
        Some(_) => {}
    }
    let gone = svc
        .store
        .lock()
        .map(|s| q::remove(s.connection(), &id))
        .unwrap_or(false);
    if !gone {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "STORE_FAILED",
            "the question could not be removed",
        );
    }
    g.list.remove(&id);
    drop(g);
    svc.log_event("question.forgotten", &id, &[]);
    Json(json!({"forgotten": true})).into_response()
}

/// Where the model an agent works with is, for the line that says a question or a coding task started: on this computer, or
/// at a host (in confidential hardware, the only other place an agent is allowed).
pub(super) fn where_the_model_is(svc: &Service) -> Value {
    let r = svc.route_for_owner();
    json!({"stays_here": r["stays_here"] == true, "class": r["class"], "host": r["host"]})
}
