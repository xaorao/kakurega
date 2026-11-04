use serde_json::json;

pub fn run(args: &[String]) -> u8 {
    let admin = match crate::approve::connect(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    match args.first().map(String::as_str) {
        Some("pair") => match admin.post("/admin/extension/pairing", json!({})) {
            Ok((200, v)) => {
                let port = std::fs::read_to_string(crate::data_dir(args).join("admin.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                    .and_then(|j| j["port"].as_u64())
                    .unwrap_or(0);
                println!("Open the Kakurega extension and enter:");
                println!("  Port: {port}");
                println!("  Code: {}", v["code"].as_str().unwrap_or(""));
                println!("The code works once and expires in 5 minutes. Pairing again replaces the extension that was paired before.");
                0
            }
            other => {
                eprintln!("could not create a pairing code: {other:?}");
                1
            }
        },
        Some("status") => match admin.get("/admin/extension") {
            Ok((200, v)) => {
                if v["paired"] == true {
                    println!("Paired with {}", v["origin"].as_str().unwrap_or("?"));
                } else {
                    println!("No extension is paired.");
                }
                0
            }
            other => {
                eprintln!("could not read the status: {other:?}");
                1
            }
        },
        Some("revoke") => match admin.post("/admin/extension/revoke", json!({})) {
            Ok((200, v)) => {
                println!(
                    "{}",
                    if v["revoked"] == true {
                        "The extension was disconnected."
                    } else {
                        "No extension was paired."
                    }
                );
                0
            }
            other => {
                eprintln!("could not revoke: {other:?}");
                1
            }
        },
        _ => {
            eprintln!("usage: kaku extension pair | status | revoke");
            2
        }
    }
}
