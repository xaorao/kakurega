use crate::app::{Method, Req, Resp};
use serde_json::Value;
use std::path::Path;

pub struct Http {
    agent: ureq::Agent,
    admin_agent: ureq::Agent,
    base: String,
    token: String,
    csrf: String,
}

fn build_agent() -> ureq::Agent {
    let cfg = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
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
        };
        h.pair()?;
        Ok(h)
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

    pub fn run(&self, req: &Req) -> Resp {
        Resp {
            id: req.id,
            result: self.call(req.admin, req.method, &req.path, req.body.as_ref()),
        }
    }
}
