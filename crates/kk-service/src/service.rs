use crate::deliver::{capsule_dir, deliver, CapsuleFile};
use crate::keys::{load_keys, open_vault, Keys};
use crate::snapshot::Snapshot;
use axum::extract::{Path as UrlPath, Query};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use kk_companion::app::{error, AppState, Extension};
use kk_effects::{Ledger, State as EffState, SystemClock};
use kk_evidence::capsule::ClaimSpec;
use kk_review::job::JobReport;
use kk_review::release::{build_capsule, Lease, LeaseState};
use kk_vault::Vault;
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Serialize, Deserialize)]
struct Event {
    at: i64,
    stage: String,
    detail: String,
}

#[derive(Clone, Serialize, Deserialize)]
enum JobState {
    Running,
    Ready,
    NothingToRelease(String),
    Failed(String),
}

#[derive(Clone, Serialize, Deserialize)]
struct Delivered {
    name: String,
    digest: String,
    path: PathBuf,
    signer: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Job {
    id: String,
    repo: PathBuf,
    recipient: String,
    created: i64,
    state: JobState,
    events: Vec<Event>,
    approval: Option<String>,
    delivered: Option<Delivered>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ApprovalRec {
    job: String,
    digest: String,
    recipient: String,
    expires: i64,
    state: String,
    denied: bool,
}

impl ApprovalRec {
    fn lease(&self) -> Lease {
        Lease {
            digest: self.digest.clone(),
            recipient: self.recipient.clone(),
            expires: self.expires,
            state: match self.state.as_str() {
                "approved" => LeaseState::Approved,
                "delivering" => LeaseState::Delivering,
                "delivered" => LeaseState::Delivered,
                "uncertain" => LeaseState::Uncertain,
                "revoked" => LeaseState::Revoked,
                _ => LeaseState::Previewed,
            },
        }
    }

    fn set(&mut self, l: &Lease) {
        self.state = match l.state {
            LeaseState::Previewed => "waiting",
            LeaseState::Approved => "approved",
            LeaseState::Delivering => "delivering",
            LeaseState::Delivered => "delivered",
            LeaseState::Uncertain => "uncertain",
            LeaseState::Revoked => "revoked",
        }
        .to_string();
    }
}

struct Inner {
    jobs: BTreeMap<String, Job>,
    snaps: BTreeMap<String, Arc<Snapshot>>,
    approvals: BTreeMap<String, ApprovalRec>,
    next: u64,
}

pub struct Service {
    data_dir: PathBuf,
    roots: Vec<PathBuf>,
    runner: Arc<dyn JobRunner>,
    keys: Keys,
    inner: Mutex<Inner>,
    store: Mutex<Vault>,
    ledger: Mutex<Ledger<SystemClock>>,
    pub test_mode: bool,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn init_store(v: &Vault) -> Result<(), String> {
    v.connection()
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, meta TEXT NOT NULL, snap TEXT);
             CREATE TABLE IF NOT EXISTS approvals(id TEXT PRIMARY KEY, body TEXT NOT NULL);",
        )
        .map_err(|e| e.to_string())
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
        let store = open_vault(data_dir, "jobs.db")?;
        init_store(&store)?;
        let mut inner = Inner {
            jobs: BTreeMap::new(),
            snaps: BTreeMap::new(),
            approvals: BTreeMap::new(),
            next: 1,
        };
        {
            let c = store.connection();
            let mut stmt = c
                .prepare("SELECT id, meta, snap FROM jobs")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            for row in rows.flatten() {
                let (id, meta, snap) = row;
                if let Ok(mut job) = serde_json::from_str::<Job>(&meta) {
                    if matches!(job.state, JobState::Running) {
                        job.state = JobState::Failed(
                            "the service stopped while this review was running; start it again"
                                .into(),
                        );
                        job.events.push(Event {
                            at: now(),
                            stage: "interrupted".into(),
                            detail: "the service stopped".into(),
                        });
                    }
                    if let Some(n) = id.strip_prefix("job-").and_then(|n| n.parse::<u64>().ok()) {
                        inner.next = inner.next.max(n + 1);
                    }
                    if let Some(sn) = snap.and_then(|t| serde_json::from_str::<Snapshot>(&t).ok()) {
                        inner.snaps.insert(id.clone(), Arc::new(sn));
                    }
                    inner.jobs.insert(id, job);
                }
            }
            let mut stmt = c
                .prepare("SELECT id, body FROM approvals")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(|e| e.to_string())?;
            for (id, body) in rows.flatten() {
                if let Ok(mut a) = serde_json::from_str::<ApprovalRec>(&body) {
                    if let Some(n) = id.strip_prefix("appr-").and_then(|n| n.parse::<u64>().ok()) {
                        inner.next = inner.next.max(n + 1);
                    }
                    let mut l = a.lease();
                    l.recover_after_crash();
                    a.set(&l);
                    inner.approvals.insert(id, a);
                }
            }
        }
        let roots = roots
            .into_iter()
            .filter_map(|r| r.canonicalize().ok())
            .collect();
        Ok(Arc::new(Service {
            data_dir: data_dir.to_path_buf(),
            roots,
            runner,
            keys,
            inner: Mutex::new(inner),
            store: Mutex::new(store),
            ledger: Mutex::new(ledger),
            test_mode,
        }))
    }

    pub fn signer_public(&self) -> [u8; 32] {
        self.keys.signer.public()
    }

    fn persist_job(&self, job: &Job, snap: Option<&Snapshot>) {
        let meta = serde_json::to_string(job).unwrap_or_default();
        let snap_text = snap.and_then(|s| serde_json::to_string(s).ok());
        if let Ok(v) = self.store.lock() {
            let _ = v.connection().execute(
                "INSERT INTO jobs(id, meta, snap) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET meta = excluded.meta, snap = COALESCE(excluded.snap, jobs.snap)",
                (&job.id, &meta, &snap_text),
            );
        }
    }

    fn persist_approval(&self, id: &str, a: &ApprovalRec) {
        let body = serde_json::to_string(a).unwrap_or_default();
        if let Ok(v) = self.store.lock() {
            let _ = v.connection().execute(
                "INSERT INTO approvals(id, body) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET body = excluded.body",
                (id, &body),
            );
        }
    }

    fn allowed_dir(&self, p: &str) -> Option<PathBuf> {
        let c = Path::new(p).canonicalize().ok()?;
        (c.is_dir() && self.roots.iter().any(|r| c.starts_with(r))).then_some(c)
    }

    fn push_event(self: &Arc<Self>, id: &str, stage: &str, detail: &str) {
        let mut g = self.inner.lock().expect("lock");
        if let Some(j) = g.jobs.get_mut(id) {
            j.events.push(Event {
                at: now(),
                stage: stage.into(),
                detail: detail.into(),
            });
            let j = j.clone();
            drop(g);
            self.persist_job(&j, None);
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
        let (id, first) = {
            let mut g = self.inner.lock().expect("lock");
            let id = format!("job-{}", g.next);
            g.next += 1;
            let job = Job {
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
                approval: None,
                delivered: None,
            };
            g.jobs.insert(id.clone(), job.clone());
            (id, job)
        };
        self.persist_job(&first, None);
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
            let (job, snap) = {
                let mut g = svc.inner.lock().expect("lock");
                let Some(job) = g.jobs.get_mut(&jid) else {
                    return;
                };
                let mut snap_out = None;
                match report {
                    Ok(r) => {
                        let snap = Snapshot::from_report(&r);
                        job.state = match (&snap.error, &snap.draft) {
                            (Some(e), _) => JobState::Failed(e.clone()),
                            (None, Some(_)) => JobState::Ready,
                            (None, None) => JobState::NothingToRelease(
                                snap.rejection.clone().unwrap_or_default(),
                            ),
                        };
                        snap_out = Some(Arc::new(snap));
                    }
                    Err(_) => {
                        job.state = JobState::Failed("the review stopped unexpectedly".into())
                    }
                }
                job.events.push(Event {
                    at: now(),
                    stage: "finished".into(),
                    detail: String::new(),
                });
                let job = job.clone();
                if let Some(s) = &snap_out {
                    g.snaps.insert(jid.clone(), s.clone());
                }
                (job, snap_out)
            };
            svc.persist_job(&job, snap.as_deref());
        });
        Ok(id)
    }

    fn job_json(&self, j: &Job, snap: Option<&Arc<Snapshot>>) -> Value {
        let (state, note) = match &j.state {
            JobState::Running => ("running", String::new()),
            JobState::Ready => ("ready", String::new()),
            JobState::NothingToRelease(n) => ("nothing_to_release", n.clone()),
            JobState::Failed(m) => ("failed", m.clone()),
        };
        json!({
            "id": j.id, "repo": j.repo.to_string_lossy(), "recipient": j.recipient, "created": j.created,
            "state": state, "note": note,
            "events": j.events.iter().map(|e| json!({"at": e.at, "stage": e.stage, "detail": e.detail})).collect::<Vec<_>>(),
            "summary": snap.map(|s| s.summary.clone()),
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
    Json(
        json!({"jobs": g.jobs.values().rev().map(|j| svc.job_json(j, g.snaps.get(&j.id))).collect::<Vec<_>>()}),
    )
}

async fn get_job(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let g = svc.inner.lock().expect("lock");
    match g.jobs.get(&id) {
        Some(j) => Json(svc.job_json(j, g.snaps.get(&id))).into_response(),
        None => error(StatusCode::NOT_FOUND, "NO_JOB", "no such review"),
    }
}

#[allow(clippy::result_large_err)]
fn snap_of(svc: &Service, id: &str) -> Result<Arc<Snapshot>, Response> {
    let g = svc.inner.lock().expect("lock");
    if !g.jobs.contains_key(id) {
        return Err(error(StatusCode::NOT_FOUND, "NO_JOB", "no such review"));
    }
    g.snaps.get(id).cloned().ok_or_else(|| {
        error(
            StatusCode::CONFLICT,
            "NOT_READY",
            "the review has not finished",
        )
    })
}

async fn files(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    match snap_of(&svc, &id) {
        Err(r) => r,
        Ok(s) => Json(json!({"files": s.files.iter().map(|f| json!({
            "path": f.path, "bytes": f.bytes, "level": f.level, "compartments": f.compartments, "withheld": f.withheld, "notes": f.notes,
        })).collect::<Vec<_>>()}))
        .into_response(),
    }
}

async fn file(
    axum::Extension(svc): Svc,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<PathQuery>,
) -> Response {
    let s = match snap_of(&svc, &id) {
        Err(r) => return r,
        Ok(s) => s,
    };
    let Some(path) = q.path else {
        return error(StatusCode::BAD_REQUEST, "BAD_PATH", "name a file");
    };
    match s.files.iter().find(|f| f.path == path) {
        Some(f) if f.withheld => {
            Json(json!({"path": f.path, "withheld": true, "text": null})).into_response()
        }
        Some(f) => Json(json!({"path": f.path, "withheld": false, "text": f.text})).into_response(),
        None => error(StatusCode::NOT_FOUND, "NO_FILE", "no such file"),
    }
}

async fn audit(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    match snap_of(&svc, &id) {
        Err(r) => r,
        Ok(s) => Json(json!({"audit": s.audit.iter().map(|e| json!({"request": e.request, "op": e.op, "outcome": e.outcome, "detail": e.detail})).collect::<Vec<_>>()})).into_response(),
    }
}

async fn findings(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    match snap_of(&svc, &id) {
        Err(r) => r,
        Ok(s) => Json(json!({"findings": s.findings.iter().map(|f| json!({
            "title": f.title, "file": f.file, "line": f.line, "explanation": f.explanation, "reproducer": f.reproducer,
            "status": f.status, "output": f.output, "sample": f.sample,
        })).collect::<Vec<_>>(), "unparsed": s.unparsed}))
        .into_response(),
    }
}

fn draft_json(s: &Snapshot) -> Value {
    match s.release_draft() {
        Some(d) => json!({
            "available": true, "digest": d.digest, "recipient": d.recipient, "policy_version": d.policy_version, "expires": d.expires,
            "files": d.files.iter().map(|(n, b)| json!({"name": n, "bytes": b.len(), "sha256": kk_evidence::capsule::sha256_hex(b), "text": if n.ends_with(".txt") || n.ends_with(".py") || n.ends_with(".json") { Value::String(String::from_utf8_lossy(b).into_owned()) } else { Value::Null }})).collect::<Vec<_>>(),
            "removed": d.removed.iter().map(|x| json!({"what": x.what, "why": x.why})).collect::<Vec<_>>(),
            "not_confirmed": d.not_confirmed.iter().map(|(t, w)| json!({"title": t, "why": w})).collect::<Vec<_>>(),
            "not_established": kk_review::release::standard_not_established(),
        }),
        None => {
            json!({"available": false, "reason": s.rejection.clone().unwrap_or_default(), "not_confirmed": s.findings.iter().map(|f| json!({"title": f.title, "why": f.status})).collect::<Vec<_>>()})
        }
    }
}

async fn release(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    match snap_of(&svc, &id) {
        Err(r) => r,
        Ok(s) => Json(draft_json(&s)).into_response(),
    }
}

async fn request_approval(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let s = match snap_of(&svc, &id) {
        Err(r) => return r,
        Ok(s) => s,
    };
    let Some(draft) = s.release_draft() else {
        return error(
            StatusCode::CONFLICT,
            "NOTHING_TO_RELEASE",
            "there is nothing to approve",
        );
    };
    let (aid, rec, job) = {
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
        let rec = ApprovalRec {
            job: id.clone(),
            digest: draft.digest.clone(),
            recipient: draft.recipient.clone(),
            expires: draft.expires,
            state: "waiting".into(),
            denied: false,
        };
        g.approvals.insert(aid.clone(), rec.clone());
        let job = g.jobs.get_mut(&id).map(|j| {
            j.approval = Some(aid.clone());
            j.events.push(Event {
                at: now(),
                stage: "approval".into(),
                detail: "waiting for your approval in the terminal".into(),
            });
            j.clone()
        });
        (aid, rec, job)
    };
    svc.persist_approval(&aid, &rec);
    if let Some(j) = job {
        svc.persist_job(&j, None);
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
    let state = if a.denied { "denied" } else { a.state.as_str() };
    Json(json!({"approval": aid, "state": state, "digest": a.digest})).into_response()
}

async fn do_deliver(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let s = match snap_of(&svc, &id) {
        Err(r) => return r,
        Ok(s) => s,
    };
    let Some(draft) = s.release_draft() else {
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
        if a.denied {
            return error(
                StatusCode::CONFLICT,
                "NOT_APPROVED",
                "the release was refused",
            );
        }
        let mut lease = a.lease();
        if let Err(e) = lease.begin_delivery(&draft, now()) {
            return error(
                StatusCode::CONFLICT,
                "NOT_APPROVED",
                &format!("the release cannot be delivered: {e:?}"),
            );
        }
        a.set(&lease);
        let rec = a.clone();
        let digest = a.digest.clone();
        drop(g);
        svc.persist_approval(&aid, &rec);
        (aid, digest)
    };
    let claims: Vec<ClaimSpec> = s
        .findings
        .iter()
        .filter(|f| f.reproduced)
        .enumerate()
        .map(|(i, f)| ClaimSpec {
            statement: f.title.clone(),
            evidence: vec![format!("finding-{}.txt", i + 1), format!("repro-{}.py", i + 1), format!("expected-{}.txt", i + 1), format!("sample-{}.json", i + 1)],
            how_to_check: "Run run-check.py with Python 3: it rebuilds the minimal sample and compares the output with expected-N.txt.".into(),
        })
        .collect();
    let bytes = build_capsule(
        &draft,
        &svc.keys.signer,
        &s.commitment,
        s.manifest_files,
        &s.route_class,
        &claims,
    );
    let dir = match capsule_dir(&svc.data_dir) {
        Ok(d) => d,
        Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, "DISK", &e.to_string()),
    };
    let name = format!("{}.kkg", &digest[..16]);
    let file = CapsuleFile {
        path: dir.join(&name),
        bytes,
    };
    let outcome = deliver(
        &mut svc.ledger.lock().expect("ledger"),
        &format!("release-{digest}"),
        &aid,
        &digest,
        &file,
    );
    let (response, rec, job) = {
        let mut g = svc.inner.lock().expect("lock");
        let a = g.approvals.get_mut(&aid).expect("approval exists");
        let mut lease = a.lease();
        let mut job_out = None;
        let response = match outcome {
            Ok(EffState::Confirmed) => {
                let _ = lease.complete();
                a.set(&lease);
                let signer = kk_evidence::keys::KeyId::of(&svc.signer_public())
                    .as_str()
                    .to_string();
                let rec = a.clone();
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
                    job_out = Some(j.clone());
                }
                (
                    Json(json!({"name": name, "digest": digest, "signer": signer})).into_response(),
                    rec,
                )
            }
            other => {
                lease.recover_after_crash();
                a.set(&lease);
                let msg = match other {
                    Ok(st) => format!("delivery ended as {}", st.as_str()),
                    Err(e) => e,
                };
                (
                    error(StatusCode::CONFLICT, "NOT_DELIVERED", &msg),
                    a.clone(),
                )
            }
        };
        (response.0, response.1, job_out)
    };
    svc.persist_approval(&aid, &rec);
    if let Some(j) = job {
        svc.persist_job(&j, None);
    }
    response
}

async fn download(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let path = svc
        .inner
        .lock()
        .expect("lock")
        .jobs
        .get(&id)
        .and_then(|j| j.delivered.as_ref().map(|d| d.path.clone()));
    let Some(path) = path else {
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

async fn admin_list(axum::Extension(svc): Svc) -> Json<Value> {
    let g = svc.inner.lock().expect("lock");
    Json(
        json!({"approvals": g.approvals.iter().filter(|(_, a)| a.state == "waiting" && !a.denied).map(|(id, a)| json!({"id": id, "job": a.job, "recipient": a.recipient})).collect::<Vec<_>>()}),
    )
}

async fn admin_show(axum::Extension(svc): Svc, UrlPath(aid): UrlPath<String>) -> Response {
    let (job, digest) = {
        let g = svc.inner.lock().expect("lock");
        match g.approvals.get(&aid) {
            Some(a) => (a.job.clone(), a.digest.clone()),
            None => return error(StatusCode::NOT_FOUND, "NO_APPROVAL", "no such approval"),
        }
    };
    match snap_of(&svc, &job) {
        Err(r) => r,
        Ok(s) => {
            let mut d = draft_json(&s);
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
    let (resp, rec) = {
        let mut g = svc.inner.lock().expect("lock");
        let Some(a) = g.approvals.get_mut(&aid) else {
            return error(StatusCode::NOT_FOUND, "NO_APPROVAL", "no such approval");
        };
        if a.denied {
            return error(StatusCode::CONFLICT, "DENIED", "this approval was denied");
        }
        let mut lease = a.lease();
        match lease.approve(&b.ack, now()) {
            Ok(()) => {
                a.set(&lease);
                let rec = a.clone();
                let job = a.job.clone();
                if let Some(j) = g.jobs.get_mut(&job) {
                    j.events.push(Event {
                        at: now(),
                        stage: "approved".into(),
                        detail: "approved in the terminal".into(),
                    });
                }
                (Json(json!({"state": "approved"})).into_response(), rec)
            }
            Err(e) => return error(StatusCode::CONFLICT, "NOT_APPROVED", &format!("{e:?}")),
        }
    };
    svc.persist_approval(&aid, &rec);
    resp
}

async fn admin_deny(axum::Extension(svc): Svc, UrlPath(aid): UrlPath<String>) -> Response {
    let rec = {
        let mut g = svc.inner.lock().expect("lock");
        match g.approvals.get_mut(&aid) {
            Some(a) => {
                let mut lease = a.lease();
                lease.revoke();
                a.set(&lease);
                a.denied = true;
                a.clone()
            }
            None => return error(StatusCode::NOT_FOUND, "NO_APPROVAL", "no such approval"),
        }
    };
    svc.persist_approval(&aid, &rec);
    Json(json!({"state": "denied"})).into_response()
}

fn mcp_view(svc: &Service, j: &Job) -> Value {
    let g = svc.inner.lock().expect("lock");
    let snap = g.snaps.get(&j.id);
    let (state, note) = match &j.state {
        JobState::Running => ("running", String::new()),
        JobState::Ready => ("ready", String::new()),
        JobState::NothingToRelease(_) => ("nothing_to_release", String::new()),
        JobState::Failed(m) => ("failed", m.clone()),
    };
    json!({
        "id": j.id, "state": state, "note": note,
        "stage": j.events.last().map(|e| e.stage.clone()),
        "files_reviewed": snap.map(|s| s.files.len()),
        "files_withheld": snap.map(|s| s.files.iter().filter(|f| f.withheld).count()),
        "findings_reported": snap.map(|s| s.findings.len()),
        "findings_confirmed": snap.map(|s| s.findings.iter().filter(|f| f.reproduced).count()),
        "approval": j.approval.as_ref().and_then(|a| g.approvals.get(a)).map(|a| if a.denied { "denied".to_string() } else { a.state.clone() }),
        "delivered": j.delivered.as_ref().map(|d| json!({"digest": d.digest, "signer": d.signer})),
    })
}

async fn admin_jobs_list(axum::Extension(svc): Svc) -> Json<Value> {
    let jobs: Vec<Job> = svc
        .inner
        .lock()
        .expect("lock")
        .jobs
        .values()
        .cloned()
        .collect();
    Json(json!({"jobs": jobs.iter().rev().map(|j| mcp_view(&svc, j)).collect::<Vec<_>>()}))
}

async fn admin_job_start(axum::Extension(svc): Svc, Json(b): Json<StartBody>) -> Response {
    match svc.start_job(&b.repo, &b.recipient) {
        Ok(id) => (StatusCode::ACCEPTED, Json(json!({"id": id}))).into_response(),
        Err(m) => error(StatusCode::BAD_REQUEST, "BAD_JOB", &m),
    }
}

async fn admin_job_get(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let job = svc.inner.lock().expect("lock").jobs.get(&id).cloned();
    match job {
        Some(j) => Json(mcp_view(&svc, &j)).into_response(),
        None => error(StatusCode::NOT_FOUND, "NO_JOB", "no such review"),
    }
}

async fn admin_job_request_approval(svc: Svc, id: UrlPath<String>) -> Response {
    request_approval(svc, id).await
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
            .route("/admin/jobs", get(admin_jobs_list).post(admin_job_start))
            .route("/admin/jobs/{id}", get(admin_job_get))
            .route(
                "/admin/jobs/{id}/approval",
                post(admin_job_request_approval),
            )
            .route("/admin/approvals", get(admin_list))
            .route("/admin/approvals/{id}", get(admin_show))
            .route("/admin/approvals/{id}/approve", post(admin_approve))
            .route("/admin/approvals/{id}/deny", post(admin_deny))
            .layer(axum::Extension(self.0.clone()))
    }
}
