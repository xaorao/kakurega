//! Sharing one finding of a delivered review with someone who was not its recipient: the finding's own files with a proof
//! that they are part of the signed release, and nothing about the rest of it. The owner asks for it from their own session;
//! what comes back is a file the owner hands on themselves.

use super::*;
use kk_evidence::share as proof;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ShareBody {
    /// The finding's number, as the review lists it (from 1).
    finding: u32,
}

/// The files of a capsule that belong to finding `n`: its text, its reproducer, the output expected of it, the minimal
/// sample of project files it was checked on, its own entry of the index, and the check script.
fn finding_files<'a>(names: impl Iterator<Item = &'a String>, n: u32) -> Vec<&'a str> {
    let heads = [
        format!("finding-{n}."),
        format!("repro-{n}."),
        format!("expected-{n}."),
        format!("sample-{n}."),
        format!("meta-{n}."),
    ];
    names
        .map(String::as_str)
        // the check script is the same in every capsule and says nothing of this one; with it the reader can re-run the finding
        .filter(|name| {
            *name == "run-check.py" || heads.iter().any(|h| name.starts_with(h.as_str()))
        })
        .collect()
}

/// The findings of a delivered review that can be shared one by one: the number each has in the release, and its title.
pub(super) async fn shareable(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let path = svc
        .inner
        .lock()
        .expect("lock")
        .jobs
        .get(&id)
        .and_then(|j| j.delivered.as_ref().map(|d| d.path.clone()));
    let parts = path
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|capsule| proof::capsule_parts(&capsule, now()));
    let Some((_, files)) = parts else {
        return Json(json!({"findings": []})).into_response();
    };
    let mut list: Vec<(u32, String)> = files
        .iter()
        .filter_map(|(name, content)| {
            let n: u32 = name
                .strip_prefix("finding-")?
                .strip_suffix(".txt")?
                .parse()
                .ok()?;
            // the title is the finding's own first line, as the release has it
            let title: String = String::from_utf8_lossy(content)
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .filter(|c| !c.is_control())
                .take(160)
                .collect();
            // the release writes "Finding N: title"; the number is shown beside it already
            let title = title
                .strip_prefix(&format!("Finding {n}: "))
                .unwrap_or(&title)
                .to_string();
            Some((n, title))
        })
        .collect();
    list.sort();
    Json(json!({
        "findings": list.iter().map(|(n, title)| json!({"n": n, "title": title})).collect::<Vec<_>>(),
        "signer_public": kk_evidence::b64::encode(&svc.signer_public()),
    }))
    .into_response()
}

pub(super) async fn share_finding(
    axum::Extension(svc): Svc,
    UrlPath(id): UrlPath<String>,
    Json(b): Json<ShareBody>,
) -> Response {
    let delivered = svc
        .inner
        .lock()
        .expect("lock")
        .jobs
        .get(&id)
        .map(|j| j.delivered.clone());
    let Some(delivered) = delivered else {
        return error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "there is no review with that id",
        );
    };
    let Some(delivered) = delivered else {
        return error(
            StatusCode::CONFLICT,
            "NOT_DELIVERED",
            "a finding can be shared once its review has been approved and delivered",
        );
    };
    let Ok(capsule) = std::fs::read(&delivered.path) else {
        return error(
            StatusCode::NOT_FOUND,
            "NO_CAPSULE",
            "the capsule file is missing",
        );
    };
    let Some((release_payload, files)) = proof::capsule_parts(&capsule, now()) else {
        return error(
            StatusCode::CONFLICT,
            "CAPSULE_INVALID",
            "the capsule file on this computer no longer verifies, so nothing is shared from it",
        );
    };
    let names = finding_files(files.keys(), b.finding);
    if b.finding == 0
        || !names
            .iter()
            .any(|n| n.starts_with(&format!("finding-{}.", b.finding)))
    {
        return error(
            StatusCode::NOT_FOUND,
            "NO_SUCH_FINDING",
            "the delivered review has no finding with that number",
        );
    }
    let capsule_sha = kk_evidence::capsule::sha256_hex(&capsule);
    // the salts come from a secret this service holds and the capsule's digest: the same share every time, and nothing a
    // reader of one share can use against another capsule
    let seed = proof::seed_for(&svc.keys.commitment_key, &capsule_sha);
    let Some(disclosure) =
        proof::build_disclosure(&capsule, &release_payload, &files, &seed, &svc.keys.signer)
    else {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            "the share could not be made",
        );
    };
    let share = match proof::build_share(&disclosure, &svc.signer_public(), &files, &seed, &names) {
        Ok(s) => s,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
                "the share could not be made",
            )
        }
    };
    // made, then checked the way a reader will check it, before it is handed over
    let report = proof::verify_share(
        &share,
        &proof::ShareOptions {
            pinned_keys: Some(vec![svc.signer_public()]),
        },
    );
    if !report.valid {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            "the share did not verify, so it was not handed over",
        );
    }
    if !svc.log_event(
        "share.made",
        &id,
        &[
            ("finding", (b.finding as usize).into()),
            ("files", names.len().into()),
            (
                "capsule",
                capsule_sha.chars().take(16).collect::<String>().into(),
            ),
        ],
    ) {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "LOG_UNAVAILABLE",
            "the event log could not be written, so nothing was shared",
        );
    }
    Json(json!({
        "name": format!("{}-finding-{}.kks", &capsule_sha[..16], b.finding),
        "share": kk_evidence::b64::encode(&share),
        "files": names,
        "total_files": files.len(),
        "capsule_sha256": capsule_sha,
        "signer": kk_evidence::keys::KeyId::of(&svc.signer_public()).as_str(),
        "signer_public": kk_evidence::b64::encode(&svc.signer_public()),
        "shows": "The finding's own files (its text, reproducer, expected output and the minimal sample of project files it was checked on), the check script, the signer's key, the digests of the capsule and of its release statement, and how many files the release has.",
        "hides": "Every other file of the release (name, size and content), who the release was made for, and which model route produced it.",
    }))
    .into_response()
}
