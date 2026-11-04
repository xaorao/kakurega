use crate::deliver::{capsule_dir, deliver, CapsuleFile};
use crate::keys::{load_keys, open_vault, Keys};
use axum::extract::{Path as UrlPath, Query};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use kk_companion::app::{error, AppState, Extension};
use kk_effects::{Ledger, State as EffState, SystemClock};
use kk_evidence::capsule::ClaimSpec;
use kk_review::check::CheckStatus;
use kk_review::job::JobReport;
use kk_review::release::{build_capsule, Lease, LeaseState};
use kk_types::label::Level;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub trait JobRunner: Send + Sync {
    fn run(
        &self,
        repo: &Path,
        recipient: &str,
        job_name: &str,
        now: i64,
        progress: &dyn Fn(&str, &str),
    ) -> JobReport;
    fn route_info(&self) -> Value;
}

#[derive(Clone)]
struct Event {
    at: i64,
    stage: String,
    detail: String,
}

enum JobState {
    Running,
    Ready,
    NothingToRelease(String),
    Failed(String),
}

struct Delivered {
    name: String,
    digest: String,
    path: PathBuf,
    signer: String,
}

struct Job {
    id: String,
    repo: PathBuf,
    recipient: String,
    created: i64,
    state: JobState,
    events: Vec<Event>,
    report: Option<Arc<JobReport>>,
    approval: Option<String>,
    delivered: Option<Delivered>,
}

struct Approval {
    job: String,
    lease: Lease,
    denied: bool,
}

struct Inner {
    jobs: BTreeMap<String, Job>,
    approvals: BTreeMap<String, Approval>,
    next: u64,
}

pub struct Service {
    data_dir: PathBuf,
    roots: Vec<PathBuf>,
    runner: Arc<dyn JobRunner>,
    keys: Keys,
    inner: Mutex<Inner>,
    ledger: Mutex<Ledger<SystemClock>>,
    pub test_mode: bool,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn level_name(l: Level) -> &'static str {
    match l {
        Level::Public => "public",
        Level::Internal => "internal",
        Level::Confidential => "confidential",
        Level::Restricted => "restricted",
    }
}

impl Service {
    pub fn new(
        data_dir: &Path,
        roots: Vec<PathBuf>,
        runner: Arc<dyn JobRunner>,
        test_mode: bool,
    ) -> Result<Arc<Service>, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        let keys = load_keys(data_dir)?;
        let ledger_vault = open_vault(data_dir, "ledger.db")?;
        let ledger = Ledger::open_owner(ledger_vault, SystemClock, &data_dir.join("ledger.lock"))
            .map_err(|e| e.to_string())?;
        let roots = roots
            .into_iter()
            .filter_map(|r| r.canonicalize().ok())
            .collect();
        Ok(Arc::new(Service {
            data_dir: data_dir.to_path_buf(),
            roots,
            runner,
            keys,
            inner: Mutex::new(Inner {
                jobs: BTreeMap::new(),
                approvals: BTreeMap::new(),
                next: 1,
            }),
            ledger: Mutex::new(ledger),
            test_mode,
        }))
    }

    pub fn signer_public(&self) -> [u8; 32] {
        self.keys.signer.public()
    }

    fn allowed_dir(&self, p: &str) -> Option<PathBuf> {
        let c = Path::new(p).canonicalize().ok()?;
        (c.is_dir() && self.roots.iter().any(|r| c.starts_with(r))).then_some(c)
    }

    fn push_event(self: &Arc<Self>, id: &str, stage: &str, detail: &str) {
        if let Some(j) = self.inner.lock().expect("lock").jobs.get_mut(id) {
            j.events.push(Event {
                at: now(),
                stage: stage.into(),
                detail: detail.into(),
            });
        }
    }

    pub fn start_job(self: &Arc<Self>, repo: &str, recipient: &str) -> Result<String, String> {
        let dir = self
            .allowed_dir(repo)
            .ok_or("that folder is not available")?;
        if recipient.is_empty()
            || recipient.len() > 128
            || recipient.chars().any(|c| c.is_control())
        {
            return Err("name the recipient in 1 to 128 characters".into());
        }
        let id = {
            let mut g = self.inner.lock().expect("lock");
            let id = format!("job-{}", g.next);
            g.next += 1;
            g.jobs.insert(
                id.clone(),
                Job {
                    id: id.clone(),
                    repo: dir.clone(),
                    recipient: recipient.to_string(),
                    created: now(),
                    state: JobState::Running,
                    events: vec![Event {
                        at: now(),
                        stage: "queued".into(),
                        detail: "waiting to start".into(),
                    }],
                    report: None,
                    approval: None,
                    delivered: None,
                },
            );
            id
        };
        let svc = self.clone();
        let (jid, rec) = (id.clone(), recipient.to_string());
        std::thread::spawn(move || {
            let svc2 = svc.clone();
            let jid2 = jid.clone();
            let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                svc.runner
                    .run(&dir, &rec, &jid, now(), &move |stage, detail| {
                        svc2.push_event(&jid2, stage, detail)
                    })
            }));
            let mut g = svc.inner.lock().expect("lock");
            let Some(job) = g.jobs.get_mut(&jid) else {
                return;
            };
            match report {
                Ok(r) => {
                    job.state = match (&r.error, &r.draft) {
                        (Some(e), _) => JobState::Failed(e.clone()),
                        (None, Ok(_)) => JobState::Ready,
                        (None, Err(rej)) => JobState::NothingToRelease(format!("{rej:?}")),
                    };
                    job.report = Some(Arc::new(r));
                }
                Err(_) => job.state = JobState::Failed("the review stopped unexpectedly".into()),
            }
            job.events.push(Event {
                at: now(),
                stage: "finished".into(),
                detail: String::new(),
            });
        });
        Ok(id)
    }

    fn job_json(&self, j: &Job) -> Value {
        let (state, note) = match &j.state {
            JobState::Running => ("running", String::new()),
            JobState::Ready => ("ready", String::new()),
            JobState::NothingToRelease(n) => ("nothing_to_release", n.clone()),
            JobState::Failed(m) => ("failed", m.clone()),
        };
        let summary = j.report.as_ref().map(|r| {
            json!({
                "imported": r.imported,
                "skipped": r.skipped.iter().map(|(p, why)| json!({"path": p, "reason": why})).collect::<Vec<_>>(),
                "withheld": r.withheld.iter().map(|(p, n)| json!({"path": p, "notes": n})).collect::<Vec<_>>(),
                "injection_suspects": r.injection_suspects,
                "route": r.route_evidence.iter().map(|e| serde_json::to_value(e).unwrap_or(Value::Null)).collect::<Vec<_>>(),
                "worker_exit": r.worker_exit.as_ref().map(|e| format!("{e:?}")),
                "final_taint": r.final_taint.as_ref().map(|t| json!({"level": level_name(t.level), "integrity": format!("{:?}", t.integrity).to_lowercase(), "compartments": t.compartments})),
                "findings": r.findings.len(),
                "confirmed": r.findings.iter().filter(|(_, c)| c.status == CheckStatus::Reproduced).count(),
                "manifest_digest": r.manifest.digest,
                "files_in_manifest": r.manifest.file_count,
            })
        });
        json!({
            "id": j.id, "repo": j.repo.to_string_lossy(), "recipient": j.recipient, "created": j.created,
            "state": state, "note": note,
            "events": j.events.iter().map(|e| json!({"at": e.at, "stage": e.stage, "detail": e.detail})).collect::<Vec<_>>(),
            "summary": summary,
            "approval": j.approval.clone(),
            "delivered": j.delivered.as_ref().map(|d| json!({"name": d.name, "digest": d.digest, "signer": d.signer})),
        })
    }
}

#[derive(Deserialize)]
struct StartBody {
    repo: String,
    recipient: String,
}

#[derive(Deserialize)]
struct PathQuery {
    path: Option<String>,
}

type Svc = axum::Extension<Arc<Service>>;

async fn config(axum::Extension(svc): Svc) -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "route": svc.runner.route_info(),
        "roots": svc.roots.iter().map(|r| r.to_string_lossy()).collect::<Vec<_>>(),
        "test_mode": svc.test_mode,
        "signer": kk_evidence::keys::KeyId::of(&svc.signer_public()).as_str(),
        "signer_public": kk_evidence::b64::encode(&svc.signer_public()),
    }))
}

async fn fs_list(axum::Extension(svc): Svc, Query(q): Query<PathQuery>) -> Response {
    let start = q.path.unwrap_or_else(|| {
        svc.roots
            .first()
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let Some(dir) = svc.allowed_dir(&start) else {
        return error(
            StatusCode::FORBIDDEN,
            "NOT_ALLOWED",
            "that folder is not available",
        );
    };
    let mut dirs = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if let Ok(m) = std::fs::symlink_metadata(e.path()) {
                if m.is_dir() {
                    dirs.push(json!({"name": name, "path": e.path().to_string_lossy()}));
                }
            }
        }
    }
    dirs.sort_by_key(|d| d["name"].as_str().unwrap_or("").to_lowercase());
    let parent = dir
        .parent()
        .filter(|p| svc.roots.iter().any(|r| p.starts_with(r)))
        .map(|p| p.to_string_lossy().into_owned());
    Json(json!({"path": dir.to_string_lossy(), "parent": parent, "dirs": dirs})).into_response()
}

async fn start(axum::Extension(svc): Svc, Json(b): Json<StartBody>) -> Response {
    match svc.start_job(&b.repo, &b.recipient) {
        Ok(id) => (StatusCode::ACCEPTED, Json(json!({"id": id}))).into_response(),
        Err(m) => error(StatusCode::BAD_REQUEST, "BAD_JOB", &m),
    }
}

async fn list(axum::Extension(svc): Svc) -> Json<Value> {
    let g = svc.inner.lock().expect("lock");
    Json(json!({"jobs": g.jobs.values().rev().map(|j| svc.job_json(j)).collect::<Vec<_>>()}))
}

fn with_job<T>(svc: &Service, id: &str, f: impl FnOnce(&Job) -> T) -> Option<T> {
    svc.inner.lock().expect("lock").jobs.get(id).map(f)
}

async fn get_job(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    match with_job(&svc, &id, |j| svc.job_json(j)) {
        Some(v) => Json(v).into_response(),
        None => error(StatusCode::NOT_FOUND, "NO_JOB", "no such review"),
    }
}

#[allow(clippy::result_large_err)]
fn report_of(svc: &Service, id: &str) -> Result<Arc<JobReport>, Response> {
    match with_job(svc, id, |j| j.report.clone()) {
        None => Err(error(StatusCode::NOT_FOUND, "NO_JOB", "no such review")),
        Some(None) => Err(error(
            StatusCode::CONFLICT,
            "NOT_READY",
            "the review has not finished",
        )),
        Some(Some(r)) => Ok(r),
    }
}

async fn files(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    match report_of(&svc, &id) {
        Err(r) => r,
        Ok(r) => Json(json!({"files": r.files.iter().map(|f| json!({
            "path": f.path, "bytes": f.bytes.len(), "level": level_name(f.label.level),
            "compartments": f.label.compartments, "withheld": f.label.level >= Level::Restricted, "notes": f.notes,
        })).collect::<Vec<_>>()}))
        .into_response(),
    }
}

async fn file(
    axum::Extension(svc): Svc,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<PathQuery>,
) -> Response {
    let r = match report_of(&svc, &id) {
        Err(r) => return r,
        Ok(r) => r,
    };
    let Some(path) = q.path else {
        return error(StatusCode::BAD_REQUEST, "BAD_PATH", "name a file");
    };
    match r.files.iter().find(|f| f.path == path) {
        Some(f) if f.label.level >= Level::Restricted => {
            Json(json!({"path": f.path, "withheld": true, "text": null})).into_response()
        }
        Some(f) => Json(
            json!({"path": f.path, "withheld": false, "text": String::from_utf8_lossy(&f.bytes)}),
        )
        .into_response(),
        None => error(StatusCode::NOT_FOUND, "NO_FILE", "no such file"),
    }
}

async fn audit(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    match report_of(&svc, &id) {
        Err(r) => r,
        Ok(r) => Json(json!({"audit": r.audit.iter().map(|e| json!({"request": e.request_id, "op": e.op, "outcome": e.outcome, "detail": e.detail})).collect::<Vec<_>>()})).into_response(),
    }
}

async fn findings(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    match report_of(&svc, &id) {
        Err(r) => r,
        Ok(r) => Json(json!({"findings": r.findings.iter().map(|(f, c)| json!({
            "title": f.title, "file": f.file, "line": f.line, "explanation": f.explanation, "reproducer": f.reproducer,
            "status": match &c.status { CheckStatus::Reproduced => "reproduced".to_string(), CheckStatus::NotReproduced => "not_reproduced".into(), CheckStatus::Nondeterministic => "nondeterministic".into(), CheckStatus::Failed(m) => format!("failed: {m}"), CheckStatus::Unavailable(m) => format!("unavailable: {m}") },
            "output": c.stdout, "sample": c.sample_paths,
        })).collect::<Vec<_>>(), "unparsed": r.unparsed_findings}))
        .into_response(),
    }
}

fn draft_json(r: &JobReport) -> Value {
    match &r.draft {
        Ok(d) => json!({
            "available": true, "digest": d.digest, "recipient": d.recipient, "policy_version": d.policy_version, "expires": d.expires,
            "files": d.files.iter().map(|(n, b)| json!({"name": n, "bytes": b.len(), "sha256": kk_evidence::capsule::sha256_hex(b), "text": if n.ends_with(".txt") || n.ends_with(".py") || n.ends_with(".json") { Value::String(String::from_utf8_lossy(b).into_owned()) } else { Value::Null }})).collect::<Vec<_>>(),
            "removed": d.removed.iter().map(|x| json!({"what": x.what, "why": x.why})).collect::<Vec<_>>(),
            "not_confirmed": d.not_confirmed.iter().map(|(t, w)| json!({"title": t, "why": w})).collect::<Vec<_>>(),
            "not_established": kk_review::release::standard_not_established(),
        }),
        Err(e) => {
            json!({"available": false, "reason": format!("{e:?}"), "not_confirmed": r.findings.iter().map(|(f, c)| json!({"title": f.title, "why": format!("{:?}", c.status)})).collect::<Vec<_>>()})
        }
    }
}

async fn release(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    match report_of(&svc, &id) {
        Err(r) => r,
        Ok(r) => Json(draft_json(&r)).into_response(),
    }
}

async fn request_approval(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let r = match report_of(&svc, &id) {
        Err(r) => return r,
        Ok(r) => r,
    };
    let Ok(draft) = &r.draft else {
        return error(
            StatusCode::CONFLICT,
            "NOTHING_TO_RELEASE",
            "there is nothing to approve",
        );
    };
    let mut g = svc.inner.lock().expect("lock");
    if g.jobs.get(&id).and_then(|j| j.approval.as_ref()).is_some() {
        return error(
            StatusCode::CONFLICT,
            "ALREADY_REQUESTED",
            "an approval is already waiting",
        );
    }
    let aid = format!("appr-{}", g.next);
    g.next += 1;
    g.approvals.insert(
        aid.clone(),
        Approval {
            job: id.clone(),
            lease: Lease::preview(draft),
            denied: false,
        },
    );
    if let Some(j) = g.jobs.get_mut(&id) {
        j.approval = Some(aid.clone());
        j.events.push(Event {
            at: now(),
            stage: "approval".into(),
            detail: "waiting for your approval in the terminal".into(),
        });
    }
    (StatusCode::ACCEPTED, Json(json!({"approval": aid, "digest": draft.digest, "how": format!("run `kaku approve {aid}` in a terminal on this computer")}))).into_response()
}

async fn approval_state(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let g = svc.inner.lock().expect("lock");
    let Some(job) = g.jobs.get(&id) else {
        return error(StatusCode::NOT_FOUND, "NO_JOB", "no such review");
    };
    let Some(aid) = &job.approval else {
        return Json(json!({"state": "none"})).into_response();
    };
    let a = &g.approvals[aid];
    let state = if a.denied {
        "denied"
    } else {
        match a.lease.state {
            LeaseState::Previewed => "waiting",
            LeaseState::Approved => "approved",
            LeaseState::Delivering => "delivering",
            LeaseState::Delivered => "delivered",
            LeaseState::Uncertain => "uncertain",
            LeaseState::Revoked => "revoked",
        }
    };
    Json(json!({"approval": aid, "state": state, "digest": a.lease.digest})).into_response()
}

async fn do_deliver(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let r = match report_of(&svc, &id) {
        Err(r) => return r,
        Ok(r) => r,
    };
    let Ok(draft) = &r.draft else {
        return error(
            StatusCode::CONFLICT,
            "NOTHING_TO_RELEASE",
            "there is nothing to deliver",
        );
    };
    let (aid, digest) = {
        let mut g = svc.inner.lock().expect("lock");
        let Some(aid) = g.jobs.get(&id).and_then(|j| j.approval.clone()) else {
            return error(
                StatusCode::CONFLICT,
                "NOT_APPROVED",
                "approve the release first",
            );
        };
        let a = g.approvals.get_mut(&aid).expect("approval exists");
        if let Err(e) = a.lease.begin_delivery(draft, now()) {
            return error(
                StatusCode::CONFLICT,
                "NOT_APPROVED",
                &format!("the release cannot be delivered: {e:?}"),
            );
        }
        (aid, a.lease.digest.clone())
    };
    let claims: Vec<ClaimSpec> = r
        .findings
        .iter()
        .filter(|(_, c)| c.status == CheckStatus::Reproduced)
        .enumerate()
        .map(|(i, (f, _))| ClaimSpec {
            statement: f.title.clone(),
            evidence: vec![format!("finding-{}.txt", i + 1), format!("repro-{}.py", i + 1), format!("expected-{}.txt", i + 1), format!("sample-{}.json", i + 1)],
            how_to_check: "Run run-check.py with Python 3: it rebuilds the minimal sample and compares the output with expected-N.txt.".into(),
        })
        .collect();
    let route = r
        .route_evidence
        .first()
        .map(|e| e.class.clone())
        .unwrap_or_else(|| "device_only".into());
    let bytes = build_capsule(
        draft,
        &svc.keys.signer,
        &r.commitment,
        r.manifest.file_count as i64,
        &route,
        &claims,
    );
    let dir = match capsule_dir(&svc.data_dir) {
        Ok(d) => d,
        Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, "DISK", &e.to_string()),
    };
    let name = format!("{}.kkg", &digest[..16]);
    let file = CapsuleFile {
        path: dir.join(&name),
        bytes: bytes.clone(),
    };
    let outcome = deliver(
        &mut svc.ledger.lock().expect("ledger"),
        &format!("release-{digest}"),
        &aid,
        &digest,
        &file,
    );
    let mut g = svc.inner.lock().expect("lock");
    let a = g.approvals.get_mut(&aid).expect("approval exists");
    match outcome {
        Ok(EffState::Confirmed) => {
            let _ = a.lease.complete();
            let signer = kk_evidence::keys::KeyId::of(&svc.signer_public())
                .as_str()
                .to_string();
            if let Some(j) = g.jobs.get_mut(&id) {
                j.delivered = Some(Delivered {
                    name: name.clone(),
                    digest: digest.clone(),
                    path: file.path.clone(),
                    signer: signer.clone(),
                });
                j.events.push(Event {
                    at: now(),
                    stage: "delivered".into(),
                    detail: name.clone(),
                });
            }
            Json(json!({"name": name, "digest": digest, "signer": signer})).into_response()
        }
        Ok(other) => {
            a.lease.recover_after_crash();
            error(
                StatusCode::CONFLICT,
                "NOT_DELIVERED",
                &format!("delivery ended as {}", other.as_str()),
            )
        }
        Err(e) => {
            a.lease.recover_after_crash();
            error(StatusCode::CONFLICT, "NOT_DELIVERED", &e)
        }
    }
}

async fn download(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let Some(Some(path)) = with_job(&svc, &id, |j| j.delivered.as_ref().map(|d| d.path.clone()))
    else {
        return error(
            StatusCode::NOT_FOUND,
            "NO_CAPSULE",
            "nothing has been delivered yet",
        );
    };
    match std::fs::read(&path) {
        Ok(bytes) => {
            let mut res = Response::new(axum::body::Body::from(bytes));
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            res.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"review.kkg\""),
            );
            res
        }
        Err(_) => error(
            StatusCode::NOT_FOUND,
            "NO_CAPSULE",
            "the capsule file is missing",
        ),
    }
}

fn find_approval<'a>(g: &'a mut Inner, aid: &str) -> Option<&'a mut Approval> {
    g.approvals.get_mut(aid)
}

async fn admin_list(axum::Extension(svc): Svc) -> Json<Value> {
    let g = svc.inner.lock().expect("lock");
    Json(
        json!({"approvals": g.approvals.iter().filter(|(_, a)| a.lease.state == LeaseState::Previewed && !a.denied).map(|(id, a)| json!({"id": id, "job": a.job, "recipient": a.lease.recipient})).collect::<Vec<_>>()}),
    )
}

async fn admin_show(axum::Extension(svc): Svc, UrlPath(aid): UrlPath<String>) -> Response {
    let (job, digest) = {
        let g = svc.inner.lock().expect("lock");
        match g.approvals.get(&aid) {
            Some(a) => (a.job.clone(), a.lease.digest.clone()),
            None => return error(StatusCode::NOT_FOUND, "NO_APPROVAL", "no such approval"),
        }
    };
    match report_of(&svc, &job) {
        Err(r) => r,
        Ok(r) => {
            let mut d = draft_json(&r);
            d["approval"] = json!(aid);
            d["job"] = json!(job);
            d["digest"] = json!(digest);
            Json(d).into_response()
        }
    }
}

#[derive(Deserialize)]
struct AckBody {
    ack: String,
}

async fn admin_approve(
    axum::Extension(svc): Svc,
    UrlPath(aid): UrlPath<String>,
    Json(b): Json<AckBody>,
) -> Response {
    let mut g = svc.inner.lock().expect("lock");
    let Some(a) = find_approval(&mut g, &aid) else {
        return error(StatusCode::NOT_FOUND, "NO_APPROVAL", "no such approval");
    };
    if a.denied {
        return error(StatusCode::CONFLICT, "DENIED", "this approval was denied");
    }
    match a.lease.approve(&b.ack, now()) {
        Ok(()) => {
            let job = a.job.clone();
            if let Some(j) = g.jobs.get_mut(&job) {
                j.events.push(Event {
                    at: now(),
                    stage: "approved".into(),
                    detail: "approved in the terminal".into(),
                });
            }
            Json(json!({"state": "approved"})).into_response()
        }
        Err(e) => error(StatusCode::CONFLICT, "NOT_APPROVED", &format!("{e:?}")),
    }
}

async fn admin_deny(axum::Extension(svc): Svc, UrlPath(aid): UrlPath<String>) -> Response {
    let mut g = svc.inner.lock().expect("lock");
    match find_approval(&mut g, &aid) {
        Some(a) => {
            a.lease.revoke();
            a.denied = true;
            Json(json!({"state": "denied"})).into_response()
        }
        None => error(StatusCode::NOT_FOUND, "NO_APPROVAL", "no such approval"),
    }
}

pub struct Routes(pub Arc<Service>);

impl Extension for Routes {
    fn routes(&self, _state: AppState) -> Router<AppState> {
        Router::new()
            .route("/api/config", get(config))
            .route("/api/fs", get(fs_list))
            .route("/api/jobs", get(list).post(start))
            .route("/api/jobs/{id}", get(get_job))
            .route("/api/jobs/{id}/files", get(files))
            .route("/api/jobs/{id}/file", get(file))
            .route("/api/jobs/{id}/audit", get(audit))
            .route("/api/jobs/{id}/findings", get(findings))
            .route("/api/jobs/{id}/release", get(release))
            .route(
                "/api/jobs/{id}/approval",
                get(approval_state).post(request_approval),
            )
            .route("/api/jobs/{id}/deliver", post(do_deliver))
            .route("/api/jobs/{id}/capsule", get(download))
            .route("/admin/approvals", get(admin_list))
            .route("/admin/approvals/{id}", get(admin_show))
            .route("/admin/approvals/{id}/approve", post(admin_approve))
            .route("/admin/approvals/{id}/deny", post(admin_deny))
            .layer(axum::Extension(self.0.clone()))
    }
}
