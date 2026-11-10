use kk_types::chat::ToolSpec;
use kk_worker::{
    finding_tool, fix_tool, load_state, memory_tool, run, AgentConfig, AgentState, Outcome,
    StdioLink, WorkerError,
};
use serde_json::json;
use std::io::{stdin, stdout, BufReader};

fn main() {
    std::process::exit(real_main());
}

fn real_main() -> i32 {
    let system = std::env::var("KK_SYSTEM").unwrap_or_default();
    let task = std::env::var("KK_TASK").unwrap_or_default();
    let max_turns = std::env::var("KK_MAX_TURNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);
    let tool_specs: Vec<ToolSpec> = std::env::var("KK_TOOLS")
        .ok()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    let mut link = StdioLink::new(BufReader::new(stdin().lock()), stdout().lock());
    let state = match load_state(&mut link) {
        Ok(Some(s)) => s,
        Ok(None) => AgentState::new(&system, &task),
        Err(e) => return report(&e),
    };
    let config = AgentConfig {
        tools: tool_specs,
        local_tools: vec![finding_tool(), fix_tool(), memory_tool()],
        max_turns,
        max_tokens: 1024,
        min_looks: 4,
    };
    match run(&mut link, state, &config, &|| false) {
        Ok(Outcome::Finished(text)) => {
            let _ = text;
            0
        }
        Ok(Outcome::TurnLimit) => 4,
        Ok(Outcome::Stuck(reason)) => {
            eprintln!("{}", json!({"worker_stopped": reason}));
            6
        }
        Ok(Outcome::Cancelled) => 5,
        Err(e) => report(&e),
    }
}

fn report(e: &WorkerError) -> i32 {
    eprintln!("{}", json!({"worker_error": e.to_string()}));
    3
}
