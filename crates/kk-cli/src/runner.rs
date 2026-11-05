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
    /// The context window asked of the local model, in tokens.
    pub context_tokens: u32,
}

impl ReviewRunner {
    fn route(&self) -> Result<OllamaRoute, kk_route::RouteError> {
        OllamaRoute::new(&self.ollama, &self.model)
            .map(|r| r.with_context_tokens(self.context_tokens))
    }
}

/// What the owner is told when the local model fails. A conversation larger than the model's window gets advice, not a status.
fn model_failure(e: kk_route::RouteError) -> String {
    match e {
        kk_route::RouteError::ContextTooLong { requested, window } => format!(
            "this conversation is too long for the model: it needs about {requested} tokens and the model's window holds {window}. Start a new conversation, send a shorter message, or choose fewer notes and web pages"
        ),
        other => format!("the local model did not answer: {other}"),
    }
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
        match self.route() {
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
            if messages
                .first()
                .is_some_and(|m| m.content.contains("FILE markers"))
            {
                let first = last.split("=== FILE ").nth(1).unwrap_or("");
                let name = first.split(" ===").next().unwrap_or("").to_string();
                let line = first.lines().nth(1).unwrap_or("").to_string();
                let text = json!({"answer": "Scripted test reply: see the passages below.", "citations": [{"file": name, "quote": line}, {"file": name, "quote": "This sentence was invented by the scripted model."}]}).to_string();
                return Ok(kk_service::ChatReply {
                    text,
                    route_id: "scripted-test-model".into(),
                    class: "device_only".into(),
                    simulated: true,
                    prompt_tokens: None,
                });
            }
            return Ok(kk_service::ChatReply {
                text: format!("[test model] I received {} characters. This is a scripted reply, not a real model.", last.chars().count()),
                route_id: "scripted-test-model".into(),
                class: "device_only".into(),
                simulated: true,
                prompt_tokens: None,
            });
        }
        let route = self.route().map_err(|e| e.to_string())?;
        let done = route
            .complete(&kk_route::CompletionRequest {
                messages,
                tools: Vec::new(),
                max_tokens: 500,
                temperature: 0.2,
            })
            .map_err(model_failure)?;
        Ok(kk_service::ChatReply {
            prompt_tokens: (done.prompt_tokens > 0).then_some(done.prompt_tokens),
            text: done.message.content,
            route_id: route.id().to_string(),
            class: "device_only".into(),
            simulated: false,
        })
    }

    fn chat_stream(
        &self,
        messages: Vec<kk_types::chat::ChatMessage>,
        on_chunk: &mut dyn FnMut(&str) -> bool,
    ) -> Result<kk_service::StreamedReply, String> {
        if self.scripted.is_some() {
            let reply = self.chat(messages)?;
            let mut sent = String::new();
            for word in reply.text.split_inclusive(' ') {
                sent.push_str(word);
                if !on_chunk(word) {
                    return Ok(kk_service::StreamedReply {
                        reply: kk_service::ChatReply {
                            text: sent,
                            ..reply
                        },
                        stopped: true,
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            return Ok(kk_service::StreamedReply {
                reply,
                stopped: false,
            });
        }
        let route = self.route().map_err(|e| e.to_string())?;
        let done = route
            .stream(
                &kk_route::CompletionRequest {
                    messages,
                    tools: Vec::new(),
                    max_tokens: 500,
                    temperature: 0.2,
                },
                on_chunk,
            )
            .map_err(model_failure)?;
        Ok(kk_service::StreamedReply {
            reply: kk_service::ChatReply {
                prompt_tokens: (done.prompt_tokens > 0).then_some(done.prompt_tokens),
                text: done.text,
                route_id: route.id().to_string(),
                class: "device_only".into(),
                simulated: false,
            },
            stopped: done.stopped,
        })
    }

    fn embedder(&self) -> Option<std::sync::Arc<dyn kk_memory::Embedder>> {
        if self.scripted.is_some() {
            return None;
        }
        let model =
            std::env::var("KAKU_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());
        self.route().ok().map(|route| {
            std::sync::Arc::new(OllamaEmbedder { route, model })
                as std::sync::Arc<dyn kk_memory::Embedder>
        })
    }

    fn repairable(&self) -> Vec<String> {
        if self.scripted.is_some() {
            Vec::new()
        } else {
            vec!["model".to_string()]
        }
    }

    fn repair(&self, id: &str, progress: &dyn Fn(&str)) -> Result<String, String> {
        if id != "model" || self.scripted.is_some() {
            return Err("this check cannot be fixed automatically".into());
        }
        let route = self.route().map_err(|e| e.to_string())?;
        route
            .pull(&mut |status, pct| {
                progress(&match pct {
                    Some(p) => format!("{status} {p}%"),
                    None => status.to_string(),
                });
                true
            })
            .map_err(|e| format!("could not install {}: {e}", self.model))?;
        Ok(format!("{} is installed", self.model))
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
        json!({"id": format!("ollama:{}", self.model), "class": "device_only", "model": if self.scripted.is_some() { "scripted test model".to_string() } else { self.model.clone() }, "simulated": self.scripted.is_some(), "endpoint": self.ollama, "context_tokens": self.context_tokens,
               "transport": match &self.transport { Transport::Local => "local sandbox".to_string(), Transport::Lima { vm, .. } => format!("sandbox in the {vm} virtual machine") }})
    }
}

/// Recall by meaning through the embedding model of the local Ollama. If the model is not installed, embedding fails and
/// recall quietly uses keywords and wording only.
struct OllamaEmbedder {
    route: OllamaRoute,
    model: String,
}

impl OllamaEmbedder {
    /// nomic-embed-text is trained with a task prefix on both sides; without them its scores bunch together and unrelated
    /// text scores almost as high as related text (measured on the real model).
    fn prefixed(&self) -> bool {
        self.model.starts_with("nomic-embed")
    }
}

impl kk_memory::Embedder for OllamaEmbedder {
    fn model(&self) -> String {
        if self.prefixed() {
            format!("{}+prefixes", self.model)
        } else {
            self.model.clone()
        }
    }
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let prepared: Vec<String> = if self.prefixed() {
            texts
                .iter()
                .map(|t| format!("search_document: {t}"))
                .collect()
        } else {
            texts.to_vec()
        };
        self.route
            .embed_with(&self.model, &prepared)
            .map_err(|e| e.to_string())
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        let prepared = if self.prefixed() {
            format!("search_query: {text}")
        } else {
            text.to_string()
        };
        self.route
            .embed_with(&self.model, &[prepared])
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| "no vector came back".to_string())
    }
    fn min_similarity(&self) -> f32 {
        if self.prefixed() {
            0.5
        } else {
            0.6
        }
    }
}

#[cfg(test)]
mod live {
    use super::*;
    use kk_service::JobRunner;

    /// Needs a local Ollama with qwen2.5:1.5b. Run with `cargo test -p kk-cli --bin kaku -- --ignored live::`.
    #[test]
    #[ignore]
    fn live_a_real_chat_reply_carries_the_model_s_own_prompt_count() {
        let runner = ReviewRunner {
            model: "qwen2.5:1.5b".into(),
            ollama: "http://127.0.0.1:11434".into(),
            transport: Transport::Local,
            worker_binary: PathBuf::new(),
            scratch: std::env::temp_dir(),
            commitment_key: [0; 32],
            max_turns: 1,
            scripted: None,
            context_tokens: kk_route::DEFAULT_CONTEXT_TOKENS,
        };
        let messages = vec![
            ChatMessage::new("system", "Answer in one short sentence."),
            ChatMessage::new(
                "user",
                "तिमाही रिपोर्ट में क्या बताया गया है? Answer in English.",
            ),
        ];
        let estimated = kk_route::tokens::estimate_messages(&messages);
        let reply = runner.chat(messages).expect("the local model answers");
        println!(
            "estimated {estimated}, counted {:?}, reply {:?}",
            reply.prompt_tokens, reply.text
        );
        let counted = reply
            .prompt_tokens
            .expect("a real reply carries the model's own count");
        assert!(!reply.simulated && counted > 0);
        assert!(
            (counted as f64) < (estimated as f64) * 3.0
                && (counted as f64) > (estimated as f64) / 3.0,
            "estimate {estimated} and count {counted} are in the same range"
        );
    }
}
