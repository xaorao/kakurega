//! Hiding personal values from a model that is not on this computer: the names the owner listed and the values the
//! personal-data detector finds are swapped for placeholders in everything sent, and swapped back in what comes back.
//! Every question to the answering model goes through `ask`, `ask_stream` or `ask_summary`, or through `hide_messages`
//! where the exact text sent is also shown to the owner.

use super::*;
use kk_secrets::swap::Swap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NamesBody {
    #[serde(default)]
    add: Option<String>,
    #[serde(default)]
    remove: Option<String>,
}

impl Service {
    pub(super) fn hidden_names(&self) -> Vec<(String, i64)> {
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        c.prepare("SELECT name, added FROM hidden_names ORDER BY added, name")
            .and_then(|mut st| {
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
    }

    /// Whether what is sent to the answering model leaves this computer.
    fn answers_elsewhere(&self) -> bool {
        !self.route_for_owner()["stays_here"]
            .as_bool()
            .unwrap_or(false)
    }

    /// Hides personal values in these messages, in place, when the answering model is not on this computer. Returns the
    /// swap that turns its answer back, or `None` when nothing leaves (the messages are then untouched).
    pub(super) fn hide_messages(
        &self,
        messages: &mut Vec<kk_types::chat::ChatMessage>,
    ) -> Option<Swap> {
        if !self.answers_elsewhere() {
            return None;
        }
        let names: Vec<String> = self.hidden_names().into_iter().map(|(n, _)| n).collect();
        let mut swap = Swap::new(&names);
        for m in messages.iter_mut() {
            m.content = swap.hide(&m.content);
        }
        // the record of something leaving: where to and how much, never what
        let host = self.route_for_owner()["host"]
            .as_str()
            .unwrap_or("another computer")
            .to_string();
        let chars: usize = messages.iter().map(|m| m.content.chars().count()).sum();
        self.log_event(
            "outbound.model",
            &host,
            &[("chars", chars.into()), ("swapped", swap.total().into())],
        );
        if swap.total() > 0 {
            // told once, after the product's own instruction, so the model keeps the placeholders as they are
            let at = usize::from(messages.first().is_some_and(|m| m.role == "system"));
            messages.insert(
                at,
                kk_types::chat::ChatMessage::new("system", kk_secrets::swap::NOTICE),
            );
        }
        Some(swap)
    }

    /// Asks the answering model. On a route that leaves this computer the question is sent with personal values hidden
    /// and the answer comes back with them restored.
    pub(super) fn ask(
        &self,
        messages: Vec<kk_types::chat::ChatMessage>,
    ) -> Result<ChatReply, String> {
        self.ask_counted(messages).map(|(reply, _)| reply)
    }

    /// `ask`, also saying how many different values went as placeholders.
    pub(super) fn ask_counted(
        &self,
        mut messages: Vec<kk_types::chat::ChatMessage>,
    ) -> Result<(ChatReply, usize), String> {
        let swap = self.hide_messages(&mut messages);
        let mut reply = self.runner.chat(messages)?;
        let mut swapped = 0;
        if let Some(s) = &swap {
            reply.text = s.show(&reply.text);
            swapped = s.total();
        }
        Ok((reply, swapped))
    }

    /// `ask`, handing the answer over in pieces. A route that leaves this computer answers in one piece (it is held until
    /// it is whole), so restoring each piece restores the answer.
    pub(super) fn ask_stream(
        &self,
        mut messages: Vec<kk_types::chat::ChatMessage>,
        on_chunk: &mut dyn FnMut(&str) -> bool,
    ) -> Result<StreamedReply, String> {
        let swap = self.hide_messages(&mut messages);
        let mut out = match &swap {
            Some(s) => self
                .runner
                .chat_stream(messages, &mut |piece| on_chunk(&s.show(piece)))?,
            None => self.runner.chat_stream(messages, on_chunk)?,
        };
        if let Some(s) = &swap {
            out.reply.text = s.show(&out.reply.text);
        }
        Ok(out)
    }

    /// A summary of earlier turns, asked the same way.
    pub(super) fn ask_summary(
        &self,
        mut messages: Vec<kk_types::chat::ChatMessage>,
    ) -> Result<String, String> {
        let swap = self.hide_messages(&mut messages);
        let text = self.runner.summarize(messages)?;
        Ok(match &swap {
            Some(s) => s.show(&text),
            None => text,
        })
    }
}

pub(super) async fn names_get(axum::Extension(svc): Svc) -> Response {
    Json(json!({
        "names": svc.hidden_names().iter().map(|(n, at)| json!({"name": n, "added_at": at})).collect::<Vec<_>>(),
        "max": kk_secrets::swap::MAX_NAMES,
        "applies": svc.answers_elsewhere(),
        "note": "When the model that answers is not on this computer, these names and any email address, phone, card, account or identity number are replaced by placeholders before a question is sent, and put back in the answer. A name that is not on this list is sent as written.",
    }))
    .into_response()
}

/// Adds or removes one name to hide. Only the owner's browser or terminal session reaches this.
pub(super) async fn names_set(axum::Extension(svc): Svc, Json(b): Json<NamesBody>) -> Response {
    let refuse = |why: &str| error(StatusCode::UNPROCESSABLE_ENTITY, "NAME_REFUSED", why);
    let (name, add) = match (&b.add, &b.remove) {
        (Some(n), None) => (n.as_str(), true),
        (None, Some(n)) => (n.as_str(), false),
        _ => return refuse("give exactly one of add or remove"),
    };
    let name = if add {
        match kk_secrets::swap::check_name(name) {
            Ok(n) if kk_secrets::holds_secret(&n) => {
                return refuse("that looks like a secret, not a name")
            }
            Ok(n) => n,
            Err(why) => return refuse(why),
        }
    } else {
        name.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let changed = {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        if add {
            let n: i64 = c
                .query_row("SELECT COUNT(*) FROM hidden_names", [], |r| r.get(0))
                .unwrap_or(0);
            let known: i64 = c
                .query_row(
                    "SELECT COUNT(*) FROM hidden_names WHERE lower(name) = lower(?1)",
                    [&name],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if known > 0 {
                0
            } else if n as usize >= kk_secrets::swap::MAX_NAMES {
                return refuse("the list is full; remove a name first");
            } else {
                c.execute(
                    "INSERT INTO hidden_names(name, added) VALUES(?1, ?2)",
                    kk_vault::rusqlite::params![name, now()],
                )
                .unwrap_or(0)
            }
        } else {
            c.execute(
                "DELETE FROM hidden_names WHERE lower(name) = lower(?1)",
                [&name],
            )
            .unwrap_or(0)
        }
    };
    if changed > 0 {
        // the log holds how many names there are, never a name
        svc.log_event(
            if add {
                "hidden_name.added"
            } else {
                "hidden_name.removed"
            },
            "owner",
            &[("names", svc.hidden_names().len().into())],
        );
    }
    Json(json!({"changed": changed > 0, "names": svc.hidden_names().len()})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RouteBody {
    #[serde(default)]
    route: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    forget_key: bool,
}

/// The routes the owner can choose between, and the one in use, with the route as it is shown everywhere else.
pub(super) async fn route_options(axum::Extension(svc): Svc) -> Response {
    let mut v = svc.runner.route_options();
    v["route"] = svc.route_for_owner();
    v["note"] = json!("The model on this computer keeps everything here. NEAR AI Cloud runs the model on hardware that proves what it runs, and your questions go there encrypted; the proof is checked before an answer is shown. The account key is kept in a file in this program's data folder that only you can read.");
    Json(v).into_response()
}

/// Switches the route. The key, when one is typed, goes to the runner and into its private file and nowhere else: it is
/// not logged, not stored by the service and not in any answer.
pub(super) async fn route_choose(axum::Extension(svc): Svc, Json(b): Json<RouteBody>) -> Response {
    let refuse = |why: &str| error(StatusCode::UNPROCESSABLE_ENTITY, "ROUTE_REFUSED", why);
    if b.model.as_deref().is_some_and(|m| m.chars().count() > 200)
        || b.key.as_deref().is_some_and(|k| k.len() > 500)
    {
        return refuse("that is too long to be a model name or a key");
    }
    let choice =
        json!({"route": b.route, "model": b.model, "key": b.key, "forget_key": b.forget_key});
    let svc2 = svc.clone();
    let out = tokio::task::spawn_blocking(move || svc2.runner.choose_route(&choice)).await;
    match out {
        Ok(Ok(mut v)) => {
            svc.log_event(
                if b.forget_key {
                    "route.key_forgotten"
                } else {
                    "route.chosen"
                },
                v["current"].as_str().unwrap_or("?"),
                &[],
            );
            v["route"] = svc.route_for_owner();
            Json(v).into_response()
        }
        // the runner's reasons are written for the owner and never hold the key
        Ok(Err(why)) => refuse(&why),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ROUTE_FAILED",
            "the route could not be changed",
        ),
    }
}
