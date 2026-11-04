use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MAX_MESSAGE: usize = 1500;

/// Read-only on purpose: the bridge has no way to change anything on the service.
pub trait Admin {
    fn get(&mut self, path: &str) -> Result<Value, String>;
}

pub struct Bridge {
    pub bound_chat: Option<i64>,
    pairing_code: Option<(String, Instant, u8)>,
    seen_states: BTreeMap<String, String>,
    seen_approvals: BTreeMap<String, bool>,
    primed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Out {
    pub chat: i64,
    pub text: String,
}

fn plain(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect()
}

fn state_word(s: &str) -> &'static str {
    match s {
        "running" => "running",
        "ready" => "finished, with a release waiting for your approval",
        "nothing_to_release" => "finished, nothing confirmed so nothing can be sent",
        "failed" => "stopped with an error",
        _ => "in an unknown state",
    }
}

impl Bridge {
    pub fn new(bound_chat: Option<i64>) -> Bridge {
        Bridge {
            bound_chat,
            pairing_code: None,
            seen_states: BTreeMap::new(),
            seen_approvals: BTreeMap::new(),
            primed: false,
        }
    }

    #[cfg(test)]
    pub fn set_pairing_code(&mut self, code: &str) {
        self.set_pairing_code_aged(code, Duration::ZERO);
    }

    /// `age` is how long ago the code was issued, so a code read from disk keeps its real five-minute window.
    pub fn set_pairing_code_aged(&mut self, code: &str, age: Duration) {
        if self
            .pairing_code
            .as_ref()
            .is_some_and(|(c, _, _)| c == code)
        {
            return;
        }
        let issued = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
        self.pairing_code = Some((code.to_string(), issued, 0));
    }

    pub fn handle_update(
        &mut self,
        admin: &mut dyn Admin,
        update: &Value,
    ) -> (Vec<Out>, Option<i64>) {
        let Some(msg) = update.get("message") else {
            return (vec![], None);
        };
        let (Some(chat), Some(text)) = (msg["chat"]["id"].as_i64(), msg["text"].as_str()) else {
            return (vec![], None);
        };
        if msg["chat"]["type"].as_str() != Some("private") {
            return (vec![], None);
        }
        let word = text.split_whitespace().next().unwrap_or("");
        let word = word.split('@').next().unwrap_or("").to_ascii_lowercase();
        match self.bound_chat {
            None => {
                if word == "/pair" {
                    let given = text.split_whitespace().nth(1).unwrap_or("").to_string();
                    let Some((code, issued, attempts)) = self.pairing_code.clone() else {
                        return (vec![], None);
                    };
                    let fresh = issued.elapsed() < Duration::from_secs(300);
                    if fresh && !given.is_empty() && code == given {
                        self.pairing_code = None;
                        self.bound_chat = Some(chat);
                        return (vec![Out { chat, text: "Paired. I will tell you when a review finishes or an approval is waiting. I never send file contents. Commands: /status /reviews /help.".into() }], Some(chat));
                    }
                    // While a pairing window is open the owner needs to know why it did not work. Five wrong tries close it.
                    if !fresh || attempts + 1 >= 5 {
                        self.pairing_code = None;
                    } else {
                        self.pairing_code = Some((code, issued, attempts + 1));
                    }
                    return (vec![Out { chat, text: "That pairing code is wrong or has expired. Run `kaku telegram pair` again for a new one.".into() }], None);
                }
                (vec![], None)
            }
            Some(bound) if bound != chat => (vec![], None),
            Some(_) => {
                let reply = match word.as_str() {
                    "/status" => match admin.get("/admin/overview") {
                        Ok(v) => {
                            let r = &v["reviews"];
                            let m = v["memories_by_level"].as_object().map(|o| o.values().filter_map(Value::as_u64).sum::<u64>()).unwrap_or(0);
                            format!("Setup: {}. Reviews: {} running, {} ready, {} nothing to release, {} failed. Memories saved: {}. Log entries: {}.", if v["ready"] == true { "ready" } else { "needs attention on your computer" }, r["running"].as_u64().unwrap_or(0), r["ready"].as_u64().unwrap_or(0), r["nothing_to_release"].as_u64().unwrap_or(0), r["failed"].as_u64().unwrap_or(0), m, v["log"]["entries"].as_u64().unwrap_or(0))
                        }
                        Err(_) => "Kakurega is not reachable on your computer right now.".to_string(),
                    },
                    "/reviews" => match admin.get("/admin/jobs") {
                        Ok(v) => {
                            let lines: Vec<String> = v["jobs"].as_array().map(|a| a.iter().take(15).map(|j| format!("{} {}", plain(j["id"].as_str().unwrap_or("?")), plain(j["state"].as_str().unwrap_or("?")).replace('_', " "))).collect()).unwrap_or_default();
                            if lines.is_empty() { "No reviews yet.".to_string() } else { lines.join("\n") }
                        }
                        Err(_) => "Kakurega is not reachable on your computer right now.".to_string(),
                    },
                    "/help" | "/start" => "I tell you when a review finishes or an approval is waiting. /status shows counts, /reviews lists ids and states. I never send file contents, and approvals are only done on your computer.".to_string(),
                    _ => "I only understand /status, /reviews and /help. I cannot approve or send anything.".to_string(),
                };
                (
                    vec![Out {
                        chat,
                        text: reply.chars().take(MAX_MESSAGE).collect(),
                    }],
                    None,
                )
            }
        }
    }

    pub fn poll_notifications(&mut self, admin: &mut dyn Admin) -> Vec<Out> {
        let Some(chat) = self.bound_chat else {
            return vec![];
        };
        let Ok(v) = admin.get("/admin/jobs") else {
            return vec![];
        };
        let mut out = Vec::new();
        for j in v["jobs"].as_array().cloned().unwrap_or_default() {
            let id = plain(j["id"].as_str().unwrap_or(""));
            let state = plain(j["state"].as_str().unwrap_or("")).to_string();
            if id.is_empty() {
                continue;
            }
            let prev = self.seen_states.insert(id.clone(), state.clone());
            if self.primed && prev.as_deref() != Some(state.as_str()) && state != "running" {
                out.push(Out {
                    chat,
                    text: format!(
                        "Review {id} {}. {} confirmed finding(s). Decide on your computer.",
                        state_word(&state),
                        j["findings_confirmed"].as_u64().unwrap_or(0)
                    ),
                });
            }
            let waiting = j["approval"].as_str().is_some() && j["delivered"].is_null();
            let was = self.seen_approvals.insert(id.clone(), waiting);
            if self.primed && waiting && was != Some(true) {
                out.push(Out { chat, text: format!("An approval for {id} is waiting on your computer. Nothing is sent until you approve there.") });
            }
        }
        self.primed = true;
        out
    }
}

pub fn scrub(text: &str, token: &str) -> String {
    if token.is_empty() {
        text.to_string()
    } else {
        text.replace(token, "<token>")
    }
}

pub fn read_token(file: Option<&Path>) -> Result<String, String> {
    if let Ok(t) = std::env::var("TELEGRAM_BOT_TOKEN") {
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let path = file
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("KAKU_TELEGRAM_ENV").map(PathBuf::from))
        .ok_or("set TELEGRAM_BOT_TOKEN or pass --token-file FILE")?;
    let text =
        std::fs::read_to_string(&path).map_err(|_| "cannot read the token file".to_string())?;
    text.lines()
        .find_map(|l| l.strip_prefix("TELEGRAM_BOT_TOKEN="))
        .map(|t| t.trim().trim_matches('"').to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "the token file has no TELEGRAM_BOT_TOKEN".to_string())
}

struct HttpAdmin(crate::approve::Admin);

impl Admin for HttpAdmin {
    fn get(&mut self, path: &str) -> Result<Value, String> {
        match self.0.get(path) {
            Ok((200, v)) => Ok(v),
            Ok((c, _)) => Err(format!("status {c}")),
            Err(e) => Err(e),
        }
    }
}

fn bound_path(dir: &Path) -> PathBuf {
    dir.join("telegram.json")
}

fn load_bound(dir: &Path) -> Option<i64> {
    std::fs::read_to_string(bound_path(dir))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["chat"].as_i64())
}

pub fn run(args: &[String]) -> u8 {
    let dir = crate::data_dir(args);
    let admin = match crate::approve::connect(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mut admin = HttpAdmin(admin);
    if args.first().map(String::as_str) == Some("pair") {
        let code: String = {
            let mut b = [0u8; 4];
            getrandom_fill(&mut b);
            b.iter().map(|x| format!("{x:02x}")).collect()
        };
        let _ = std::fs::remove_file(bound_path(&dir));
        let _ = std::fs::write(dir.join("telegram.pairing"), &code);
        println!("Send this to your bot in a private chat within 5 minutes:\n  /pair {code}\nThen run `kaku telegram` to start the bridge. Any earlier pairing was removed.");
        return 0;
    }
    let token = match read_token(
        crate::flag(args, "--token-file")
            .map(PathBuf::from)
            .as_deref(),
    ) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let api = crate::flag(args, "--api-base").unwrap_or_else(|| "https://api.telegram.org".into());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(40)))
        .build()
        .into();
    let mut bridge = Bridge::new(load_bound(&dir));
    let reload_code = |bridge: &mut Bridge| {
        let path = dir.join("telegram.pairing");
        if let Ok(code) = std::fs::read_to_string(&path) {
            let age = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .unwrap_or(Duration::from_secs(301));
            bridge.set_pairing_code_aged(code.trim(), age);
        }
    };
    reload_code(&mut bridge);
    println!("Telegram bridge running. It sends counts and ids only, never file contents. Ctrl-C stops it.");
    let mut offset: i64 = 0;
    let mut last_poll = Instant::now() - Duration::from_secs(10);
    loop {
        reload_code(&mut bridge);
        let url = format!("{api}/bot{token}/getUpdates?timeout=2&offset={offset}&allowed_updates=%5B%22message%22%5D");
        match agent.get(&url).call() {
            Ok(mut r) if r.status().as_u16() == 200 => {
                let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
                for u in v["result"].as_array().cloned().unwrap_or_default() {
                    offset = offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
                    let (outs, bound) = bridge.handle_update(&mut admin, &u);
                    if let Some(chat) = bound {
                        let _ = std::fs::write(bound_path(&dir), json!({"chat": chat}).to_string());
                        let _ = std::fs::remove_file(dir.join("telegram.pairing"));
                    }
                    for o in outs {
                        send(&agent, &api, &token, &o);
                    }
                }
            }
            Ok(r) => {
                eprintln!("telegram answered {}", r.status());
                std::thread::sleep(Duration::from_secs(3));
            }
            Err(e) => {
                eprintln!(
                    "telegram is not reachable: {}",
                    scrub(&e.to_string(), &token)
                );
                std::thread::sleep(Duration::from_secs(3));
            }
        }
        if last_poll.elapsed() >= Duration::from_secs(2) {
            last_poll = Instant::now();
            for o in bridge.poll_notifications(&mut admin) {
                send(&agent, &api, &token, &o);
            }
        }
    }
}

fn getrandom_fill(b: &mut [u8]) {
    let mut f = std::fs::File::open("/dev/urandom").expect("urandom");
    std::io::Read::read_exact(&mut f, b).expect("random bytes");
}

fn send(agent: &ureq::Agent, api: &str, token: &str, o: &Out) {
    let url = format!("{api}/bot{token}/sendMessage");
    if let Err(e) = agent
        .post(&url)
        .send_json(json!({"chat_id": o.chat, "text": o.text, "disable_web_page_preview": true}))
    {
        eprintln!("could not send a message: {}", scrub(&e.to_string(), token));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        jobs: Value,
        overview: Value,
        down: bool,
        asked: Vec<String>,
    }

    impl Admin for Fake {
        fn get(&mut self, path: &str) -> Result<Value, String> {
            self.asked.push(path.to_string());
            if self.down {
                return Err("down".into());
            }
            match path {
                "/admin/jobs" => Ok(self.jobs.clone()),
                "/admin/overview" => Ok(self.overview.clone()),
                _ => Err("no".into()),
            }
        }
    }

    fn fake() -> Fake {
        Fake {
            jobs: json!({"jobs": [{"id": "job-1", "state": "running", "recipient": "Acme Secret Corp", "approval": null, "delivered": null, "findings_confirmed": 0}]}),
            overview: json!({"ready": true, "reviews": {"running": 1, "ready": 2}, "memories_by_level": {"internal": 3}, "log": {"entries": 9}}),
            down: false,
            asked: vec![],
        }
    }

    fn msg(chat: i64, text: &str) -> Value {
        json!({"update_id": 1, "message": {"chat": {"id": chat, "type": "private"}, "text": text}})
    }

    #[test]
    fn tg01_pairing_binds_one_private_chat_with_a_one_time_code_and_strangers_get_silence() {
        let mut f = fake();
        let mut b = Bridge::new(None);
        assert!(
            b.handle_update(&mut f, &msg(111, "/pair 1234"))
                .0
                .is_empty(),
            "no code issued yet"
        );
        b.set_pairing_code("abcd1234");
        assert!(
            b.handle_update(&mut f, &msg(111, "/status")).0.is_empty(),
            "unpaired chats get no answer"
        );
        let wrong = b.handle_update(&mut f, &msg(111, "/pair wrong")).0;
        assert!(
            wrong.len() == 1 && wrong[0].text.contains("wrong or has expired"),
            "while a pairing window is open a wrong code is explained"
        );
        assert!(b.handle_update(&mut f, &json!({"update_id": 2, "message": {"chat": {"id": -5, "type": "group"}, "text": "/pair abcd1234"}})).0.is_empty(), "groups cannot pair");
        let (out, bound) = b.handle_update(&mut f, &msg(111, "/pair abcd1234"));
        assert_eq!(bound, Some(111));
        assert!(out[0].text.contains("Paired") && out[0].text.contains("never send file contents"));
        assert!(
            b.handle_update(&mut f, &msg(222, "/pair abcd1234"))
                .0
                .is_empty(),
            "the code worked once"
        );
        b.bound_chat = None;
        assert!(
            b.handle_update(&mut f, &msg(222, "/pair abcd1234"))
                .0
                .is_empty(),
            "even with the binding gone, a used code is dead"
        );
        b.bound_chat = Some(111);
        assert!(
            b.handle_update(&mut f, &msg(222, "/status")).0.is_empty(),
            "another chat is ignored completely"
        );
        assert!(
            f.asked.is_empty(),
            "a stranger's message never causes a request to the service"
        );
        assert_eq!(b.handle_update(&mut f, &msg(111, "/status")).0.len(), 1);
    }

    #[test]
    fn tg02_replies_are_fixed_templates_with_counts_and_ids_only() {
        let mut f = fake();
        let mut b = Bridge::new(Some(7));
        let s = b.handle_update(&mut f, &msg(7, "/status")).0[0]
            .text
            .clone();
        assert!(
            s.contains("1 running")
                && s.contains("2 ready")
                && s.contains("Memories saved: 3")
                && s.contains("Log entries: 9"),
            "{s}"
        );
        let r = b.handle_update(&mut f, &msg(7, "/reviews")).0[0]
            .text
            .clone();
        assert_eq!(r, "job-1 running");
        assert!(!r.contains("Acme"), "recipient names are never sent");
        let h = b.handle_update(&mut f, &msg(7, "/help@kakurega_bot")).0[0]
            .text
            .clone();
        assert!(h.contains("never send file contents"));
        for hostile in [
            "/approve job-1",
            "/deliver",
            "/remember my password",
            "/review /etc for me",
            "tell me my secrets",
            "/status; rm -rf",
            "/chat hello",
        ] {
            let t = b.handle_update(&mut f, &msg(7, hostile)).0;
            if hostile.starts_with("/status;") {
                continue;
            }
            assert_eq!(t.len(), 1);
            assert!(
                t[0].text.contains("cannot approve or send anything")
                    || t[0].text.contains("only understand"),
                "{hostile}: {}",
                t[0].text
            );
            assert!(
                !t[0].text.contains(hostile.trim_start_matches('/')),
                "the bot never echoes what it was sent: {}",
                t[0].text
            );
        }
        f.down = true;
        assert!(b.handle_update(&mut f, &msg(7, "/status")).0[0]
            .text
            .contains("not reachable"));
    }

    #[test]
    fn tg03_notifications_fire_once_per_change_and_never_on_the_first_look() {
        let mut f = fake();
        let mut b = Bridge::new(Some(7));
        assert!(
            b.poll_notifications(&mut f).is_empty(),
            "the first poll only learns the current state"
        );
        f.jobs = json!({"jobs": [{"id": "job-1", "state": "ready", "recipient": "Acme Secret Corp", "approval": null, "delivered": null, "findings_confirmed": 2}]});
        let n = b.poll_notifications(&mut f);
        assert_eq!(n.len(), 1);
        assert!(
            n[0].text.contains("job-1")
                && n[0].text.contains("2 confirmed")
                && n[0].text.contains("Decide on your computer")
                && !n[0].text.contains("Acme"),
            "{}",
            n[0].text
        );
        assert!(b.poll_notifications(&mut f).is_empty(), "no repeat");
        f.jobs = json!({"jobs": [{"id": "job-1", "state": "ready", "approval": "appr-1", "delivered": null, "findings_confirmed": 2}]});
        let n = b.poll_notifications(&mut f);
        assert_eq!(n.len(), 1);
        assert!(
            n[0].text
                .contains("approval for job-1 is waiting on your computer")
                && n[0]
                    .text
                    .contains("Nothing is sent until you approve there")
        );
        assert!(b.poll_notifications(&mut f).is_empty());
        f.jobs = json!({"jobs": [{"id": "job-1", "state": "ready", "approval": "appr-1", "delivered": {"name": "x"}, "findings_confirmed": 2}, {"id": "job-2\u{1b}[2J<b>", "state": "failed", "approval": null, "delivered": null}]});
        let n = b.poll_notifications(&mut f);
        assert_eq!(
            n.len(),
            1,
            "a delivered release is not an approval waiting, a new failure is one message"
        );
        assert!(
            !n[0].text.contains('\u{1b}') && !n[0].text.contains('<'),
            "ids are reduced to safe characters: {}",
            n[0].text
        );
        let mut late = Bridge::new(Some(7));
        f.jobs = json!({"jobs": [{"id": "job-9", "state": "ready", "approval": "appr-9", "delivered": null, "findings_confirmed": 1}]});
        assert!(
            late.poll_notifications(&mut f).is_empty(),
            "a bridge that starts after things happened does not replay old news"
        );
        let mut unpaired = Bridge::new(None);
        assert!(
            unpaired.poll_notifications(&mut f).is_empty(),
            "an unpaired bridge sends nothing anywhere"
        );
    }

    #[test]
    fn tg04_the_token_never_appears_in_error_text_and_files_are_read_safely() {
        assert_eq!(
            scrub(
                "error for https://api.telegram.org/botSECRET123/getUpdates",
                "SECRET123"
            ),
            "error for https://api.telegram.org/bot<token>/getUpdates"
        );
        assert_eq!(scrub("nothing", ""), "nothing");
        let dir = std::env::temp_dir().join(format!("kk-tg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.env");
        std::fs::write(&ok, "OTHER=1\nTELEGRAM_BOT_TOKEN=\"abc:123\"\n").unwrap();
        std::env::remove_var("TELEGRAM_BOT_TOKEN");
        assert_eq!(read_token(Some(&ok)).unwrap(), "abc:123");
        std::fs::write(dir.join("empty.env"), "TELEGRAM_BOT_TOKEN=\n").unwrap();
        assert!(read_token(Some(&dir.join("empty.env"))).is_err());
        let missing = read_token(Some(&dir.join("nope.env"))).unwrap_err();
        assert!(
            !missing.contains("nope.env"),
            "errors do not echo paths or contents"
        );
    }

    #[test]
    fn tg05_long_replies_are_cut_and_the_bridge_cannot_write() {
        let mut f = fake();
        f.jobs = json!({"jobs": (0..40).map(|i| json!({"id": format!("job-{i}"), "state": "ready"})).collect::<Vec<_>>()});
        let mut b = Bridge::new(Some(7));
        let r = b.handle_update(&mut f, &msg(7, "/reviews")).0[0]
            .text
            .clone();
        assert!(
            r.chars().count() <= MAX_MESSAGE && r.lines().count() <= 15,
            "{}",
            r.lines().count()
        );
        assert!(
            f.asked
                .iter()
                .all(|p| p.starts_with("/admin/jobs") || p.starts_with("/admin/overview")),
            "{:?}",
            f.asked
        );
    }

    #[test]
    fn tg16_pairing_codes_expire_close_after_five_wrong_tries_and_a_newer_code_replaces_an_older_one(
    ) {
        let mut f = fake();
        let mut b = Bridge::new(None);
        b.set_pairing_code_aged("oldcode1", Duration::from_secs(301));
        let r = b.handle_update(&mut f, &msg(111, "/pair oldcode1")).0;
        assert!(
            r.len() == 1 && r[0].text.contains("expired") && b.bound_chat.is_none(),
            "an expired code never pairs, even when typed right"
        );
        assert!(
            b.handle_update(&mut f, &msg(111, "/pair oldcode1"))
                .0
                .is_empty(),
            "and the dead window is silent again"
        );
        b.set_pairing_code("newcode22");
        for i in 0..4 {
            let r = b
                .handle_update(&mut f, &msg(111, &format!("/pair guess{i}")))
                .0;
            assert_eq!(r.len(), 1);
        }
        let r = b.handle_update(&mut f, &msg(111, "/pair guess4")).0;
        assert_eq!(r.len(), 1);
        assert!(
            b.handle_update(&mut f, &msg(111, "/pair newcode22"))
                .0
                .is_empty(),
            "after five wrong tries even the right code is dead"
        );
        assert!(b.bound_chat.is_none());
        b.set_pairing_code("first111");
        b.set_pairing_code("second22");
        assert!(
            b.handle_update(&mut f, &msg(111, "/pair first111"))
                .1
                .is_none(),
            "the older code is replaced"
        );
        b.set_pairing_code("second22");
        assert_eq!(
            b.handle_update(&mut f, &msg(111, "/pair second22")).1,
            Some(111)
        );
        b.set_pairing_code("same3333");
        b.set_pairing_code_aged("same3333", Duration::from_secs(400));
        assert!(
            b.handle_update(&mut f, &msg(222, "/pair same3333"))
                .1
                .is_none()
                || b.bound_chat == Some(111)
        );
    }
}
