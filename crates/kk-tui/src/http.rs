use crate::app::{Method, Req, Resp};
use serde_json::Value;
use std::path::Path;

pub struct Http {
    agent: ureq::Agent,
    admin_agent: ureq::Agent,
    base: String,
    token: String,
    csrf: String,
    /// Where the service writes its address and token: read again when the service was started anew.
    data_dir: std::path::PathBuf,
}

/// How long a request that makes a model read documents and answer may take. A model on this computer can need
/// minutes; the ordinary limit would call a working answer a failure.
const MODEL_ANSWER_SECONDS: u64 = 900;

fn build_agent() -> ureq::Agent {
    let cfg = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .proxy(None)
        .build();
    cfg.into()
}

impl Http {
    pub fn connect(data_dir: &Path) -> Result<Http, String> {
        let text = std::fs::read_to_string(data_dir.join("admin.json")).map_err(|_| {
            "kaku serve is not running (no admin.json in the data directory)".to_string()
        })?;
        let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let port = v["port"].as_u64().ok_or("damaged admin.json")?;
        let token = v["token"].as_str().ok_or("damaged admin.json")?.to_string();
        let mut h = Http {
            agent: build_agent(),
            admin_agent: build_agent(),
            base: format!("http://127.0.0.1:{port}"),
            token,
            csrf: String::new(),
            data_dir: data_dir.to_path_buf(),
        };
        h.pair()?;
        Ok(h)
    }

    /// Connects again from what the service wrote when it last started: a service that was stopped and started has a new
    /// token, and its paired session is gone. Leaves this connection as it was when that fails.
    pub fn reconnect(&mut self) -> Result<(), String> {
        let fresh = Http::connect(&self.data_dir)?;
        *self = fresh;
        Ok(())
    }

    /// Whether an answer says this connection is no longer the service's: it could not be reached at all, or it no longer
    /// knows the session (it was started again). An answer that broke off part way is not one: asking again could run
    /// the same thing twice.
    pub fn lost(resp: &Resp) -> bool {
        match &resp.result {
            Ok((401, _)) => true,
            Ok(_) => false,
            Err(e) => e != "the answer ended before it was finished",
        }
    }

    fn pair(&mut self) -> Result<(), String> {
        let (st, code) = self.call(
            true,
            Method::Post,
            "/admin/pairing",
            Some(&Value::Object(Default::default())),
        )?;
        let code = code["code"]
            .as_str()
            .filter(|_| st == 200)
            .ok_or("the service refused to issue a pairing code")?
            .to_string();
        let (st, session) = self.call(
            false,
            Method::Post,
            "/api/pair",
            Some(&serde_json::json!({"code": code})),
        )?;
        self.csrf = session["csrf"]
            .as_str()
            .filter(|_| st == 200)
            .ok_or("pairing failed")?
            .to_string();
        Ok(())
    }

    fn call(
        &self,
        admin: bool,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), String> {
        let url = format!("{}{path}", self.base);
        let mut resp = match method {
            Method::Get => {
                if admin {
                    self.admin_agent
                        .get(&url)
                        .header("Authorization", &format!("Bearer {}", self.token))
                        .call()
                } else {
                    self.agent.get(&url).call()
                }
            }
            Method::Post => {
                let mut r = if admin {
                    self.admin_agent.post(&url)
                } else if path == "/api/ask" {
                    // the same connection and session, with a limit a model that reads documents can meet
                    self.agent
                        .post(&url)
                        .config()
                        .timeout_global(Some(std::time::Duration::from_secs(MODEL_ANSWER_SECONDS)))
                        .build()
                } else {
                    self.agent.post(&url)
                };
                if admin {
                    r = r.header("Authorization", &format!("Bearer {}", self.token));
                } else {
                    r = r
                        .header("Origin", &self.base)
                        .header("Sec-Fetch-Site", "same-origin")
                        .header("x-kk-csrf", &self.csrf);
                }
                r.send_json(body.cloned().unwrap_or(Value::Null))
            }
        }
        .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
    }

    /// Runs a streamed request whose lines are events of several kinds (a roundtable): `on_event` gets every line that is not
    /// the end. Setting `cancel` closes the connection, which stops the model. The result is the last object of the stream
    /// (`done`, `stopped`, or the error).
    pub fn run_events(
        &self,
        req: &Req,
        cancel: &std::sync::atomic::AtomicBool,
        on_event: &mut dyn FnMut(&Value),
    ) -> Resp {
        use std::io::BufRead;
        use std::sync::atomic::Ordering;
        let result = (|| -> Result<(u16, Value), String> {
            let url = format!("{}{}", self.base, req.path);
            let mut r = self
                .agent
                .post(&url)
                .config()
                .timeout_global(Some(std::time::Duration::from_secs(1800)))
                .build()
                .header("Origin", &self.base)
                .header("Sec-Fetch-Site", "same-origin")
                .header("x-kk-csrf", &self.csrf)
                .send_json(req.body.clone().unwrap_or(Value::Null))
                .map_err(|e| e.to_string())?;
            let status = r.status().as_u16();
            if status != 200 {
                let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
                return Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)));
            }
            let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
            let reader = r.into_body().into_reader();
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(reader);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            let _ = tx.send(Err("the answer ended before it was finished".into()));
                            return;
                        }
                        Ok(_) => {
                            if tx.send(Ok(line)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    }
                }
            });
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Ok((200, serde_json::json!({"stopped": true})));
                }
                let line = match rx.recv_timeout(std::time::Duration::from_millis(80)) {
                    Ok(l) => l?,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err("the answer ended before it was finished".into())
                    }
                };
                let v: Value = serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
                if v.get("error").is_some() {
                    return Ok((502, v));
                } else if v["done"] == true || v["stopped"] == true {
                    return Ok((200, v));
                }
                on_event(&v);
            }
        })();
        Resp { id: req.id, result }
    }

    /// Runs a streamed request: `on_delta` gets each piece as it arrives. Setting `cancel` closes the connection, which
    /// stops the model. The result is the final object of the stream, `{"stopped": true}` after a cancel, or the error.
    pub fn run_stream(
        &self,
        req: &Req,
        cancel: &std::sync::atomic::AtomicBool,
        on_delta: &mut dyn FnMut(&str),
    ) -> Resp {
        use std::io::BufRead;
        use std::sync::atomic::Ordering;
        let result = (|| -> Result<(u16, Value), String> {
            let url = format!("{}{}", self.base, req.path);
            // the same agent as every other request, so the paired session is sent; only the time allowed is longer
            let mut r = self
                .agent
                .post(&url)
                .config()
                .timeout_global(Some(std::time::Duration::from_secs(900)))
                .build()
                .header("Origin", &self.base)
                .header("Sec-Fetch-Site", "same-origin")
                .header("x-kk-csrf", &self.csrf)
                .send_json(req.body.clone().unwrap_or(Value::Null))
                .map_err(|e| e.to_string())?;
            let status = r.status().as_u16();
            if status != 200 {
                let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
                return Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)));
            }
            // read on another thread so a cancel is noticed even while the model is silent; when the reader is dropped
            // the connection closes and the model stops
            let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
            let reader = r.into_body().into_reader();
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(reader);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            let _ = tx.send(Err("the answer ended before it was finished".into()));
                            return;
                        }
                        Ok(_) => {
                            if tx.send(Ok(line)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    }
                }
            });
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Ok((200, serde_json::json!({"stopped": true})));
                }
                let line = match rx.recv_timeout(std::time::Duration::from_millis(80)) {
                    Ok(l) => l?,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err("the answer ended before it was finished".into())
                    }
                };
                let v: Value = serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
                if let Some(d) = v["delta"].as_str() {
                    on_delta(d);
                } else if v.get("error").is_some() {
                    return Ok((502, v));
                } else if v["done"] == true || v["stopped"] == true {
                    return Ok((200, v));
                }
            }
        })();
        Resp { id: req.id, result }
    }

    /// A request in the owner's session whose answer takes as long as a model takes (notes from a long transcript): the
    /// same agent and headers as any other, with fifteen minutes allowed.
    pub fn post_slow(&self, path: &str, body: &Value) -> Result<(u16, Value), String> {
        let mut resp = self
            .agent
            .post(&format!("{}{path}", self.base))
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(900)))
            .build()
            .header("Origin", &self.base)
            .header("Sec-Fetch-Site", "same-origin")
            .header("x-kk-csrf", &self.csrf)
            .send_json(body.clone())
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
    }

    pub fn run(&self, req: &Req) -> Resp {
        Resp {
            id: req.id,
            result: self.call(req.admin, req.method, &req.path, req.body.as_ref()),
        }
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn px03_the_terminal_talks_to_the_local_service_and_never_through_an_environment_proxy() {
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
        let _ = build_agent()
            .get(&format!("http://{}/api/health", svc_addr))
            .call();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(proxy.accept().is_err(), "the proxy was contacted");
        assert!(
            reached.join().unwrap(),
            "the service was contacted directly"
        );
    }
}
