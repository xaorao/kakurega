//! What left this computer, and what was held back, read from the signed log: one list the owner can watch.
//! It holds counts, kinds and destinations. It never holds words: the log it is read from has none.

use super::*;

/// How far back in the log the list looks.
const WINDOW: usize = 1000;
const MAX_ROWS: usize = 200;

/// The row an entry of the log gives in the list, or nothing when the entry is not about something leaving or being held.
pub(super) fn row_of(e: &Value) -> Option<Value> {
    let d = &e["data"];
    let n = |k: &str| d[k].as_i64().unwrap_or(0);
    let subject = e["subject"].as_str().unwrap_or("");
    let (what, to, detail) = match e["kind"].as_str()? {
        "outbound.model" => (
            "model",
            subject.to_string(),
            json!({"chars": n("chars"), "placeholders": n("swapped")}),
        ),
        "search.run" => ("search", String::new(), json!({"results": n("results")})),
        "research.fetched" => ("pages", String::new(), json!({"pages": n("addresses")})),
        // "mcp.call" is an agent calling IN to this computer: it is listed under Activity, never here
        // an outside program on this computer was given arguments; it may pass on what it was given
        "mcp.called" => ("program", subject.to_string(), json!({"bytes": n("bytes")})),
        // a read from GitHub: the request (which repository, issue, file or search words) went to GitHub
        "connector.read" => (
            "connector",
            "GitHub".to_string(),
            json!({"action": d["action"], "items": n("items")}),
        ),
        "release.delivery_finished" => {
            ("release", subject.to_string(), json!({"state": d["state"]}))
        }
        // a question about a project, or a coding task, that worked with a model elsewhere: the files it read went there
        "outbound.agent" => ("agent", subject.to_string(), json!({"door": d["door"]})),
        // a review that began with a model elsewhere, written before any file went; what it used follows when it ends
        "outbound.review_started" => (
            "review",
            subject.to_string(),
            json!({"started": true, "job": d["job"]}),
        ),
        "outbound.review_used" => (
            "review_used",
            subject.to_string(),
            json!({"calls": n("calls"), "tokens": n("tokens")}),
        ),
        // logs written before reviews were recorded at their start hold one entry, made when the review ended
        "outbound.review" => (
            "review",
            subject.to_string(),
            json!({"calls": n("calls"), "tokens": n("tokens")}),
        ),
        "held.back" => (
            "held",
            subject.to_string(),
            json!({"secrets": n("secrets"), "instructions": n("instructions")}),
        ),
        // entries the log could not write: what they were is not known, so the list says how many are missing
        "log.unwritten" => (
            "gap",
            String::new(),
            json!({"entries": n("entries"), "since": n("since")}),
        ),
        _ => return None,
    };
    Some(json!({"at": e["at"], "seq": e["seq"], "what": what, "to": to, "detail": detail}))
}

impl Service {
    /// Written when something was kept from a model: how many parts held a secret and how many read like orders.
    pub(super) fn log_held_back(&self, door: &str, secrets: usize, instructions: usize) {
        if secrets + instructions > 0 {
            self.log_event(
                "held.back",
                door,
                &[
                    ("secrets", secrets.into()),
                    ("instructions", instructions.into()),
                ],
            );
        }
    }
}

pub(super) async fn list(axum::Extension(svc): Svc) -> Response {
    let entries: Vec<Value> = svc
        .log
        .recent(WINDOW)
        .unwrap_or_default()
        .iter()
        .filter_map(|e| serde_json::from_str::<Value>(e).ok())
        .collect();
    let rows: Vec<Value> = entries.iter().filter_map(row_of).collect();
    let mut totals = serde_json::Map::new();
    for what in [
        "model",
        "review",
        "search",
        "pages",
        "tool",
        "connector",
        "release",
        "held",
        "gap",
        "program",
        "agent",
    ] {
        totals.insert(
            what.into(),
            json!(rows.iter().filter(|r| r["what"] == what).count()),
        );
    }
    Json(json!({
        "rows": rows.iter().take(MAX_ROWS).collect::<Vec<_>>(),
        "totals": totals,
        "looked_at": entries.len(),
        "route": svc.route_for_owner(),
        "note": "Read from the signed log of this program. A question to a model on this computer is not listed: it does not leave. A roundtable is one entry for all its experts. Messages of the Telegram bridge are not listed here. Counts and destinations only; the log holds no words.",
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ob01_only_entries_about_leaving_or_holding_back_become_rows_and_no_row_holds_more_than_the_log_did(
    ) {
        let e = |kind: &str, subject: &str, data: Value| json!({"at": 5, "seq": 9, "kind": kind, "subject": subject, "data": data, "prev": "x", "v": 1});
        assert_eq!(
            row_of(&e(
                "outbound.model",
                "cloud-api.near.ai",
                json!({"chars": 1200, "swapped": 3})
            ))
            .unwrap(),
            json!({"at": 5, "seq": 9, "what": "model", "to": "cloud-api.near.ai", "detail": {"chars": 1200, "placeholders": 3}})
        );
        assert_eq!(
            row_of(&e(
                "held.back",
                "chat",
                json!({"secrets": 2, "instructions": 1})
            ))
            .unwrap()["detail"],
            json!({"secrets": 2, "instructions": 1})
        );
        // an agent calling in to this computer is not something that left it
        assert!(row_of(&e("mcp.call", "list_reviews", json!({"outcome": "ok"}))).is_none());
        assert!(row_of(&e("mcp.call", "unknown", json!({"outcome": "refused"}))).is_none());
        // a call out to a program on this computer is
        assert_eq!(
            row_of(&e("mcp.called", "search_issues", json!({"bytes": 12}))).unwrap()["to"],
            json!("search_issues")
        );
        assert_eq!(
            row_of(&e(
                "release.delivery_finished",
                "job-3",
                json!({"state": "delivered"})
            ))
            .unwrap()["what"],
            json!("release")
        );
        assert_eq!(
            row_of(&e(
                "outbound.review",
                "verified_tee",
                json!({"calls": 9, "tokens": 21000})
            ))
            .unwrap()["detail"],
            json!({"calls": 9, "tokens": 21000})
        );
        assert!(row_of(&e("telegram.sent", "s", json!({}))).is_none());
        for other in [
            "chat.turn",
            "memory.remembered",
            "route.chosen",
            "break.attempt",
            "",
        ] {
            assert!(row_of(&e(other, "s", json!({}))).is_none(), "{other}");
        }
        assert!(row_of(&json!({"no": "kind"})).is_none());
    }

    #[test]
    fn ob02_a_read_from_github_is_a_row_with_its_kind_and_count_and_no_words() {
        let e = json!({"kind": "connector.read", "subject": "github", "at": 5, "seq": 9, "data": {"action": "issue", "items": 3}});
        let r = row_of(&e).unwrap();
        assert_eq!(
            (
                r["what"].as_str(),
                r["to"].as_str(),
                r["detail"]["items"].as_i64()
            ),
            (Some("connector"), Some("GitHub"), Some(3)),
            "{r}"
        );
        assert!(row_of(
            &json!({"kind": "connector.read_refused", "subject": "github", "data": {}})
        )
        .is_none());
    }
}
