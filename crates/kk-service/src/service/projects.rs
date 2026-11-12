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
    /// How far its material may go: "any" (the model you choose), "confidential" (confidential hardware or this computer),
    /// "here" (only a model on this computer).
    #[serde(default)]
    floor: Option<String>,
}

/// The words for a level a project allows, and back.
fn floor_word(level: i64) -> &'static str {
    match level {
        3 => "here",
        2 => "confidential",
        _ => "any",
    }
}

fn floor_level(word: &str) -> Option<i64> {
    match word {
        "here" => Some(3),
        "confidential" => Some(2),
        "any" => Some(0),
        _ => None,
    }
}

/// What the owner is told when a project's material would go to a model less private than the project allows.
pub(super) fn floor_words(project: &str) -> String {
    format!("the project {project} is kept more privately than the model in use now allows, so none of its material is sent to it; choose a more private model in Settings, or change where the project's work may go under Projects")
}

impl Service {
    /// The least private model a project's material may be given to, as a route level (see `route_level`): 0 any, 2
    /// confidential hardware or this computer, 3 only this computer. A project with no choice made is 0.
    pub(super) fn project_floor(&self, project: Option<&str>) -> i64 {
        let Some(p) = project.filter(|p| !p.is_empty()) else {
            return 0;
        };
        let store = self.store.lock().expect("lock");
        store
            .connection()
            .query_row(
                "SELECT level FROM project_floor WHERE project = ?1",
                [p],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// The highest floor of the projects whose folder holds this path, with that project's name.
    pub(super) fn folder_floor_named(&self, path: &std::path::Path) -> Option<(String, i64)> {
        let rows: Vec<(String, String, i64)> = {
            let store = self.store.lock().expect("lock");
            let c = store.connection();
            c.prepare("SELECT p.name, p.folder, f.level FROM projects p JOIN project_floor f ON f.project = p.name WHERE p.folder IS NOT NULL")
                .and_then(|mut st| {
                    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                        .map(|it| it.flatten().collect())
                })
                .unwrap_or_default()
        };
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        rows.into_iter()
            .filter(|(_, folder, _)| {
                let f = std::path::Path::new(folder);
                let f = f.canonicalize().unwrap_or_else(|_| f.to_path_buf());
                // the project's folder, anything inside it, or a folder that holds it all
                path.starts_with(&f) || f.starts_with(&path)
            })
            .max_by_key(|(_, _, level)| *level)
            .map(|(name, _, level)| (name, level))
    }

    pub(super) fn folder_floor(&self, path: &std::path::Path) -> i64 {
        self.folder_floor_named(path).map_or(0, |(_, l)| l)
    }

    /// The project (by name, or by holding this folder) whose floor is above the route in use now, if any.
    pub(super) fn floor_above_route(
        &self,
        project: Option<&str>,
        folder: Option<&std::path::Path>,
    ) -> Option<(String, i64)> {
        let level = self.route_level();
        if let Some(p) = project.filter(|p| !p.is_empty()) {
            let f = self.project_floor(Some(p));
            if f > level {
                return Some((p.to_string(), f));
            }
        }
        folder
            .and_then(|d| self.folder_floor_named(d))
            .filter(|(_, f)| *f > level)
    }

    pub(super) fn floor_refusal(
        &self,
        project: Option<&str>,
        folder: Option<&std::path::Path>,
    ) -> Option<Response> {
        self.floor_above_route(project, folder).map(|(name, _)| {
            error(
                StatusCode::CONFLICT,
                "PROJECT_KEPT_PRIVATELY",
                &floor_words(&name),
            )
        })
    }
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
        // a project taken off the list keeps where its work may go (its conversations and notes are kept too), so the
        // setting stays in sight and can be changed
        "SELECT project FROM project_floor",
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
            let floor: i64 = c
                .query_row("SELECT level FROM project_floor WHERE project = ?1", [&name], |r| r.get(0))
                .unwrap_or(0);
            json!({"name": name, "folder": folder, "folder_available": folder_ok, "conversations": conversations, "notes_only": notes_only, "created": created, "listed": listed, "floor": floor_word(floor)})
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
            Json(json!({"changed": n > 0, "projects": all(&svc), "kept": "The project's conversations, notes, instructions and where its work may go were not deleted. Forget them where they are shown."})).into_response()
        }
        (Some(name), None) => {
            if !valid(name) {
                return refuse("a project name is a folder name of 1 to 80 plain characters");
            }
            let floor = match b.floor.as_deref() {
                None => None,
                Some(w) => match floor_level(w) {
                    Some(l) => Some(l),
                    None => {
                        return refuse("where a project's work may go is any, confidential or here")
                    }
                },
            };
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
            if let Some(level) = floor {
                let store = svc.store.lock().expect("lock");
                let c = store.connection();
                let done = if level == 0 {
                    c.execute("DELETE FROM project_floor WHERE project = ?1", [name])
                } else {
                    c.execute(
                        "INSERT INTO project_floor(project, level, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(project) DO UPDATE SET level = ?2, updated_at = ?3",
                        kk_vault::rusqlite::params![name, level, now()],
                    )
                };
                if done.is_err() {
                    return refuse("where the project's work may go could not be saved");
                }
                drop(store);
                svc.log_event(
                    "project.floor_set",
                    "owner",
                    &[("level", (level as usize).into())],
                );
                // only the floor was asked for: the project is not made or changed otherwise
                if b.folder.is_none() {
                    return Json(json!({"changed": true, "projects": all(&svc)})).into_response();
                }
            }
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
