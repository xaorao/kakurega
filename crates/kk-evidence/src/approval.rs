use crate::b64;
use crate::canon::{self, Value};
use crate::dsse;
use crate::hw;
use crate::keys::{Signer, TrustedKeys};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Approvers {
    pub ed: TrustedKeys,
    pub p256: Vec<hw::P256Public>,
}

pub const APPROVAL_TYPE: &str = "application/vnd.kakurega.approval+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub approval: String,
    pub job: String,
    pub digest: String,
    pub recipient: String,
    pub policy: String,
    pub expires: i64,
    pub nonce: String,
}

pub const ACTION_TYPE: &str = "application/vnd.kakurega.approver-action+json";
pub const ACTION_WINDOW_SECS: i64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub action: String,
    pub subject: String,
    pub at: i64,
}

pub fn canonical_action(a: &Action) -> String {
    canon::serialize(&Value::Object(vec![
        ("action".into(), Value::Str(a.action.clone())),
        ("at".into(), Value::Int(a.at)),
        ("subject".into(), Value::Str(a.subject.clone())),
        ("v".into(), Value::Int(1)),
    ]))
}

pub fn sign_action(signer: &Signer, a: &Action) -> String {
    dsse::encode(&dsse::sign(
        ACTION_TYPE,
        canonical_action(a).as_bytes(),
        signer,
    ))
}

/// The signed statement must name the action and subject, be at most ACTION_WINDOW_SECS old (or that far in the future), and come from a trusted key.
fn hw_reject(e: hw::HwReject) -> Reject {
    match e {
        hw::HwReject::NotAnEnvelope => Reject::NotAnEnvelope,
        hw::HwReject::WrongType => Reject::WrongType,
        hw::HwReject::UnknownKey => Reject::UnknownKey,
        hw::HwReject::BadSignature => Reject::BadSignature,
    }
}

fn check(proof: &str, kind: &str, trusted: &Approvers) -> Result<(Vec<u8>, String), Reject> {
    if proof.contains(hw::ALG) {
        let p = hw::verify(proof, kind, &trusted.p256).map_err(hw_reject)?;
        return Ok((p.payload, hw::key_id(&p.public)));
    }
    let env = dsse::decode(proof.as_bytes()).map_err(|_| Reject::NotAnEnvelope)?;
    let verified = dsse::verify(&env, &trusted.ed, &[kind]).map_err(|e| match e {
        dsse::VerifyError::UnknownPayloadType => Reject::WrongType,
        dsse::VerifyError::WrongSignatureCount => Reject::WrongSignatureCount,
        dsse::VerifyError::UnknownKey => Reject::UnknownKey,
        dsse::VerifyError::BadSignature => Reject::BadSignature,
        dsse::VerifyError::PayloadNotCanonical => Reject::NotCanonical,
    })?;
    Ok((env.payload, verified.key_id.as_str().to_string()))
}

pub fn verify(proof: &str, trusted: &Approvers, expected: &Statement) -> Result<String, Reject> {
    let (payload, id) = check(proof, APPROVAL_TYPE, trusted)?;
    if payload != canonical(expected).as_bytes() {
        return Err(Reject::DifferentStatement);
    }
    Ok(id)
}

pub fn verify_action(
    proof: &str,
    trusted: &Approvers,
    action: &str,
    subject: &str,
    now: i64,
) -> Result<String, Reject> {
    let (payload, id) = check(proof, ACTION_TYPE, trusted)?;
    let text = std::str::from_utf8(&payload).map_err(|_| Reject::NotCanonical)?;
    match canon::canonicalize(text.as_bytes()) {
        Ok(c) if c == text => {}
        _ => return Err(Reject::NotCanonical),
    }
    let Ok(Value::Object(m)) = canon::parse_strict(text.as_bytes()) else {
        return Err(Reject::NotCanonical);
    };
    let get = |k: &str| m.iter().find(|(n, _)| n == k).map(|(_, v)| v);
    let (Some(Value::Str(a)), Some(Value::Str(sub)), Some(Value::Int(at))) =
        (get("action"), get("subject"), get("at"))
    else {
        return Err(Reject::DifferentStatement);
    };
    if a != action || sub != subject || (now - at).abs() > ACTION_WINDOW_SECS {
        return Err(Reject::DifferentStatement);
    }
    Ok(id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    NotAnEnvelope,
    WrongType,
    WrongSignatureCount,
    UnknownKey,
    BadSignature,
    NotCanonical,
    DifferentStatement,
}

impl std::fmt::Display for Reject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Reject::NotAnEnvelope => "it is not a signed approval",
            Reject::WrongType => "it is signed, but it is not an approval",
            Reject::WrongSignatureCount => "it must carry exactly one signature",
            Reject::UnknownKey => "it was signed by a key that is not enrolled to approve here",
            Reject::BadSignature => "the signature does not match what was signed",
            Reject::NotCanonical => "the signed text is not in the exact form Kakurega signs",
            Reject::DifferentStatement => "it approves something other than this release (a different digest, recipient or expiry)",
        })
    }
}

pub fn canonical(s: &Statement) -> String {
    canon::serialize(&Value::Object(vec![
        ("approval".into(), Value::Str(s.approval.clone())),
        ("digest".into(), Value::Str(s.digest.clone())),
        ("expires".into(), Value::Int(s.expires)),
        ("job".into(), Value::Str(s.job.clone())),
        ("nonce".into(), Value::Str(s.nonce.clone())),
        ("policy".into(), Value::Str(s.policy.clone())),
        ("recipient".into(), Value::Str(s.recipient.clone())),
        ("v".into(), Value::Int(1)),
    ]))
}

pub fn sign(signer: &Signer, s: &Statement) -> String {
    dsse::encode(&dsse::sign(APPROVAL_TYPE, canonical(s).as_bytes(), signer))
}

#[derive(Debug, Clone)]
pub struct EnclaveKey {
    pub helper: std::path::PathBuf,
    pub blob: std::path::PathBuf,
    pub data_dir: std::path::PathBuf,
}

impl EnclaveKey {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        use std::process::{Command, Stdio};
        let child = Command::new(&self.helper)
            .args(args)
            .arg("--blob")
            .arg(&self.blob)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", self.helper.display()))?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr)
                .trim()
                .chars()
                .take(300)
                .collect());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn public(&self) -> Result<hw::P256Public, String> {
        let text = self.run(&["public"])?;
        b64::decode(&text)
            .ok()
            .and_then(|p| hw::valid_public(&p))
            .ok_or_else(|| "the helper returned an invalid public key".to_string())
    }
}

#[derive(Debug)]
pub enum ApproverKey {
    Software(Signer),
    Enclave(EnclaveKey),
}

impl ApproverKey {
    pub fn public_bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            ApproverKey::Software(s) => Ok(s.public().to_vec()),
            ApproverKey::Enclave(e) => Ok(e.public()?.to_vec()),
        }
    }

    pub fn key_id(&self) -> Result<String, String> {
        match self {
            ApproverKey::Software(s) => Ok(s.key_id().as_str().to_string()),
            ApproverKey::Enclave(e) => Ok(hw::key_id(&e.public()?)),
        }
    }

    pub fn is_hardware(&self) -> bool {
        matches!(self, ApproverKey::Enclave(_))
    }

    /// The Secure Enclave helper fetches the release from the kernel itself and shows its own dialog:
    /// the only thing a caller supplies is the approval id, never bytes to sign.
    pub fn sign_statement(&self, s: &Statement) -> Result<String, String> {
        match self {
            ApproverKey::Software(sw) => Ok(dsse::encode(&dsse::sign(
                APPROVAL_TYPE,
                canonical(s).as_bytes(),
                sw,
            ))),
            ApproverKey::Enclave(e) => {
                let dir = e.data_dir.to_string_lossy().to_string();
                e.run(&["approve", "--data-dir", &dir, "--approval", &s.approval])
            }
        }
    }

    pub fn sign_change(&self, a: &Action) -> Result<String, String> {
        match self {
            ApproverKey::Software(sw) => Ok(sign_action(sw, a)),
            ApproverKey::Enclave(e) => {
                let dir = e.data_dir.to_string_lossy().to_string();
                e.run(&[
                    "change",
                    "--data-dir",
                    &dir,
                    "--action",
                    &a.action,
                    "--subject",
                    &a.subject,
                ])
            }
        }
    }
}

pub fn find_helper() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("KAKU_ENCLAVE_BIN") {
        return Some(p.into());
    }
    let beside = std::env::current_exe().ok()?.parent()?.join("kaku-enclave");
    beside.is_file().then_some(beside)
}

/// Prefers a Secure Enclave key (`approver.enclave`) over a software key (`approver.key`).
pub fn load_approver(dir: &Path) -> Result<Option<ApproverKey>, KeyFileError> {
    let blob = dir.join("approver.enclave");
    if blob.is_file() {
        let helper = find_helper().ok_or_else(|| {
            KeyFileError::Io(
                "the Secure Enclave helper kaku-enclave was not found (set KAKU_ENCLAVE_BIN)"
                    .into(),
            )
        })?;
        return Ok(Some(ApproverKey::Enclave(EnclaveKey {
            helper,
            blob,
            data_dir: dir.to_path_buf(),
        })));
    }
    Ok(load_key(&dir.join("approver.key"))?.map(ApproverKey::Software))
}

#[derive(Debug)]
pub enum KeyFileError {
    Io(String),
    Permissions,
    Damaged,
}

pub fn load_key(path: &Path) -> Result<Option<Signer>, KeyFileError> {
    use std::os::unix::fs::PermissionsExt;
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(KeyFileError::Io(e.to_string())),
    };
    if meta.permissions().mode() & 0o077 != 0 || !meta.is_file() {
        return Err(KeyFileError::Permissions);
    }
    let text = std::fs::read_to_string(path).map_err(|e| KeyFileError::Io(e.to_string()))?;
    let raw = b64::decode(text.trim()).map_err(|_| KeyFileError::Damaged)?;
    let seed: [u8; 32] = raw.try_into().map_err(|_| KeyFileError::Damaged)?;
    Ok(Some(Signer::from_seed(seed)))
}

pub fn create_key(path: &Path) -> Result<Signer, KeyFileError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut seed = [0u8; 32];
    let mut f = std::fs::File::open("/dev/urandom").map_err(|e| KeyFileError::Io(e.to_string()))?;
    std::io::Read::read_exact(&mut f, &mut seed).map_err(|e| KeyFileError::Io(e.to_string()))?;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| KeyFileError::Io(e.to_string()))?;
    out.write_all(b64::encode(&seed).as_bytes())
        .map_err(|e| KeyFileError::Io(e.to_string()))?;
    Ok(Signer::from_seed(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> Statement {
        Statement {
            approval: "appr-1".into(),
            job: "job-1".into(),
            digest: "ab".repeat(32),
            recipient: "acme-audit".into(),
            policy: "policy-1".into(),
            expires: 1_700_086_400,
            nonce: "n0nce-value".into(),
        }
    }

    fn trusted(s: &Signer) -> Approvers {
        let mut t = Approvers::default();
        t.ed.add(&s.public()).unwrap();
        t
    }

    #[test]
    fn ap01_a_proof_verifies_only_for_the_exact_statement_it_signed() {
        let s = Signer::from_seed([5; 32]);
        let proof = sign(&s, &st());
        assert_eq!(
            verify(&proof, &trusted(&s), &st()),
            Ok(s.key_id().as_str().to_string())
        );
        for (name, other) in [
            (
                "approval",
                Statement {
                    approval: "appr-2".into(),
                    ..st()
                },
            ),
            (
                "job",
                Statement {
                    job: "job-2".into(),
                    ..st()
                },
            ),
            (
                "digest",
                Statement {
                    digest: "cd".repeat(32),
                    ..st()
                },
            ),
            (
                "recipient",
                Statement {
                    recipient: "other".into(),
                    ..st()
                },
            ),
            (
                "policy",
                Statement {
                    policy: "policy-2".into(),
                    ..st()
                },
            ),
            (
                "expires",
                Statement {
                    expires: 1_700_086_401,
                    ..st()
                },
            ),
            (
                "nonce",
                Statement {
                    nonce: "another".into(),
                    ..st()
                },
            ),
        ] {
            assert_eq!(
                verify(&proof, &trusted(&s), &other),
                Err(Reject::DifferentStatement),
                "changing {name} must void the proof"
            );
        }
    }

    #[test]
    fn ap02_the_wrong_key_a_tampered_proof_and_junk_are_refused() {
        let s = Signer::from_seed([5; 32]);
        let other = Signer::from_seed([6; 32]);
        let proof = sign(&s, &st());
        assert_eq!(
            verify(&proof, &trusted(&other), &st()),
            Err(Reject::UnknownKey)
        );
        assert_eq!(
            verify(&sign(&other, &st()), &trusted(&s), &st()),
            Err(Reject::UnknownKey),
            "a stranger's valid signature is not an approval"
        );
        let mut tampered = proof.clone().into_bytes();
        let mid = tampered.len() / 2;
        tampered[mid] = if tampered[mid] == b'A' { b'B' } else { b'A' };
        assert!(verify(&String::from_utf8(tampered).unwrap(), &trusted(&s), &st()).is_err());
        for junk in ["", "not an envelope", "{}", "null", &"x".repeat(100_000)] {
            assert!(verify(junk, &trusted(&s), &st()).is_err());
        }
        let wrong_type = dsse::encode(&dsse::sign(
            "application/vnd.kakurega.release+json",
            canonical(&st()).as_bytes(),
            &s,
        ));
        assert_eq!(
            verify(&wrong_type, &trusted(&s), &st()),
            Err(Reject::WrongType),
            "a signature made for another purpose cannot be replayed as an approval"
        );
        let empty = Approvers::default();
        assert_eq!(verify(&proof, &empty, &st()), Err(Reject::UnknownKey));
    }

    #[test]
    fn ap04_a_signed_approver_action_names_its_action_subject_and_time() {
        let s = Signer::from_seed([5; 32]);
        let other = Signer::from_seed([6; 32]);
        let t = trusted(&s);
        let proof = sign_action(
            &s,
            &Action {
                action: "enroll".into(),
                subject: "key-A".into(),
                at: 1000,
            },
        );
        assert_eq!(
            verify_action(&proof, &t, "enroll", "key-A", 1000),
            Ok(s.key_id().as_str().to_string())
        );
        assert_eq!(
            verify_action(&proof, &t, "enroll", "key-A", 1120),
            Ok(s.key_id().as_str().to_string()),
            "inside the window"
        );
        assert_eq!(
            verify_action(&proof, &t, "enroll", "key-A", 1121),
            Err(Reject::DifferentStatement),
            "too old"
        );
        assert_eq!(
            verify_action(&proof, &t, "enroll", "key-A", 879),
            Err(Reject::DifferentStatement),
            "too far in the future"
        );
        assert_eq!(
            verify_action(&proof, &t, "revoke", "key-A", 1000),
            Err(Reject::DifferentStatement),
            "another action"
        );
        assert_eq!(
            verify_action(&proof, &t, "enroll", "key-B", 1000),
            Err(Reject::DifferentStatement),
            "another subject"
        );
        assert_eq!(
            verify_action(
                &sign_action(
                    &other,
                    &Action {
                        action: "enroll".into(),
                        subject: "key-A".into(),
                        at: 1000
                    }
                ),
                &t,
                "enroll",
                "key-A",
                1000
            ),
            Err(Reject::UnknownKey)
        );
        let approval_proof = sign(&s, &st());
        assert_eq!(
            verify_action(&approval_proof, &t, "enroll", "key-A", 1000),
            Err(Reject::WrongType),
            "an approval signature is not an approver action"
        );
        assert_eq!(
            verify(&proof, &t, &st()),
            Err(Reject::WrongType),
            "and an approver action is not an approval"
        );
    }

    #[test]
    fn ap05_a_hardware_p256_key_can_approve_and_authorise_changes_like_an_ed25519_key() {
        use p256::ecdsa::signature::Signer as _;
        use p256::ecdsa::{Signature, SigningKey};
        let sk = SigningKey::from_slice(&[9; 32]).unwrap();
        let public: hw::P256Public = sk
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .try_into()
            .unwrap();
        let sign_hw = |kind: &str, payload: &str| {
            let sig: Signature = sk.sign(&crate::dsse::pae(kind, payload.as_bytes()));
            hw::envelope(kind, payload.as_bytes(), &public, &sig.to_bytes().into())
        };
        let mut a = Approvers::default();
        a.p256.push(public);
        let proof = sign_hw(APPROVAL_TYPE, &canonical(&st()));
        assert_eq!(verify(&proof, &a, &st()), Ok(hw::key_id(&public)));
        let other = Statement {
            recipient: "x".into(),
            ..st()
        };
        assert_eq!(verify(&proof, &a, &other), Err(Reject::DifferentStatement));
        assert_eq!(
            verify(&proof, &Approvers::default(), &st()),
            Err(Reject::UnknownKey)
        );
        let act = sign_hw(
            ACTION_TYPE,
            &canonical_action(&Action {
                action: "enroll".into(),
                subject: "k".into(),
                at: 1000,
            }),
        );
        assert_eq!(
            verify_action(&act, &a, "enroll", "k", 1000),
            Ok(hw::key_id(&public))
        );
        assert_eq!(
            verify_action(&act, &a, "enroll", "k", 2000),
            Err(Reject::DifferentStatement)
        );
        assert_eq!(
            verify_action(&proof, &a, "enroll", "k", 1000),
            Err(Reject::WrongType),
            "an approval is not an action"
        );
    }

    #[test]
    fn ap03_key_files_are_created_private_and_refused_when_loose_or_damaged() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("kk-ap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("approver.key");
        let _ = std::fs::remove_file(&p);
        assert!(load_key(&p).unwrap().is_none());
        let made = create_key(&p).unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(
            matches!(create_key(&p), Err(KeyFileError::Io(_))),
            "an existing key is never overwritten"
        );
        assert_eq!(load_key(&p).unwrap().unwrap().public(), made.public());
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(load_key(&p), Err(KeyFileError::Permissions)));
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&p, "not base64url!").unwrap();
        assert!(matches!(load_key(&p), Err(KeyFileError::Damaged)));
    }
}
