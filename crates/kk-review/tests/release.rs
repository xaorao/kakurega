use kk_evidence::capsule::{self, ClaimSpec, VerifyOptions};
use kk_evidence::keys::Signer;
use kk_review::check::{CheckResult, CheckStatus, Finding};
use kk_review::import::{scan_tree, ImportedFile};
use kk_review::release::{
    assemble, build_capsule, release_digest, Lease, LeaseError, LeaseState, Rejection,
};
use kk_types::label::Level;
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
        matches!(r, Err(Rejection::ContainsWithheldText { .. })),
        "{r:?}"
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
