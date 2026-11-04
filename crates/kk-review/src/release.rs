use crate::check::{clean_text, CheckResult, CheckStatus, Finding};
use crate::import::ImportedFile;
use kk_evidence::canon::{self, Value};
use kk_evidence::capsule::sha256_hex;
use kk_types::label::Level;
use std::collections::BTreeMap;

pub const MAX_RELEASE_BYTES: usize = 8 << 20;

const VERIFY_SCRIPT: &str = r#"import json, os, subprocess, sys, tempfile

here = os.path.dirname(os.path.abspath(__file__))
index = json.load(open(os.path.join(here, "index.json")))
failed = 0
for item in index["findings"]:
    with tempfile.TemporaryDirectory() as work:
        sample = json.load(open(os.path.join(here, item["sample"])))
        for path, text in sample.items():
            dest = os.path.join(work, path)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            open(dest, "w").write(text)
        env = dict(os.environ, PYTHONPATH=work, PYTHONDONTWRITEBYTECODE="1")
        out = subprocess.run([sys.executable, os.path.join(here, item["reproducer"])], cwd=work, env=env, capture_output=True, text=True)
        expected = open(os.path.join(here, item["expected"])).read()
        ok = out.returncode == 0 and out.stdout == expected
        print(("OK   " if ok else "FAIL ") + item["title"])
        failed += 0 if ok else 1
sys.exit(1 if failed else 0)
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    ContainsWithheldText { file: String, withheld: String },
    ContainsSecret { file: String },
    TooLarge,
    NothingConfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub what: String,
    pub why: String,
}

#[derive(Debug, Clone)]
pub struct ReleaseDraft {
    pub recipient: String,
    pub policy_version: String,
    pub created: i64,
    pub expires: i64,
    pub files: BTreeMap<String, Vec<u8>>,
    pub removed: Vec<Removal>,
    pub not_confirmed: Vec<(String, String)>,
    pub digest: String,
}

pub fn release_digest(
    recipient: &str,
    policy_version: &str,
    expires: i64,
    files: &BTreeMap<String, Vec<u8>>,
) -> String {
    let entries: Vec<Value> = files
        .iter()
        .map(|(name, bytes)| {
            Value::Object(vec![
                ("name".into(), Value::Str(name.clone())),
                ("sha256".into(), Value::Str(sha256_hex(bytes))),
                ("bytes".into(), Value::Int(bytes.len() as i64)),
            ])
        })
        .collect();
    let v = Value::Object(vec![
        ("kind".into(), Value::Str("release-draft".into())),
        ("recipient".into(), Value::Str(recipient.into())),
        ("policy_version".into(), Value::Str(policy_version.into())),
        ("expires".into(), Value::Int(expires)),
        ("files".into(), Value::Array(entries)),
    ]);
    sha256_hex(canon::serialize(&v).as_bytes())
}

fn contains_secret_marker(text: &str) -> bool {
    ["-----BEGIN ", "sk_live_", "AKIA", "ghp_", "xoxb-"]
        .iter()
        .any(|m| text.contains(m))
}

fn withheld_tokens(withheld: &[&ImportedFile]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for f in withheld {
        let text = String::from_utf8_lossy(&f.bytes).to_string();
        for token in text.split(|c: char| {
            c.is_whitespace() || matches!(c, '=' | '"' | '\'' | ',' | ';' | '(' | ')')
        }) {
            if token.len() >= 12 {
                out.push((f.path.clone(), token.to_string()));
            }
        }
    }
    out
}

pub fn assemble(
    results: &[(Finding, CheckResult)],
    project_files: &BTreeMap<String, &ImportedFile>,
    recipient: &str,
    policy_version: &str,
    created: i64,
    ttl_secs: i64,
) -> Result<ReleaseDraft, Rejection> {
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut not_confirmed = Vec::new();
    let mut index_entries: Vec<Value> = Vec::new();
    let mut n = 0;
    for (finding, check) in results {
        if check.status != CheckStatus::Reproduced {
            not_confirmed.push((finding.title.clone(), format!("{:?}", check.status)));
            continue;
        }
        n += 1;
        let text = format!(
            "Finding {n}: {}\nLocation: {}:{}\n\n{}\n\nHow to check: run run-check.py. It rebuilds the minimal sample from sample-{n}.json, runs repro-{n}.py and compares the output with expected-{n}.txt.\n",
            clean_text(&finding.title, 200),
            finding.file,
            finding.line,
            clean_text(&finding.explanation, 4000)
        );
        files.insert(format!("finding-{n}.txt"), text.into_bytes());
        files.insert(
            format!("repro-{n}.py"),
            finding.reproducer.clone().into_bytes(),
        );
        files.insert(
            format!("expected-{n}.txt"),
            check.stdout.clone().into_bytes(),
        );
        let sample = Value::Object(
            check
                .sample_paths
                .iter()
                .map(|p| {
                    (
                        p.clone(),
                        Value::Str(String::from_utf8_lossy(&project_files[p].bytes).into_owned()),
                    )
                })
                .collect(),
        );
        files.insert(
            format!("sample-{n}.json"),
            canon::serialize(&sample).into_bytes(),
        );
        index_entries.push(Value::Object(vec![
            ("title".into(), Value::Str(clean_text(&finding.title, 200))),
            ("sample".into(), Value::Str(format!("sample-{n}.json"))),
            ("reproducer".into(), Value::Str(format!("repro-{n}.py"))),
            ("expected".into(), Value::Str(format!("expected-{n}.txt"))),
        ]));
    }
    if n == 0 {
        return Err(Rejection::NothingConfirmed);
    }
    let index = Value::Object(vec![("findings".into(), Value::Array(index_entries))]);
    files.insert("index.json".into(), canon::serialize(&index).into_bytes());
    files.insert("run-check.py".into(), VERIFY_SCRIPT.as_bytes().to_vec());

    let withheld: Vec<&ImportedFile> = project_files
        .values()
        .filter(|f| f.label.level >= Level::Restricted)
        .copied()
        .collect();
    let lines = withheld_tokens(&withheld);
    let mut total = 0usize;
    let mut removed = Vec::new();
    for (name, bytes) in &files {
        total += bytes.len();
        let text = String::from_utf8_lossy(bytes);
        if name != "run-check.py" && contains_secret_marker(&text) {
            return Err(Rejection::ContainsSecret { file: name.clone() });
        }
        for (src, line) in &lines {
            if text.contains(line.as_str()) {
                return Err(Rejection::ContainsWithheldText {
                    file: name.clone(),
                    withheld: src.clone(),
                });
            }
        }
    }
    if total > MAX_RELEASE_BYTES {
        return Err(Rejection::TooLarge);
    }
    if project_files
        .values()
        .any(|f| f.label.level >= Level::Restricted)
    {
        removed.push(Removal {
            what: format!("{} withheld file(s)", withheld.len()),
            why: "labelled secret or restricted; never read by the agent and never released".into(),
        });
    }
    let expires = created + ttl_secs;
    let digest = release_digest(recipient, policy_version, expires, &files);
    Ok(ReleaseDraft {
        recipient: recipient.into(),
        policy_version: policy_version.into(),
        created,
        expires,
        files,
        removed,
        not_confirmed,
        digest,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseState {
    Previewed,
    Approved,
    Delivering,
    Delivered,
    Uncertain,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    WrongState(LeaseState),
    DigestMismatch,
    Expired,
    AcknowledgementTooShort,
}

#[derive(Debug, Clone)]
pub struct Lease {
    pub digest: String,
    pub recipient: String,
    pub expires: i64,
    pub state: LeaseState,
}

impl Lease {
    pub fn preview(draft: &ReleaseDraft) -> Lease {
        Lease {
            digest: draft.digest.clone(),
            recipient: draft.recipient.clone(),
            expires: draft.expires,
            state: LeaseState::Previewed,
        }
    }

    pub fn approve(&mut self, acknowledged_digest: &str, now: i64) -> Result<(), LeaseError> {
        if self.state != LeaseState::Previewed {
            return Err(LeaseError::WrongState(self.state));
        }
        if now >= self.expires {
            return Err(LeaseError::Expired);
        }
        if acknowledged_digest.len() < 12 {
            return Err(LeaseError::AcknowledgementTooShort);
        }
        if !self.digest.starts_with(acknowledged_digest) {
            return Err(LeaseError::DigestMismatch);
        }
        self.state = LeaseState::Approved;
        Ok(())
    }

    pub fn begin_delivery(&mut self, draft: &ReleaseDraft, now: i64) -> Result<(), LeaseError> {
        if self.state != LeaseState::Approved {
            return Err(LeaseError::WrongState(self.state));
        }
        if now >= self.expires {
            return Err(LeaseError::Expired);
        }
        let actual = release_digest(
            &draft.recipient,
            &draft.policy_version,
            draft.expires,
            &draft.files,
        );
        if actual != self.digest || draft.recipient != self.recipient {
            return Err(LeaseError::DigestMismatch);
        }
        self.state = LeaseState::Delivering;
        Ok(())
    }

    pub fn complete(&mut self) -> Result<(), LeaseError> {
        if self.state != LeaseState::Delivering {
            return Err(LeaseError::WrongState(self.state));
        }
        self.state = LeaseState::Delivered;
        Ok(())
    }

    pub fn recover_after_crash(&mut self) {
        if self.state == LeaseState::Delivering {
            self.state = LeaseState::Uncertain;
        }
    }

    pub fn revoke(&mut self) {
        if matches!(self.state, LeaseState::Previewed | LeaseState::Approved) {
            self.state = LeaseState::Revoked;
        }
    }
}

pub fn standard_not_established() -> Vec<String> {
    vec![
        "That the private source repository is the one that was reviewed: only a keyed commitment to its file list is signed, and the key stays with the owner.".to_string(),
        "That the defect exists in the full private code: the check ran on the released minimal sample only.".to_string(),
        "That no other defects exist: the review lists what it found, not what is absent.".to_string(),
    ]
}

pub fn build_capsule(
    draft: &ReleaseDraft,
    signer: &kk_evidence::keys::Signer,
    source_commitment: &str,
    manifest_files: i64,
    route: &str,
    claims: &[kk_evidence::capsule::ClaimSpec],
) -> Vec<u8> {
    let spec = kk_evidence::capsule::ReleaseSpec {
        capsule_id: draft.digest[..32].to_string(),
        created: draft.created,
        expires: draft.expires,
        recipient: draft.recipient.clone(),
        policy_version: draft.policy_version.clone(),
        binding_level: "B1".into(),
        route: route.into(),
        not_established: standard_not_established(),
        source_commitment_scheme: "hmac-sha256-keyed-manifest".into(),
        source_commitment_digest: source_commitment.into(),
        source_manifest_files: manifest_files,
    };
    kk_evidence::capsule::build(&spec, claims, &draft.files, signer)
}
