//! Projects as things of their own: a name, optionally the folder that holds its documents, and what belongs to it
//! (its kept conversations, its instructions, whether it answers only from its own material). Until now a project was
//! only a word typed into a box; this gives it a list, a page and a folder.

use super::*;

const MAX_PROJECTS: i64 = 200;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectBody {
    #[serde(default)]
    name: Option<String>,
    /// The folder that holds the project's documents, inside a folder this program may read. Empty text takes it away.
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    remove: Option<String>,
}

fn all(svc: &Service) -> Vec<Value> {
    let store = svc.store.lock().expect("lock");
    let c = store.connection();
    // every name a project is known by: made here, or already used for a conversation or a setting
    let mut names: std::collections::BTreeMap<String, (Option<String>, i64, bool)> =
        std::collections::BTreeMap::new();
    if let Ok(mut st) = c.prepare("SELECT name, folder, created FROM projects") {
        if let Ok(rows) = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        }) {
            for (n, f, at) in rows.flatten() {
                names.insert(n, (f, at, true));
            }
        }
    }
    for sql in [
        "SELECT DISTINCT project FROM sessions WHERE project IS NOT NULL AND project != ''",
        "SELECT project FROM project_settings",
    ] {
        if let Ok(mut st) = c.prepare(sql) {
            if let Ok(rows) = st.query_map([], |r| r.get::<_, String>(0)) {
                for n in rows.flatten() {
                    names.entry(n).or_insert((None, 0, false));
                }
            }
        }
    }
    names
        .into_iter()
        .map(|(name, (folder, created, listed))| {
            let conversations: i64 = c
                .query_row("SELECT COUNT(*) FROM sessions WHERE project = ?1", [&name], |r| r.get(0))
                .unwrap_or(0);
            let notes_only: bool = c
                .query_row("SELECT notes_only FROM project_settings WHERE project = ?1", [&name], |r| r.get::<_, i64>(0))
                .map(|n| n != 0)
                .unwrap_or(false);
            // a folder that has since gone, or is no longer inside an allowed folder, is said to be gone and not used
            let folder_ok = folder.as_deref().is_some_and(|f| svc.allowed_dir(f).is_some());
            json!({"name": name, "folder": folder, "folder_available": folder_ok, "conversations": conversations, "notes_only": notes_only, "created": created, "listed": listed})
        })
        .collect()
}

pub(super) async fn list(axum::Extension(svc): Svc) -> Response {
    Json(json!({"projects": all(&svc), "max": MAX_PROJECTS})).into_response()
}

/// Makes a project, sets or clears its folder, or takes it off the list.
pub(super) async fn change(axum::Extension(svc): Svc, Json(b): Json<ProjectBody>) -> Response {
    let refuse = |why: &str| error(StatusCode::UNPROCESSABLE_ENTITY, "PROJECT_REFUSED", why);
    let valid = |n: &str| kk_memory::valid_project(n) && n != "." && n != "..";
    match (&b.name, &b.remove) {
        (None, Some(name)) => {
            if !valid(name) {
                return refuse("a project name is a folder name of 1 to 80 plain characters");
            }
            let n = {
                let store = svc.store.lock().expect("lock");
                store
                    .connection()
                    .execute("DELETE FROM projects WHERE name = ?1", [name])
                    .unwrap_or(0)
            };
            if n > 0 {
                svc.log_event("project.removed", "owner", &[]);
            }
            // its conversations, notes and instructions are the owner's and are not deleted with the list entry
            Json(json!({"changed": n > 0, "projects": all(&svc), "kept": "The project's conversations, notes and instructions were not deleted. Forget them where they are shown."})).into_response()
        }
        (Some(name), None) => {
            if !valid(name) {
                return refuse("a project name is a folder name of 1 to 80 plain characters");
            }
            let folder = match b.folder.as_deref().map(str::trim) {
                None => None,
                Some("") => Some(None),
                Some(f) => match svc.allowed_dir(f) {
                    Some(dir) => Some(Some(dir.to_string_lossy().to_string())),
                    None => {
                        return refuse("that folder is not inside a folder this program may read")
                    }
                },
            };
            {
                let store = svc.store.lock().expect("lock");
                let c = store.connection();
                let known: i64 = c
                    .query_row(
                        "SELECT COUNT(*) FROM projects WHERE name = ?1",
                        [name],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                let count: i64 = c
                    .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
                    .unwrap_or(0);
                if known == 0 && count >= MAX_PROJECTS {
                    return refuse("200 projects are listed at most; remove one first");
                }
                let _ = c.execute(
                    "INSERT INTO projects(name, folder, created) VALUES(?1, NULL, ?2) ON CONFLICT(name) DO NOTHING",
                    kk_vault::rusqlite::params![name, now()],
                );
                if let Some(f) = &folder {
                    let _ = c.execute(
                        "UPDATE projects SET folder = ?2 WHERE name = ?1",
                        kk_vault::rusqlite::params![name, f],
                    );
                }
            }
            // the log has that a project was made or changed, never its name or its folder
            svc.log_event(
                "project.saved",
                "owner",
                &[(
                    "has_folder",
                    i64::from(matches!(folder, Some(Some(_)))).into(),
                )],
            );
            Json(json!({"changed": true, "projects": all(&svc)})).into_response()
        }
        _ => refuse("give a name to make or change a project, or remove with a name"),
    }
}
