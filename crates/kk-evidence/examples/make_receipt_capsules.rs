//! Writes capsules for the recipient check in `tests/e2e/verify_receipts.mjs`: one with two model receipts, one whose receipt was
//! altered after signing, valid for a year from now. `cargo run -p kk-evidence --example make_receipt_capsules -- DIR`
use kk_evidence::capsule::{self, sha256_hex, ClaimSpec, ReceiptSpec, ReleaseSpec};
use kk_evidence::keys::Signer;
use std::collections::BTreeMap;

fn receipt(seed: u8, id: &str, request: &[u8], answer: &[u8]) -> ReceiptSpec {
    use ed25519_dalek::Signer as _;
    let key = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
    let text = format!(
        "vendor/model:{}:{}",
        sha256_hex(request),
        sha256_hex(answer)
    );
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    ReceiptSpec {
        completion_id: id.into(),
        signer: hex(&key.verifying_key().to_bytes()),
        signature: hex(&key.sign(text.as_bytes()).to_bytes()),
        signed_text: text,
    }
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("output folder"));
    std::fs::create_dir_all(&dir).expect("folder");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let signer = Signer::from_seed([1u8; 32]);
    let mut files = BTreeMap::new();
    files.insert(
        "finding.md".to_string(),
        b"Token reuse after logout in auth/session.rs line 88.\n".to_vec(),
    );
    let claim = ClaimSpec {
        statement: "Session tokens remain valid after logout.".into(),
        evidence: vec!["finding.md".into()],
        how_to_check: "Read finding.md".into(),
    };
    let spec = |receipts: Vec<ReceiptSpec>| ReleaseSpec {
        capsule_id: "00112233445566778899aabbccddeeff".into(),
        created: now,
        expires: now + 365 * 86_400,
        recipient: "acme-audit".into(),
        policy_version: "policy-1".into(),
        binding_level: "B1".into(),
        route: "verified_tee".into(),
        not_established: vec![
            "That the private source repository is the one that was reviewed.".into(),
        ],
        route_receipts: receipts,
        source_commitment_scheme: "hmac-sha256-keyed".into(),
        source_commitment_digest: sha256_hex(b"manifest"),
        source_manifest_files: 3,
    };
    let two = vec![
        receipt(7, "chat-1", b"request one", b"answer one"),
        receipt(8, "chat-2", b"request two", b"answer two"),
    ];
    std::fs::write(
        dir.join("two.kkg"),
        capsule::build(
            &spec(two.clone()),
            std::slice::from_ref(&claim),
            &files,
            &signer,
        ),
    )
    .unwrap();
    let mut forged = two;
    forged[1].signed_text = receipt(8, "chat-2", b"another request", b"answer two").signed_text;
    std::fs::write(
        dir.join("forged.kkg"),
        capsule::build(&spec(forged), &[claim], &files, &signer),
    )
    .unwrap();
}
