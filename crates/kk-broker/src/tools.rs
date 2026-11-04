use crate::protocol::ErrorCode;
use kk_kernel::{ArtifactHandle, JobKey, Kernel, KernelError, StepId};
use kk_policy::{Decision, Engine, Op, Resource, StepFacts, WorkerFacts};
use kk_route::ToolSpec;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ToolFailure {
    pub code: ErrorCode,
    pub detail: String,
}

impl ToolFailure {
    pub fn new(code: ErrorCode, detail: &str) -> Self {
        ToolFailure {
            code,
            detail: detail.to_string(),
        }
    }
}

pub struct ToolCtx<'a> {
    pub(crate) kernel: &'a mut Kernel,
    pub(crate) policy: &'a Engine,
    pub(crate) worker: &'a WorkerFacts,
    pub(crate) step: StepId,
    pub(crate) job: &'a JobKey,
    pub(crate) files: &'a BTreeMap<String, u64>,
    pub(crate) human_approved: bool,
    pub(crate) bytes_read: &'a mut u64,
}

impl ToolCtx<'_> {
    pub fn files(&self) -> &BTreeMap<String, u64> {
        self.files
    }

    fn facts(&self) -> StepFacts {
        StepFacts {
            taint: self
                .kernel
                .taint(self.step)
                .cloned()
                .unwrap_or_else(kk_types::label::Label::bottom),
            human_approved: self.human_approved,
        }
    }

    pub fn can_read(&self, index: u64) -> bool {
        let handle = ArtifactHandle::of(self.job, index);
        let Some(label) = self.kernel.label_of(&handle) else {
            return false;
        };
        matches!(
            self.policy.decide(
                self.worker,
                Op::ReadArtifact,
                &Resource::Artifact {
                    id: index.to_string(),
                    label
                },
                &self.facts()
            ),
            Decision::Allow { .. }
        )
    }

    pub fn read(&mut self, index: u64) -> Result<Vec<u8>, ToolFailure> {
        let handle = ArtifactHandle::of(self.job, index);
        let label = self
            .kernel
            .label_of(&handle)
            .ok_or_else(|| ToolFailure::new(ErrorCode::UnknownHandle, "no such artifact"))?;
        let decision = self.policy.decide(
            self.worker,
            Op::ReadArtifact,
            &Resource::Artifact {
                id: index.to_string(),
                label,
            },
            &self.facts(),
        );
        if !decision.is_allow() {
            return Err(ToolFailure::new(
                ErrorCode::DenyPolicy,
                "policy refused this read",
            ));
        }
        let bytes = self
            .kernel
            .read_artifact(self.step, &handle)
            .map_err(|e| match e {
                KernelError::LabelExceedsClearance => {
                    ToolFailure::new(ErrorCode::LabelExceedsClearance, "label exceeds clearance")
                }
                _ => ToolFailure::new(ErrorCode::UnknownHandle, "no such artifact"),
            })?;
        *self.bytes_read += bytes.len() as u64;
        Ok(bytes)
    }
}

pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn readonly(&self) -> bool;
    fn call(&self, ctx: &mut ToolCtx<'_>, args: &Value) -> Result<Value, ToolFailure>;
}

pub struct ListFiles;
pub struct ReadFile;
pub struct SearchText;

fn string_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, ToolFailure> {
    args.get(name).and_then(Value::as_str).ok_or_else(|| {
        ToolFailure::new(
            ErrorCode::BadRequest,
            &format!("argument `{name}` must be a string"),
        )
    })
}

fn only_keys(args: &Value, allowed: &[&str]) -> Result<(), ToolFailure> {
    match args {
        Value::Object(m) if m.keys().all(|k| allowed.contains(&k.as_str())) => Ok(()),
        _ => Err(ToolFailure::new(
            ErrorCode::BadRequest,
            "unexpected arguments",
        )),
    }
}

impl Tool for ListFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_files".into(),
            description: "List the paths of project files you are allowed to read, optionally under a prefix.".into(),
            parameters: json!({"type":"object","properties":{"prefix":{"type":"string"}}}),
        }
    }

    fn readonly(&self) -> bool {
        true
    }

    fn call(&self, ctx: &mut ToolCtx<'_>, args: &Value) -> Result<Value, ToolFailure> {
        only_keys(args, &["prefix"])?;
        let prefix = args.get("prefix").and_then(Value::as_str).unwrap_or("");
        let paths: Vec<&String> = ctx
            .files()
            .iter()
            .filter(|(p, idx)| p.starts_with(prefix) && ctx.can_read(**idx))
            .map(|(p, _)| p)
            .collect();
        Ok(json!({"paths": paths}))
    }
}

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read one project file by its path and return its text.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        }
    }

    fn readonly(&self) -> bool {
        true
    }

    fn call(&self, ctx: &mut ToolCtx<'_>, args: &Value) -> Result<Value, ToolFailure> {
        only_keys(args, &["path"])?;
        let path = string_arg(args, "path")?;
        let missing = || ToolFailure::new(ErrorCode::UnknownHandle, "no such file");
        let index = *ctx.files().get(path).ok_or_else(missing)?;
        if !ctx.can_read(index) {
            return Err(missing());
        }
        let bytes = ctx.read(index)?;
        Ok(json!({"path": path, "text": String::from_utf8_lossy(&bytes)}))
    }
}

impl Tool for SearchText {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search_text".into(),
            description:
                "Find lines containing a literal string in the project files you may read.".into(),
            parameters: json!({"type":"object","properties":{"needle":{"type":"string"},"prefix":{"type":"string"}},"required":["needle"]}),
        }
    }

    fn readonly(&self) -> bool {
        true
    }

    fn call(&self, ctx: &mut ToolCtx<'_>, args: &Value) -> Result<Value, ToolFailure> {
        only_keys(args, &["needle", "prefix"])?;
        let needle = string_arg(args, "needle")?;
        if needle.is_empty() || needle.len() > 256 {
            return Err(ToolFailure::new(
                ErrorCode::BadRequest,
                "needle must be 1 to 256 bytes",
            ));
        }
        let prefix = args.get("prefix").and_then(Value::as_str).unwrap_or("");
        let candidates: Vec<(String, u64)> = ctx
            .files()
            .iter()
            .filter(|(p, idx)| p.starts_with(prefix) && ctx.can_read(**idx))
            .map(|(p, i)| (p.clone(), *i))
            .collect();
        let mut hits = Vec::new();
        for (path, index) in candidates {
            let bytes = ctx.read(index)?;
            for (n, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
                if line.contains(needle) && hits.len() < 200 {
                    hits.push(json!({"path": path, "line": n + 1, "text": line}));
                }
            }
        }
        Ok(json!({"matches": hits}))
    }
}
