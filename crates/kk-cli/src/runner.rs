use kk_review::job::{run_job, JobConfig, JobReport};
use kk_route::{ChatMessage, OllamaRoute, Route, RouteClass, RouteSet, ScriptedRoute};
use kk_sandbox::Transport;
use kk_service::JobRunner;
use kk_types::label::Level;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct ReviewRunner {
    pub model: String,
    pub ollama: String,
    pub transport: Transport,
    pub worker_binary: PathBuf,
    pub scratch: PathBuf,
    pub commitment_key: [u8; 32],
    pub max_turns: u32,
    pub scripted: Option<PathBuf>,
}

impl JobRunner for ReviewRunner {
    fn run(
        &self,
        repo: &Path,
        recipient: &str,
        job_name: &str,
        now: i64,
        extra: Vec<kk_review::import::ImportedFile>,
        progress: &dyn Fn(&str, &str),
    ) -> JobReport {
        let cfg = JobConfig {
            repo: repo.to_path_buf(),
            recipient: recipient.to_string(),
            default_level: Level::Internal,
            worker_binary: self.worker_binary.clone(),
            transport: self.transport.clone(),
            scratch: self.scratch.clone(),
            python: "/usr/bin/python3".into(),
            max_turns: self.max_turns,
            now,
            ttl_secs: 86_400,
            policy_version: "policy-1".into(),
            commitment_key: self.commitment_key,
            job_name: job_name.to_string(),
            extra_files: extra,
        };
        if let Some(path) = &self.scripted {
            let script: Vec<ChatMessage> = match std::fs::read_to_string(path)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
            {
                Some(s) => s,
                None => {
                    let mut r = run_job(&cfg, RouteSet::new(vec![]), vec![], progress);
                    r.error = Some("the scripted test model file is unreadable".into());
                    return r;
                }
            };
            let route =
                ScriptedRoute::new("scripted-test-model", RouteClass::DeviceOnly, false, script);
            let ev = vec![route.evidence()];
            return run_job(
                &cfg,
                RouteSet::new(vec![Box::new(route) as Box<dyn Route>]),
                ev,
                progress,
            );
        }
        match OllamaRoute::new(&self.ollama, &self.model) {
            Ok(route) => {
                let ev = vec![route.evidence()];
                run_job(
                    &cfg,
                    RouteSet::new(vec![Box::new(route) as Box<dyn Route>]),
                    ev,
                    progress,
                )
            }
            Err(e) => {
                let mut r = run_job(&cfg, RouteSet::new(vec![]), vec![], progress);
                r.error = Some(e.to_string());
                r
            }
        }
    }

    fn chat(
        &self,
        messages: Vec<kk_types::chat::ChatMessage>,
    ) -> Result<kk_service::ChatReply, String> {
        if self.scripted.is_some() {
            let last = messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default();
            return Ok(kk_service::ChatReply {
                text: format!("[test model] I received {} characters. This is a scripted reply, not a real model.", last.chars().count()),
                route_id: "scripted-test-model".into(),
                class: "device_only".into(),
                simulated: true,
            });
        }
        let route = OllamaRoute::new(&self.ollama, &self.model).map_err(|e| e.to_string())?;
        let done = route
            .complete(&kk_route::CompletionRequest {
                messages,
                tools: Vec::new(),
                max_tokens: 500,
                temperature: 0.2,
            })
            .map_err(|e| format!("the local model did not answer: {e}"))?;
        Ok(kk_service::ChatReply {
            text: done.message.content,
            route_id: route.id().to_string(),
            class: "device_only".into(),
            simulated: false,
        })
    }

    fn readiness(&self) -> Vec<Value> {
        let check = |id: &str, label: &str, ok: bool, detail: String, fix: &str| json!({"id": id, "label": label, "ok": ok, "detail": detail, "fix": if ok { "" } else { fix }});
        let mut out = Vec::new();
        if self.scripted.is_some() {
            out.push(check(
                "model",
                "Model",
                true,
                "scripted test model (test mode)".into(),
                "",
            ));
        } else {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(2)))
                .build()
                .into();
            match agent
                .get(&format!("{}/api/tags", self.ollama.trim_end_matches('/')))
                .call()
            {
                Ok(mut r) => {
                    let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
                    out.push(check(
                        "ollama",
                        "Local model service (Ollama)",
                        true,
                        format!("running at {}", self.ollama),
                        "",
                    ));
                    let has = v["models"].as_array().is_some_and(|m| {
                        m.iter().any(|x| {
                            let n = x["name"].as_str().unwrap_or("");
                            n == self.model
                                || n.strip_suffix(":latest") == Some(self.model.as_str())
                        })
                    });
                    out.push(check(
                        "model",
                        "Model",
                        has,
                        if has {
                            format!("{} is installed", self.model)
                        } else {
                            format!("{} is not installed", self.model)
                        },
                        &format!("Run in a terminal: ollama pull {}", self.model),
                    ));
                }
                Err(_) => {
                    out.push(check(
                        "ollama",
                        "Local model service (Ollama)",
                        false,
                        format!("nothing answered at {}", self.ollama),
                        "Start it with: ollama serve",
                    ));
                }
            }
        }
        let worker_fix = "Build it: cargo build -p kk-worker (for the virtual machine, scripts/e2e builds it there)";
        match &self.transport {
            Transport::Local => {
                out.push(check(
                    "worker",
                    "Review worker",
                    self.worker_binary.exists(),
                    self.worker_binary.display().to_string(),
                    worker_fix,
                ));
                let ok = std::process::Command::new("bwrap")
                    .arg("--version")
                    .output()
                    .is_ok_and(|o| o.status.success());
                out.push(check(
                    "sandbox",
                    "Sandbox (bubblewrap)",
                    ok,
                    if ok {
                        "available".into()
                    } else {
                        "bwrap not found".into()
                    },
                    "Install bubblewrap with your package manager",
                ));
            }
            Transport::Lima { vm, .. } => {
                let status = std::process::Command::new("limactl")
                    .args(["list", vm, "--format", "{{.Status}}"])
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                let running = status == "Running";
                out.push(check(
                    "sandbox",
                    "Sandbox virtual machine",
                    running,
                    if status.is_empty() {
                        format!("{vm}: not found")
                    } else {
                        format!("{vm}: {status}")
                    },
                    &format!("Start it with: limactl start {vm}"),
                ));
                if running {
                    let present = std::process::Command::new("limactl")
                        .args(["shell", vm, "--", "test", "-x"])
                        .arg(&self.worker_binary)
                        .status()
                        .is_ok_and(|s| s.success());
                    out.push(check(
                        "worker",
                        "Review worker",
                        present,
                        self.worker_binary.display().to_string(),
                        worker_fix,
                    ));
                } else {
                    out.push(check(
                        "worker",
                        "Review worker",
                        false,
                        "not checked: the virtual machine is not running".into(),
                        worker_fix,
                    ));
                }
            }
        }
        out
    }

    fn route_info(&self) -> Value {
        json!({"id": format!("ollama:{}", self.model), "class": "device_only", "model": if self.scripted.is_some() { "scripted test model".to_string() } else { self.model.clone() }, "simulated": self.scripted.is_some(), "endpoint": self.ollama,
               "transport": match &self.transport { Transport::Local => "local sandbox".to_string(), Transport::Lima { vm, .. } => format!("sandbox in the {vm} virtual machine") }})
    }
}
