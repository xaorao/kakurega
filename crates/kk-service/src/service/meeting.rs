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
    let (transcript, withheld) = withhold_secret_lines(&b.text);
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
    svc.log_held_back("meeting notes", withheld, 0);
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

/// Text about to go to a model, with every line that reads like a password or code said in a sentence replaced in place,
/// and how many lines that was. Used for passages of the owner's documents, where one line can go without the rest.
pub(super) fn hide_spoken(text: &str) -> (String, usize) {
    let mut n = 0usize;
    let out: Vec<&str> = text
        .lines()
        .map(|line| {
            if kk_secrets::spoken::holds(line) {
                n += 1;
                "[this line was withheld: it reads like a password or code]"
            } else {
                line
            }
        })
        .collect();
    (out.join("\n"), n)
}

/// What is left of a line once a speaker's name in front of it is taken away ("Asha: ...").
fn spoken(line: &str) -> &str {
    match line.split_once(':') {
        Some((who, rest)) if who.chars().count() <= 40 && !who.contains("//") => rest.trim(),
        _ => line.trim(),
    }
}

fn key_material(line: &str) -> bool {
    let s = spoken(line);
    s.len() >= 16
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'))
}

/// The transcript with every line that holds a secret replaced in place, so line numbers stay the transcript's, and how
/// many were replaced. A key block read out over several lines is withheld whole: from its BEGIN line to its END line, or
/// for as long as the lines are nothing but key material.
pub(super) fn withhold_secret_lines(text: &str) -> (String, usize) {
    let hits = kk_secrets::scan(text);
    let mut withheld = 0usize;
    let mut in_block = false;
    let mut at = 0usize;
    let mut out: Vec<&str> = Vec::new();
    for raw in text.split_inclusive('\n') {
        let (start, end) = (at, at + raw.len());
        at = end;
        let line = raw.trim_end_matches(['\n', '\r']);
        let begins = line.contains("-----BEGIN") && line.contains("KEY");
        let ends = line.contains("-----END");
        let touched = hits.iter().any(|h| h.start < end && start < h.end);
        let hold = if in_block {
            if ends {
                in_block = false;
                true
            } else if key_material(line) || line.trim().is_empty() {
                true
            } else {
                in_block = false;
                touched || kk_secrets::holds_secret(line)
            }
        } else {
            touched || begins || kk_secrets::holds_secret(line) || kk_secrets::spoken::holds(line)
        };
        if begins && !ends {
            in_block = true;
        }
        if hold {
            withheld += 1;
            out.push(WITHHELD_LINE);
        } else {
            out.push(line);
        }
    }
    (out.join("\n"), withheld)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt30_a_key_block_read_out_over_several_lines_is_withheld_whole_and_line_numbers_stay() {
        let body: Vec<String> = (0..6)
            .map(|i| {
                format!(
                    "{}{}",
                    "Qk1hbnlMaW5lc09mS2V5TWF0ZXJpYWw",
                    "x".repeat(30 + i)
                )
            })
            .collect();
        let mut lines = vec![
            "Asha: here is the key".to_string(),
            format!("Asha: -----BEGIN RSA {} KEY-----", "PRIVATE"),
        ];
        lines.extend(body.iter().map(|b| format!("Asha: {b}")));
        lines.push(format!("Asha: -----END RSA {} KEY-----", "PRIVATE"));
        lines.push("Ben: thanks, I will rotate it.".to_string());
        let (out, n) = withhold_secret_lines(&lines.join("\n"));
        let got: Vec<&str> = out.lines().collect();
        assert_eq!(got.len(), lines.len());
        assert_eq!(n, 8, "{out}");
        assert_eq!(got[0], "Asha: here is the key");
        assert_eq!(*got.last().unwrap(), "Ben: thanks, I will rotate it.");
        assert!(body.iter().all(|b| !out.contains(b.as_str())), "{out}");
        // a block that never ends stops being withheld at the first ordinary sentence
        let open = format!("-----BEGIN {} KEY-----\n{}\nBen: that was the key, now the agenda.\nCara: agreed on Friday.", "PRIVATE", body[0]);
        let (out, n) = withhold_secret_lines(&open);
        assert_eq!(n, 2, "{out}");
        assert!(
            out.ends_with("Ben: that was the key, now the agenda.\nCara: agreed on Friday."),
            "{out}"
        );
        // ordinary talk is untouched, also when it mentions keys
        let plain = "Asha: the key point is pricing.\nBen: BEGIN with the roadmap, END with Q&A.";
        assert_eq!(withhold_secret_lines(plain), (plain.to_string(), 0));
    }
}
