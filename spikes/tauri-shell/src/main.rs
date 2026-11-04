use serde_json::Value;
use std::time::{Duration, Instant};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

fn admin(dir: &std::path::Path) -> Result<(u16, String), String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("admin.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok((v["port"].as_u64().ok_or("port")? as u16, v["token"].as_str().ok_or("token")?.to_string()))
}

fn call(port: u16, token: &str, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
    let url = format!("http://127.0.0.1:{port}{path}");
    let mut r = if method == "GET" {
        agent.get(&url).header("Authorization", &format!("Bearer {token}")).call()
    } else {
        agent.post(&url).header("Authorization", &format!("Bearer {token}")).send_json(body.unwrap_or(Value::Null))
    }
    .map_err(|e| e.to_string())?;
    r.body_mut().read_json().map_err(|e| e.to_string())
}

fn main() {
    let started = Instant::now();
    let data = std::env::var("KAKUREGA_DATA_DIR").expect("KAKUREGA_DATA_DIR");
    let data = std::path::PathBuf::from(data);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let (port, token) = admin(&data)?;
            let code = call(port, &token, "POST", "/admin/pairing", Some(serde_json::json!({})))?["code"].as_str().ok_or("code")?.to_string();
            let origin = format!("http://127.0.0.1:{port}");
            let nav_origin = origin.clone();
            let url = format!("{origin}/#pair={code}").parse().map_err(|e: url::ParseError| e.to_string())?;
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Kakurega")
                .inner_size(1180.0, 820.0)
                .on_navigation(move |u| u.as_str().starts_with(&nav_origin))
                .on_page_load(move |_w, payload| {
                    if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                        println!("LOAD_MS {}", started.elapsed().as_millis());
                    }
                })
                .build()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut ready = false;
                let mut seen: Vec<String> = Vec::new();
                loop {
                    std::thread::sleep(Duration::from_millis(20));
                    if !ready {
                        if let Some(w) = handle.get_webview_window("main") {
                            if w.title().map(|t| t == "READY").unwrap_or(false) {
                                println!("READY_MS {}", started.elapsed().as_millis());
                                ready = true;
                            }
                        }
                    }
                    if ready {
                        break;
                    }
                }
                loop {
                    std::thread::sleep(Duration::from_millis(1000));
                    let Ok(list) = call(port, &token, "GET", "/admin/approvals", None) else { continue };
                    for item in list["approvals"].as_array().cloned().unwrap_or_default() {
                        let Some(id) = item["id"].as_str().map(str::to_string) else { continue };
                        if seen.contains(&id) {
                            continue;
                        }
                        seen.push(id.clone());
                        let Ok(d) = call(port, &token, "GET", &format!("/admin/approvals/{id}"), None) else { continue };
                        let digest = d["digest"].as_str().unwrap_or("").to_string();
                        let text = format!("Recipient: {}\nDigest: {}\nFiles: {}", d["recipient"].as_str().unwrap_or("?"), digest, d["files"].as_array().map(|a| a.len()).unwrap_or(0));
                        let yes = handle.dialog().message(text).title("Approve release").buttons(MessageDialogButtons::OkCancelCustom("Approve".into(), "Refuse".into())).blocking_show();
                        if yes {
                            let _ = call(port, &token, "POST", &format!("/admin/approvals/{id}/approve"), Some(serde_json::json!({"ack": digest.chars().take(12).collect::<String>()})));
                        } else {
                            let _ = call(port, &token, "POST", &format!("/admin/approvals/{id}/deny"), Some(serde_json::json!({})));
                        }
                    }
                }
            });
            let _ = window;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run");
}
