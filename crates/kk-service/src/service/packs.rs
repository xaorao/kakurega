//! Signed method packs: the publishers the owner pinned and the packs imported from them.

use super::*;
use kk_evidence::keys::{KeyId, TrustedKeys};
use kk_packs::{Context, Version};

const MAX_PUBLISHERS: i64 = 10;
const MAX_LABEL: usize = 80;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PublisherBody {
    public: String,
    label: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct KeyBody {
    key_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImportBody {
    pack: String,
    /// The publisher's signed catalog this pack was chosen from, when it was: the pack must then be exactly a file the
    /// catalog lists, under the name and version it lists.
    #[serde(default)]
    catalog: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogBody {
    catalog: String,
}

fn refuse(why: &str) -> Response {
    error(StatusCode::UNPROCESSABLE_ENTITY, "PACK_REFUSED", why)
}

impl Service {
    fn pack_publishers(&self) -> Vec<(String, Vec<u8>, String, i64)> {
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        let Ok(mut st) = c.prepare(
            "SELECT key_id, public, label, added FROM pack_publishers ORDER BY added, key_id",
        ) else {
            return vec![];
        };
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map(|it| it.flatten().collect())
            .unwrap_or_default()
    }

    fn pack_trusted(&self) -> TrustedKeys {
        let mut trusted = TrustedKeys::new();
        for (_, public, _, _) in self.pack_publishers() {
            if let Ok(k) = <[u8; 32]>::try_from(public.as_slice()) {
                trusted.add(&k);
            }
        }
        trusted
    }

    /// Checks a catalog against the pinned publishers and the newest list already seen from the same one, and remembers
    /// its issue time, so an earlier list from that publisher is refused from then on.
    fn catalog_checked(&self, text: &str) -> Result<kk_packs::catalog::Catalog, String> {
        let seen = |key_id: &str| -> Option<i64> {
            let store = self.store.lock().expect("lock");
            store
                .connection()
                .query_row(
                    "SELECT issued FROM catalog_seen WHERE key_id = ?1",
                    [key_id],
                    |r| r.get(0),
                )
                .ok()
        };
        let c =
            kk_packs::catalog::check_catalog(text.as_bytes(), &self.pack_trusted(), now(), &seen)
                .map_err(|r| {
                r.sentence()
                    .replace("the pack", "the catalog")
                    .replace("one you already imported", "one you already opened")
            })?;
        let store = self.store.lock().expect("lock");
        let _ = store.connection().execute(
            "INSERT INTO catalog_seen(key_id, issued, at) VALUES (?1, ?2, ?3) ON CONFLICT(key_id) DO UPDATE SET issued = max(issued, ?2), at = ?3",
            kk_vault::rusqlite::params![c.key_id.as_str(), c.issued, now()],
        );
        Ok(c)
    }

    fn pack_seen_version(&self, name: &str, key_id: &str) -> Option<Version> {
        let store = self.store.lock().expect("lock");
        let v: String = store
            .connection()
            .query_row(
                "SELECT version FROM pack_seen WHERE name = ?1 AND key_id = ?2",
                [name, key_id],
                |r| r.get(0),
            )
            .ok()?;
        Version::parse(&v)
    }
}

pub(super) async fn packs_get(axum::Extension(svc): Svc) -> Response {
    let publishers: Vec<Value> = svc
        .pack_publishers()
        .into_iter()
        .map(|(id, public, label, added)| {
            let key: Option<[u8; 32]> = public.as_slice().try_into().ok();
            json!({"key_id": id, "public": key.map(|k| kk_evidence::keys::public_key_b64(&k)), "label": label, "added": added})
        })
        .collect();
    let seen: Vec<Value> = {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        c.prepare("SELECT name, version, at FROM pack_seen ORDER BY name")
            .ok()
            .map(|mut st| {
                st.query_map([], |r| Ok(json!({"name": r.get::<_, String>(0)?, "version": r.get::<_, String>(1)?, "at": r.get::<_, i64>(2)?})))
                    .map(|it| it.flatten().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    };
    Json(json!({"publishers": publishers, "imported": seen, "limits": {"publishers": MAX_PUBLISHERS}})).into_response()
}

/// Pins a publisher's public key. Pinning a key again changes its label. A small-order key is refused: it could sign anything.
pub(super) async fn packs_publisher_add(
    axum::Extension(svc): Svc,
    Json(b): Json<PublisherBody>,
) -> Response {
    let label = b.label.trim();
    if label.is_empty()
        || label.chars().count() > MAX_LABEL
        || label.chars().any(|c| c.is_control())
        || kk_types::injection::hidden_chars(label)
    {
        return refuse("a label is 1 to 80 plain characters");
    }
    let Ok(raw) = kk_evidence::b64::decode(&b.public) else {
        return refuse("the key is not 43 characters of base64");
    };
    let Ok(public): Result<[u8; 32], _> = raw.as_slice().try_into() else {
        return refuse("the key must be 32 bytes");
    };
    let mut keys = TrustedKeys::new();
    let Some(id) = keys.add(&public) else {
        return refuse("that key cannot be used to sign packs");
    };
    let done = {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        let known: i64 = c
            .query_row(
                "SELECT count(*) FROM pack_publishers WHERE key_id = ?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let all: i64 = c
            .query_row("SELECT count(*) FROM pack_publishers", [], |r| r.get(0))
            .unwrap_or(0);
        if known == 0 && all >= MAX_PUBLISHERS {
            return refuse(&format!(
                "at most {MAX_PUBLISHERS} publishers can be pinned"
            ));
        }
        c.execute(
            "INSERT INTO pack_publishers(key_id, public, label, added) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(key_id) DO UPDATE SET label = ?3",
            kk_vault::rusqlite::params![id.as_str(), public.to_vec(), label, now()],
        )
    };
    match done {
        Ok(_) => {
            svc.log_event("pack.publisher_pinned", id.as_str(), &[]);
            Json(json!({"key_id": id.as_str(), "label": label})).into_response()
        }
        Err(e) => refuse(&e.to_string()),
    }
}

pub(super) async fn packs_publisher_remove(
    axum::Extension(svc): Svc,
    Json(b): Json<KeyBody>,
) -> Response {
    let n = {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        // what a publisher's packs suggested goes with the publisher: it is not shown as coming from a pack the owner trusts
        let _ = c.execute(
            "DELETE FROM pack_suggestions WHERE key_id = ?1",
            [&b.key_id],
        );
        c.execute("DELETE FROM pack_publishers WHERE key_id = ?1", [&b.key_id])
            .unwrap_or(0)
    };
    svc.log_event("pack.publisher_removed", &b.key_id, &[]);
    Json(json!({"removed": n > 0})).into_response()
}

/// Imports a pack: every method is saved unapproved, or none is. A name already in use stops the whole import.
pub(super) async fn packs_import(axum::Extension(svc): Svc, Json(b): Json<ImportBody>) -> Response {
    let trusted = svc.pack_trusted();
    let seen = |name: &str, key_id: &str| svc.pack_seen_version(name, key_id);
    let cx = Context {
        trusted: &trusted,
        now: now(),
        seen: &seen,
    };
    let checked = match kk_packs::check_pack(b.pack.as_bytes(), &cx) {
        Ok(c) => c,
        Err(r) => return refuse(&r.sentence()),
    };
    if let Some(text) = &b.catalog {
        let catalog = match svc.catalog_checked(text) {
            Ok(c) => c,
            Err(why) => return refuse(&why),
        };
        match kk_packs::catalog::entry_for(&catalog, b.pack.as_bytes()) {
            Some(e) if e.name == checked.name && e.version == checked.version && catalog.key_id == checked.key_id => {}
            Some(_) => return refuse("the catalog lists this file under another name, version or publisher; nothing was imported"),
            None => return refuse("this file is not one the catalog lists (its SHA-256 differs); nothing was imported"),
        }
    }
    let text_all: String = checked
        .methods
        .iter()
        .map(|m| format!("{}\n{}\n", m.description, m.body))
        .chain(
            checked
                .servers
                .iter()
                .map(|s| format!("{}\n{}\n{}\n", s.args.join("\n"), s.program, s.why)),
        )
        .collect();
    if !kk_secrets::scan(&text_all).is_empty() || kk_secrets::looks_secret(&text_all).is_some() {
        return refuse("a method in the pack looks like it holds a secret, so the pack is refused");
    }
    let existing: Vec<String> = svc
        .methods_all()
        .into_iter()
        .map(|(m, _, _)| m.name.to_lowercase())
        .collect();
    if let Some(clash) = checked
        .methods
        .iter()
        .find(|m| existing.contains(&m.name.to_lowercase()))
    {
        return error(
            StatusCode::CONFLICT,
            "PACK_NAME_EXISTS",
            &format!(
                "a method named \"{}\" already exists; nothing was imported",
                clash.name
            ),
        );
    }
    let total: i64 = {
        let store = svc.store.lock().expect("lock");
        store
            .connection()
            .query_row("SELECT count(*) FROM methods", [], |r| r.get(0))
            .unwrap_or(0)
    };
    if total + checked.methods.len() as i64 > super::methods::MAX_METHODS {
        return refuse("importing this pack would pass the limit of saved methods");
    }
    for m in &checked.methods {
        if let Err(e) = svc.method_save(m) {
            for done in &checked.methods {
                let store = svc.store.lock().expect("lock");
                let c = store.connection();
                let _ = c.execute("DELETE FROM methods WHERE name = ?1", [&done.name]);
                // no name of the pack was in use before it (checked above), so every earlier text kept for it is the pack's
                let _ = c.execute("DELETE FROM method_versions WHERE name = ?1", [&done.name]);
            }
            return refuse(&e);
        }
    }
    // the outside programs the pack suggests are kept as suggestions; nothing is added or run
    {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        // a newer version of the same publisher's pack replaces its suggestions; another publisher's pack of that name keeps its own
        let _ = c.execute(
            "DELETE FROM pack_suggestions WHERE pack = ?1 AND key_id = ?2",
            kk_vault::rusqlite::params![checked.name, checked.key_id.as_str()],
        );
        for s in &checked.servers {
            let body = json!({"program": s.program, "args": s.args, "env": s.env, "why": s.why})
                .to_string();
            let _ = c.execute(
                "INSERT INTO pack_suggestions(pack, name, key_id, body, dismissed, at) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
                kk_vault::rusqlite::params![checked.name, s.name, checked.key_id.as_str(), body, now()],
            );
        }
    }
    {
        let store = svc.store.lock().expect("lock");
        let _ = store.connection().execute(
            "INSERT INTO pack_seen(name, version, key_id, at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(name, key_id) DO UPDATE SET version = ?2, at = ?4",
            kk_vault::rusqlite::params![checked.name, checked.version.to_string(), checked.key_id.as_str(), now()],
        );
    }
    svc.log_event(
        "pack.imported",
        &checked.name,
        &[
            ("methods", checked.methods.len().into()),
            ("suggested_programs", checked.servers.len().into()),
        ],
    );
    let _: &KeyId = &checked.key_id;
    Json(json!({
        "pack": checked.name,
        "version": checked.version.to_string(),
        "imported": checked.methods.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
        "suggested_programs": checked.servers.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
    }))
    .into_response()
}

/// Reads a publisher's signed catalog and says, for each pack it lists, what the owner already has. Nothing is imported
/// and nothing is fetched: the owner then chooses the pack files themselves, and each must be the exact file listed.
pub(super) async fn catalog_read(
    axum::Extension(svc): Svc,
    Json(b): Json<CatalogBody>,
) -> Response {
    let catalog = match svc.catalog_checked(&b.catalog) {
        Ok(c) => c,
        Err(why) => return refuse(&why),
    };
    let publisher = svc
        .pack_publishers()
        .into_iter()
        .find(|(id, _, _, _)| id == catalog.key_id.as_str())
        .map(|(_, _, label, _)| label)
        .unwrap_or_default();
    let entries: Vec<Value> = catalog
        .entries
        .iter()
        .map(|e| {
            let have = svc.pack_seen_version(&e.name, catalog.key_id.as_str());
            let state = match have {
                Some(v) if v == e.version => "imported",
                Some(v) if v < e.version => "update_available",
                Some(_) => "older_listed",
                None => "new",
            };
            json!({
                "name": e.name, "version": e.version.to_string(), "description": e.description, "file": e.file,
                "sha256": e.sha256, "methods": e.methods, "state": state, "have": have.map(|v| v.to_string()),
            })
        })
        .collect();
    svc.log_event(
        "pack.catalog_opened",
        catalog.key_id.as_str(),
        &[("packs", entries.len().into())],
    );
    Json(json!({"publisher": publisher, "key_id": catalog.key_id.as_str(), "issued": catalog.issued, "not_after": catalog.not_after, "packs": entries})).into_response()
}
