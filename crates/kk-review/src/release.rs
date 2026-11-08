use crate::check::{clean_text, names_sandbox_path, CheckResult, CheckStatus, Finding, Language};
use crate::import::ImportedFile;
use kk_evidence::canon::{self, Value};
use kk_evidence::capsule::sha256_hex;
use kk_types::label::Level;
use std::collections::BTreeMap;

pub const MAX_RELEASE_BYTES: usize = 8 << 20;

const VERIFY_SCRIPT: &str = r#"import json, os, shutil, subprocess, sys, tempfile

here = os.path.dirname(os.path.abspath(__file__))
index = json.load(open(os.path.join(here, "index.json"), encoding="utf-8"))
failed = 0
missing = 0
for item in index["findings"]:
    language = item.get("language", "python")
    rustc = None
    if language == "rust":
        rustc = shutil.which("rustc")
        if rustc is None:
            print("MISSING " + item["title"] + " (this check needs the Rust compiler, rustc: install it from rust-lang.org and run this again)")
            missing += 1
            continue
        argv = None
    elif language in ("javascript", "typescript"):
        node = shutil.which("node")
        if node is None:
            print("MISSING " + item["title"] + " (this check needs Node.js 22 or newer: install it from nodejs.org and run this again)")
            missing += 1
            continue
        if language == "typescript":
            probe = subprocess.run([node, "-p", "process.features.typescript"], capture_output=True, text=True, encoding="utf-8")
            if probe.returncode != 0 or probe.stdout.strip() not in ("strip", "transform"):
                print("MISSING " + item["title"] + " (this check runs TypeScript with Node.js itself: use the official Node.js 22.18 or newer from nodejs.org; some packaged builds leave TypeScript out)")
                missing += 1
                continue
        argv = [node, os.path.join(here, item["reproducer"])]
    else:
        argv = [sys.executable, os.path.join(here, item["reproducer"])]
    with tempfile.TemporaryDirectory() as work:
        root = os.path.abspath(work)
        sample = json.load(open(os.path.join(here, item["sample"]), encoding="utf-8"))
        escaped = False
        for path, text in sample.items():
            dest = os.path.abspath(os.path.join(root, path))
            if not dest.startswith(root + os.sep):
                escaped = True
                break
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            open(dest, "w", encoding="utf-8", newline="").write(text)
        if escaped:
            print("FAIL " + item["title"] + " (the sample names a file outside its folder)")
            failed += 1
            continue
        env = dict(os.environ, PYTHONPATH=root, PYTHONDONTWRITEBYTECODE="1")
        env.pop("NODE_OPTIONS", None)
        env.pop("NODE_PATH", None)
        built = tempfile.TemporaryDirectory()
        if language == "rust":
            info = item["rust"]
            base = ["--cap-lints", "allow", "-C", "opt-level=0"]
            lib = os.path.join(built.name, "lib" + info["name"] + ".rlib")
            exe = os.path.join(built.name, "repro" + (".exe" if os.name == "nt" else ""))
            first = subprocess.run([rustc] + base + ["--edition", info["edition"], "--crate-type", "lib", "--crate-name", info["name"], "-o", lib, os.path.join(root, info["root"])], capture_output=True, text=True, encoding="utf-8")
            second = first if first.returncode != 0 else subprocess.run([rustc] + base + ["--edition", "2021", "--extern", "project=" + lib, "-o", exe, os.path.join(here, item["reproducer"])], capture_output=True, text=True, encoding="utf-8")
            if second.returncode != 0:
                print("FAIL " + item["title"] + " (the crate or the script did not build with this Rust compiler)")
                failed += 1
                built.cleanup()
                continue
            argv = [exe]
        out = subprocess.run(argv, cwd=root, env=env, capture_output=True, text=True, encoding="utf-8")
        built.cleanup()
        expected = open(os.path.join(here, item["expected"]), encoding="utf-8", newline="").read()
        ok = out.returncode == 0 and out.stdout == expected
        print(("OK   " if ok else "FAIL ") + item["title"])
        failed += 0 if ok else 1
sys.exit(1 if failed else (2 if missing else 0))
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
    kk_secrets::looks_secret(text).is_some()
}

/// Everything from a withheld file that must not appear in a release, in the forms a leak can take: whole long tokens,
/// the values of assignments (any length from 6, since a short password is still a password), and for values of at
/// least 8 characters their base64, hex, percent-encoded and reversed forms and their URL pieces.
fn withheld_needles(withheld: &[&ImportedFile]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for f in withheld {
        let text = String::from_utf8_lossy(&f.bytes).to_string();
        let mut push = |needle: String| {
            if !out.iter().any(|(p, n)| *p == f.path && *n == needle) {
                out.push((f.path.clone(), needle));
            }
        };
        for token in text.split(|c: char| {
            c.is_whitespace() || matches!(c, '=' | '"' | '\'' | ',' | ';' | '(' | ')')
        }) {
            // a variable's NAME (PAYMENTS_API_KEY) is not its value; only values are withheld
            let env_name = token
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
            if token.len() >= 12 && !env_name {
                push(token.to_string());
            }
        }
        for line in text.lines() {
            let Some((_, value)) = line.split_once('=').or_else(|| line.split_once(": ")) else {
                continue;
            };
            let v = value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'' || c == '`');
            if v.len() >= 6 && !v.contains(char::is_whitespace) {
                push(v.to_string());
            }
            for variant in kk_secrets::variants(v, 8) {
                push(variant);
            }
            for piece in kk_secrets::pieces(v, 6) {
                for variant in kk_secrets::variants(&piece, 6) {
                    push(variant);
                }
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
        if names_sandbox_path(&finding.reproducer) {
            not_confirmed.push((
                finding.title.clone(),
                "the script names a path that exists only inside the sandbox, so the recipient could not re-run it"
                    .into(),
            ));
            continue;
        }
        let rust_plan = if finding.language == Language::Rust {
            match crate::rust::rust_crate(&finding.file, project_files) {
                Ok(plan) => Some(plan),
                Err(_) => {
                    not_confirmed.push((
                        finding.title.clone(),
                        "the crate could not be described for the recipient".into(),
                    ));
                    continue;
                }
            }
        } else {
            None
        };
        n += 1;
        let (script_name, how) = match finding.language {
            Language::Python => (
                format!("repro-{n}.py"),
                format!("run run-check.py (Python 3). It rebuilds the minimal sample from sample-{n}.json, runs repro-{n}.py and compares the output with expected-{n}.txt."),
            ),
            Language::JavaScript => (
                format!("repro-{n}.mjs"),
                format!("run run-check.py (Python 3, with Node.js 22 or newer on the path). It rebuilds the minimal sample from sample-{n}.json, runs repro-{n}.mjs with Node.js and compares the output with expected-{n}.txt."),
            ),
            Language::Rust => (
                format!("repro-{n}.rs"),
                format!("run run-check.py (Python 3, with the Rust compiler rustc on the path). It rebuilds the minimal sample from sample-{n}.json, builds the project's library crate and repro-{n}.rs with rustc, runs the program and compares the output with expected-{n}.txt."),
            ),
            Language::TypeScript => (
                format!("repro-{n}.mts"),
                format!("run run-check.py (Python 3, with the official Node.js 22.18 or newer on the path: it runs TypeScript itself, with no build step). It rebuilds the minimal sample from sample-{n}.json, runs repro-{n}.mts with Node.js and compares the output with expected-{n}.txt."),
            ),
        };
        let bound: String = check
            .sample_paths
            .iter()
            .map(|p| format!("  {}  sha256 {}\n", p, project_files[p].sha256))
            .collect();
        let text = format!(
            "Finding {n}: {}\nLocation: {}:{}\n\n{}\n\nHow to check: {how}\n\nWhat this establishes: the script printed CONTROL_OK (normal behaviour works), then REPRODUCED, twice with identical output, and did not print REPRODUCED when the cited file was emptied. It was checked on exactly these files and nothing else:\n{}It does not establish that the defect exists in any other version of the code.\n",
            clean_text(&finding.title, 200),
            finding.file,
            finding.line,
            clean_text(&finding.explanation, 4000),
            bound
        );
        files.insert(format!("finding-{n}.txt"), text.into_bytes());
        files.insert(script_name.clone(), finding.reproducer.clone().into_bytes());
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
        let mut entry = vec![
            ("title".into(), Value::Str(clean_text(&finding.title, 200))),
            ("sample".into(), Value::Str(format!("sample-{n}.json"))),
            ("reproducer".into(), Value::Str(script_name)),
            ("expected".into(), Value::Str(format!("expected-{n}.txt"))),
        ];
        if finding.language != Language::Python {
            entry.push((
                "language".into(),
                Value::Str(finding.language.as_str().into()),
            ));
        }
        if let Some(plan) = rust_plan {
            // what the recipient needs to build the crate the script links against: its root file, edition and name
            entry.push((
                "rust".into(),
                Value::Object(vec![
                    ("root".into(), Value::Str(plan.root)),
                    ("edition".into(), Value::Str(plan.edition)),
                    ("name".into(), Value::Str(plan.crate_name)),
                ]),
            ));
        }
        index_entries.push(Value::Object(entry));
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
    let lines = withheld_needles(&withheld);
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

/// The capsule claims for a draft, read from the draft's own `index.json`, so each claim names exactly the files that were
/// released for that finding (the script is `repro-N.py` or `repro-N.mjs`) whatever was left out along the way.
pub fn claims_for(draft: &ReleaseDraft) -> Vec<kk_evidence::capsule::ClaimSpec> {
    let Some(bytes) = draft.files.get("index.json") else {
        return Vec::new();
    };
    let Ok(index) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Vec::new();
    };
    let Some(entries) = index["findings"].as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let get = |k: &str| e[k].as_str().map(str::to_string);
        let (Some(title), Some(script), Some(expected), Some(sample)) = (
            get("title"),
            get("reproducer"),
            get("expected"),
            get("sample"),
        ) else {
            return Vec::new();
        };
        let finding_file = format!("finding-{}.txt", i + 1);
        let evidence = vec![finding_file, script, expected, sample];
        if evidence.iter().any(|n| !draft.files.contains_key(n)) {
            return Vec::new();
        }
        let how_to_check = match e["language"].as_str() {
            Some("javascript") => "Run run-check.py with Python 3, with Node.js 22 or newer on the path: it rebuilds the minimal sample, runs the Node.js script and compares the output with the expected output file.",
            Some("rust") => "Run run-check.py with Python 3, with the Rust compiler rustc on the path: it rebuilds the minimal sample, builds the project's library crate and the Rust script with rustc, runs the program and compares the output with the expected output file.",
            Some("typescript") => "Run run-check.py with Python 3, with the official Node.js 22.18 or newer on the path: it rebuilds the minimal sample, runs the TypeScript script with Node.js itself and compares the output with the expected output file.",
            _ => "Run run-check.py with Python 3: it rebuilds the minimal sample and compares the output with the expected output file.",
        };
        out.push(kk_evidence::capsule::ClaimSpec {
            statement: title,
            evidence,
            how_to_check: how_to_check.into(),
        });
    }
    out
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
