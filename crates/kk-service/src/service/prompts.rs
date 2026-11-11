//! Prompts the owner saved to use again: a name and a text, kept in the encrypted store, put into the chat box on a
//! click. A prompt is the owner's own words for the model; nothing here is ever followed by itself.

use super::*;

const MAX_PROMPTS: i64 = 100;
const MAX_NAME_CHARS: usize = 60;
const MAX_TEXT_CHARS: usize = 4000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PromptBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    remove: Option<String>,
}

fn all(svc: &Service) -> Vec<Value> {
    let store = svc.store.lock().expect("lock");
    let c = store.connection();
    c.prepare("SELECT name, text, added FROM saved_prompts ORDER BY lower(name)")
        .and_then(|mut st| {
            st.query_map([], |r| {
                Ok(json!({"name": r.get::<_, String>(0)?, "text": r.get::<_, String>(1)?, "added_at": r.get::<_, i64>(2)?}))
            })
            .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default()
}

pub(super) async fn list(axum::Extension(svc): Svc) -> Response {
    Json(json!({"prompts": all(&svc), "max": MAX_PROMPTS, "max_chars": MAX_TEXT_CHARS}))
        .into_response()
}

/// Saves a prompt under a name (a name already in use is replaced), or removes one.
pub(super) async fn change(axum::Extension(svc): Svc, Json(b): Json<PromptBody>) -> Response {
    let refuse = |why: &str| error(StatusCode::UNPROCESSABLE_ENTITY, "PROMPT_REFUSED", why);
    let tidy = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let changed = match (&b.name, &b.text, &b.remove) {
        (None, None, Some(name)) => {
            let store = svc.store.lock().expect("lock");
            store
                .connection()
                .execute(
                    "DELETE FROM saved_prompts WHERE lower(name) = lower(?1)",
                    [tidy(name)],
                )
                .unwrap_or(0)
        }
        (Some(name), Some(text), None) => {
            let name = tidy(name);
            let text = text.trim().to_string();
            if name.is_empty()
                || name.chars().count() > MAX_NAME_CHARS
                || name.chars().any(char::is_control)
            {
                return refuse("a prompt's name is 1 to 60 characters");
            }
            if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
                return refuse("a prompt is 1 to 4,000 characters");
            }
            if kk_secrets::holds_secret(&text) || kk_secrets::holds_secret(&name) {
                return refuse("that holds what looks like a secret; a saved prompt is sent to the model every time it is used");
            }
            let store = svc.store.lock().expect("lock");
            let c = store.connection();
            let known: i64 = c
                .query_row(
                    "SELECT COUNT(*) FROM saved_prompts WHERE lower(name) = lower(?1)",
                    [&name],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let count: i64 = c
                .query_row("SELECT COUNT(*) FROM saved_prompts", [], |r| r.get(0))
                .unwrap_or(0);
            if known == 0 && count >= MAX_PROMPTS {
                return refuse("100 prompts are kept at most; remove one first");
            }
            let _ = c.execute(
                "DELETE FROM saved_prompts WHERE lower(name) = lower(?1)",
                [&name],
            );
            c.execute(
                "INSERT INTO saved_prompts(name, text, added) VALUES(?1, ?2, ?3)",
                kk_vault::rusqlite::params![name, text, now()],
            )
            .unwrap_or(0)
        }
        _ => return refuse("give a name and a text to save, or remove with a name"),
    };
    let prompts = all(&svc);
    if changed > 0 {
        // the log has how many there are, never a name or a word
        svc.log_event(
            if b.remove.is_some() {
                "prompt.removed"
            } else {
                "prompt.saved"
            },
            "owner",
            &[("prompts", prompts.len().into())],
        );
    }
    Json(json!({"changed": changed > 0, "prompts": prompts})).into_response()
}
