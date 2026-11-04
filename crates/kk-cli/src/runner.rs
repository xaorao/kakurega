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

    fn route_info(&self) -> Value {
        json!({"id": format!("ollama:{}", self.model), "class": "device_only", "model": if self.scripted.is_some() { "scripted test model".to_string() } else { self.model.clone() }, "simulated": self.scripted.is_some(), "endpoint": self.ollama,
               "transport": match &self.transport { Transport::Local => "local sandbox".to_string(), Transport::Lima { vm, .. } => format!("sandbox in the {vm} virtual machine") }})
    }
}
