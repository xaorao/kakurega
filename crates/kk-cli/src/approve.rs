use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// The owner's own door to the service, by the address and token the service writes when it starts. A program that lives
/// longer than one service (the Telegram bridge, the server for agents) keeps working when the service is started again:
/// a request that cannot be sent, or that the service no longer knows the token of, is sent once more with what the
/// service wrote since.
pub struct Admin {
    conn: std::sync::Mutex<(String, String)>,
    dir: std::path::PathBuf,
    agent: ureq::Agent,
}

fn read_admin(dir: &std::path::Path) -> Result<(String, String), String> {
    let text = std::fs::read_to_string(dir.join("admin.json")).map_err(|_| {
        "kaku serve is not running (no admin.json in the data directory)".to_string()
    })?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let port = v["port"].as_u64().ok_or("damaged admin.json")?;
    let token = v["token"].as_str().ok_or("damaged admin.json")?.to_string();
    Ok((format!("http://127.0.0.1:{port}"), token))
}

pub fn connect(args: &[String]) -> Result<Admin, String> {
    let dir = crate::data_dir(args);
    let conn = read_admin(&dir)?;
    let cfg = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .proxy(None)
        .build();
    Ok(Admin {
        conn: std::sync::Mutex::new(conn),
        dir,
        agent: cfg.into(),
    })
}

impl Admin {
    /// Runs one request with the address and token in use; when it could not be sent, or the token is not the service's
    /// any more, and the service has written a new address or token since, runs it once more with those.
    fn with<T>(
        &self,
        send: impl Fn(&str, &str) -> Result<(u16, T), String>,
    ) -> Result<(u16, T), String> {
        let (base, token) = self.conn.lock().expect("lock").clone();
        let first = send(&base, &token);
        if !matches!(&first, Err(_) | Ok((401, _))) {
            return first;
        }
        let out = match read_admin(&self.dir) {
            Ok(fresh) if fresh != (base, token) => {
                *self.conn.lock().expect("lock") = fresh.clone();
                send(&fresh.0, &fresh.1)
            }
            _ => first,
        };
        // a request that could not be sent at all: the service is not running (or stopped), said in words
        out.map_err(|e| {
            if e.starts_with("io:") || e.contains("onnection") {
                "Kakurega's service is not answering: it is not running, or it stopped. Start it with `kaku serve` (or open the app), then try again".to_string()
            } else {
                e
            }
        })
    }

    pub fn get(&self, path: &str) -> Result<(u16, Value), String> {
        self.with(|base, token| {
            let mut r = self
                .agent
                .get(&format!("{base}{path}"))
                .header("Authorization", &format!("Bearer {token}"))
                .call()
                .map_err(|e| e.to_string())?;
            Ok((
                r.status().as_u16(),
                r.body_mut().read_json().unwrap_or(Value::Null),
            ))
        })
    }

    pub fn get_text(&self, path: &str) -> Result<(u16, String), String> {
        self.with(|base, token| {
            let mut r = self
                .agent
                .get(&format!("{base}{path}"))
                .header("Authorization", &format!("Bearer {token}"))
                .call()
                .map_err(|e| e.to_string())?;
            Ok((
                r.status().as_u16(),
                r.body_mut().read_to_string().map_err(|e| e.to_string())?,
            ))
        })
    }

    pub fn post(&self, path: &str, body: Value) -> Result<(u16, Value), String> {
        self.with(|base, token| {
            let mut r = self
                .agent
                .post(&format!("{base}{path}"))
                .header("Authorization", &format!("Bearer {token}"))
                .send_json(body.clone())
                .map_err(|e| e.to_string())?;
            Ok((
                r.status().as_u16(),
                r.body_mut().read_json().unwrap_or(Value::Null),
            ))
        })
    }

    /// Like `post`, but gives up after `secs` seconds: for notes the caller does not wait on, such as a record of a call.
    pub fn post_within(&self, path: &str, body: Value, secs: u64) -> Result<(u16, Value), String> {
        self.with(|base, token| {
            let mut r = self
                .agent
                .post(&format!("{base}{path}"))
                .header("Authorization", &format!("Bearer {token}"))
                .config()
                .timeout_global(Some(std::time::Duration::from_secs(secs)))
                .build()
                .send_json(body.clone())
                .map_err(|e| e.to_string())?;
            Ok((
                r.status().as_u16(),
                r.body_mut().read_json().unwrap_or(Value::Null),
            ))
        })
    }
}

pub fn pair(args: &[String]) -> u8 {
    match connect(args).and_then(|a| a.post("/admin/pairing", json!({}))) {
        Ok((200, v)) => {
            println!(
                "Open this link in your browser (valid for 5 minutes, works once):\n  {}{}",
                v["url"].as_str().unwrap_or(""),
                v["code"].as_str().unwrap_or("")
            );
            0
        }
        Ok((c, v)) => {
            eprintln!("refused ({c}): {v}");
            1
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

pub fn run(args: &[String]) -> u8 {
    let admin = match connect(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let id = match args
        .iter()
        .find(|a| {
            !a.starts_with("--") && Some(a.as_str()) != crate::flag(args, "--data-dir").as_deref()
        })
        .cloned()
    {
        Some(i) => i,
        None => match admin.get("/admin/approvals") {
            Ok((200, v)) => match v["approvals"].as_array().map(|a| a.as_slice()) {
                Some([one]) => one["id"].as_str().unwrap_or("").to_string(),
                Some([]) | None => {
                    println!("Nothing is waiting for approval.");
                    return 0;
                }
                Some(many) => {
                    println!("Several approvals are waiting; name one:");
                    for a in many {
                        println!(
                            "  {}  for {}",
                            a["id"].as_str().unwrap_or("?"),
                            a["recipient"].as_str().unwrap_or("?")
                        );
                    }
                    return 2;
                }
            },
            _ => {
                eprintln!("could not list approvals");
                return 1;
            }
        },
    };
    let (code, d) = match admin.get(&format!("/admin/approvals/{id}")) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    if code != 200 {
        eprintln!("no such approval: {d}");
        return 1;
    }
    println!("\nRELEASE FOR APPROVAL  {id}");
    println!("Recipient: {}", d["recipient"].as_str().unwrap_or("?"));
    println!("Expires:   {}", d["expires"]);
    println!("Digest:    {}", d["digest"].as_str().unwrap_or("?"));
    println!("\nThese exact files will be signed and handed over (nothing else):\n");
    for f in d["files"].as_array().into_iter().flatten() {
        println!(
            "  {:<18} {:>7} bytes  sha256 {}",
            f["name"].as_str().unwrap_or("?"),
            f["bytes"],
            &f["sha256"].as_str().unwrap_or("")[..16.min(f["sha256"].as_str().unwrap_or("").len())]
        );
    }
    for f in d["files"].as_array().into_iter().flatten() {
        if let Some(t) = f["text"].as_str() {
            if f["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("finding-") || n.starts_with("repro-"))
            {
                println!(
                    "\n----- {} -----\n{}",
                    f["name"].as_str().unwrap_or(""),
                    t.trim_end()
                );
            }
        }
    }
    if let Some(dis) = d.get("disclosure").filter(|v| v.is_object()) {
        println!("{}", kk_types::words::counted(&format!(
            "\nCumulative disclosure: {} of {} project files would have been shown to this recipient ({} earlier release(s)).",
            dis["files_after"], dis["project_files"], dis["releases_before"]
        )));
        for r in dis["reasons"].as_array().into_iter().flatten() {
            println!("  ! {}", r.as_str().unwrap_or(""));
        }
        println!(
            "  Not checked: {}",
            dis["not_checked"]
                .as_array()
                .map(|a| a
                    .iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(" "))
                .unwrap_or_default()
        );
    }
    println!("\nWithheld and never released:");
    for r in d["removed"].as_array().into_iter().flatten() {
        println!(
            "  {} ({})",
            r["what"].as_str().unwrap_or(""),
            r["why"].as_str().unwrap_or("")
        );
    }
    println!("\nNot confirmed by the kernel check, so not included:");
    let nc = d["not_confirmed"].as_array().cloned().unwrap_or_default();
    if nc.is_empty() {
        println!("  (none)");
    }
    for n in nc {
        println!(
            "  {} ({})",
            n["title"].as_str().unwrap_or(""),
            n["why"].as_str().unwrap_or("")
        );
    }
    println!("\nThis release does not establish:");
    for n in d["not_established"].as_array().into_iter().flatten() {
        println!("  - {}", n.as_str().unwrap_or(""));
    }
    // the digest is typed by a person: a program that pipes it in, such as an agent running commands for the owner, is refused.
    // An agent that can run commands as the owner can still fake a terminal; only an approval key that asks for the owner
    // (Touch ID, `kaku approver enroll --enclave`) stops that, and with one enrolled every release needs it.
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        eprintln!("kaku approve: the digest must be typed by a person at a terminal; a digest from a pipe or a program is refused, and nothing was approved");
        return 1;
    }
    print!("\nType the first 12 characters of the digest to approve, or press Enter to refuse: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return 1;
    }
    let ack = line.trim();
    if ack.is_empty() {
        let _ = admin.post(&format!("/admin/approvals/{id}/deny"), json!({}));
        println!("Refused. Nothing was approved.");
        return 1;
    }
    let mut body = json!({"ack": ack});
    if d["requires_proof"] == true {
        let signer = match kk_evidence::approval::load_approver(&crate::data_dir(args)) {
            Ok(Some(s)) => s,
            _ => {
                eprintln!("This service requires a signed approval, and this terminal has no usable approver key. Run `kaku approver enroll` on this computer first.");
                return 1;
            }
        };
        let st = &d["statement"];
        let statement = kk_evidence::approval::Statement {
            approval: st["approval"].as_str().unwrap_or("").into(),
            job: st["job"].as_str().unwrap_or("").into(),
            digest: st["digest"].as_str().unwrap_or("").into(),
            recipient: st["recipient"].as_str().unwrap_or("").into(),
            policy: st["policy"].as_str().unwrap_or("").into(),
            expires: st["expires"].as_i64().unwrap_or(0),
            nonce: st["nonce"].as_str().unwrap_or("").into(),
        };
        if signer.is_hardware() {
            println!("Waiting for your Mac to confirm with Touch ID or your password...");
        }
        match signer.sign_statement(&statement) {
            Ok(p) => body["proof"] = json!(p),
            Err(e) => {
                eprintln!("The approval was not signed ({e}). Nothing was approved.");
                return 1;
            }
        }
    }
    match admin.post(&format!("/admin/approvals/{id}/approve"), body) {
        Ok((200, _)) => {
            println!("Approved. Go back to the browser and press Deliver.");
            0
        }
        Ok((_, v)) => {
            eprintln!(
                "Not approved: {}",
                v["error"]["message"].as_str().unwrap_or("refused")
            );
            1
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod proxy_tests {
    use std::net::TcpListener;

    #[test]
    fn px04_the_admin_token_never_travels_through_an_environment_proxy() {
        let _env = crate::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        /// Puts the process environment back as it was, whatever the test does: other tests share it.
        struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (k, v) in self.0.drain(..) {
                    match v {
                        Some(v) => std::env::set_var(k, v),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }
        let names = [
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
        ];
        let _restore = Restore(names.iter().map(|k| (*k, std::env::var_os(k))).collect());
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        proxy.set_nonblocking(true).unwrap();
        let purl = format!("http://{}", proxy.local_addr().unwrap());
        for k in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
            std::env::set_var(k, &purl);
        }
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
        let service = TcpListener::bind("127.0.0.1:0").unwrap();
        let svc_addr = service.local_addr().unwrap();
        let reached = std::thread::spawn(move || service.accept().is_ok());
        let dir = std::env::temp_dir().join(format!("kk-px04-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("admin.json"),
            format!("{{\"port\": {}, \"token\": \"tok\"}}", svc_addr.port()),
        )
        .unwrap();
        let admin = super::connect(&["--data-dir".to_string(), dir.to_string_lossy().into_owned()])
            .unwrap();
        let _ = admin
            .agent
            .get(&format!(
                "{}/admin/health",
                admin.conn.lock().expect("lock").0
            ))
            .call();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(proxy.accept().is_err(), "the proxy was contacted");
        assert!(
            reached.join().unwrap(),
            "the service was contacted directly"
        );
    }
}
