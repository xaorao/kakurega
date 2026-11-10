//! The owner's methods: named guidance texts that reach a conversation only after the owner approves their exact content.

use super::*;
use kk_methods::{Approval, Approved, Draft, Limits, Method, Refusal};

pub(super) const MAX_METHODS: i64 = 100;
const MAX_PROPOSALS: i64 = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApproveBody {
    name: String,
    hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdBody {
    id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NameBody {
    name: String,
}

fn refuse(why: &str) -> Response {
    error(StatusCode::UNPROCESSABLE_ENTITY, "METHOD_REFUSED", why)
}

fn method_from_row(body: &str, hash: &str) -> Option<Method> {
    let v: Value = serde_json::from_str(body).ok()?;
    let m = Method::new(Draft::from_json(&v).ok()?).ok()?;
    (m.hash == hash).then_some(m)
}

impl Service {
    /// Keeps a method a review suggested, as a proposal the owner can read. It must read as a valid method and hold no secret, or it
    /// is dropped; at most twenty wait at once. It is never a method until the owner keeps it, and never approved until they approve it.
    pub(super) fn method_propose(&self, job: &str, suggestion: &Value) {
        let Ok(draft) = Draft::from_json(suggestion) else {
            return;
        };
        let Ok(m) = Method::new(draft) else { return };
        let text = format!("{}\n{}", m.description, m.body);
        if !kk_secrets::scan(&text).is_empty() || kk_secrets::looks_secret(&text).is_some() {
            return;
        }
        let mut body = kk_methods::to_json(&m);
        if let Some(o) = body.as_object_mut() {
            o.remove("hash");
        }
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        let waiting: i64 = c
            .query_row("SELECT count(*) FROM method_proposals", [], |r| r.get(0))
            .unwrap_or(0);
        if waiting >= MAX_PROPOSALS {
            return;
        }
        let _ = c.execute(
            "INSERT INTO method_proposals(job, body, created) VALUES (?1, ?2, ?3)",
            kk_vault::rusqlite::params![job, body.to_string(), now()],
        );
    }

    /// Every saved method with the hash the owner approved for it, if any. A row that no longer reads as a valid method is
    /// left out; it cannot be used.
    pub(super) fn methods_all(&self) -> Vec<(Method, Option<String>, Option<i64>)> {
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        let Ok(mut st) = c.prepare(
            "SELECT m.body, m.hash, a.hash, a.approved_at FROM methods m LEFT JOIN method_approvals a ON a.name = m.name ORDER BY m.name",
        ) else {
            return vec![];
        };
        let rows: Vec<(String, String, Option<String>, Option<i64>)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map(|it| it.flatten().collect())
            .unwrap_or_default();
        rows.into_iter()
            .filter_map(|(body, hash, approved, at)| {
                method_from_row(&body, &hash).map(|m| (m, approved, at))
            })
            .collect()
    }

    /// The methods that are active now: approved for exactly their current content.
    pub(super) fn methods_active(&self) -> Vec<Approved> {
        self.methods_all()
            .into_iter()
            .filter_map(|(m, approved, _)| {
                let hash = approved?;
                let a = Approved {
                    approval: Approval {
                        name: m.name.clone(),
                        hash,
                    },
                    method: m,
                };
                a.active().then_some(a)
            })
            .collect()
    }

    /// The text to add to a conversation for this message, and the names of the methods it comes from.
    pub(super) fn methods_block(&self, message: &str) -> Option<(String, Vec<String>)> {
        let active = self.methods_active();
        if active.is_empty() {
            return None;
        }
        let chosen = kk_methods::select(message, &active, Limits::default()).chosen;
        if chosen.is_empty() {
            return None;
        }
        let names = chosen.iter().map(|a| a.method.name.clone()).collect();
        Some((kk_methods::render(&chosen), names))
    }

    fn method_save(&self, m: &Method) -> Result<(), String> {
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        let known: i64 = c
            .query_row(
                "SELECT count(*) FROM methods WHERE name = ?1",
                [&m.name],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let all: i64 = c
            .query_row("SELECT count(*) FROM methods", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if known == 0 && all >= MAX_METHODS {
            return Err(format!("at most {MAX_METHODS} methods can be saved"));
        }
        let mut body = kk_methods::to_json(m);
        if let Some(o) = body.as_object_mut() {
            o.remove("hash");
        }
        c.execute(
            "INSERT INTO methods(name, body, hash, updated_at) VALUES(?1, ?2, ?3, ?4) ON CONFLICT(name) DO UPDATE SET body = ?2, hash = ?3, updated_at = ?4",
            kk_vault::rusqlite::params![m.name, body.to_string(), m.hash, now()],
        )
        .map_err(|e| e.to_string())?;
        // an approval is of one exact content: a different hash leaves the method waiting for the owner again
        c.execute(
            "DELETE FROM method_approvals WHERE name = ?1 AND hash <> ?2",
            kk_vault::rusqlite::params![m.name, m.hash],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub(super) async fn methods_get(axum::Extension(svc): Svc) -> Response {
    let rows = svc.methods_all();
    Json(json!({
        "methods": rows.iter().map(|(m, approved, at)| {
            let mut v = kk_methods::to_json(m);
            v["approved"] = json!(approved.as_deref() == Some(m.hash.as_str()));
            v["approved_at"] = json!(at);
            v
        }).collect::<Vec<_>>(),
        "limits": {"methods": MAX_METHODS, "body_chars": kk_methods::MAX_BODY, "triggers": kk_methods::MAX_TRIGGERS},
    }))
    .into_response()
}

/// Saves or replaces a method. It is saved unapproved: changing the text of an approved method takes its approval away.
/// Only the owner's browser or terminal session reaches this.
pub(super) async fn methods_set(axum::Extension(svc): Svc, Json(v): Json<Value>) -> Response {
    let draft = match Draft::from_json(&v) {
        Ok(d) => d,
        Err(why) => return refuse(why.sentence()),
    };
    if draft.body.chars().count() > kk_methods::MAX_BODY * 2 {
        return refuse(Refusal::Body.sentence());
    }
    let method = match Method::new(draft) {
        Ok(m) => m,
        Err(why) => return refuse(why.sentence()),
    };
    let text = format!("{}\n{}", method.description, method.body);
    if !kk_secrets::scan(&text).is_empty() || kk_secrets::looks_secret(&text).is_some() {
        return refuse(
            "this looks like a secret; methods are sent to the model, so keep secrets out of them",
        );
    }
    if let Err(e) = svc.method_save(&method) {
        return refuse(&e);
    }
    svc.log_event(
        "method.saved",
        &method.name,
        &[("chars", method.body.chars().count().into())],
    );
    let approved = svc
        .methods_all()
        .iter()
        .any(|(m, a, _)| m.name == method.name && a.as_deref() == Some(m.hash.as_str()));
    Json(json!({"name": method.name, "hash": method.hash, "approved": approved})).into_response()
}

/// Approves one exact content: the hash must be the one the method has now, so the owner approves what they saw.
pub(super) async fn methods_approve(
    axum::Extension(svc): Svc,
    Json(b): Json<ApproveBody>,
) -> Response {
    let found = svc
        .methods_all()
        .into_iter()
        .find(|(m, _, _)| m.name == b.name);
    let Some((m, _, _)) = found else {
        return error(
            StatusCode::NOT_FOUND,
            "METHOD_NOT_FOUND",
            "there is no method with that name",
        );
    };
    if m.hash != b.hash {
        return error(
            StatusCode::CONFLICT,
            "METHOD_CHANGED",
            "the method changed after you read it; read it again before approving",
        );
    }
    let done = {
        let store = svc.store.lock().expect("lock");
        store.connection().execute(
            "INSERT INTO method_approvals(name, hash, approved_at) VALUES(?1, ?2, ?3) ON CONFLICT(name) DO UPDATE SET hash = ?2, approved_at = ?3",
            kk_vault::rusqlite::params![m.name, m.hash, now()],
        )
    };
    match done {
        Ok(_) => {
            svc.log_event("method.approved", &m.name, &[]);
            Json(json!({"name": m.name, "approved": true})).into_response()
        }
        Err(e) => refuse(&e.to_string()),
    }
}

pub(super) async fn methods_revoke(axum::Extension(svc): Svc, Json(b): Json<NameBody>) -> Response {
    let n = {
        let store = svc.store.lock().expect("lock");
        store
            .connection()
            .execute("DELETE FROM method_approvals WHERE name = ?1", [&b.name])
            .unwrap_or(0)
    };
    svc.log_event("method.revoked", &b.name, &[]);
    Json(json!({"name": b.name, "revoked": n > 0})).into_response()
}

pub(super) async fn methods_remove(axum::Extension(svc): Svc, Json(b): Json<NameBody>) -> Response {
    let n = {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        let _ = c.execute("DELETE FROM method_approvals WHERE name = ?1", [&b.name]);
        c.execute("DELETE FROM methods WHERE name = ?1", [&b.name])
            .unwrap_or(0)
    };
    svc.log_event("method.removed", &b.name, &[]);
    Json(json!({"name": b.name, "removed": n > 0})).into_response()
}

fn proposal_rows(svc: &Service) -> Vec<(i64, String, Method)> {
    let store = svc.store.lock().expect("lock");
    let c = store.connection();
    let Ok(mut st) = c.prepare("SELECT id, job, body FROM method_proposals ORDER BY id") else {
        return vec![];
    };
    let rows: Vec<(i64, String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map(|it| it.flatten().collect())
        .unwrap_or_default();
    rows.into_iter()
        .filter_map(|(id, job, body)| {
            let v: Value = serde_json::from_str(&body).ok()?;
            Some((id, job, Method::new(Draft::from_json(&v).ok()?).ok()?))
        })
        .collect()
}

pub(super) async fn proposals_get(axum::Extension(svc): Svc) -> Response {
    let list: Vec<Value> = proposal_rows(&svc)
        .into_iter()
        .map(|(id, job, m)| {
            let mut v = kk_methods::to_json(&m);
            v["id"] = json!(id);
            v["job"] = json!(job);
            v
        })
        .collect();
    Json(json!({"proposals": list})).into_response()
}

/// Turns a proposal into a saved method that still waits for the owner's approval. A name already in use is not replaced.
pub(super) async fn proposals_keep(axum::Extension(svc): Svc, Json(b): Json<IdBody>) -> Response {
    let Some((id, _, m)) = proposal_rows(&svc)
        .into_iter()
        .find(|(id, _, _)| *id == b.id)
    else {
        return error(
            StatusCode::NOT_FOUND,
            "PROPOSAL_NOT_FOUND",
            "there is no suggestion with that number",
        );
    };
    if svc.methods_all().iter().any(|(x, _, _)| x.name == m.name) {
        return error(
            StatusCode::CONFLICT,
            "METHOD_EXISTS",
            "a method with that name already exists; dismiss this suggestion or remove the old method first",
        );
    }
    if let Err(e) = svc.method_save(&m) {
        return refuse(&e);
    }
    {
        let store = svc.store.lock().expect("lock");
        let _ = store
            .connection()
            .execute("DELETE FROM method_proposals WHERE id = ?1", [id]);
    }
    svc.log_event("method.kept_from_suggestion", &m.name, &[]);
    Json(json!({"name": m.name, "hash": m.hash, "approved": false})).into_response()
}

pub(super) async fn proposals_dismiss(
    axum::Extension(svc): Svc,
    Json(b): Json<IdBody>,
) -> Response {
    let n = {
        let store = svc.store.lock().expect("lock");
        store
            .connection()
            .execute("DELETE FROM method_proposals WHERE id = ?1", [b.id])
            .unwrap_or(0)
    };
    if n == 0 {
        return error(
            StatusCode::NOT_FOUND,
            "PROPOSAL_NOT_FOUND",
            "there is no suggestion with that number",
        );
    }
    svc.log_event("method.suggestion_dismissed", &b.id.to_string(), &[]);
    Json(json!({"dismissed": true})).into_response()
}
