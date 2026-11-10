use kk_evidence::capsule::{self, ClaimSpec, VerifyOptions};
use kk_evidence::keys::Signer;
use kk_review::check::{CheckResult, CheckStatus, Finding};
use kk_review::import::{scan_tree, ImportedFile};
use kk_review::release::{
    assemble, build_capsule, release_digest, Lease, LeaseError, LeaseState, Rejection,
};
use kk_types::label::{Label, Level};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

fn files() -> Vec<ImportedFile> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dst = std::env::temp_dir().join(format!(
        "kk-release-fixture-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dst);
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
        &dst,
    );
    scan_tree(&dst, Level::Internal).files
}

fn index(f: &[ImportedFile]) -> BTreeMap<String, &ImportedFile> {
    f.iter().map(|x| (x.path.clone(), x)).collect()
}

const REPRO: &str = "from payments.auth import session\nt = session.login('alice')\nassert session.validate(t)\nprint('CONTROL_OK')\nsession.logout(t)\nprint('REPRODUCED' if session.validate(t) else 'NOT REPRODUCED')\n";

fn finding() -> Finding {
    Finding {
        title: "Token not revoked".into(),
        file: "payments/auth/session.py".into(),
        line: 15,
        explanation:
            "logout() only clears a local variable, so validate() still accepts the token.".into(),
        reproducer: REPRO.into(),
        language: kk_review::check::Language::Python,
    }
}

fn confirmed() -> CheckResult {
    CheckResult {
        status: CheckStatus::Reproduced,
        stdout: "CONTROL_OK\nREPRODUCED\n".into(),
        sample_paths: vec![
            "payments/__init__.py".into(),
            "payments/auth/__init__.py".into(),
            "payments/auth/session.py".into(),
            "payments/auth/tokens.py".into(),
        ],
    }
}

#[test]
fn rs01_a_release_is_a_fixed_set_of_files_and_the_recipient_can_check_it_with_python_alone() {
    let f = files();
    let idx = index(&f);
    let draft = assemble(
        &[(finding(), confirmed())],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("assemble");
    let names: Vec<&str> = draft.files.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        vec![
            "expected-1.txt",
            "finding-1.txt",
            "index.json",
            "repro-1.py",
            "run-check.py",
            "sample-1.json"
        ]
    );
    assert!(draft.removed.iter().any(|r| r.what.contains("withheld")));
    let dir = std::env::temp_dir().join(format!("kk-recipient-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (n, b) in &draft.files {
        fs::write(dir.join(n), b).unwrap();
    }
    let out = Command::new("python3")
        .arg(dir.join("run-check.py"))
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("OK   Token not revoked"));
    fs::write(dir.join("repro-1.py"), REPRO.replace("logout(t)", "pass")).unwrap();
    let tampered = Command::new("python3")
        .arg(dir.join("run-check.py"))
        .output()
        .unwrap();
    assert!(
        !tampered.status.success(),
        "a changed reproducer must fail the recipient's check"
    );
}

#[test]
fn rs02_the_digest_covers_every_byte_the_recipient_and_the_expiry() {
    let f = files();
    let idx = index(&f);
    let d = assemble(
        &[(finding(), confirmed())],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .unwrap();
    let again = assemble(
        &[(finding(), confirmed())],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .unwrap();
    assert_eq!(d.digest, again.digest);
    assert_ne!(
        d.digest,
        assemble(
            &[(finding(), confirmed())],
            &idx,
            "other-firm",
            "policy-1",
            1_700_000_000,
            86_400
        )
        .unwrap()
        .digest
    );
    assert_ne!(
        d.digest,
        assemble(
            &[(finding(), confirmed())],
            &idx,
            "acme-audit",
            "policy-2",
            1_700_000_000,
            86_400
        )
        .unwrap()
        .digest
    );
    assert_ne!(
        d.digest,
        assemble(
            &[(finding(), confirmed())],
            &idx,
            "acme-audit",
            "policy-1",
            1_700_000_000,
            3_600
        )
        .unwrap()
        .digest
    );
    let mut changed = d.files.clone();
    changed.get_mut("finding-1.txt").unwrap().push(b' ');
    assert_ne!(
        d.digest,
        release_digest(&d.recipient, &d.policy_version, d.expires, &changed)
    );
}

#[test]
fn rs03_text_from_withheld_files_and_secret_markers_can_never_be_released() {
    let f = files();
    let idx = index(&f);
    let mut leak = finding();
    leak.explanation = "The database url is postgres://payments:hunter2@db.internal:5432/payments in the environment.".into();
    let r = assemble(&[(leak, confirmed())], &idx, "acme", "p", 0, 60);
    assert!(
        matches!(
            r,
            Err(Rejection::ContainsWithheldText { .. }) | Err(Rejection::ContainsSecret { .. })
        ),
        "a withheld URL must be stopped by the withheld-text check or the shared secret detector"
    );
    let mut marker = finding();
    marker.explanation = format!("Key {}{} was found.", "sk_live", "_abcdefghijklmnop");
    assert!(matches!(
        assemble(&[(marker, confirmed())], &idx, "acme", "p", 0, 60),
        Err(Rejection::ContainsSecret { .. })
    ));
    let mut in_repro = finding();
    let secret_line = ["PAYMENTS_API_KEY=sk_live", "_51NfakeFAKEfakeFAKE0000000000"].concat();
    in_repro.reproducer = ["# ", secret_line.as_str(), "\n", REPRO].concat();
    assert!(assemble(&[(in_repro, confirmed())], &idx, "acme", "p", 0, 60).is_err());
}

#[test]
fn rs04_only_confirmed_findings_are_released_and_the_rest_are_listed_for_the_owner() {
    let f = files();
    let idx = index(&f);
    let mut unconfirmed = finding();
    unconfirmed.title = "Random jitter".into();
    let not_ok = CheckResult {
        status: CheckStatus::NotReproduced,
        stdout: "NOT REPRODUCED\n".into(),
        sample_paths: vec![],
    };
    let d = assemble(
        &[
            (finding(), confirmed()),
            (unconfirmed.clone(), not_ok.clone()),
        ],
        &idx,
        "acme",
        "p",
        0,
        60,
    )
    .unwrap();
    assert!(!d
        .files
        .values()
        .any(|b| String::from_utf8_lossy(b).contains("Random jitter")));
    assert_eq!(d.not_confirmed.len(), 1);
    assert!(matches!(
        assemble(&[(unconfirmed, not_ok)], &idx, "acme", "p", 0, 60),
        Err(Rejection::NothingConfirmed)
    ));
}

#[test]
fn rs05_titles_and_explanations_lose_control_and_direction_override_characters() {
    let f = files();
    let idx = index(&f);
    let mut x = finding();
    x.title = "Safe\u{202e}gnirts\u{0007}".into();
    let d = assemble(&[(x, confirmed())], &idx, "acme", "p", 0, 60).unwrap();
    let text = String::from_utf8_lossy(&d.files["finding-1.txt"]).to_string();
    assert!(
        !text.contains('\u{202e}') && !text.contains('\u{0007}'),
        "{text}"
    );
}

#[test]
fn rs06_the_lease_binds_approval_to_the_exact_bytes_and_delivery_rechecks_them() {
    let f = files();
    let idx = index(&f);
    let draft = assemble(&[(finding(), confirmed())], &idx, "acme", "p", 1000, 600).unwrap();
    let mut lease = Lease::preview(&draft);
    assert_eq!(
        lease.approve("abc", 1100),
        Err(LeaseError::AcknowledgementTooShort)
    );
    assert_eq!(
        lease.approve("000000000000", 1100),
        Err(LeaseError::DigestMismatch)
    );
    assert_eq!(
        lease.approve(&draft.digest[..12], 1700),
        Err(LeaseError::Expired)
    );
    lease.approve(&draft.digest[..12], 1100).unwrap();
    assert_eq!(
        lease.approve(&draft.digest[..12], 1100),
        Err(LeaseError::WrongState(LeaseState::Approved))
    );
    let mut tampered = draft.clone();
    tampered
        .files
        .get_mut("repro-1.py")
        .unwrap()
        .extend_from_slice(b"# changed after approval\n");
    let mut lease_t = lease.clone();
    assert_eq!(
        lease_t.begin_delivery(&tampered, 1200),
        Err(LeaseError::DigestMismatch),
        "bytes changed after approval"
    );
    assert_eq!(lease_t.state, LeaseState::Approved);
    let mut redirected = draft.clone();
    redirected.recipient = "someone-else".into();
    assert_eq!(
        lease.clone().begin_delivery(&redirected, 1200),
        Err(LeaseError::DigestMismatch)
    );
    assert_eq!(
        lease.clone().begin_delivery(&draft, 1700),
        Err(LeaseError::Expired)
    );
    lease.begin_delivery(&draft, 1200).unwrap();
    let mut crashed = lease.clone();
    crashed.recover_after_crash();
    assert_eq!(
        crashed.state,
        LeaseState::Uncertain,
        "a crash mid-delivery is unknown, never success"
    );
    lease.complete().unwrap();
    assert_eq!(lease.state, LeaseState::Delivered);
    let mut other = Lease::preview(&draft);
    other.revoke();
    assert_eq!(
        other.approve(&draft.digest[..12], 1100),
        Err(LeaseError::WrongState(LeaseState::Revoked))
    );
}

#[test]
fn rs07_a_released_draft_becomes_a_capsule_that_both_verifiers_accept_and_that_tampering_breaks() {
    let f = files();
    let idx = index(&f);
    let draft = assemble(
        &[(finding(), confirmed())],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .unwrap();
    let signer = Signer::from_seed([5u8; 32]);
    let claim = ClaimSpec {
        statement: "Session tokens remain valid after logout.".into(),
        evidence: vec![
            "finding-1.txt".into(),
            "repro-1.py".into(),
            "expected-1.txt".into(),
            "sample-1.json".into(),
        ],
        how_to_check: "Run run-check.py with Python 3.".into(),
    };
    let bytes = build_capsule(
        &draft,
        &signer,
        &"ab".repeat(32),
        38,
        "device_only",
        &[claim],
        &[],
    );
    let opts = VerifyOptions {
        pinned_keys: Some(vec![signer.public()]),
        now: 1_700_000_100,
        expected_recipient: Some("acme-audit".into()),
    };
    let report = capsule::verify(&bytes, &opts);
    assert!(report.valid, "{report:?}");
    assert_eq!(report.binding_level.as_deref(), Some("B1"));
    assert!(report
        .not_established
        .iter()
        .any(|n| n.contains("private source repository")));
    let mut tampered = bytes.clone();
    let marker = b"\"repro-1.py\":\"";
    let pos = tampered
        .windows(marker.len())
        .rposition(|w| w == marker)
        .unwrap()
        + marker.len();
    tampered[pos] = if tampered[pos] == b'A' { b'B' } else { b'A' };
    assert!(!capsule::verify(&tampered, &opts).valid);
}

fn b64(s: &str) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in s.as_bytes().chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
        }
    }
    out
}

#[test]
fn rs08_withheld_values_cannot_leave_in_a_transformed_or_partial_form() {
    // Audit finding (matrix F, S6): only exact whole tokens of 12 characters or more were checked, so a short password,
    // a fragment of a URL, or an encoded copy of a secret passed.
    let f = files();
    let idx = index(&f);
    let key = ["sk_live", "_51NfakeFAKEfakeFAKE0000000000"].concat();
    let hex: String = key.bytes().map(|b| format!("{b:02x}")).collect();
    let rev: String = key.chars().rev().collect();
    let attacks: Vec<(&str, String)> = vec![
        (
            "short password alone",
            "The service logs in with the password hunter2.".to_string(),
        ),
        (
            "url password piece",
            "The credential is the part between the colon and the at sign: hunter2.".to_string(),
        ),
        (
            "base64 of the key",
            format!("Look at this blob: {}", b64(&key)),
        ),
        (
            "base64 of the password",
            format!("token {}", b64("hunter2")),
        ),
        ("hex of the key", format!("dump: {hex}")),
        ("reversed key", format!("mirror: {rev}")),
        (
            "percent-encoded url",
            "see postgres%3A%2F%2Fpayments%3Ahunter2%40db.internal".to_string(),
        ),
    ];
    let mut leaked = Vec::new();
    for (name, text) in attacks {
        let mut fnd = finding();
        fnd.explanation = text;
        let r = assemble(&[(fnd, confirmed())], &idx, "acme", "p", 0, 60);
        if r.is_ok() {
            leaked.push(name);
        }
    }
    assert!(leaked.is_empty(), "these leaked: {leaked:?}");
    let mut ok = finding();
    ok.explanation = "The variable PAYMENTS_API_KEY is read from the environment and is never validated before use.".into();
    assert!(
        assemble(&[(ok, confirmed())], &idx, "acme", "p", 0, 60).is_ok(),
        "naming a variable is not a leak"
    );
}

#[test]
fn rs09_provider_secrets_that_the_old_five_markers_missed_are_rejected_at_release() {
    let f = files();
    let idx = index(&f);
    for (name, secret) in [
        (
            "restricted stripe key",
            ["rk_live", "_", "abcdEFGH1234ijklMNOP5678"].concat(),
        ),
        (
            "openai key",
            [
                "sk",
                "-proj-",
                "aB3dE5gH7jK9mN2pQ4rS6tU8vW1xY0zCfLhJkMnPqRsTuVwX",
            ]
            .concat(),
        ),
        (
            "anthropic key",
            [
                "sk",
                "-ant-api03-",
                "aB3dE5gH7jK9mN2pQ4rS6tU8vW1xY0zCfLhJkMnPqRsTuVwXyZaB3dE5gH7jK9mN2pQ4rS6tU8vW1xY",
            ]
            .concat(),
        ),
        (
            "near key",
            format!(
                "ed25519:{}",
                &"5Jr8Qk2mN7pXvB4cD9eF3gH6jK1LzW8yTuRsAqPo3nMbVc2XdZ4aSfGhJk7LpQwErTy9Uip"
                    .repeat(2)[..88]
            ),
        ),
        (
            "jwt",
            [
                "eyJ",
                "hbGciOiJIUzI1NiJ9.",
                "eyJ",
                "zdWIiOiIxMjM0NTY3ODkwIn0.",
                "dBjftJeZ4CVPmB92K27uhbUJU1p1r",
            ]
            .concat(),
        ),
        ("basic auth blob", b64("deploy:Zk9fQ2x8Lm4vPq7R")),
    ] {
        let mut fnd = finding();
        fnd.explanation = format!("The code contains {secret} in plain text.");
        let r = assemble(&[(fnd, confirmed())], &idx, "acme", "p", 0, 60);
        assert!(r.is_err(), "{name} must be rejected");
    }
}

#[test]
fn rs10_short_values_from_a_withheld_file_are_stopped_even_when_they_are_not_in_a_url() {
    let mut f = files();
    f.push(ImportedFile {
        path: "ops/.env.admin".into(),
        bytes: b"ADMIN_PASSWORD=hunter9x\nDEPLOY_NOTE=ab12cd\n".to_vec(),
        label: Label::new(Level::Restricted).with_compartment("secrets"),
        sha256: "0".repeat(64),
        notes: vec![],
    });
    let idx = index(&f);
    for text in [
        "The admin password is hunter9x and it never rotates.",
        "Deploy note says ab12cd.",
    ] {
        let mut fnd = finding();
        fnd.explanation = text.into();
        assert!(
            assemble(&[(fnd, confirmed())], &idx, "acme", "p", 0, 60).is_err(),
            "{text}"
        );
    }
    let mut ok = finding();
    ok.explanation = "ADMIN_PASSWORD is read from the environment.".into();
    assert!(assemble(&[(ok, confirmed())], &idx, "acme", "p", 0, 60).is_ok());
}

fn js_project() -> Vec<ImportedFile> {
    let label = Label::new(Level::Internal);
    let mk = |path: &str, text: &str| {
        let bytes = text.as_bytes().to_vec();
        ImportedFile {
            path: path.into(),
            sha256: kk_evidence::capsule::sha256_hex(&bytes),
            bytes,
            label: label.clone(),
            notes: vec![],
        }
    };
    vec![
        mk("src/auth/tokens.js", "let n = 0;\nexports.make = (u) => `${u}-${++n}-é`;\n"),
        mk("src/auth/session.js", "const tokens = require('./tokens');\nconst ACTIVE = new Set();\nfunction login(u) { const t = tokens.make(u); ACTIVE.add(t); return t; }\n// logout only clears a local copy, so the token stays valid\nfunction logout(t) { let copy = t; copy = null; return copy; }\nfunction validate(t) { return ACTIVE.has(t); }\nmodule.exports = { login, logout, validate };\n"),
    ]
}

const JS_RELEASE_REPRO: &str = "import { createRequire } from 'node:module';\nconst require = createRequire(process.cwd() + '/');\nconst s = require('./src/auth/session.js');\nconst t = s.login('alice');\nif (!s.validate(t)) throw new Error('control');\nconsole.log('CONTROL_OK');\ns.logout(t);\nconsole.log(s.validate(t) ? 'REPRODUCED' : 'NOT REPRODUCED');\n";

fn js_finding_for_release(reproducer: &str) -> Finding {
    Finding {
        title: "JS token not revoked".into(),
        file: "src/auth/session.js".into(),
        line: 5,
        explanation: "logout only clears a local copy, so validate() still accepts the token."
            .into(),
        reproducer: reproducer.into(),
        language: kk_review::check::Language::JavaScript,
    }
}

fn js_confirmed() -> CheckResult {
    CheckResult {
        status: CheckStatus::Reproduced,
        stdout: "CONTROL_OK\nREPRODUCED\n".into(),
        sample_paths: vec!["src/auth/session.js".into(), "src/auth/tokens.js".into()],
    }
}

fn write_release(draft: &kk_review::release::ReleaseDraft, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kk-recipient-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (n, b) in &draft.files {
        fs::write(dir.join(n), b).unwrap();
    }
    dir
}

fn recipient(dir: &Path, path_env: Option<&str>) -> std::process::Output {
    let mut c = Command::new("/usr/bin/python3");
    c.arg(dir.join("run-check.py"));
    if let Some(p) = path_env {
        c.env("PATH", p);
    }
    c.output().expect("python3")
}

fn text(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn rj01_a_javascript_finding_is_released_as_a_node_script_the_recipient_can_re_run() {
    let proj = js_project();
    let idx = index(&proj);
    let draft = assemble(
        &[(js_finding_for_release(JS_RELEASE_REPRO), js_confirmed())],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("assemble");
    let names: Vec<&str> = draft.files.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        vec![
            "expected-1.txt",
            "finding-1.txt",
            "index.json",
            "repro-1.mjs",
            "run-check.py",
            "sample-1.json"
        ]
    );
    let readme = String::from_utf8_lossy(&draft.files["finding-1.txt"]).into_owned();
    assert!(
        readme.contains("Node.js") && readme.contains("repro-1.mjs"),
        "{readme}"
    );
    let index_json = String::from_utf8_lossy(&draft.files["index.json"]).into_owned();
    assert!(
        index_json.contains("\"language\":\"javascript\""),
        "{index_json}"
    );
    let dir = write_release(&draft, "js");
    let ok = recipient(&dir, None);
    assert!(ok.status.success(), "{}", text(&ok));
    assert!(
        text(&ok).contains("OK   JS token not revoked"),
        "{}",
        text(&ok)
    );
    // the recipient's own NODE_OPTIONS / NODE_PATH must not change what the check runs
    let hostile_env = Command::new("/usr/bin/python3")
        .arg(dir.join("run-check.py"))
        .env("NODE_OPTIONS", "--no-such-node-flag")
        .env("NODE_PATH", "/nonexistent")
        .output()
        .unwrap();
    assert!(hostile_env.status.success(), "{}", text(&hostile_env));
    // a module that exists only on the recipient's NODE_PATH must not be found: the check runs against the sample alone
    let extra = dir.join("elsewhere");
    fs::create_dir_all(extra.join("fakepkg")).unwrap();
    fs::write(extra.join("fakepkg/index.js"), "module.exports = 1;\n").unwrap();
    let kept = fs::read_to_string(dir.join("repro-1.mjs")).unwrap();
    let kept_expected = fs::read_to_string(dir.join("expected-1.txt")).unwrap();
    fs::write(dir.join("repro-1.mjs"), "import { createRequire } from 'node:module';\ncreateRequire(process.cwd() + '/')('fakepkg');\nconsole.log('CONTROL_OK');\nconsole.log('REPRODUCED');\n").unwrap();
    let leaked = Command::new("/usr/bin/python3")
        .arg(dir.join("run-check.py"))
        .env("NODE_PATH", &extra)
        .output()
        .unwrap();
    assert_eq!(
        leaked.status.code(),
        Some(1),
        "NODE_PATH reached the check: {}",
        text(&leaked)
    );
    fs::write(dir.join("repro-1.mjs"), kept).unwrap();
    fs::write(dir.join("expected-1.txt"), kept_expected).unwrap();
    // a changed script, a changed sample and a changed expected output each fail the check
    let original = fs::read_to_string(dir.join("repro-1.mjs")).unwrap();
    fs::write(
        dir.join("repro-1.mjs"),
        original.replace("'REPRODUCED'", "'REPRODUCED?'"),
    )
    .unwrap();
    assert_eq!(recipient(&dir, None).status.code(), Some(1));
    fs::write(dir.join("repro-1.mjs"), &original).unwrap();
    let sample = fs::read_to_string(dir.join("sample-1.json")).unwrap();
    fs::write(
        dir.join("sample-1.json"),
        sample.replace("return ACTIVE.has(t)", "return false"),
    )
    .unwrap();
    assert_eq!(recipient(&dir, None).status.code(), Some(1));
    fs::write(dir.join("sample-1.json"), &sample).unwrap();
    fs::write(dir.join("expected-1.txt"), "CONTROL_OK\n").unwrap();
    assert_eq!(recipient(&dir, None).status.code(), Some(1));
}

#[test]
fn rj02_without_node_the_recipient_is_told_so_and_python_findings_are_still_checked() {
    let mut proj = files();
    proj.extend(js_project());
    let idx = index(&proj);
    let draft = assemble(
        &[
            (finding(), confirmed()),
            (js_finding_for_release(JS_RELEASE_REPRO), js_confirmed()),
        ],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("assemble");
    assert!(draft.files.contains_key("repro-1.py") && draft.files.contains_key("repro-2.mjs"));
    let dir = write_release(&draft, "mixed");
    let both = recipient(&dir, None);
    assert!(both.status.success(), "{}", text(&both));
    assert!(
        text(&both).contains("OK   Token not revoked")
            && text(&both).contains("OK   JS token not revoked")
    );
    // a PATH with nothing in it, whatever this machine's layout (python is started by its full path)
    let empty = std::env::temp_dir().join(format!("kk-empty-path-{}", std::process::id()));
    fs::create_dir_all(&empty).unwrap();
    let none = recipient(&dir, Some(empty.to_str().unwrap()));
    assert_eq!(none.status.code(), Some(2), "{}", text(&none));
    let t = text(&none);
    assert!(t.contains("OK   Token not revoked"), "{t}");
    assert!(
        t.contains("MISSING JS token not revoked") && t.contains("Node.js"),
        "{t}"
    );
    assert!(!t.contains("OK   JS token"), "{t}");
}

#[test]
fn rj03_a_script_that_names_the_sandbox_path_is_not_released_and_the_sample_cannot_escape_its_folder(
) {
    let proj = js_project();
    let idx = index(&proj);
    let hardcoded = JS_RELEASE_REPRO.replace(
        "createRequire(process.cwd() + '/')",
        "createRequire('/sample/')",
    );
    let r = assemble(
        &[(js_finding_for_release(&hardcoded), js_confirmed())],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    );
    assert!(matches!(r, Err(Rejection::NothingConfirmed)), "{r:?}");
    let mut proj = files();
    proj.extend(js_project());
    let idx = index(&proj);
    let draft = assemble(
        &[
            (js_finding_for_release(&hardcoded), js_confirmed()),
            (finding(), confirmed()),
        ],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("the other finding still goes out");
    assert_eq!(draft.not_confirmed.len(), 1);
    assert!(
        draft.not_confirmed[0].1.contains("sandbox"),
        "{:?}",
        draft.not_confirmed
    );
    // the recipient's runner refuses a sample path that leaves its working folder
    let dir = write_release(&draft, "escape");
    let victim = dir
        .parent()
        .unwrap()
        .join(format!("kk-escape-{}.txt", std::process::id()));
    let _ = fs::remove_file(&victim);
    let sample = format!("{{\"../kk-escape-{}.txt\":\"pwned\"}}", std::process::id());
    fs::write(dir.join("sample-1.json"), sample).unwrap();
    let o = recipient(&dir, None);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        !victim.exists(),
        "a file was written outside the working folder"
    );
}

#[test]
fn rj04_the_capsule_claims_name_exactly_the_files_that_were_released_for_each_language() {
    let mut proj = files();
    proj.extend(js_project());
    let idx = index(&proj);
    let hardcoded = JS_RELEASE_REPRO.replace(
        "createRequire(process.cwd() + '/')",
        "createRequire('/sample/')",
    );
    // order: a Python finding, one that is left out, a JavaScript finding, another Python finding
    let second_python = Finding {
        title: "Second Python finding".into(),
        ..finding()
    };
    let draft = assemble(
        &[
            (finding(), confirmed()),
            (js_finding_for_release(&hardcoded), js_confirmed()),
            (js_finding_for_release(JS_RELEASE_REPRO), js_confirmed()),
            (second_python, confirmed()),
        ],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("assemble");
    let claims = kk_review::release::claims_for(&draft);
    assert_eq!(claims.len(), 3);
    let titles: Vec<&str> = claims.iter().map(|c| c.statement.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Token not revoked",
            "JS token not revoked",
            "Second Python finding"
        ]
    );
    for c in &claims {
        assert_eq!(c.evidence.len(), 4);
        for name in &c.evidence {
            assert!(
                draft.files.contains_key(name),
                "{name} is not in the release"
            );
        }
    }
    assert_eq!(
        claims[0].evidence,
        [
            "finding-1.txt",
            "repro-1.py",
            "expected-1.txt",
            "sample-1.json"
        ]
    );
    assert_eq!(
        claims[1].evidence,
        [
            "finding-2.txt",
            "repro-2.mjs",
            "expected-2.txt",
            "sample-2.json"
        ]
    );
    assert!(
        claims[1].how_to_check.contains("Node.js"),
        "{}",
        claims[1].how_to_check
    );
    assert!(
        !claims[0].how_to_check.contains("Node.js"),
        "{}",
        claims[0].how_to_check
    );
    assert!(claims[0].how_to_check.contains("run-check.py"));
    // a claim that names a file the release does not hold is never made
    let mut missing_script = draft.clone();
    missing_script.files.remove("repro-2.mjs");
    assert!(kk_review::release::claims_for(&missing_script).is_empty());
    let mut missing_sample = draft.clone();
    missing_sample.files.remove("sample-3.json");
    assert!(kk_review::release::claims_for(&missing_sample).is_empty());
    // a draft with no usable index makes no claims rather than wrong ones
    let mut broken = draft.clone();
    broken
        .files
        .insert("index.json".into(), b"not json".to_vec());
    assert!(kk_review::release::claims_for(&broken).is_empty());
}

fn ts_project() -> Vec<ImportedFile> {
    let label = Label::new(Level::Internal);
    let mk = |path: &str, text: &str| {
        let bytes = text.as_bytes().to_vec();
        ImportedFile {
            path: path.into(),
            sha256: kk_evidence::capsule::sha256_hex(&bytes),
            bytes,
            label: label.clone(),
            notes: vec![],
        }
    };
    vec![
        mk("src/auth/tokens.ts", "let n = 0;\nexport const make = (u: string): string => `${u}-${++n}-é`;\n"),
        mk("src/auth/session.ts", "import { make } from './tokens.ts';\nconst ACTIVE: Set<string> = new Set();\nexport function login(u: string): string { const t = make(u); ACTIVE.add(t); return t; }\n// logout only clears a local copy, so the token stays valid\nexport function logout(t: string): null { let copy: string | null = t; copy = null; return copy; }\nexport function validate(t: string): boolean { return ACTIVE.has(t); }\n"),
    ]
}

const TS_RELEASE_REPRO: &str = "const s = await import(process.cwd() + '/src/auth/session.ts');\nconst t: string = s.login('alice');\nif (!s.validate(t)) throw new Error('control');\nconsole.log('CONTROL_OK');\ns.logout(t);\nconsole.log(s.validate(t) ? 'REPRODUCED' : 'NOT REPRODUCED');\n";

fn ts_finding_for_release(reproducer: &str) -> Finding {
    Finding {
        title: "TS token not revoked".into(),
        file: "src/auth/session.ts".into(),
        line: 5,
        explanation: "logout only clears a local copy, so validate() still accepts the token."
            .into(),
        reproducer: reproducer.into(),
        language: kk_review::check::Language::TypeScript,
    }
}

fn ts_confirmed() -> CheckResult {
    CheckResult {
        status: CheckStatus::Reproduced,
        stdout: "CONTROL_OK\nREPRODUCED\n".into(),
        sample_paths: vec!["src/auth/session.ts".into(), "src/auth/tokens.ts".into()],
    }
}

fn host_node_has_typescript() -> bool {
    Command::new("node")
        .args(["-p", "process.features.typescript"])
        .output()
        .ok()
        .is_some_and(|o| {
            o.status.success()
                && kk_review::check::node_has_typescript(&String::from_utf8_lossy(&o.stdout))
        })
}

/// A `node` that answers the capability question the way a build without the TypeScript parser does.
fn node_without_typescript() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kk-fake-node-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("node");
    fs::write(&script, "#!/bin/sh\necho false\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

#[test]
fn rj05_a_typescript_finding_is_released_as_an_mts_script_and_the_recipient_without_typescript_is_told_so(
) {
    assert!(
        host_node_has_typescript(),
        "this test runs the recipient's check with the host's Node.js: install the official Node.js 22.18 or newer from nodejs.org (a packaged build may leave TypeScript out)"
    );
    let mut proj = files();
    proj.extend(ts_project());
    let idx = index(&proj);
    let draft = assemble(
        &[
            (finding(), confirmed()),
            (ts_finding_for_release(TS_RELEASE_REPRO), ts_confirmed()),
        ],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("assemble");
    assert!(draft.files.contains_key("repro-1.py") && draft.files.contains_key("repro-2.mts"));
    let readme = String::from_utf8_lossy(&draft.files["finding-2.txt"]).into_owned();
    assert!(
        readme.contains("22.18")
            && readme.contains("repro-2.mts")
            && readme.contains("no build step"),
        "{readme}"
    );
    let index_json = String::from_utf8_lossy(&draft.files["index.json"]).into_owned();
    assert!(
        index_json.contains("\"language\":\"typescript\""),
        "{index_json}"
    );
    let dir = write_release(&draft, "ts");
    let both = recipient(&dir, None);
    assert!(both.status.success(), "{}", text(&both));
    assert!(
        text(&both).contains("OK   Token not revoked")
            && text(&both).contains("OK   TS token not revoked"),
        "{}",
        text(&both)
    );
    // the same checks as for JavaScript: a changed script, sample or expected output each fail
    let original = fs::read_to_string(dir.join("repro-2.mts")).unwrap();
    fs::write(
        dir.join("repro-2.mts"),
        original.replace("'REPRODUCED'", "'REPRODUCED?'"),
    )
    .unwrap();
    assert_eq!(recipient(&dir, None).status.code(), Some(1));
    fs::write(dir.join("repro-2.mts"), &original).unwrap();
    let sample = fs::read_to_string(dir.join("sample-2.json")).unwrap();
    fs::write(
        dir.join("sample-2.json"),
        sample.replace("return ACTIVE.has(t)", "return false"),
    )
    .unwrap();
    assert_eq!(recipient(&dir, None).status.code(), Some(1));
    fs::write(dir.join("sample-2.json"), &sample).unwrap();
    assert!(recipient(&dir, None).status.success());
    // a Node.js that says it has no TypeScript parser: that finding is MISSING (exit 2), never a failed check, and the
    // Python finding is still checked
    let fake = node_without_typescript();
    let none = recipient(&dir, Some(fake.to_str().unwrap()));
    assert_eq!(none.status.code(), Some(2), "{}", text(&none));
    let t = text(&none);
    assert!(t.contains("OK   Token not revoked"), "{t}");
    assert!(
        t.contains("MISSING TS token not revoked")
            && t.contains("22.18")
            && t.contains("nodejs.org"),
        "{t}"
    );
    assert!(!t.contains("OK   TS token") && !t.contains("FAIL"), "{t}");
    // no node at all: the same, with the older sentence
    let empty = std::env::temp_dir().join(format!("kk-empty-path-ts-{}", std::process::id()));
    fs::create_dir_all(&empty).unwrap();
    let nonode = recipient(&dir, Some(empty.to_str().unwrap()));
    assert_eq!(nonode.status.code(), Some(2), "{}", text(&nonode));
    assert!(text(&nonode).contains("MISSING TS token not revoked"));
}

fn rs_project() -> Vec<ImportedFile> {
    let label = Label::new(Level::Internal);
    let mk = |path: &str, text: &str| {
        let bytes = text.as_bytes().to_vec();
        ImportedFile {
            path: path.into(),
            sha256: kk_evidence::capsule::sha256_hex(&bytes),
            bytes,
            label: label.clone(),
            notes: vec![],
        }
    };
    vec![
        mk("Cargo.toml", "[package]\nname = \"payments-auth\"\nedition = \"2021\"\n"),
        mk("src/lib.rs", "pub mod session;\n"),
        mk("src/session.rs", "use std::cell::RefCell;\nuse std::collections::HashSet;\nthread_local! { static ACTIVE: RefCell<HashSet<String>> = RefCell::new(HashSet::new()); }\npub fn login(u: &str) -> String { let t = format!(\"{u}-é\"); ACTIVE.with(|a| a.borrow_mut().insert(t.clone())); t }\n// logout only drops the caller's copy, so the token stays valid\npub fn logout(t: &str) { let _c = t.to_string(); }\npub fn validate(t: &str) -> bool { ACTIVE.with(|a| a.borrow().contains(t)) }\n"),
    ]
}

const RS_RELEASE_REPRO: &str = "use project::session::{login, logout, validate};\nfn main() {\n    let t = login(\"alice\");\n    assert!(validate(&t));\n    println!(\"CONTROL_OK\");\n    logout(&t);\n    println!(\"{}\", if validate(&t) { \"REPRODUCED\" } else { \"NOT REPRODUCED\" });\n}\n";

fn rs_finding_for_release(reproducer: &str) -> Finding {
    Finding {
        title: "Rust token not revoked".into(),
        file: "src/session.rs".into(),
        line: 5,
        explanation: "logout only drops the caller's copy, so validate() still accepts the token."
            .into(),
        reproducer: reproducer.into(),
        language: kk_review::check::Language::Rust,
    }
}

fn rs_confirmed() -> CheckResult {
    CheckResult {
        status: CheckStatus::Reproduced,
        stdout: "CONTROL_OK\nREPRODUCED\n".into(),
        sample_paths: vec!["src/lib.rs".into(), "src/session.rs".into()],
    }
}

fn host_has_rustc() -> bool {
    Command::new("rustc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn rj06_a_rust_finding_is_released_as_an_rs_script_with_its_crate_and_the_recipient_can_rebuild_and_re_run_it(
) {
    assert!(
        host_has_rustc(),
        "this test runs the recipient's check with the host's rustc"
    );
    let mut proj = files();
    proj.extend(rs_project());
    let idx = index(&proj);
    let draft = assemble(
        &[
            (finding(), confirmed()),
            (rs_finding_for_release(RS_RELEASE_REPRO), rs_confirmed()),
        ],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("assemble");
    assert!(draft.files.contains_key("repro-1.py") && draft.files.contains_key("repro-2.rs"));
    let readme = String::from_utf8_lossy(&draft.files["finding-2.txt"]).into_owned();
    assert!(
        readme.contains("rustc") && readme.contains("repro-2.rs"),
        "{readme}"
    );
    let index_json = String::from_utf8_lossy(&draft.files["index.json"]).into_owned();
    assert!(index_json.contains("\"language\":\"rust\""), "{index_json}");
    assert!(
        index_json.contains(
            "\"rust\":{\"edition\":\"2021\",\"name\":\"payments_auth\",\"root\":\"src/lib.rs\"}"
        ) || (index_json.contains("\"root\":\"src/lib.rs\"")
            && index_json.contains("\"name\":\"payments_auth\"")
            && index_json.contains("\"edition\":\"2021\"")),
        "{index_json}"
    );
    // the released sample is the crate's module tree, and not the manifest
    let sample = String::from_utf8_lossy(&draft.files["sample-2.json"]).into_owned();
    assert!(
        sample.contains("src/lib.rs")
            && sample.contains("src/session.rs")
            && !sample.contains("Cargo.toml"),
        "{sample}"
    );
    let dir = write_release(&draft, "rs");
    // the recipient's own compiler (a different version from the sandbox's) builds the crate and the script and agrees
    let both = recipient(&dir, None);
    assert!(both.status.success(), "{}", text(&both));
    assert!(
        text(&both).contains("OK   Token not revoked")
            && text(&both).contains("OK   Rust token not revoked"),
        "{}",
        text(&both)
    );
    // a changed script, a changed sample and a changed expected output each fail
    let original = fs::read_to_string(dir.join("repro-2.rs")).unwrap();
    fs::write(
        dir.join("repro-2.rs"),
        original.replace("\"REPRODUCED\"", "\"REPRODUCED?\""),
    )
    .unwrap();
    assert_eq!(recipient(&dir, None).status.code(), Some(1));
    fs::write(dir.join("repro-2.rs"), &original).unwrap();
    let sample = fs::read_to_string(dir.join("sample-2.json")).unwrap();
    fs::write(
        dir.join("sample-2.json"),
        sample.replace(
            "pub fn logout(t: &str) { let _c = t.to_string(); }",
            "pub fn logout(t: &str) { ACTIVE.with(|a| a.borrow_mut().remove(t)); }",
        ),
    )
    .unwrap();
    let changed = recipient(&dir, None);
    assert_eq!(changed.status.code(), Some(1), "{}", text(&changed));
    fs::write(dir.join("sample-2.json"), &sample).unwrap();
    // a sample that no longer builds is a failed check, said plainly, not a crash
    fs::write(
        dir.join("sample-2.json"),
        sample.replace("pub mod session;", "pub mod nowhere;"),
    )
    .unwrap();
    let broken = recipient(&dir, None);
    assert_eq!(broken.status.code(), Some(1), "{}", text(&broken));
    assert!(text(&broken).contains("did not build"), "{}", text(&broken));
    fs::write(dir.join("sample-2.json"), &sample).unwrap();
    fs::write(dir.join("expected-2.txt"), "CONTROL_OK\nNOT REPRODUCED\n").unwrap();
    assert_eq!(recipient(&dir, None).status.code(), Some(1));
    fs::write(dir.join("expected-2.txt"), "CONTROL_OK\nREPRODUCED\n").unwrap();
    assert!(recipient(&dir, None).status.success());
    // no rustc on the path: that finding is MISSING (exit 2), never a failed check, and the Python finding is still checked
    let empty = std::env::temp_dir().join(format!("kk-empty-path-rs-{}", std::process::id()));
    fs::create_dir_all(&empty).unwrap();
    let none = recipient(&dir, Some(empty.to_str().unwrap()));
    assert_eq!(none.status.code(), Some(2), "{}", text(&none));
    let t = text(&none);
    assert!(t.contains("OK   Token not revoked"), "{t}");
    assert!(
        t.contains("MISSING Rust token not revoked") && t.contains("rust-lang.org"),
        "{t}"
    );
    assert!(!t.contains("OK   Rust token") && !t.contains("FAIL"), "{t}");
    // the capsule names the files released for the Rust finding
    let claims = kk_review::release::claims_for(&draft);
    assert_eq!(claims.len(), 2);
    assert!(
        claims[1].evidence.contains(&"repro-2.rs".to_string())
            && claims[1].how_to_check.contains("rustc"),
        "{:?}",
        claims[1]
    );
}

#[test]
fn rj07_a_crate_without_an_edition_is_rebuilt_by_the_recipient_as_2015_and_the_script_as_2021() {
    assert!(
        host_has_rustc(),
        "this test runs the recipient's check with the host's rustc"
    );
    let mut proj = files();
    proj.extend(rs_project());
    for f in proj.iter_mut() {
        if f.path == "Cargo.toml" {
            f.bytes = b"[package]\nname = \"payments-auth\"\n".to_vec();
        }
        if f.path == "src/session.rs" {
            let text = String::from_utf8_lossy(&f.bytes).replace(
                "pub fn validate(t: &str) -> bool {",
                "pub fn validate(t: &str) -> bool { let async = 1; let _ = async;",
            );
            f.bytes = text.into_bytes();
        }
    }
    let idx = index(&proj);
    let draft = assemble(
        &[(rs_finding_for_release(RS_RELEASE_REPRO), rs_confirmed())],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("assemble");
    let index_json = String::from_utf8_lossy(&draft.files["index.json"]).into_owned();
    assert!(index_json.contains("\"edition\":\"2015\""), "{index_json}");
    let dir = write_release(&draft, "rs2015");
    let out = recipient(&dir, None);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("OK   Rust token not revoked"),
        "{}",
        text(&out)
    );
}

#[test]
fn rj08_a_confirmed_finding_whose_check_used_a_file_the_project_does_not_hold_is_left_out_not_a_crash(
) {
    let proj = files();
    let idx = index(&proj);
    let (good_finding, good_check) = (finding(), confirmed());
    let (bad_finding, mut bad_check) = (finding(), confirmed());
    bad_check.sample_paths = vec!["src/never_imported.py".into()];
    let draft = assemble(
        &[(bad_finding, bad_check), (good_finding, good_check)],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("a finding with a missing file must not stop the release");
    assert!(
        draft.files.contains_key("repro-1.py"),
        "the other finding is still released"
    );
    assert!(!draft.files.contains_key("repro-2.py"));
    assert!(
        draft
            .not_confirmed
            .iter()
            .any(|(_, why)| why.contains("not part of the project")
                && why.contains("never_imported.py")),
        "{:?}",
        draft.not_confirmed
    );
}

fn model_receipt(
    seed: u8,
    id: &str,
    model: &str,
    request: &[u8],
    answer: &[u8],
) -> kk_evidence::capsule::ReceiptSpec {
    use ed25519_dalek::Signer as _;
    let key = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
    let text = format!(
        "{model}:{}:{}",
        capsule::sha256_hex(request),
        capsule::sha256_hex(answer)
    );
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    kk_evidence::capsule::ReceiptSpec {
        completion_id: id.into(),
        signer: hex(&key.verifying_key().to_bytes()),
        signature: hex(&key.sign(text.as_bytes()).to_bytes()),
        signed_text: text,
    }
}

#[test]
fn rs08_a_capsule_built_from_a_review_that_used_a_verified_model_carries_the_models_signatures_and_a_forged_one_breaks_it(
) {
    let f = files();
    let idx = index(&f);
    let draft = assemble(
        &[(finding(), confirmed())],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .unwrap();
    let signer = Signer::from_seed([5u8; 32]);
    let receipts = vec![
        model_receipt(7, "chat-1", "vendor/model", b"request one", b"answer one"),
        model_receipt(7, "chat-2", "vendor/model", b"request two", b"answer two"),
    ];
    let opts = VerifyOptions {
        pinned_keys: Some(vec![signer.public()]),
        now: 1_700_000_100,
        expected_recipient: Some("acme-audit".into()),
    };
    let bytes = build_capsule(
        &draft,
        &signer,
        &"ab".repeat(32),
        38,
        "verified_tee",
        &[],
        &receipts,
    );
    let report = capsule::verify(&bytes, &opts);
    assert!(report.valid, "{report:?}");
    assert_eq!(report.route.as_deref(), Some("verified_tee"));
    assert_eq!(report.receipts.len(), 2);
    assert_eq!(report.receipts[0].completion_id, "chat-1");
    assert_eq!(report.receipts[0].model, "vendor/model");
    assert_eq!(
        report.receipts[0].request_sha256,
        capsule::sha256_hex(b"request one")
    );
    assert!(
        report
            .established
            .iter()
            .any(|e| e.contains("2 model answer(s)")),
        "{:?}",
        report.established
    );
    assert!(
        report
            .not_established
            .iter()
            .any(|n| n.contains("attested confidential hardware")),
        "{:?}",
        report.not_established
    );
    // without receipts the capsule says nothing about a model's signature
    let plain = build_capsule(
        &draft,
        &signer,
        &"ab".repeat(32),
        38,
        "device_only",
        &[],
        &[],
    );
    let r = capsule::verify(&plain, &opts);
    assert!(r.valid && r.receipts.is_empty());
    assert!(!r.established.iter().any(|e| e.contains("model answer")));
    // a receipt whose signature was made for other text is refused, however the release is signed
    let mut forged = receipts.clone();
    forged[1].signed_text =
        model_receipt(7, "chat-2", "vendor/model", b"other request", b"answer two").signed_text;
    let bad = build_capsule(
        &draft,
        &signer,
        &"ab".repeat(32),
        38,
        "verified_tee",
        &[],
        &forged,
    );
    let r = capsule::verify(&bad, &opts);
    assert!(!r.valid);
    assert_eq!(r.reason, Some("receipt_signature"));
}
