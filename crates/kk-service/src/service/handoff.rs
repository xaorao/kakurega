//! The owner hands a piece of text to the coding agent installed on this computer (Claude Code). Two steps, never one:
//! first the exact text is shown back with what it holds and where it will go; only a second, separate yes sends it.
//! What is sent is those bytes and nothing else. It is not private, and every screen and reply says so.

use super::*;

const MAX_CHARS: usize = 20_000;
const KEEP_SECS: i64 = 600;
pub(super) const NOTICE: &str = "This is not private. The text below goes to the coding agent installed on this computer, which sends it to its maker under your own account, together with its own instructions. Your settings, hooks and instruction files for it are not loaded. It is run for one answer with all of its tools switched off, in an empty folder: it is given this text and nothing else from this program.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HandoffBody {
    text: String,
}

pub(super) async fn status(axum::Extension(svc): Svc) -> Response {
    let svc2 = svc.clone();
    let tool = tokio::task::spawn_blocking(move || svc2.runner.handoff_tool())
        .await
        .ok()
        .flatten();
    Json(json!({"available": tool.is_some(), "tool": tool, "notice": NOTICE, "max_chars": MAX_CHARS})).into_response()
}

/// Step one: nothing is sent. The text is checked, kept for ten minutes under an id, and shown back exactly.
pub(super) async fn propose(axum::Extension(svc): Svc, Json(b): Json<HandoffBody>) -> Response {
    let refuse = |why: &str| error(StatusCode::UNPROCESSABLE_ENTITY, "HANDOFF_REFUSED", why);
    let text = b.text.trim().to_string();
    if text.is_empty() {
        return refuse("write what you want to hand over first");
    }
    if text.chars().count() > MAX_CHARS {
        return refuse("that is longer than can be handed over in one go (20,000 characters)");
    }
    if kk_secrets::holds_secret(&text) || kk_secrets::spoken::holds(&text) {
        return refuse("that holds what looks like a key or a password; take it out first, because this text leaves your computer");
    }
    let svc2 = svc.clone();
    let Some(tool) = tokio::task::spawn_blocking(move || svc2.runner.handoff_tool())
        .await
        .ok()
        .flatten()
    else {
        return error(
            StatusCode::CONFLICT,
            "NO_AGENT",
            "no coding agent that text can be handed to is installed on this computer",
        );
    };
    let mut raw = [0u8; 8];
    getrandom::fill(&mut raw).expect("operating system randomness");
    let id = format!(
        "ho-{}",
        raw.iter().map(|x| format!("{x:02x}")).collect::<String>()
    );
    let digest = kk_evidence::capsule::sha256_hex(text.as_bytes());
    {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        let _ = c.execute("DELETE FROM handoffs WHERE at < ?1", [now() - KEEP_SECS]);
        let _ = c.execute(
            "INSERT INTO handoffs(id, text, at) VALUES(?1, ?2, ?3)",
            kk_vault::rusqlite::params![id, text, now()],
        );
    }
    let personal = kk_secrets::pii::describe(&kk_secrets::pii::scan(&text));
    svc.log_event(
        "handoff.proposed",
        "owner",
        &[("chars", text.chars().count().into())],
    );
    Json(json!({
        "id": id,
        "text": text,
        "sha256": digest,
        "chars": text.chars().count(),
        "personal": personal,
        "tool": tool,
        "notice": NOTICE,
        "valid_for_seconds": KEEP_SECS,
    }))
    .into_response()
}

/// Step two: the owner said yes to that id. The kept text is sent as it was shown, once.
pub(super) async fn run(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    if !id.starts_with("ho-") || id.len() != 19 || !id[3..].chars().all(|c| c.is_ascii_hexdigit()) {
        return error(
            StatusCode::NOT_FOUND,
            "NO_HANDOFF",
            "there is no such hand-over",
        );
    }
    // taken out of the store before it is sent, so a second yes cannot send it twice
    let kept: Option<(String, i64)> = {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        let t = c
            .query_row(
                "SELECT text, at FROM handoffs WHERE id = ?1 AND at >= ?2",
                kk_vault::rusqlite::params![id, now() - KEEP_SECS],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        let _ = c.execute("DELETE FROM handoffs WHERE id = ?1", [&id]);
        t
    };
    let Some((text, shown_at)) = kept else {
        return error(StatusCode::NOT_FOUND, "NO_HANDOFF", "that hand-over was already sent, was cancelled, or is older than ten minutes; show the text again first");
    };
    let chars = text.chars().count();
    if !svc.log_event(
        "outbound.model",
        "Claude Code on this computer, then its maker",
        &[("chars", chars.into()), ("swapped", 0usize.into())],
    ) {
        // kept again, so the same yes works once there is room for the record
        {
            let store = svc.store.lock().expect("lock");
            let _ = store.connection().execute(
                "INSERT INTO handoffs(id, text, at) VALUES(?1, ?2, ?3)",
                kk_vault::rusqlite::params![id, text, shown_at],
            );
        }
        return error(
            StatusCode::INSUFFICIENT_STORAGE,
            "LOG_UNAVAILABLE",
            super::swap::LOG_UNWRITABLE,
        );
    }
    let svc2 = svc.clone();
    let started = std::time::Instant::now();
    let out = tokio::task::spawn_blocking(move || svc2.runner.handoff_run(&text)).await;
    match out {
        Ok(Ok(reply)) => {
            svc.log_event(
                "handoff.answered",
                "owner",
                &[("chars_out", reply.chars().count().into())],
            );
            Json(json!({"reply": reply, "chars_sent": chars, "elapsed_ms": started.elapsed().as_millis() as u64, "notice": NOTICE})).into_response()
        }
        Ok(Err(why)) => error(StatusCode::BAD_GATEWAY, "HANDOFF_FAILED", &why),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "HANDOFF_FAILED",
            "the hand-over stopped unexpectedly",
        ),
    }
}

/// The owner changed their mind: the kept text is dropped.
pub(super) async fn cancel(axum::Extension(svc): Svc, UrlPath(id): UrlPath<String>) -> Response {
    let store = svc.store.lock().expect("lock");
    let n = store
        .connection()
        .execute("DELETE FROM handoffs WHERE id = ?1", [&id])
        .unwrap_or(0);
    Json(json!({"cancelled": n > 0})).into_response()
}
