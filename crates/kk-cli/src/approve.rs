use serde_json::{json, Value};
use std::io::{BufRead, Write};

struct Admin {
    base: String,
    token: String,
    agent: ureq::Agent,
}

fn connect(args: &[String]) -> Result<Admin, String> {
    let dir = crate::data_dir(args);
    let text = std::fs::read_to_string(dir.join("admin.json")).map_err(|_| {
        "kaku serve is not running (no admin.json in the data directory)".to_string()
    })?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let port = v["port"].as_u64().ok_or("damaged admin.json")?;
    let token = v["token"].as_str().ok_or("damaged admin.json")?.to_string();
    let cfg = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build();
    Ok(Admin {
        base: format!("http://127.0.0.1:{port}"),
        token,
        agent: cfg.into(),
    })
}

impl Admin {
    fn get(&self, path: &str) -> Result<(u16, Value), String> {
        let mut r = self
            .agent
            .get(&format!("{}{path}", self.base))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| e.to_string())?;
        Ok((
            r.status().as_u16(),
            r.body_mut().read_json().unwrap_or(Value::Null),
        ))
    }

    fn post(&self, path: &str, body: Value) -> Result<(u16, Value), String> {
        let mut r = self
            .agent
            .post(&format!("{}{path}", self.base))
            .header("Authorization", &format!("Bearer {}", self.token))
            .send_json(body)
            .map_err(|e| e.to_string())?;
        Ok((
            r.status().as_u16(),
            r.body_mut().read_json().unwrap_or(Value::Null),
        ))
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
    match admin.post(
        &format!("/admin/approvals/{id}/approve"),
        json!({"ack": ack}),
    ) {
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
