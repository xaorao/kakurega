use kk_companion::{start, Options};
use kk_evidence::capsule::{self, VerifyOptions};
use kk_review::check::{CheckResult, CheckStatus, Finding};
use kk_review::import::{keyed_commitment, manifest, scan_tree};
use kk_review::job::JobReport;
use kk_review::release::assemble;
use kk_route::RouteEvidence;
use kk_service::{JobRunner, Routes, Service};
use kk_types::label::{Integrity, Label, Level};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const REPRO: &str = "from payments.auth import session\nt = session.login('alice')\nassert session.validate(t)\nsession.logout(t)\nprint('REPRODUCED' if session.validate(t) else 'NOT REPRODUCED')\n";

struct CannedRunner {
    confirm: bool,
}

impl JobRunner for CannedRunner {
    fn run(
        &self,
        repo: &Path,
        recipient: &str,
        _job: &str,
        now: i64,
        progress: &dyn Fn(&str, &str),
    ) -> JobReport {
        progress("import", "reading the project");
        let imp = scan_tree(repo, Level::Internal);
        let man = manifest(&imp.files);
        let commitment = keyed_commitment(&[3u8; 32], &man);
        progress("review", "canned review (test runner)");
        let finding = Finding {
            title: "Token not revoked".into(),
            file: "payments/auth/session.py".into(),
            line: 15,
            explanation: "logout() leaves the token valid".into(),
            reproducer: REPRO.into(),
        };
        let status = if self.confirm {
            CheckStatus::Reproduced
        } else {
            CheckStatus::NotReproduced
        };
        let check = CheckResult {
            status,
            stdout: "REPRODUCED\n".into(),
            sample_paths: vec![
                "payments/__init__.py".into(),
                "payments/auth/__init__.py".into(),
                "payments/auth/session.py".into(),
                "payments/auth/tokens.py".into(),
            ],
        };
        let index: BTreeMap<String, &kk_review::import::ImportedFile> =
            imp.files.iter().map(|f| (f.path.clone(), f)).collect();
        let findings = vec![(finding, check)];
        let draft = assemble(&findings, &index, recipient, "policy-1", now, 86_400);
        JobReport {
            imported: imp.files.len(),
            skipped: vec![],
            withheld: imp
                .files
                .iter()
                .filter(|f| f.label.level >= Level::Restricted)
                .map(|f| (f.path.clone(), f.notes.clone()))
                .collect(),
            injection_suspects: imp
                .files
                .iter()
                .filter(|f| f.notes.iter().any(|n| n.contains("injection")))
                .map(|f| f.path.clone())
                .collect(),
            manifest: man,
            commitment,
            worker_exit: None,
            worker_stderr: String::new(),
            findings,
            unparsed_findings: 0,
            draft,
            audit: vec![],
            route_evidence: vec![RouteEvidence {
                route_id: "canned".into(),
                class: "device_only".into(),
                model: "canned".into(),
                model_digest: None,
                endpoint: "in-process".into(),
                tee_verified: false,
                simulated: true,
            }],
            final_taint: Some(Label::new(Level::Internal).untrusted()),
            error: None,
            files: imp.files,
        }
    }

    fn route_info(&self) -> Value {
        json!({"id": "canned", "class": "device_only", "simulated": true})
    }
}

struct Web {
    base: String,
    agent: ureq::Agent,
    cookie: String,
    csrf: String,
}

impl Web {
    fn new(addr: std::net::SocketAddr) -> Web {
        let cfg = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .build();
        Web {
            base: format!("http://127.0.0.1:{}", addr.port()),
            agent: cfg.into(),
            cookie: String::new(),
            csrf: String::new(),
        }
    }

    fn pair(&mut self, code: &str) {
        let mut r = self
            .agent
            .post(&format!("{}/api/pair", self.base))
            .header("Origin", &self.base)
            .send_json(json!({"code": code}))
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);
        self.cookie = r
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        let v: Value = r.body_mut().read_json().unwrap();
        self.csrf = v["csrf"].as_str().unwrap().to_string();
    }

    fn get(&self, path: &str) -> (u16, Value) {
        let mut r = self
            .agent
            .get(&format!("{}{}", self.base, path))
            .header("Cookie", &self.cookie)
            .call()
            .unwrap();
        (
            r.status().as_u16(),
            r.body_mut().read_json().unwrap_or(Value::Null),
        )
    }

    fn get_bytes(&self, path: &str) -> (u16, Vec<u8>) {
        let mut r = self
            .agent
            .get(&format!("{}{}", self.base, path))
            .header("Cookie", &self.cookie)
            .call()
            .unwrap();
        (r.status().as_u16(), r.body_mut().read_to_vec().unwrap())
    }

    fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let mut r = self
            .agent
            .post(&format!("{}{}", self.base, path))
            .header("Cookie", &self.cookie)
            .header("Origin", &self.base)
            .header("X-KK-CSRF", &self.csrf)
            .send_json(body)
            .unwrap();
        (
            r.status().as_u16(),
            r.body_mut().read_json().unwrap_or(Value::Null),
        )
    }
}

fn admin(base: &str, token: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let cfg = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = cfg.into();
    let url = format!("{base}{path}");
    let mut r = if method == "GET" {
        agent
            .get(&url)
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .unwrap()
    } else {
        agent
            .post(&url)
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(body.unwrap_or(json!({})))
            .unwrap()
    };
    (
        r.status().as_u16(),
        r.body_mut().read_json().unwrap_or(Value::Null),
    )
}

fn repo_copy(tag: &str) -> (PathBuf, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/service-tmp")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let repo = root.join("repo");
    fn copy(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for e in fs::read_dir(src).unwrap().flatten() {
            let n = e.file_name();
            let t = if n == "_env" {
                dst.join(".env")
            } else {
                dst.join(&n)
            };
            if e.path().is_dir() {
                copy(&e.path(), &t);
            } else {
                fs::copy(e.path(), t).unwrap();
            }
        }
    }
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic-repo"),
        &repo,
    );
    (root, repo)
}

fn boot(
    tag: &str,
    confirm: bool,
) -> (kk_companion::Companion, Web, PathBuf, PathBuf, Arc<Service>) {
    let (root, repo) = repo_copy(tag);
    let data = root.join("data");
    let svc = Service::new(
        &data,
        vec![root.clone()],
        Arc::new(CannedRunner { confirm }),
        true,
    )
    .expect("service");
    let c = start(Options {
        extension: Arc::new(Routes(svc.clone())),
        ..Options::default()
    })
    .unwrap();
    let mut w = Web::new(c.addr);
    let code = c.security.issue_pairing_code();
    w.pair(&code);
    (c, w, repo, root, svc)
}

fn wait_for(w: &Web, id: &str, want: &[&str]) -> Value {
    for _ in 0..200 {
        let (_, v) = w.get(&format!("/api/jobs/{id}"));
        if want.contains(&v["state"].as_str().unwrap_or("")) {
            return v;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("job never reached {want:?}");
}

#[test]
fn sv01_the_whole_journey_over_http_ends_in_a_capsule_that_verifies() {
    let (c, w, repo, _root, svc) = boot("flow", true);
    let (code, cfg) = w.get("/api/config");
    assert_eq!(code, 200);
    assert_eq!(cfg["route"]["simulated"], true);
    let (code, started) = w.post(
        "/api/jobs",
        json!({"repo": repo.to_string_lossy(), "recipient": "acme-audit"}),
    );
    assert_eq!(code, 202, "{started}");
    let id = started["id"].as_str().unwrap().to_string();
    let job = wait_for(&w, &id, &["ready"]);
    let stages: Vec<&str> = job["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["stage"].as_str().unwrap())
        .collect();
    assert_eq!(stages.first(), Some(&"queued"));
    assert_eq!(stages.last(), Some(&"finished"));
    assert!(job["summary"]["withheld"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["path"] == ".env"));
    assert_eq!(job["summary"]["confirmed"], 1);
    let (_, files) = w.get(&format!("/api/jobs/{id}/files"));
    let env = files["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == ".env")
        .unwrap();
    assert_eq!(env["withheld"], true);
    let (_, env_file) = w.get(&format!("/api/jobs/{id}/file?path=.env"));
    assert_eq!(env_file["withheld"], true);
    assert!(
        env_file["text"].is_null(),
        "withheld content is never sent to the page"
    );
    let (_, session_file) = w.get(&format!(
        "/api/jobs/{id}/file?path=payments/auth/session.py"
    ));
    assert!(session_file["text"]
        .as_str()
        .unwrap()
        .contains("def logout"));
    let (_, release) = w.get(&format!("/api/jobs/{id}/release"));
    assert_eq!(release["available"], true);
    let digest = release["digest"].as_str().unwrap().to_string();
    assert!(!release.to_string().contains("hunter2"));

    let (code, early) = w.post(&format!("/api/jobs/{id}/deliver"), json!({}));
    assert_eq!(code, 409, "{early}");
    let (code, req) = w.post(&format!("/api/jobs/{id}/approval"), json!({}));
    assert_eq!(code, 202, "{req}");
    let aid = req["approval"].as_str().unwrap().to_string();
    let (_, st) = w.get(&format!("/api/jobs/{id}/approval"));
    assert_eq!(st["state"], "waiting");
    let (code, again) = w.post(&format!("/api/jobs/{id}/approval"), json!({}));
    assert_eq!(code, 409, "{again}");

    let token = c.security.admin_token().to_string();
    let base = format!("http://127.0.0.1:{}", c.addr.port());
    let (code, pending) = admin(&base, &token, "GET", "/admin/approvals", None);
    assert_eq!(code, 200);
    assert_eq!(pending["approvals"][0]["id"], aid.as_str());
    let (_, shown) = admin(
        &base,
        &token,
        "GET",
        &format!("/admin/approvals/{aid}"),
        None,
    );
    assert_eq!(shown["digest"], digest.as_str());
    let (code, wrong) = admin(
        &base,
        &token,
        "POST",
        &format!("/admin/approvals/{aid}/approve"),
        Some(json!({"ack": "000000000000"})),
    );
    assert_eq!(code, 409, "{wrong}");
    let (code, short) = admin(
        &base,
        &token,
        "POST",
        &format!("/admin/approvals/{aid}/approve"),
        Some(json!({"ack": &digest[..6]})),
    );
    assert_eq!(code, 409, "{short}");
    let (code, ok) = admin(
        &base,
        &token,
        "POST",
        &format!("/admin/approvals/{aid}/approve"),
        Some(json!({"ack": &digest[..12]})),
    );
    assert_eq!(code, 200, "{ok}");
    assert_eq!(
        w.get(&format!("/api/jobs/{id}/approval")).1["state"],
        "approved"
    );

    let (code, delivered) = w.post(&format!("/api/jobs/{id}/deliver"), json!({}));
    assert_eq!(code, 200, "{delivered}");
    assert_eq!(delivered["digest"], digest.as_str());
    let (code, bytes) = w.get_bytes(&format!("/api/jobs/{id}/capsule"));
    assert_eq!(code, 200);
    let report = capsule::verify(
        &bytes,
        &VerifyOptions {
            pinned_keys: Some(vec![svc.signer_public()]),
            now: kk_now(),
            expected_recipient: Some("acme-audit".into()),
        },
    );
    assert!(report.valid, "{report:?}");
    assert_eq!(
        report.signer.unwrap().as_str(),
        cfg["signer"].as_str().unwrap()
    );
    let (code, twice) = w.post(&format!("/api/jobs/{id}/deliver"), json!({}));
    assert_eq!(
        code, 409,
        "a delivered release cannot be delivered again: {twice}"
    );
}

fn kk_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[test]
fn sv03_folders_outside_the_allowed_roots_are_refused() {
    let (_c, w, repo, root, _svc) = boot("roots", true);
    let outside = std::env::temp_dir();
    for bad in [
        outside.to_string_lossy().to_string(),
        format!("{}/../..", repo.to_string_lossy()),
        "/etc".to_string(),
        "/".to_string(),
        "".to_string(),
        "nonexistent".to_string(),
    ] {
        let (code, v) = w.post("/api/jobs", json!({"repo": bad, "recipient": "x"}));
        assert_eq!(code, 400, "{bad}: {v}");
    }
    let link = root.join("escape");
    std::os::unix::fs::symlink("/etc", &link).unwrap();
    let (code, _) = w.post(
        "/api/jobs",
        json!({"repo": link.to_string_lossy(), "recipient": "x"}),
    );
    assert_eq!(code, 400, "a symlink out of the root is refused");
    assert_eq!(w.get(&format!("/api/fs?path={}", "/etc")).0, 403);
    assert_eq!(w.get("/api/fs").0, 200);
    let (code, _) = w.post(
        "/api/jobs",
        json!({"repo": repo.to_string_lossy(), "recipient": ""}),
    );
    assert_eq!(code, 400);
}

#[test]
fn sv06_a_review_that_confirms_nothing_cannot_be_approved_or_delivered() {
    let (_c, w, repo, _root, _svc) = boot("nothing", false);
    let (_, started) = w.post(
        "/api/jobs",
        json!({"repo": repo.to_string_lossy(), "recipient": "acme"}),
    );
    let id = started["id"].as_str().unwrap().to_string();
    let job = wait_for(&w, &id, &["nothing_to_release", "ready"]);
    assert_eq!(job["state"], "nothing_to_release");
    assert_eq!(
        w.post(&format!("/api/jobs/{id}/approval"), json!({})).0,
        409
    );
    assert_eq!(w.post(&format!("/api/jobs/{id}/deliver"), json!({})).0, 409);
    assert_eq!(w.get_bytes(&format!("/api/jobs/{id}/capsule")).0, 404);
    let (_, rel) = w.get(&format!("/api/jobs/{id}/release"));
    assert_eq!(rel["available"], false);
    assert_eq!(rel["not_confirmed"][0]["title"], "Token not revoked");
}

#[test]
fn sv08_without_a_session_nothing_about_jobs_is_reachable() {
    let (c, _w, _repo, _root, _svc) = boot("anon", true);
    let anon = Web::new(c.addr);
    for p in [
        "/api/config",
        "/api/jobs",
        "/api/jobs/job-1",
        "/api/jobs/job-1/files",
        "/api/jobs/job-1/file?path=x",
        "/api/jobs/job-1/release",
        "/api/jobs/job-1/capsule",
        "/api/fs",
    ] {
        assert_eq!(anon.get(p).0, 401, "{p}");
    }
    assert_eq!(
        anon.post("/api/jobs", json!({"repo": "x", "recipient": "y"}))
            .0,
        401
    );
    let _ = Integrity::Trusted;
}

#[test]
fn sv09_the_page_cannot_reach_the_approval_interface_with_its_own_session() {
    let (c, w, repo, _root, _svc) = boot("noadmin", true);
    let (_, started) = w.post(
        "/api/jobs",
        json!({"repo": repo.to_string_lossy(), "recipient": "acme"}),
    );
    let id = started["id"].as_str().unwrap().to_string();
    wait_for(&w, &id, &["ready"]);
    let (_, req) = w.post(&format!("/api/jobs/{id}/approval"), json!({}));
    let aid = req["approval"].as_str().unwrap();
    let (code, v) = w.post(
        &format!("/admin/approvals/{aid}/approve"),
        json!({"ack": "whatever0000"}),
    );
    assert_eq!(code, 403, "{v}");
    let (code, _) = w.get("/admin/approvals");
    assert_eq!(code, 403);
    assert_eq!(c.security.active_sessions(), 1);
}
