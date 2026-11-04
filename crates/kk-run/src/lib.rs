use kk_broker::{Broker, SessionId};
use kk_sandbox::{
    serve_lines_with, Bind, Limits, SandboxError, SandboxSpec, ServeOutcome, Transport,
};
use std::path::{Path, PathBuf};

pub struct WorkerLaunch {
    pub binary: PathBuf,
    pub system_prompt: String,
    pub task: String,
    pub tools_json: String,
    pub max_turns: u32,
    pub limits: Limits,
    pub transport: Transport,
}

pub fn run_worker(
    broker: &mut Broker,
    session: SessionId,
    launch: &WorkerLaunch,
) -> Result<ServeOutcome, SandboxError> {
    let mut spec = SandboxSpec::new("/opt/kk/worker");
    spec.binds.push(Bind {
        host: launch.binary.clone(),
        inside: Path::new("/opt/kk/worker").to_path_buf(),
        writable: false,
    });
    spec.env = vec![
        ("KK_SYSTEM".into(), launch.system_prompt.clone()),
        ("KK_TASK".into(), launch.task.clone()),
        ("KK_TOOLS".into(), launch.tools_json.clone()),
        ("KK_MAX_TURNS".into(), launch.max_turns.to_string()),
    ];
    spec.limits = launch.limits.clone();
    let outcome = serve_lines_with(&spec, &launch.transport, |line| {
        broker.handle_line(session, line)
    });
    broker.close_session(session);
    outcome
}

pub fn run_script(
    broker: &mut Broker,
    session: SessionId,
    program: &str,
    script: &Path,
    limits: Limits,
    transport: &Transport,
) -> Result<ServeOutcome, SandboxError> {
    let mut spec = SandboxSpec::new(program);
    spec.args = vec!["-I".into(), "/tmp/w.py".into()];
    spec.binds.push(Bind {
        host: script.to_path_buf(),
        inside: "/tmp/w.py".into(),
        writable: false,
    });
    spec.limits = limits;
    let outcome = serve_lines_with(&spec, transport, |line| broker.handle_line(session, line));
    broker.close_session(session);
    outcome
}
