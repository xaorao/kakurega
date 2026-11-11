//! Notes from a meeting transcript: the owner pastes or drops the text, the answering model writes the points, and every
//! point is kept as a fact only when the words it quotes are in the transcript. Lines that hold a secret are never sent.

use super::*;
use kk_review::meeting;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MeetingBody {
    text: String,
    #[serde(default)]
    title: Option<String>,
}

const WITHHELD_LINE: &str = "[this line was withheld: it holds a secret]";

pub(super) async fn notes(axum::Extension(svc): Svc, Json(b): Json<MeetingBody>) -> Response {
    let refuse = |why: &str| error(StatusCode::UNPROCESSABLE_ENTITY, "MEETING_REFUSED", why);
    if b.text.trim().chars().count() < 40 {
        return refuse(
            "paste the transcript first: there is too little text here to take notes from",
        );
    }
    let title: String = b
        .title
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect();
    // a line that holds a secret is replaced in place, so line numbers stay the transcript's
    let mut withheld = 0usize;
    let transcript: String = b
        .text
        .lines()
        .map(|line| {
            if kk_secrets::holds_secret(line) {
                withheld += 1;
                WITHHELD_LINE
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let Some(parts) = meeting::parts(&transcript) else {
        return refuse("this transcript is too long to take notes from in one go; split it into two and do each half");
    };
    let svc2 = svc.clone();
    let started = std::time::Instant::now();
    let total = parts.len();
    let asked = tokio::task::spawn_blocking(move || {
        let mut replies: Vec<Option<String>> = Vec::new();
        let mut swapped = 0usize;
        let mut first_error: Option<String> = None;
        for (i, (_, part)) in parts.iter().enumerate() {
            let messages = vec![
                kk_types::chat::ChatMessage::new("system", meeting::SYSTEM),
                kk_types::chat::ChatMessage::new("user", &meeting::question(part, i, total)),
            ];
            match svc2.ask_counted(messages) {
                Ok((reply, n)) => {
                    swapped = swapped.max(n);
                    replies.push(Some(reply.text));
                }
                Err(e) => {
                    first_error.get_or_insert(e);
                    replies.push(None);
                }
            }
        }
        (replies, swapped, first_error)
    })
    .await;
    let Ok((replies, swapped, first_error)) = asked else {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "MEETING_FAILED",
            "the notes could not be written",
        );
    };
    if replies.iter().all(Option::is_none) {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "NO_MODEL",
            &first_error.unwrap_or_else(|| "no model answered".to_string()),
        );
    }
    let notes = meeting::assemble(&transcript, &replies);
    svc.log_event(
        "meeting.notes",
        "owner",
        &[
            ("parts", notes.parts.into()),
            ("checked", notes.checked.len().into()),
            ("unchecked", notes.unchecked.len().into()),
            ("withheld_lines", withheld.into()),
            ("swapped", swapped.into()),
        ],
    );
    let mut out = meeting::to_json(&notes);
    out["title"] = json!(title);
    out["markdown"] = json!(meeting::to_markdown(&title, &notes));
    out["withheld_lines"] = json!(withheld);
    out["swapped"] = json!(swapped);
    out["elapsed_ms"] = json!(started.elapsed().as_millis() as u64);
    out["route"] = svc.route_for_owner();
    out["limits"] = json!("A point is listed as checked when the words it quotes are in the transcript. That shows the words were said, not that the point reads them correctly. Something said in the meeting can be missing from the notes.");
    Json(out).into_response()
}
