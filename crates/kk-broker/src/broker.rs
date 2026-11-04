use crate::protocol::{parse_line, ErrorCode, Reply, Request, MAX_LINE_BYTES};
use crate::tools::{ListFiles, ReadFile, SearchText, Tool, ToolCtx, ToolFailure};
use kk_evidence::b64;
use kk_kernel::{ArtifactHandle, JobKey, Kernel, KernelError, StepId};
use kk_policy::{Decision, DenyReason, Engine, Op, Resource, StepFacts, WorkerFacts};
use kk_route::{CompletionRequest, RouteSet};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone)]
pub struct BrokerConfig {
    pub max_calls: u32,
    pub max_bytes_read: u64,
    pub max_model_tokens: u64,
    pub max_write_bytes: usize,
    pub max_checkpoint_bytes: usize,
    pub allowed_ops: BTreeSet<&'static str>,
}

impl Default for BrokerConfig {
    fn default() -> Self {
        BrokerConfig {
            max_calls: 500,
            max_bytes_read: 64 << 20,
            max_model_tokens: 200_000,
            max_write_bytes: 8 << 20,
            max_checkpoint_bytes: 8 << 20,
            allowed_ops: [
                "read.artifact",
                "write.artifact",
                "model.complete",
                "tool.call",
                "checkpoint",
                "checkpoint.load",
                "log.note",
                "release.propose",
            ]
            .into_iter()
            .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub session: SessionId,
    pub request_id: u64,
    pub op: String,
    pub outcome: String,
    pub detail: String,
}

struct Session {
    job: JobKey,
    step: StepId,
    worker: WorkerFacts,
    last_id: Option<u64>,
    calls: u32,
    bytes_read: u64,
    model_tokens: u64,
    human_approved: bool,
    open: bool,
    config: BrokerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenArtifact {
    pub job: JobKey,
    pub handle: u64,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDraft {
    pub session: SessionId,
    pub handles: Vec<u64>,
    pub recipient: String,
}

struct StoredCheckpoint {
    bytes: Vec<u8>,
    taint: kk_types::label::Label,
}

pub struct Broker {
    pub kernel: Kernel,
    checkpoints: HashMap<JobKey, StoredCheckpoint>,
    final_taints: HashMap<SessionId, kk_types::label::Label>,
    policy: Engine,
    routes: RouteSet,
    tools: BTreeMap<String, Box<dyn Tool>>,
    files: HashMap<JobKey, BTreeMap<String, u64>>,
    sessions: HashMap<SessionId, Session>,
    next_session: u64,
    pub audit: Vec<AuditEvent>,
    pub release_drafts: Vec<ReleaseDraft>,
    pub written: Vec<WrittenArtifact>,
}

struct Failure {
    code: ErrorCode,
    detail: String,
}

fn fail<T>(code: ErrorCode, detail: &str) -> Result<T, Failure> {
    Err(Failure {
        code,
        detail: detail.to_string(),
    })
}

impl From<ToolFailure> for Failure {
    fn from(f: ToolFailure) -> Self {
        Failure {
            code: f.code,
            detail: f.detail,
        }
    }
}

fn decision_to_result(d: Decision) -> Result<(), Failure> {
    match d {
        Decision::Allow { .. } => Ok(()),
        Decision::Deny(DenyReason::NoMatchingPermit) | Decision::Deny(DenyReason::Forbidden(_)) => {
            fail(ErrorCode::DenyPolicy, "policy refused this operation")
        }
        Decision::Deny(DenyReason::EvaluationError(_))
        | Decision::Deny(DenyReason::InvalidRequest(_)) => fail(
            ErrorCode::DenyError,
            "policy could not be evaluated, which counts as a refusal",
        ),
    }
}

impl Broker {
    pub fn new(policy: Engine, routes: RouteSet) -> Self {
        let mut tools: BTreeMap<String, Box<dyn Tool>> = BTreeMap::new();
        for t in [
            Box::new(ListFiles) as Box<dyn Tool>,
            Box::new(ReadFile),
            Box::new(SearchText),
        ] {
            tools.insert(t.spec().name, t);
        }
        Broker {
            kernel: Kernel::new(),
            checkpoints: HashMap::new(),
            final_taints: HashMap::new(),
            policy,
            routes,
            tools,
            files: HashMap::new(),
            sessions: HashMap::new(),
            next_session: 1,
            audit: Vec::new(),
            release_drafts: Vec::new(),
            written: Vec::new(),
        }
    }

    pub fn tool_specs(&self) -> Vec<kk_route::ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.spec().name, tool);
    }

    pub fn import_file(
        &mut self,
        job: &JobKey,
        path: &str,
        bytes: Vec<u8>,
        label: kk_types::label::Label,
    ) -> ArtifactHandle {
        let handle = self.kernel.import(job, bytes, label);
        self.files
            .entry(job.clone())
            .or_default()
            .insert(path.to_string(), handle.index());
        handle
    }

    pub fn open_session(
        &mut self,
        job: &JobKey,
        worker: WorkerFacts,
        config: BrokerConfig,
    ) -> SessionId {
        let step = self.kernel.open_step(job, worker.clearance.clone());
        let id = SessionId(self.next_session);
        self.next_session += 1;
        self.sessions.insert(
            id,
            Session {
                job: job.clone(),
                step,
                worker,
                last_id: None,
                calls: 0,
                bytes_read: 0,
                model_tokens: 0,
                human_approved: false,
                open: true,
                config,
            },
        );
        id
    }

    pub fn close_session(&mut self, id: SessionId) {
        if let Some(s) = self.sessions.get_mut(&id) {
            if s.open {
                if let Some(t) = self.kernel.taint(s.step) {
                    self.final_taints.insert(id, t.clone());
                }
            }
            s.open = false;
            self.kernel.close_step(s.step);
        }
    }

    pub fn checkpoint_of(&self, id: SessionId) -> Option<&[u8]> {
        let job = &self.sessions.get(&id)?.job;
        self.checkpoints.get(job).map(|c| c.bytes.as_slice())
    }

    pub fn final_taint(&self, id: SessionId) -> Option<kk_types::label::Label> {
        self.final_taints.get(&id).cloned()
    }

    pub fn taint_of(&self, id: SessionId) -> Option<kk_types::label::Label> {
        self.sessions
            .get(&id)
            .and_then(|s| self.kernel.taint(s.step).cloned())
    }

    pub fn handle_line(&mut self, id: SessionId, line: &str) -> String {
        let reply = self.handle_line_inner(id, line);
        serde_json::to_string(&reply).unwrap_or_else(|_| "{\"id\":0,\"ok\":false}".to_string())
    }

    fn handle_line_inner(&mut self, id: SessionId, line: &str) -> Reply {
        if line.len() > MAX_LINE_BYTES {
            return Reply::failure(0, ErrorCode::BadRequest, "request line is too large");
        }
        let envelope = match parse_line(line) {
            Ok(e) => e,
            Err(e) => {
                return Reply::failure(
                    0,
                    ErrorCode::BadRequest,
                    &format!("not a valid request: {e}"),
                )
            }
        };
        let req_id = envelope.id;
        let op = envelope.request.op_name();
        let outcome = self.dispatch(id, req_id, envelope.request);
        match outcome {
            Ok(v) => {
                self.audit.push(AuditEvent {
                    session: id,
                    request_id: req_id,
                    op: op.to_string(),
                    outcome: "ok".into(),
                    detail: String::new(),
                });
                Reply::success(req_id, v)
            }
            Err(f) => {
                self.audit.push(AuditEvent {
                    session: id,
                    request_id: req_id,
                    op: op.to_string(),
                    outcome: format!("{:?}", f.code),
                    detail: f.detail.clone(),
                });
                Reply::failure(req_id, f.code, &f.detail)
            }
        }
    }

    fn dispatch(&mut self, id: SessionId, req_id: u64, request: Request) -> Result<Value, Failure> {
        {
            let s = self.sessions.get_mut(&id).ok_or(Failure {
                code: ErrorCode::Expired,
                detail: "no such session".into(),
            })?;
            if !s.open {
                return fail(ErrorCode::Expired, "session is closed");
            }
            if let Some(last) = s.last_id {
                if req_id <= last {
                    return fail(ErrorCode::Replay, "request ids must increase");
                }
            }
            s.last_id = Some(req_id);
            if !s.config.allowed_ops.contains(request.op_name()) {
                return fail(
                    ErrorCode::OpNotAllowed,
                    "this operation is not part of the plan",
                );
            }
            s.calls += 1;
            if s.calls > s.config.max_calls {
                return fail(ErrorCode::BudgetExceeded, "call budget used up");
            }
        }
        match request {
            Request::ReadArtifact { handle } => self.op_read(id, handle),
            Request::WriteArtifact {
                bytes_b64,
                derived_from_hint,
                kind,
            } => self.op_write(id, &bytes_b64, &derived_from_hint, &kind),
            Request::ModelComplete {
                messages,
                tools,
                max_tokens,
            } => self.op_model(id, messages, tools, max_tokens),
            Request::ToolCall {
                request_id,
                name,
                args,
            } => self.op_tool(id, &request_id, &name, &args),
            Request::Checkpoint { state_b64 } => self.op_checkpoint(id, &state_b64),
            Request::CheckpointLoad {} => self.op_checkpoint_load(id),
            Request::LogNote { code } => {
                if code.len() > 64
                    || !code
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
                {
                    return fail(ErrorCode::BadRequest, "note codes are short identifiers");
                }
                Ok(json!({}))
            }
            Request::ReleasePropose { handles, recipient } => {
                self.op_propose(id, handles, &recipient)
            }
        }
    }

    fn facts(&self, id: SessionId) -> (WorkerFacts, StepFacts, JobKey, StepId) {
        let s = &self.sessions[&id];
        (
            s.worker.clone(),
            StepFacts {
                taint: self
                    .kernel
                    .taint(s.step)
                    .cloned()
                    .unwrap_or_else(kk_types::label::Label::bottom),
                human_approved: s.human_approved,
            },
            s.job.clone(),
            s.step,
        )
    }

    fn op_read(&mut self, id: SessionId, handle: u64) -> Result<Value, Failure> {
        let (worker, step_facts, job, step) = self.facts(id);
        let h = ArtifactHandle::of(&job, handle);
        let label = self.kernel.label_of(&h).ok_or(Failure {
            code: ErrorCode::UnknownHandle,
            detail: "no such artifact".into(),
        })?;
        decision_to_result(self.policy.decide(
            &worker,
            Op::ReadArtifact,
            &Resource::Artifact {
                id: handle.to_string(),
                label,
            },
            &step_facts,
        ))?;
        let bytes = self.kernel.read_artifact(step, &h).map_err(|e| match e {
            KernelError::LabelExceedsClearance => Failure {
                code: ErrorCode::LabelExceedsClearance,
                detail: "label exceeds clearance".into(),
            },
            _ => Failure {
                code: ErrorCode::UnknownHandle,
                detail: "no such artifact".into(),
            },
        })?;
        let s = self.sessions.get_mut(&id).expect("session");
        s.bytes_read += bytes.len() as u64;
        if s.bytes_read > s.config.max_bytes_read {
            return fail(ErrorCode::BudgetExceeded, "read budget used up");
        }
        Ok(json!({"bytes_b64": b64::encode(&bytes)}))
    }

    fn op_write(
        &mut self,
        id: SessionId,
        bytes_b64: &str,
        hint: &[u64],
        kind: &str,
    ) -> Result<Value, Failure> {
        let (worker, step_facts, job, step) = self.facts(id);
        if kind.is_empty() || kind.len() > 64 {
            return fail(ErrorCode::BadRequest, "kind must be 1 to 64 characters");
        }
        let max = self.sessions[&id].config.max_write_bytes;
        let bytes = b64::decode(bytes_b64).map_err(|_| Failure {
            code: ErrorCode::BadRequest,
            detail: "bytes are not valid base64url".into(),
        })?;
        if bytes.len() > max {
            return fail(ErrorCode::BudgetExceeded, "write too large");
        }
        decision_to_result(self.policy.decide(
            &worker,
            Op::WriteArtifact,
            &Resource::Artifact {
                id: "new".into(),
                label: step_facts.taint.clone(),
            },
            &step_facts,
        ))?;
        let hints: Vec<ArtifactHandle> =
            hint.iter().map(|i| ArtifactHandle::of(&job, *i)).collect();
        let h = self
            .kernel
            .write_artifact(step, bytes, &hints)
            .map_err(|_| Failure {
                code: ErrorCode::BadRequest,
                detail: "step is closed".into(),
            })?;
        self.written.push(WrittenArtifact {
            job: job.clone(),
            handle: h.index(),
            kind: kind.to_string(),
        });
        Ok(json!({"handle": h.index()}))
    }

    fn op_model(
        &mut self,
        id: SessionId,
        messages: Vec<kk_route::ChatMessage>,
        tools: Vec<kk_route::ToolSpec>,
        max_tokens: u32,
    ) -> Result<Value, Failure> {
        let (worker, step_facts, _job, step) = self.facts(id);
        let route = self.routes.select(&step_facts.taint).map_err(|_| Failure {
            code: ErrorCode::NoEligibleRoute,
            detail: "no route is eligible for this data".into(),
        })?;
        decision_to_result(self.policy.decide(
            &worker,
            Op::ModelComplete,
            &Resource::Route {
                id: route.id().to_string(),
                class: route.class().as_str().to_string(),
                tee_verified: route.tee_verified(),
            },
            &step_facts,
        ))?;
        {
            let s = &self.sessions[&id];
            if s.model_tokens >= s.config.max_model_tokens {
                return fail(ErrorCode::BudgetExceeded, "model token budget used up");
            }
        }
        let completion = route
            .complete(&CompletionRequest {
                messages,
                tools,
                max_tokens: max_tokens.min(8192),
                temperature: 0.0,
            })
            .map_err(|e| Failure {
                code: ErrorCode::ToolError,
                detail: format!("model route failed: {e}"),
            })?;
        let used = completion.prompt_tokens + completion.completion_tokens;
        self.sessions.get_mut(&id).expect("session").model_tokens += used;
        let _ = self
            .kernel
            .absorb(step, &kk_types::label::Label::bottom().untrusted());
        Ok(json!({
            "message": completion.message,
            "route": route.id(),
            "prompt_tokens": completion.prompt_tokens,
            "completion_tokens": completion.completion_tokens,
        }))
    }

    fn op_tool(
        &mut self,
        id: SessionId,
        _request_id: &str,
        name: &str,
        args: &Value,
    ) -> Result<Value, Failure> {
        let (worker, step_facts, job, step) = self.facts(id);
        let tool = self.tools.get(name).ok_or(Failure {
            code: ErrorCode::UnknownTool,
            detail: "no such tool".into(),
        })?;
        decision_to_result(self.policy.decide(
            &worker,
            Op::ToolCall,
            &Resource::Tool {
                id: name.to_string(),
                readonly: tool.readonly(),
            },
            &step_facts,
        ))?;
        let files = self.files.get(&job).cloned().unwrap_or_default();
        let session = self.sessions.get_mut(&id).expect("session");
        let human = session.human_approved;
        let mut bytes_read = session.bytes_read;
        let mut ctx = ToolCtx {
            kernel: &mut self.kernel,
            policy: &self.policy,
            worker: &worker,
            step,
            job: &job,
            files: &files,
            human_approved: human,
            bytes_read: &mut bytes_read,
        };
        let out = tool.call(&mut ctx, args)?;
        let session = self.sessions.get_mut(&id).expect("session");
        session.bytes_read = bytes_read;
        if session.bytes_read > session.config.max_bytes_read {
            return fail(ErrorCode::BudgetExceeded, "read budget used up");
        }
        Ok(out)
    }

    fn op_checkpoint(&mut self, id: SessionId, state_b64: &str) -> Result<Value, Failure> {
        let max = self.sessions[&id].config.max_checkpoint_bytes;
        let bytes = b64::decode(state_b64).map_err(|_| Failure {
            code: ErrorCode::BadRequest,
            detail: "state is not valid base64url".into(),
        })?;
        if bytes.len() > max {
            return fail(ErrorCode::BudgetExceeded, "checkpoint too large");
        }
        let (job, taint) = {
            let s = &self.sessions[&id];
            (
                s.job.clone(),
                self.kernel
                    .taint(s.step)
                    .cloned()
                    .unwrap_or_else(kk_types::label::Label::bottom),
            )
        };
        self.checkpoints
            .insert(job, StoredCheckpoint { bytes, taint });
        Ok(json!({}))
    }

    fn op_checkpoint_load(&mut self, id: SessionId) -> Result<Value, Failure> {
        let (job, step) = {
            let s = &self.sessions[&id];
            (s.job.clone(), s.step)
        };
        let Some(stored) = self.checkpoints.get(&job) else {
            return Ok(json!({"state_b64": Value::Null}));
        };
        self.kernel
            .absorb(step, &stored.taint)
            .map_err(|_| Failure {
                code: ErrorCode::LabelExceedsClearance,
                detail: "the saved state holds data above this worker's clearance".into(),
            })?;
        Ok(json!({"state_b64": b64::encode(&stored.bytes)}))
    }

    fn op_propose(
        &mut self,
        id: SessionId,
        handles: Vec<u64>,
        recipient: &str,
    ) -> Result<Value, Failure> {
        let (worker, step_facts, job, _step) = self.facts(id);
        if recipient.is_empty()
            || recipient.len() > 128
            || handles.is_empty()
            || handles.len() > 256
        {
            return fail(
                ErrorCode::BadRequest,
                "a release names a recipient and 1 to 256 artifacts",
            );
        }
        decision_to_result(self.policy.decide(
            &worker,
            Op::ReleasePropose,
            &Resource::Recipient {
                id: recipient.to_string(),
                clearance_level: 0,
            },
            &step_facts,
        ))?;
        for h in &handles {
            if self
                .kernel
                .label_of(&ArtifactHandle::of(&job, *h))
                .is_none()
            {
                return fail(ErrorCode::UnknownHandle, "no such artifact");
            }
        }
        self.release_drafts.push(ReleaseDraft {
            session: id,
            handles,
            recipient: recipient.to_string(),
        });
        Ok(json!({"draft": self.release_drafts.len()}))
    }
}
