use kk_update::download::{download_with, final_name};
use kk_update::{Artifact, Offer, Refusal, Version};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-dl-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn offer(bytes: &[u8]) -> Offer {
    Offer {
        version: Version(2, 0, 0),
        notes: String::new(),
        artifact: Artifact {
            platform: "macos-arm64".into(),
            url: "https://updates.example/k.dmg".into(),
            sha256: kk_evidence::capsule::sha256_hex(bytes),
            size: bytes.len() as u64,
        },
    }
}

fn files(d: &PathBuf) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn dl01_the_exact_signed_bytes_end_up_in_one_private_non_executable_file_named_from_the_signed_data(
) {
    let d = dir("ok");
    let payload = b"pretend installer bytes".to_vec();
    let o = offer(&payload);
    let path = download_with(&o, "macos-arm64", &d, |out| {
        // arrives in awkward pieces
        for chunk in payload.chunks(5) {
            out.write_all(chunk).map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), payload);
    assert_eq!(
        path.file_name().unwrap().to_string_lossy(),
        final_name(&o, "macos-arm64")
    );
    assert_eq!(
        files(&d),
        vec![final_name(&o, "macos-arm64")],
        "no temporary file is left"
    );
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "owner only, and never executable");
    assert_eq!(
        std::fs::metadata(&d).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn dl02_one_wrong_byte_a_short_stream_and_a_failing_source_leave_nothing_behind() {
    let d = dir("bad");
    let payload = b"pretend installer bytes".to_vec();
    let o = offer(&payload);
    // same size, one byte different
    let mut wrong = payload.clone();
    wrong[3] ^= 1;
    let r = download_with(&o, "macos-arm64", &d, |out| {
        out.write_all(&wrong).map_err(|e| e.to_string())
    });
    assert!(
        matches!(r, Err(Refusal::BadArtifact(m)) if m.contains("checksum")),
        "{r:?}"
    );
    // one byte short
    let r = download_with(&o, "macos-arm64", &d, |out| {
        out.write_all(&payload[..payload.len() - 1])
            .map_err(|e| e.to_string())
    });
    assert!(
        matches!(r, Err(Refusal::BadArtifact(m)) if m.contains("before the signed size")),
        "{r:?}"
    );
    // the connection fails half way
    let r = download_with(&o, "macos-arm64", &d, |out| {
        out.write_all(&payload[..10]).map_err(|e| e.to_string())?;
        Err("connection reset".into())
    });
    assert!(r.is_err());
    // nothing at all arrives
    assert!(download_with(&o, "macos-arm64", &d, |_| Ok(())).is_err());
    assert!(
        files(&d).is_empty(),
        "failures leave no file: {:?}",
        files(&d)
    );
}

#[test]
fn dl03_a_stream_that_never_ends_is_cut_at_the_signed_size_and_never_written_past_it() {
    let d = dir("endless");
    let payload = vec![7u8; 4096];
    let o = offer(&payload);
    let mut offered = 0u64;
    let r = download_with(&o, "macos-arm64", &d, |out| {
        // a hostile server that would send a terabyte: stop at the first refusal, as a real transport does
        let chunk = vec![7u8; 1000];
        for _ in 0..1_000_000_000u64 {
            out.write_all(&chunk).map_err(|e| e.to_string())?;
            offered += chunk.len() as u64;
        }
        Ok(())
    });
    assert!(r.is_err());
    assert!(
        offered <= 4096,
        "the source was stopped within the signed size: {offered}"
    );
    assert!(files(&d).is_empty());
}

#[test]
fn dl04_a_failed_download_never_replaces_an_earlier_good_file() {
    let d = dir("keep");
    let payload = b"version two installer".to_vec();
    let o = offer(&payload);
    let good = download_with(&o, "macos-arm64", &d, |out| {
        out.write_all(&payload).map_err(|e| e.to_string())
    })
    .unwrap();
    let before = std::fs::read(&good).unwrap();
    let mut wrong = payload.clone();
    wrong[0] ^= 0xff;
    assert!(download_with(&o, "macos-arm64", &d, |out| out
        .write_all(&wrong)
        .map_err(|e| e.to_string()))
    .is_err());
    assert_eq!(std::fs::read(&good).unwrap(), before);
    assert_eq!(files(&d).len(), 1);
    // a later good download replaces it with the same bytes
    assert!(download_with(&o, "macos-arm64", &d, |out| out
        .write_all(&payload)
        .map_err(|e| e.to_string()))
    .is_ok());
}

#[test]
fn dl05_a_platform_name_cannot_make_the_file_land_anywhere_else() {
    let d = dir("names");
    let payload = b"x".to_vec();
    let o = offer(&payload);
    for bad in [
        "../evil",
        "a/b",
        "",
        "UPPER",
        "with space",
        "dot.dot",
        &"x".repeat(41),
        "nul\0x",
    ] {
        let r = download_with(&o, bad, &d, |out| {
            out.write_all(&payload).map_err(|e| e.to_string())
        });
        assert!(matches!(r, Err(Refusal::BadArtifact(_))), "{bad:?}");
    }
    assert!(files(&d).is_empty());
    assert!(!d.parent().unwrap().join("evil.update").exists());
}

#[test]
fn dl06_a_signed_announcement_with_an_odd_platform_name_is_refused_before_anything_is_offered() {
    use kk_update::{check, root::Root, sign_manifest, Context, Verdict};
    let signer = kk_evidence::keys::Signer::from_seed([42u8; 32]);
    let root = Root::single(&signer.public()).unwrap();
    let manifest = |platform: &str| {
        serde_json::json!({"channel": "stable", "version": "2.0.0", "not_after": 4_000_000_000u64, "notes": "n",
            "artifacts": [{"platform": platform, "url": "https://updates.example/k.dmg", "sha256": "a".repeat(64), "size": 10}]}).to_string()
    };
    // the signing tool refuses it, and so does a client given one signed by hand
    assert!(sign_manifest(manifest("../x").as_bytes(), &signer).is_err());
    let mut env = kk_evidence::dsse::sign(
        kk_update::MANIFEST_TYPE,
        kk_evidence::canon::canonicalize(manifest("../x").as_bytes())
            .unwrap()
            .as_bytes(),
        &signer,
    );
    env.signatures.truncate(1);
    let bytes = kk_evidence::dsse::encode(&env).into_bytes();
    let cx = Context {
        root: &root,
        channel: "stable",
        platform: "../x",
        current: Version(1, 0, 0),
        highest_seen: None,
        now: 1_900_000_000,
    };
    assert!(matches!(check(&bytes, &cx), Err(Refusal::BadArtifact(_))));
    let _ = Verdict::UpToDate;
}
