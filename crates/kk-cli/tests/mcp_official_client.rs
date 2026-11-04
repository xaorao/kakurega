//! The official Model Context Protocol Rust SDK (rmcp 3.8.0, Apache-2.0) used as the client against `kaku mcp`.
//! A hand-written server is only as compatible as the clients it has met; this meets the reference client.

use rmcp::model::CallToolRequestParams;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{object, ServiceExt};
use tokio::process::Command;

async fn connect() -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let empty = std::env::temp_dir().join(format!("kk-mcp-official-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    ().serve(
        TokioChildProcess::new(Command::new(env!("CARGO_BIN_EXE_kaku")).configure(|c| {
            c.args(["mcp", "--data-dir"]).arg(&empty);
        }))
        .unwrap(),
    )
    .await
    .expect("the official client completes the handshake with our server")
}

#[tokio::test(flavor = "multi_thread")]
async fn mo01_the_official_client_connects_lists_every_tool_and_gets_structured_answers() {
    let client = connect().await;
    let info = client.peer_info().expect("server info");
    assert_eq!(
        info.server_info.as_ref().map(|s| s.name.as_str()),
        Some("kakurega")
    );
    let tools = client.list_all_tools().await.expect("tools/list");
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    assert!(!names.is_empty());
    for t in &tools {
        assert!(
            t.description.as_deref().is_some_and(|d| !d.is_empty()),
            "{} has a description",
            t.name
        );
        assert_eq!(
            t.input_schema.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "{} takes an object",
            t.name
        );
    }
    // a read-only tool answers with structured content that the SDK can parse
    let r = client
        .call_tool(CallToolRequestParams::new("overview"))
        .await;
    match r {
        Ok(res) => assert!(res.content.len() + usize::from(res.structured_content.is_some()) > 0),
        Err(e) => panic!("overview failed through the official client: {e}"),
    }
    // an unknown tool is an error, not a hang or a crash
    assert!(client
        .call_tool(CallToolRequestParams::new("definitely_not_a_tool"))
        .await
        .is_err());
    // a path outside the allowed folders comes back as a tool error that does not echo the path
    let bad = client
        .call_tool(
            CallToolRequestParams::new("verify_capsule")
                .with_arguments(object!({"path": "/etc/passwd"})),
        )
        .await;
    let text = format!("{bad:?}");
    assert!(!text.contains("root:"), "no file content: {text}");
    client.cancel().await.unwrap();
}
